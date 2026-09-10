//! Native review of one context's saved LSP commands. No subprocess is created
//! by capture, approval, cancellation or revocation.
use super::{
    evidence::{self, Evidence},
    settings::Settings,
};
use crate::{
    definitions::{Definitions, ExecutionDefinitions},
    host::Host,
    platform::{
        definition_write::DefinitionTarget,
        project_probe::ProjectLease,
        storage_paths::{display, ProtectedStorage},
    },
};
use code_pad_lib::lsp::{
    DocumentError, EnvironmentAllowlist, LspConfig, LspExecutionAuthority, LspManagerError,
    ManagedInstaller, ResolvedProcess, ReviewedLspExecution, RuntimeKind, RuntimeResolver,
    ServerRef,
};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
pub(super) const FILE: &str = "execution-approval.json";
const TTL: Duration = Duration::from_secs(180);
const MAX_PENDING: usize = 4;
#[derive(Clone, Default)]
pub(super) struct Activities {
    pub(super) context: crate::core::context_activity::ContextActivity,
    pub(super) filesystem: crate::core::context_activity::ContextActivity,
    pub(super) installation: crate::core::context_activity::ContextActivity,
}

fn native_target(context: &ProjectContext) -> bool {
    #[cfg(all(test, unix))]
    if matches!(&context.target,ExecutionTarget::Wsl {distro_id} if distro_id == "native-test-fixture")
    {
        return true;
    }
    context.target == ExecutionTarget::Windows
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Record {
    pub(super) schema_version: u32,
    pub(super) context: ProjectContext,
    pub(super) config_revision: String,
    pub(super) digest: String,
}
pub(super) fn record(bytes: Option<&[u8]>, context: &ProjectContext) -> Result<Option<Record>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.len() > 8192 {
        return Err("lsp_approval_invalid");
    }
    let value: Option<Record> =
        serde_json::from_slice(bytes).map_err(|_| "lsp_approval_invalid")?;
    if let Some(value) = &value {
        if value.schema_version != 1 {
            return Err("lsp_approval_future");
        }
        if value.context != *context
            || [&value.config_revision, &value.digest]
                .iter()
                .any(|digest| digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err("lsp_approval_invalid");
        }
    }
    Ok(value)
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
        output.insert(value.to_owned());
    } else {
        if local || value.to_string_lossy().contains(['/', '\\']) {
            output.insert(root.join(value));
        }
        if !value.to_string_lossy().contains(['/', '\\']) {
            for path in paths {
                let candidate = path.join(value);
                #[cfg(windows)]
                if value.extension().is_none() {
                    output.insert(candidate.with_extension("exe"));
                }
                output.insert(candidate);
            }
        }
    }
}

fn source_candidates(config: &LspConfig, paths: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    let root = Path::new(&config.workspace_root);
    let mut sources: BTreeSet<_> = paths.iter().cloned().collect();
    for server in config.server_by_language.values() {
        match server {
            ServerRef::Managed {
                manifest_id,
                version,
                node_path,
            } => {
                let manifest = ManagedInstaller::catalog_manifest(
                    manifest_id,
                    version,
                    ManagedInstaller::current_platform(),
                )
                .map_err(|_| "lsp_installation_unavailable")?;
                if manifest.runtime.kind == RuntimeKind::Node {
                    if let Some(node) = node_path {
                        candidates(node, root, paths, &mut sources, false);
                    } else {
                        candidates("node", root, paths, &mut sources, false);
                    }
                }
            }
            ServerRef::Custom { executable, .. } => {
                candidates(executable, root, paths, &mut sources, true)
            }
            ServerRef::Local {
                installed_path,
                executable,
                ..
            } => {
                let installed = PathBuf::from(installed_path);
                sources.insert(installed.clone());
                if let Some(executable) = executable {
                    sources.insert(installed.join(executable));
                    if let Some(parent) = installed.parent() {
                        sources.insert(parent.join(executable));
                    }
                }
            }
        }
    }
    for server in &config.custom_servers {
        candidates(&server.executable, root, paths, &mut sources, true);
        candidates(&server.runtime.executable, root, paths, &mut sources, false);
    }
    Ok(sources)
}

