use crate::Language;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub language: Language,
    pub command: String,
    pub args: Vec<String>,
    pub install: Option<String>,
}
// Primary installation references, checked 2026-10-24, are recorded in PROGRESS S4-3.
pub fn catalog() -> Vec<ServerSpec> {
    use Language::*;
    [
        (
            Rust,
            "rust-analyzer",
            vec![],
            "rustup component add rust-analyzer rust-src",
        ),
        (
            TypeScript,
            "typescript-language-server",
            vec!["--stdio"],
            "npm install -g typescript-language-server typescript@6",
        ),
        (
            TypeScript,
            "vtsls",
            vec!["--stdio"],
            "npm install -g @vtsls/language-server",
        ),
        (
            Python,
            "pyright-langserver",
            vec!["--stdio"],
            "npm install -g pyright",
        ),
        (
            Go,
            "gopls",
            vec![],
            "go install golang.org/x/tools/gopls@latest",
        ),
        (
            Bash,
            "bash-language-server",
            vec!["start"],
            "npm install -g bash-language-server",
        ),
        (
            Yaml,
            "yaml-language-server",
            vec!["--stdio"],
            "npm install -g yaml-language-server",
        ),
        (
            Toml,
            "taplo",
            vec!["lsp", "stdio"],
            "cargo install taplo-cli --locked --features lsp",
        ),
        (
            Json,
            "vscode-json-language-server",
            vec!["--stdio"],
            "npm install -g vscode-langservers-extracted",
        ),
    ]
    .into_iter()
    .map(|(language, command, args, install)| ServerSpec {
        language,
        command: command.into(),
        args: args.into_iter().map(str::to_owned).collect(),
        install: Some(install.into()),
    })
    .collect()
}
pub fn detected(language: Language, path: Option<&OsStr>) -> Option<(ServerSpec, Option<PathBuf>)> {
    detect_in(&catalog(), language, path)
}
pub(crate) fn detect_in(
    catalog: &[ServerSpec],
    language: Language,
    path: Option<&OsStr>,
) -> Option<(ServerSpec, Option<PathBuf>)> {
    let specs: Vec<_> = catalog
        .iter()
        .filter(|s| s.language == language)
        .cloned()
        .collect();
    if let Some(path) = path {
        for spec in &specs {
            if let Some(found) = find(&spec.command, path) {
                return Some((spec.clone(), Some(found)));
            }
        }
    }
    Some((specs.into_iter().next()?, None))
}
fn find(command: &str, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|p| p.is_absolute())
        .map(|p| p.join(command))
        .find(|p| executable(p))
}
fn executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_install_commands_are_fixed_and_detection_never_runs_them() {
        let specs = catalog();
        assert_eq!(specs.len(), 9);
        let rust = specs
            .iter()
            .find(|s| s.language == Language::Rust)
            .expect("rust");
        assert_eq!(
            rust.install.as_deref(),
            Some("rustup component add rust-analyzer rust-src")
        );
        let ts = detected(Language::TypeScript, Some(OsStr::new("/nonexistent"))).expect("ts");
        assert_eq!(ts.0.command, "typescript-language-server");
        assert!(ts.1.is_none());
    }
    #[test]
    #[cfg(unix)]
    fn typescript_prefers_primary_then_detects_vtsls() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("fixture");
        let put = |name: &str| {
            let p = temp.path().join(name);
            std::fs::write(&p, "#!/bin/sh\nexit 99\n").expect("fixture");
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).expect("mode");
        };
        put("vtsls");
        let path = temp.path().as_os_str();
        assert_eq!(
            detected(Language::TypeScript, Some(path))
                .expect("ts")
                .0
                .command,
            "vtsls"
        );
        put("typescript-language-server");
        assert_eq!(
            detected(Language::TypeScript, Some(path))
                .expect("ts")
                .0
                .command,
            "typescript-language-server"
        );
    }
}
