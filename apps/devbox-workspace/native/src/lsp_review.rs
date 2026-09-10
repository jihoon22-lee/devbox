//! Read-only Linux LSP command evidence. Windows owns configuration and approval;
//! observing an installed command cannot start it or provision a runtime.
use crate::{
    lsp_environment::Environment,
    lsp_evidence::{self, Evidence},
};
use code_pad_lib::lsp::{EnvironmentAllowlist, LspConfig, ResolvedProcess, RuntimeKind, ServerRef};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_CONFIG: usize = 256 * 1024;

#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Method {
    #[serde(rename = "lsp_capture")]
    Capture {
        context: ProjectContext,
        config: LspConfig,
    },
    #[serde(rename = "lsp_validate")]
    Validate {
        context: ProjectContext,
        digest: String,
    },
}
impl Method {
    pub(crate) fn context(&self) -> &ProjectContext {
        match self {
            Self::Capture { context, .. } | Self::Validate { context, .. } => context,
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandReview {
    language_id: String,
    executable: PathBuf,
    args: Vec<String>,
    runtime: Option<PathBuf>,
}
fn command_review(language: &str, process: &ResolvedProcess) -> Result<CommandReview> {
    Ok(CommandReview {
        language_id: language.into(),
        executable: process.executable.clone(),
        args: process
            .args
            .iter()
            .map(|arg| {
                arg.to_str()
                    .map(str::to_owned)
                    .ok_or("lsp_source_path_invalid")
            })
            .collect::<Result<_>>()?,
        runtime: process
            .runtime
            .as_ref()
            .map(|runtime| runtime.executable.clone()),
    })
}
fn candidates(
    value: &str,
    root: &Path,
    paths: &[PathBuf],
    output: &mut BTreeSet<PathBuf>,
    local: bool,
) {
    let value = Path::new(value);
    if value.is_absolute() {
        output.insert(value.into());
    } else {
        let text = value.to_string_lossy();
        if local || text.contains(['/', '\\']) {
            output.insert(root.join(value));
        }
        if !text.contains(['/', '\\']) {
            output.extend(paths.iter().map(|path| path.join(value)));
        }
    }
}
fn source_candidates(config: &LspConfig, paths: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let root = Path::new(&config.workspace_root);
    let mut output = BTreeSet::new();
    for server in config.server_by_language.values() {
        match server {
            ServerRef::Managed { .. } => return Err("wsl_lsp_installed_target_required"),
            ServerRef::Custom { executable, .. } => {
                candidates(executable, root, paths, &mut output, true)
            }
            ServerRef::Local {
                installed_path,
                executable,
                ..
            } => {
                let installed = PathBuf::from(installed_path);
                output.insert(installed.clone());
                if let Some(executable) = executable {
                    output.insert(installed.join(executable));
                    if let Some(parent) = installed.parent() {
                        output.insert(parent.join(executable));
                    }
                }
            }
        }
    }
    for server in &config.custom_servers {
        candidates(&server.executable, root, paths, &mut output, true);
        candidates(&server.runtime.executable, root, paths, &mut output, false);
    }
    Ok(output)
}
fn native_program(root: &Path, path: &Path, check: &dyn Fn() -> Result<()>) -> Result<()> {
    check()?;
    lsp_evidence::transport_with_admission(root, path, crate::linux_files::admit)?;
    devbox_filesystem::ensure_no_links(path).map_err(|_| "lsp_source_path_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "lsp_source_unavailable")?;
    let metadata = file.metadata().map_err(|_| "lsp_source_unavailable")?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err("lsp_executable_format_unsupported");
    }
    // The bundled helper and this initial target are x86_64 Linux. Never let
    // WSL binfmt/interop turn an approved server into a Windows executable.
    let mut header = [0; 64];
    file.read_exact(&mut header)
        .map_err(|_| "lsp_executable_format_unsupported")?;
    if header[..7] != *b"\x7fELF\x02\x01\x01"
        || !matches!(u16::from_le_bytes([header[16], header[17]]), 2 | 3)
        || u16::from_le_bytes([header[18], header[19]]) != 62
        || header[20..24] != [1, 0, 0, 0]
        || u16::from_le_bytes([header[52], header[53]]) != 64
    {
        return Err("lsp_executable_format_unsupported");
    }
    check()?;
    lsp_evidence::transport_with_admission(root, path, crate::linux_files::admit)?;
    if devbox_filesystem::filesystem_identity(path, false).ok() != Some(identity) {
        return Err("lsp_sources_changed");
    }
    Ok(())
}
pub(crate) struct Review {
    context: ProjectContext,
    config: LspConfig,
    pub(crate) processes: BTreeMap<String, ResolvedProcess>,
    pub(crate) environment: EnvironmentAllowlist,
    evidence: Evidence,
    digest: String,
}
impl Review {
    pub(crate) fn capture(
        root: &Path,
        context: ProjectContext,
        config: LspConfig,
        environment: &Environment,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        check()?;
        config.validate().map_err(|_| "lsp_config_invalid")?;
        if !config.enabled || Path::new(&config.workspace_root) != root {
            return Err("lsp_settings_save_required");
        }
        if serde_json::to_vec(&config)
            .map_err(|_| "lsp_config_invalid")?
            .len()
            > MAX_CONFIG
        {
            return Err("lsp_source_limit");
        }
        if config
            .server_by_language
            .values()
            .any(|server| matches!(server, ServerRef::Managed { .. }))
        {
            return Err("wsl_lsp_installed_target_required");
        }
        for language in config.server_by_language.keys().chain(
            config
                .custom_servers
                .iter()
                .flat_map(|server| &server.language_ids),
        ) {
            if ![
                "rust",
                "javascript",
                "typescript",
                "python",
                "html",
                "css",
                "json",
            ]
            .contains(&language.as_str())
            {
                return Err("wsl_lsp_language_unsupported");
            }
        }
        let (resolver, paths) = environment.resolver(root, check)?;
        lsp_evidence::inspect_candidates_with_admission(
            root,
            &source_candidates(&config, &paths)?,
            check,
            crate::linux_files::admit,
        )?;
        let mut evidence = Evidence::with_admission(crate::linux_files::admit);
        evidence.path(root, root, true, check)?;
        for path in &paths {
            evidence.path(root, path, true, check)?;
        }
        if let Some(home) = resolver.environment().get(OsStr::new("HOME")) {
            evidence.path(root, Path::new(home), true, check)?;
        }
        let mut processes = BTreeMap::new();
        for (language, server) in &config.server_by_language {
            check()?;
            processes.insert(
                language.clone(),
                resolver
                    .resolve_server_ref(server, root)
                    .map_err(|_| "lsp_command_unavailable")?,
            );
        }
        for server in &config.custom_servers {
            check()?;
            let process = resolver
                .resolve_custom(server, root)
                .map_err(|_| "lsp_command_unavailable")?;
            for language in &server.language_ids {
                processes
                    .entry(language.clone())
                    .or_insert_with(|| process.clone());
            }
        }
        if processes.is_empty() || processes.len() > 64 {
            return Err("lsp_command_unavailable");
        }
        let mut commands = Vec::new();
        for (language, process) in &processes {
            check()?;
            native_program(root, &process.executable, check)?;
            evidence.path(root, &process.executable, false, check)?;
            if let Some(runtime) = &process.runtime {
                native_program(root, &runtime.executable, check)?;
                evidence.path(root, &runtime.executable, false, check)?;
                if runtime.kind == RuntimeKind::Node {
                    evidence.path(
                        root,
                        Path::new(process.args.first().ok_or("lsp_command_unavailable")?),
                        false,
                        check,
                    )?;
                }
            }
            commands.push(command_review(language, process)?);
        }
        evidence.revalidate(root, check)?;
        let variables = resolver
            .environment()
            .iter()
            .map(|(key, value)| {
                Ok((
                    key.to_str().ok_or("lsp_environment_unavailable")?,
                    value.to_str().ok_or("lsp_environment_unavailable")?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let bytes =
            serde_json::to_vec(&(&context, &config, &commands, variables, evidence.digest()?))
                .map_err(|_| "lsp_source_path_invalid")?;
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(Self {
            context,
            config,
            processes,
            environment: resolver.environment().clone(),
            evidence,
            digest,
        })
    }
    pub(crate) fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub(crate) fn config(&self) -> &LspConfig {
        &self.config
    }
    pub(crate) fn revalidate(&self, digest: &str, check: &dyn Fn() -> Result<()>) -> Result<()> {
        if digest != self.digest {
            return Err("lsp_sources_changed");
        }
        self.evidence
            .revalidate(Path::new(&self.config.workspace_root), check)
    }
    pub(crate) fn view(&self) -> Result<Value> {
        let commands = self
            .processes
            .iter()
            .map(|(language, process)| command_review(language, process))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"digest":self.digest,"commands":commands,"environmentKeys":self.environment.iter().map(|(key,_)|key.to_str().ok_or("lsp_environment_unavailable")).collect::<Result<Vec<_>>>()?}),
        )
    }
}