fn resolver(
    config: &LspConfig,
    protected: &ProtectedStorage,
    check: &dyn Fn() -> Result<()>,
) -> Result<(RuntimeResolver, Vec<PathBuf>)> {
    let root = Path::new(&config.workspace_root);
    let inherited = EnvironmentAllowlist::system();
    let mut paths = Vec::new();
    if let Some(path) = inherited.get(OsStr::new("PATH")) {
        for (index, path) in std::env::split_paths(path).enumerate() {
            if index >= 256 {
                return Err("lsp_source_limit");
            }
            // Unusable ambient entries are omitted before IO. Explicit server
            // paths below are instead rejected, so no configured source hides.
            if path.is_absolute()
                && evidence::transport(root, &path).is_ok()
                && protected.ensure_user_path(&path).is_ok()
            {
                paths.push(path);
            }
        }
    }
    let sources = source_candidates(config, &paths)?;
    for source in &sources {
        evidence::transport(root, source)?;
        protected.ensure_user_path(source)?;
    }
    let mut canonical = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        check()?;
        if devbox_filesystem::ensure_no_links(&path).is_err() {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => {
                let path =
                    display(&fs::canonicalize(&path).map_err(|_| "lsp_source_unavailable")?)?;
                evidence::transport(root, &path)?;
                protected.ensure_user_path(&path)?;
                if seen.insert(path.clone()) {
                    canonical.push(path);
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("lsp_source_unavailable"),
        }
    }
    let paths = canonical;
    evidence::inspect_candidates(root, &source_candidates(config, &paths)?, check)?;
    let environment = if paths.is_empty() {
        EnvironmentAllowlist::new()
    } else {
        EnvironmentAllowlist::with_path(
            std::env::join_paths(&paths).map_err(|_| "lsp_source_path_invalid")?,
        )
    };
    let environment = environment
        .with_native_platform()
        .map_err(|_| "lsp_source_unavailable")?;
    Ok((RuntimeResolver::new().with_environment(environment), paths))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandReview {
    language_id: String,
    executable: PathBuf,
    args: Vec<String>,
    runtime: Option<PathBuf>,
}

pub(super) struct Snapshot {
    host: Arc<Host>,
    settings: Settings,
    runtime: crate::private_metadata::MetadataRoot,
    lease: ProjectLease,
    definitions: ExecutionDefinitions,
    protected: ProtectedStorage,
    installer: Arc<ManagedInstaller>,
    managed: Vec<(String, String, PathBuf)>,
    config: LspConfig,
    processes: BTreeMap<String, ResolvedProcess>,
    environment: EnvironmentAllowlist,
    evidence: Evidence,
    approval_bytes: Option<Vec<u8>>,
    digest: String,
    pub(super) active: AtomicBool,
    owner_shutdown: std::sync::OnceLock<code_pad_lib::lsp::RequestCancellation>,
    activities: std::sync::OnceLock<Activities>,
    files: std::sync::OnceLock<Arc<std::sync::Mutex<crate::files_host::FilesHost>>>,
}
impl Snapshot {
    pub(super) fn capture(
        host: Arc<Host>,
        context: &ProjectContext,
        definitions: &mut Definitions,
        installer: Arc<ManagedInstaller>,
        protected: ProtectedStorage,
        deadline: u64,
        require_existing: bool,
    ) -> Result<Self> {
        let check = || {
            if let Some(issue) = super::actor::request_issue() {
                return Err(issue);
            }
            crate::files_host::current_deadline(deadline)
        };
        check()?;
        if !native_target(context) {
            return Err("lsp_target_unavailable");
        }
        let settings = Settings::open(&host, context)?;
        let config = settings.execution_config()?;
        let approval_bytes = settings.private().read(FILE)?;
        let approval = record(approval_bytes.as_deref(), context)?;
        if require_existing
            && approval.as_ref().is_none_or(|record| {
                record.config_revision != settings.revision().unwrap_or_default()
            })
        {
            return Err("lsp_execution_approval_required");
        }
        let lease = host.projects()?.admit(context)?;
        let root = Path::new(&config.workspace_root);
        protected.ensure_user_path(root)?;
        let definitions = definitions.execution_evidence(&host, context, deadline)?;
        let (resolver, paths) = resolver(&config, &protected, &check)?;
        let mut evidence = Evidence::default();
        for path in &paths {
            evidence.path(root, path, true, &check)?;
        }
        let mut managed = Vec::new();
        let mut processes = BTreeMap::new();
        for (language, server) in &config.server_by_language {
            check()?;
            let process = if let ServerRef::Managed {
                manifest_id,
                version,
                node_path,
            } = server
            {
                let installation = installer
                    .resolve_managed_install(manifest_id, version)
                    .map_err(|_| "lsp_installation_unavailable")?;
                evidence.tree(root, &installation.installed_path, &check)?;
                let process = resolver
                    .prepare_managed(
                        &installation.manifest,
                        language,
                        &installation.installed_path,
                        node_path.as_deref(),
                        root,
                    )
                    .map_err(|_| "lsp_command_unavailable")?;
                managed.push((
                    manifest_id.clone(),
                    version.clone(),
                    installation.installed_path,
                ));
                process
            } else {
                resolver
                    .resolve_server_ref(server, root)
                    .map_err(|_| "lsp_command_unavailable")?
            };
            processes.insert(language.clone(), process);
        }
        for server in &config.custom_servers {
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
        let mut reviews = Vec::new();
        for (language, process) in &processes {
            check()?;
            #[cfg(windows)]
            if process
                .executable
                .extension()
                .and_then(OsStr::to_str)
                .is_none_or(|extension| !extension.eq_ignore_ascii_case("exe"))
            {
                return Err("lsp_executable_format_unsupported");
            }
            evidence.path(root, &process.executable, false, &check)?;
            if let Some(runtime) = &process.runtime {
                evidence.path(root, &runtime.executable, false, &check)?;
                if runtime.kind == RuntimeKind::Node {
                    evidence.path(
                        root,
                        Path::new(process.args.first().ok_or("lsp_command_unavailable")?),
                        false,
                        &check,
                    )?;
                }
            }
            reviews.push(command_review(language, process)?);
        }
        let digest = crate::definitions::digest(
            &serde_json::to_vec(&(
                settings.revision()?,
                definitions.digest(),
                &reviews,
                environment_view(resolver.environment())?,
                evidence.digest()?,
            ))
            .map_err(|_| "lsp_source_path_invalid")?,
        );
        let runtime = settings.private().child("runtime")?;
        let snapshot = Self {
            host,
            settings,
            runtime,
            lease,
            definitions,
            protected,
            installer,
            managed,
            config,
            processes,
            environment: resolver.environment().clone(),
            evidence,
            approval_bytes,
            digest,
            active: AtomicBool::new(true),
            owner_shutdown: Default::default(),
            activities: Default::default(),
            files: Default::default(),
        };
        snapshot.revalidate_metadata()?;
        snapshot
            .evidence
            .revalidate(root_from(&snapshot.config), &check)?;
        if require_existing && !snapshot.approved()? {
            return Err("lsp_execution_approval_required");
        }
        Ok(snapshot)
    }
    pub(super) fn context(&self) -> &ProjectContext {
        &self.settings.context
    }
    pub(super) fn bind_owner(&self, shutdown: code_pad_lib::lsp::RequestCancellation) {
        let _ = self.owner_shutdown.set(shutdown);
    }
    pub(super) fn bind_files(&self, files: Arc<std::sync::Mutex<crate::files_host::FilesHost>>) {
        let _ = self.files.set(files);
    }
    pub(super) fn bind_activities(&self, activities: Activities) {
        let _ = self.activities.set(activities);
    }
    pub(super) fn config(&self) -> &LspConfig {
        &self.config
    }
    pub(super) fn data_path(&self) -> PathBuf {
        self.runtime.path().to_owned()
    }
    fn check_active(&self) -> Result<()> {
        if !self.active.load(Ordering::Acquire)
            || super::actor::request_cancelled()
            || self
                .owner_shutdown
                .get()
                .is_some_and(|owner| owner.is_cancelled())
        {
            Err("lsp_context_changed")
        } else {
            Ok(())
        }
    }
    fn revalidate_metadata(&self) -> Result<()> {
        self.check_active()?;
        self.settings.revalidate(&self.host)?;
        self.runtime.revalidate()?;
        if self.lease.binding() != self.settings.binding() {
            return Err("lsp_context_changed");
        }
        self.lease.revalidate()?;
        self.definitions.revalidate()?;
        if self.settings.private().read(FILE)? != self.approval_bytes {
            return Err("lsp_execution_approval_required");
        }
        self.check_active()
    }
    fn approved(&self) -> Result<bool> {
        Ok(record(self.approval_bytes.as_deref(), self.context())?
            .is_some_and(|record| record.digest == self.digest))
    }
    fn view(&self) -> Result<Value> {
        let commands = self
            .processes
            .iter()
            .map(|(language, process)| command_review(language, process))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"approved":self.approved()?,"workspaceRoot":self.config.workspace_root,"configRevision":self.settings.revision()?,"commands":commands,"environment":environment_view(&self.environment)?,"definitionsDigest":self.definitions.digest()}),
        )
    }
    fn approve(&self, deadline: u64) -> Result<()> {
        let check = || {
            crate::files_host::current_deadline(deadline)?;
            self.check_active()
        };
        self.revalidate_metadata()?;
        self.evidence.revalidate(root_from(&self.config), &check)?;
        let value = Record {
            schema_version: 1,
            context: self.context().clone(),
            config_revision: self.settings.revision()?,
            digest: self.digest.clone(),
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| "lsp_approval_invalid")?;
        DefinitionTarget::capture(
            &self.settings.private().path().join(FILE),
            self.approval_bytes.as_deref(),
        )?
        .write_utf8(&bytes, || {
            check()?;
            self.revalidate_metadata()
        })
        .map(|_| ())
    }
    pub(super) fn reviewed(&self) -> ReviewedLspExecution {
        ReviewedLspExecution {
            config: self.config.clone(),
            processes: self.processes.clone(),
            environment: self.environment.clone(),
        }
    }
    pub(super) fn refresh_after_rename(
        &self,
        files: &mut crate::files_host::FilesHost,
        path: &str,
        file: &code_pad_lib::lsp::RenameFileResult,
    ) -> Result<Option<(code_pad_lib::commands::file::OpenedFileWire, String)>> {
        self.validate_document(Path::new(path))?;
        files.refresh_after_rename(Some((self.context(), &self.lease)), path, file)
    }
    pub(super) fn document_snapshot(
        &self,
        files: &crate::files_host::FilesHost,
        path: &str,
        revision: &str,
        verify_disk: bool,
    ) -> Result<crate::file_owner::EditorSnapshot> {
        self.validate_document(Path::new(path))?;
        files.editor_snapshot(
            Some((self.context(), &self.lease)),
            path,
            revision,
            verify_disk,
        )
    }
    pub(super) fn document_permit(
        &self,
        write: bool,
    ) -> Result<crate::core::context_activity::ContextPermit> {
        self.activities
            .get()
            .ok_or("lsp_unavailable")?
            .filesystem
            .enter(write)
            .map_err(|_| "lsp_busy")
    }
    pub(super) fn validate_document(&self, path: &Path) -> Result<()> {
        self.revalidate_metadata()?;
        if !self.approved()? {
            return Err("lsp_execution_approval_required");
        }
        let path = display(path)?;
        let root = Path::new(&self.settings.binding().root);
        // Preserve NTFS directory case before IO. Physical ancestry is checked
        // too, so a canonical result cannot switch to an alias or replacement.
        if !path.starts_with(root) {
            return Err("lsp_document_denied");
        }
        self.protected.ensure_user_path(&path)?;
        evidence::transport(root, &path)?;
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "lsp_document_denied")?;
        let canonical = display(&fs::canonicalize(&path).map_err(|_| "lsp_document_denied")?)?;
        self.protected.ensure_user_path(&canonical)?;
        if !canonical.starts_with(root) {
            return Err("lsp_document_denied");
        }
        let candidate = if canonical == root {
            canonical.as_path()
        } else {
            canonical.parent().ok_or("lsp_document_denied")?
        };
        let mut found = false;
        for parent in candidate.ancestors() {
            if devbox_filesystem::filesystem_identity(parent, true).ok()
                == Some(self.lease.native_root_identity())
            {
                found = true;
                break;
            }
            if parent == root {
                break;
            }
        }
        if !found {
            return Err("lsp_document_denied");
        }
        self.revalidate_metadata()
    }
}
fn root_from(config: &LspConfig) -> &Path {
    Path::new(&config.workspace_root)
}
fn environment_view(environment: &EnvironmentAllowlist) -> Result<BTreeMap<&str, &str>> {
    environment
        .iter()
        .map(|(key, value)| {
            Ok((
                key.to_str().ok_or("lsp_source_path_invalid")?,
                value.to_str().ok_or("lsp_source_path_invalid")?,
            ))
        })
        .collect()
}
fn command_review(language: &str, process: &ResolvedProcess) -> Result<CommandReview> {
    Ok(CommandReview {
        language_id: language.to_owned(),
        executable: display(&process.executable)?,
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
            .map(|runtime| display(&runtime.executable))
            .transpose()?,
    })
}
impl LspExecutionAuthority for Snapshot {
    fn begin_start(&self) -> std::result::Result<Option<Box<dyn Send>>, LspManagerError> {
        self.check_active()
            .map_err(|_| LspManagerError::ExecutionApprovalRequired)?;
        let activities = self
            .activities
            .get()
            .ok_or(LspManagerError::ExecutionApprovalRequired)?;
        let context = activities
            .context
            .enter(false)
            .map_err(|_| LspManagerError::ExecutionBusy)?;
        let filesystem = activities
            .filesystem
            .enter(false)
            .map_err(|_| LspManagerError::ExecutionBusy)?;
        let installation = activities
            .installation
            .enter(false)
            .map_err(|_| LspManagerError::ExecutionBusy)?;
        Ok(Some(Box::new((context, filesystem, installation))))
    }
    fn startup_cancelled(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            while self.check_active().is_ok() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
    }
    fn validate_config(
        &self,
        language: &str,
        config: &LspConfig,
    ) -> std::result::Result<(), LspManagerError> {
        if self.config != *config
            || !self.processes.contains_key(language)
            || self.revalidate_metadata().is_err()
            || !self.approved().unwrap_or(false)
        {
            return Err(LspManagerError::ExecutionApprovalRequired);
        }
        Ok(())
    }
    fn validate_process(
        &self,
        process: &ResolvedProcess,
    ) -> std::result::Result<(), LspManagerError> {
        let check = || self.check_active();
        let validate = || -> Result<()> {
            self.revalidate_metadata()?;
            if !self.approved()? || !self.processes.values().any(|expected| expected == process) {
                return Err("lsp_execution_approval_required");
            }
            for (id, version, path) in &self.managed {
                check()?;
                let installation = self
                    .installer
                    .resolve_managed_install(id, version)
                    .map_err(|_| "lsp_installation_unavailable")?;
                if installation.installed_path != *path {
                    return Err("lsp_sources_changed");
                }
            }
            self.evidence.revalidate(root_from(&self.config), &check)?;
            self.revalidate_metadata()
        };
        validate().map_err(|_| LspManagerError::ExecutionApprovalRequired)
    }
    fn validate_document_write_path(&self, path: &Path) -> std::result::Result<(), DocumentError> {
        let validate = || -> Result<()> {
            self.validate_document(path)?;
            self.files
                .get()
                .ok_or("lsp_document_denied")?
                .try_lock()
                .map_err(|_| "files_unavailable")?
                .guard_editor_write(self.context(), path.to_str().ok_or("lsp_document_denied")?)
        };
        validate().map_err(|_| DocumentError::AccessDenied)
    }
    fn validate_document_path(&self, path: &Path) -> std::result::Result<(), DocumentError> {
        self.validate_document(path)
            .map_err(|_| DocumentError::AccessDenied)
    }
}

