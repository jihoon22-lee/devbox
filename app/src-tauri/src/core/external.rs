use super::connection::IpcError;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalKind {
    Vscode,
    Explorer,
    Terminal,
}

pub fn arguments(kind: ExternalKind, distro: &str, path: &str) -> Result<Vec<String>, IpcError> {
    if distro.is_empty()
        || distro.chars().any(char::is_control)
        || distro.contains(['\\', '/', '"', ';'])
        || !path.starts_with('/')
        || path.chars().any(char::is_control)
        || path.contains(['\\', '"'])
        || path.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(IpcError::new("외부 앱으로 열 경로가 올바르지 않습니다"));
    }
    let unc = format!(r"\\wsl.localhost\{}{}", distro, path.replace('/', r"\"));
    Ok(match kind {
        ExternalKind::Vscode => vec!["--remote".into(), format!("wsl+{distro}"), path.into()],
        ExternalKind::Explorer => vec![unc],
        ExternalKind::Terminal => {
            // WT interprets semicolons as new commands even without a shell.
            if path.contains(';') {
                return Err(IpcError::new("Windows Terminal로 열 수 없는 경로입니다"));
            }
            vec!["-p".into(), distro.into(), "-d".into(), unc]
        }
    })
}

/// Resolve the executable and CLI entry next to the official code.cmd wrapper.
pub fn vscode_cli(bin: &std::path::Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let root = bin.parent()?;
    let exe = root.join("Code.exe");
    let cli = root.join("resources/app/out/cli.js");
    (bin.join("code.cmd").is_file() && exe.is_file() && cli.is_file()).then_some((exe, cli))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_approved_external_kinds_deserialize() {
        for kind in ["vscode", "explorer", "terminal"] {
            assert!(serde_json::from_str::<ExternalKind>(&format!("\"{kind}\"")).is_ok());
        }
        for kind in ["shell", "cmd.exe", "powershell"] {
            assert!(serde_json::from_str::<ExternalKind>(&format!("\"{kind}\"")).is_err());
        }
    }
    #[test]
    fn external_arguments_keep_spaces_unicode_and_shell_metacharacters_literal() {
        let path = "/home/user/프로젝트 & name";
        assert_eq!(
            arguments(ExternalKind::Vscode, "Debian", path).unwrap(),
            ["--remote", "wsl+Debian", path]
        );
        assert_eq!(
            arguments(ExternalKind::Explorer, "Debian", path).unwrap(),
            [r"\\wsl.localhost\Debian\home\user\프로젝트 & name"]
        );
        assert_eq!(
            arguments(ExternalKind::Terminal, "Debian", path).unwrap(),
            [
                "-p",
                "Debian",
                "-d",
                r"\\wsl.localhost\Debian\home\user\프로젝트 & name"
            ]
        );
    }
    #[test]
    fn invalid_paths_and_terminal_command_separators_are_rejected() {
        for path in ["relative", "/x/../y", "/x\\y", "/x\0y", "/x\ny"] {
            assert!(arguments(ExternalKind::Explorer, "Debian", path).is_err());
        }
        assert!(arguments(ExternalKind::Terminal, "Debian", "/x;new-tab cmd.exe").is_err());
        assert!(arguments(ExternalKind::Explorer, "bad\\distro", "/x").is_err());
    }
    #[test]
    fn vscode_cli_uses_only_complete_owned_wrapper_layout() {
        let fixture = tempfile::tempdir().unwrap();
        let bin = fixture.path().join("Microsoft VS Code/bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert!(vscode_cli(&bin).is_none());
        let root = bin.parent().unwrap();
        std::fs::create_dir_all(root.join("resources/app/out")).unwrap();
        for file in [
            bin.join("code.cmd"),
            root.join("Code.exe"),
            root.join("resources/app/out/cli.js"),
        ] {
            std::fs::write(file, b"owned fixture").unwrap();
        }
        assert_eq!(
            vscode_cli(&bin).unwrap(),
            (root.join("Code.exe"), root.join("resources/app/out/cli.js"))
        );
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    Vscode,
    Cursor,
}

pub fn file_arguments(
    distro: &str,
    path: &str,
    line: u32,
    column: u32,
) -> Result<Vec<String>, IpcError> {
    let mut args = arguments(ExternalKind::Vscode, distro, path)?;
    if line == 0 || column == 0 || path.contains(':') {
        return Err(IpcError::new("파일 위치가 올바르지 않습니다"));
    }
    args.pop();
    args.push("--goto".into());
    args.push(format!("{path}:{line}:{column}"));
    Ok(args)
}

pub fn editor_cli(
    bin: &std::path::Path,
    kind: EditorKind,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    match kind {
        EditorKind::Vscode => vscode_cli(bin),
        EditorKind::Cursor => {
            let parent = bin.parent()?;
            let root = if bin.ends_with("resources/app/bin") {
                parent.parent()?.parent()?
            } else {
                parent
            };
            let exe = root.join("Cursor.exe");
            let cli = root.join("resources/app/out/cli.js");
            (bin.join("cursor.cmd").is_file() && exe.is_file() && cli.is_file())
                .then_some((exe, cli))
        }
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    #[test]
    fn external_file_goto_keeps_every_location_argument_literal() {
        assert_eq!(
            file_arguments("Debian", "/owned/a & $(x); '한글'.rs", 12, 3).unwrap(),
            [
                "--remote",
                "wsl+Debian",
                "--goto",
                "/owned/a & $(x); '한글'.rs:12:3"
            ]
        );
        for path in ["--exec", "vscode://x", "/x/../y", "/x\ny", "/x:2"] {
            assert!(file_arguments("Debian", path, 1, 1).is_err());
        }
        assert!(file_arguments("Debian", "/owned/x", 0, 1).is_err());
        assert!(file_arguments("Debian", "/owned/x", 1, 0).is_err());
    }
    #[test]
    fn external_cursor_cli_requires_complete_owned_layout() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("Cursor");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        assert!(editor_cli(&bin, EditorKind::Cursor).is_none());
        std::fs::create_dir_all(root.join("resources/app/out")).unwrap();
        for file in [
            bin.join("cursor.cmd"),
            root.join("Cursor.exe"),
            root.join("resources/app/out/cli.js"),
        ] {
            std::fs::write(file, b"owned fake, never execute").unwrap();
        }
        assert_eq!(
            editor_cli(&bin, EditorKind::Cursor).unwrap().0,
            root.join("Cursor.exe")
        );
        let nested = root.join("resources/app/bin");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("cursor.cmd"), b"owned fake").unwrap();
        assert_eq!(
            editor_cli(&nested, EditorKind::Cursor).unwrap().0,
            root.join("Cursor.exe")
        );
    }
}