pub(super) enum ReviewSnapshot {
    Native(Box<Snapshot>),
    #[cfg(windows)]
    Wsl(Box<super::wsl_approval::Snapshot>),
}
impl From<Snapshot> for ReviewSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        Self::Native(Box::new(snapshot))
    }
}
#[cfg(windows)]
impl From<super::wsl_approval::Snapshot> for ReviewSnapshot {
    fn from(snapshot: super::wsl_approval::Snapshot) -> Self {
        Self::Wsl(Box::new(snapshot))
    }
}
impl ReviewSnapshot {
    fn context(&self) -> &ProjectContext {
        match self {
            Self::Native(snapshot) => snapshot.context(),
            #[cfg(windows)]
            Self::Wsl(snapshot) => snapshot.context(),
        }
    }
    fn view(&self) -> Result<Value> {
        match self {
            Self::Native(snapshot) => snapshot.view(),
            #[cfg(windows)]
            Self::Wsl(snapshot) => snapshot.view(),
        }
    }
    fn approve(&self, deadline: u64) -> Result<()> {
        match self {
            Self::Native(snapshot) => snapshot.approve(deadline),
            #[cfg(windows)]
            Self::Wsl(snapshot) => snapshot.approve(deadline),
        }
    }
}
struct Pending {
    snapshot: ReviewSnapshot,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Approvals {
    pending: HashMap<String, Pending>,
}
impl Approvals {
    pub(super) fn expire(&mut self) {
        self.pending
            .retain(|_, pending| pending.created.elapsed() < TTL);
    }
    pub(super) fn preview(&mut self, snapshot: impl Into<ReviewSnapshot>) -> Result<Value> {
        let snapshot = snapshot.into();
        self.expire();
        if self.pending.len() >= MAX_PENDING {
            return Err("lsp_review_busy");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut view = snapshot.view()?;
        view["previewId"] = json!(id);
        self.pending.insert(
            id,
            Pending {
                snapshot,
                created: Instant::now(),
            },
        );
        Ok(view)
    }
    pub(super) fn approve(
        &mut self,
        context: &ProjectContext,
        id: &str,
        deadline: u64,
    ) -> Result<Value> {
        self.expire();
        let pending = self.pending.remove(id).ok_or("lsp_review_expired")?;
        if pending.snapshot.context() != context {
            return Err("lsp_context_changed");
        }
        pending.snapshot.approve(deadline)?;
        Ok(Value::Null)
    }
    pub(super) fn cancel(&mut self, context: &ProjectContext, id: &str) -> Result<Value> {
        if self
            .pending
            .get(id)
            .is_some_and(|pending| pending.snapshot.context() != context)
        {
            return Err("lsp_context_changed");
        }
        self.pending.remove(id);
        Ok(Value::Null)
    }
    pub(super) fn revoke(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<Value> {
        self.pending
            .retain(|_, pending| pending.snapshot.context() != context);
        let settings = Settings::open(host, context)?;
        let original = settings.private().read(FILE)?;
        record(original.as_deref(), context)?;
        DefinitionTarget::capture(&settings.private().path().join(FILE), original.as_deref())?
            .write_utf8(b"null", || {
                crate::files_host::current_deadline(deadline)?;
                settings.revalidate(host)?;
                if settings.private().read(FILE)? != original {
                    return Err("lsp_approval_changed");
                }
                Ok(())
            })?;
        Ok(Value::Null)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(in crate::lsp_host) struct Fixture {
        _data: tempfile::TempDir,
        pub(in crate::lsp_host) root: tempfile::TempDir,
        canonical_root: PathBuf,
        pub(in crate::lsp_host) host: Arc<Host>,
        pub(in crate::lsp_host) context: ProjectContext,
        pub(in crate::lsp_host) installer: Arc<ManagedInstaller>,
        protected: ProtectedStorage,
    }
    impl Fixture {
        pub(in crate::lsp_host) fn new() -> Self {
            let data = tempfile::tempdir().unwrap();
            let root = tempfile::tempdir().unwrap();
            let host = Arc::new(Host::open(data.path()).unwrap());
            host.start_empty().unwrap();
            let owner = host.projects().unwrap();
            let preview = owner.preview_fixture(root.path()).unwrap();
            let context = owner
                .apply(
                    &preview.preview_id,
                    "fixture",
                    crate::project_owner::RegistrationAction::Register,
                )
                .unwrap()
                .1;
            let canonical_root = PathBuf::from(owner.binding(&context).unwrap().root);
            let executable = canonical_root.join("server.exe");
            fs::write(&executable, b"synthetic code, never executed by review").unwrap();
            let settings = Settings::open(&host, &context).unwrap();
            let mut view = settings.view().unwrap();
            view["config"]["enabled"] = json!(true);
            view["config"]["server_by_language"] =
                json!({"rust":{"kind":"custom","executable":executable,"args":[]}});
            settings.save(&host,json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false}),u64::MAX).unwrap();
            let installer =
                Arc::new(ManagedInstaller::new(host.component("files").unwrap()).unwrap());
            let protected = ProtectedStorage::new(host.storage_root(), vec![]).unwrap();
            Self {
                _data: data,
                root,
                canonical_root,
                host,
                context,
                installer,
                protected,
            }
        }
        pub(in crate::lsp_host) fn path(&self) -> &Path {
            &self.canonical_root
        }
        pub(in crate::lsp_host) fn capture(&self, require_existing: bool) -> Result<Snapshot> {
            let snapshot = Snapshot::capture(
                self.host.clone(),
                &self.context,
                &mut Definitions::default(),
                self.installer.clone(),
                self.protected.clone(),
                u64::MAX,
                require_existing,
            )?;
            snapshot.bind_activities(Activities::default());
            Ok(snapshot)
        }
    }
    #[test]
    fn startup_retains_context_and_installation_exclusion_without_blocking_offline_editing() {
        let fixture = Fixture::new();
        let pending = fixture.capture(false).unwrap();
        pending.approve(u64::MAX).unwrap();
        drop(pending);
        let snapshot = fixture.capture(true).unwrap();
        let activities = snapshot.activities.get().unwrap();
        let start = snapshot.begin_start().unwrap();
        assert!(activities.context.enter(true).is_err());
        assert!(activities.filesystem.enter(true).is_err());
        assert!(activities.installation.enter(true).is_err());
        drop(start);
        let installing = activities.installation.enter(true).unwrap();
        assert!(matches!(
            snapshot.begin_start(),
            Err(LspManagerError::ExecutionBusy)
        ));
        assert!(activities.context.enter(true).is_ok());
        assert!(activities.filesystem.enter(false).is_ok());
        drop(installing);
        assert!(snapshot.begin_start().is_ok());
    }
    #[test]
    fn approval_is_one_time_and_changed_code_or_settings_require_new_review() {
        let fixture = Fixture::new();
        assert!(matches!(
            fixture.capture(true),
            Err("lsp_execution_approval_required")
        ));
        let mut approvals = Approvals::default();
        let view = approvals.preview(fixture.capture(false).unwrap()).unwrap();
        assert_eq!(view["approved"], false);
        assert_eq!(view["commands"][0]["languageId"], "rust");
        let token = view["previewId"].as_str().unwrap();
        approvals
            .approve(&fixture.context, token, u64::MAX)
            .unwrap();
        assert_eq!(
            approvals
                .approve(&fixture.context, token, u64::MAX)
                .unwrap_err(),
            "lsp_review_expired"
        );
        let approved = fixture.capture(true).unwrap();
        let process = &approved.processes["rust"];
        approved.validate_process(process).unwrap();
        fs::write(fixture.path().join("server.exe"), b"changed executable").unwrap();
        assert!(approved.validate_process(process).is_err());
        fs::write(
            fixture.path().join("server.exe"),
            b"synthetic code, never executed by review",
        )
        .unwrap();
        approved.validate_process(process).unwrap();
        let settings = Settings::open(&fixture.host, &fixture.context).unwrap();
        let mut view = settings.view().unwrap();
        view["config"]["server_by_language"]["rust"]["args"] = json!(["changed-argument"]);
        settings.save(&fixture.host,json!({"config":view["config"],"nativeRevision":view["nativeRevision"],"recoverInvalid":false}),u64::MAX).unwrap();
        assert!(approved.validate_config("rust", &approved.config).is_err());
        assert!(matches!(
            fixture.capture(true),
            Err("lsp_execution_approval_required")
        ));
        drop(approved);
        fixture.root.close().unwrap();
        approvals
            .revoke(&fixture.host, &fixture.context, u64::MAX)
            .unwrap();
    }
    #[test]
    fn document_scope_checks_protected_paths_and_revocation_before_file_access() {
        let fixture = Fixture::new();
        let pending = fixture.capture(false).unwrap();
        pending.approve(u64::MAX).unwrap();
        drop(pending);
        let approved = fixture.capture(true).unwrap();
        let document = Path::new(&approved.config.workspace_root).join("main.rs");
        fs::write(&document, b"fn main() {}\n").unwrap();
        approved.validate_document(&document).unwrap();
        assert!(approved
            .validate_document(&fixture.host.storage_root().join("unavailable-secret-file"))
            .is_err());
        approved.active.store(false, Ordering::Release);
        assert!(approved.validate_document(&document).is_err());
        assert!(approved
            .validate_process(&approved.processes["rust"])
            .is_err());
    }
    #[test]
    fn foreign_tokens_and_future_approval_records_preserve_existing_metadata() {
        let fixture = Fixture::new();
        let other = Fixture::new();
        let mut approvals = Approvals::default();
        let view = approvals.preview(fixture.capture(false).unwrap()).unwrap();
        assert_eq!(
            approvals
                .approve(
                    &other.context,
                    view["previewId"].as_str().unwrap(),
                    u64::MAX
                )
                .unwrap_err(),
            "lsp_context_changed"
        );
        let settings = Settings::open(&fixture.host, &fixture.context).unwrap();
        let original=serde_json::to_vec(&json!({"schemaVersion":99,"context":fixture.context,"configRevision":"a".repeat(64),"digest":"b".repeat(64)})).unwrap();
        settings.private().write(FILE, &original).unwrap();
        assert_eq!(
            approvals
                .revoke(&fixture.host, &fixture.context, u64::MAX)
                .unwrap_err(),
            "lsp_approval_future"
        );
        assert_eq!(settings.private().read(FILE).unwrap().unwrap(), original);
    }
}
