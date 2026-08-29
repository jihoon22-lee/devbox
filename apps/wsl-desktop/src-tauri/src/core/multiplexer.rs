//! Optional tmux/zellij adapter policy.
//!
//! This module owns only exact argv and stable-output parsing. It never installs or downloads an
//! external tool. Native WSL remains the complete default path.

use super::workspace::MultiplexerKind;
use serde::Serialize;
use std::collections::HashSet;

const SESSION_PREFIX: &str = "wsld";
const MAX_SESSION_COMPONENT: usize = 24;
const MAX_ENVIRONMENT_BYTES: usize = 16 * 1024;
const MAX_PATH_BYTES: usize = 12 * 1024;
const MAX_PATH_ENTRIES: usize = 64;
const MAX_EXECUTABLE_CANDIDATES: usize = 72;
const MAX_POSIX_PATH_BYTES: usize = 4_096;

pub const PRINTENV_EXECUTABLES: [&str; 2] = ["/usr/bin/printenv", "/bin/printenv"];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MultiplexerSource {
    Path,
    UserLocal,
    CargoBin,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableCandidate {
    pub path: String,
    pub source: MultiplexerSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistroUserEnvironment {
    home: String,
    path: String,
}

pub fn parse_user_environment(output: &[u8]) -> Result<DistroUserEnvironment, String> {
    if output.len() > MAX_ENVIRONMENT_BYTES {
        return Err("멀티플렉서 사용자 환경 응답이 너무 큽니다".into());
    }
    let value = std::str::from_utf8(output)
        .map_err(|_| "멀티플렉서 사용자 환경 응답 형식이 올바르지 않습니다")?;
    let value = value.strip_suffix('\n').unwrap_or(value);
    let mut lines = value
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let home = lines
        .next()
        .and_then(normalize_posix_dir)
        .ok_or_else(|| "멀티플렉서 사용자 홈 경로가 올바르지 않습니다".to_string())?;
    let path = lines
        .next()
        .ok_or_else(|| "멀티플렉서 사용자 PATH 응답이 올바르지 않습니다".to_string())?;
    if lines.next().is_some() || path.len() > MAX_PATH_BYTES || path.chars().any(char::is_control) {
        return Err("멀티플렉서 사용자 PATH 응답이 올바르지 않습니다".into());
    }
    Ok(DistroUserEnvironment {
        home,
        path: path.to_string(),
    })
}

pub fn executable_candidates(
    kind: MultiplexerKind,
    environment: Option<&DistroUserEnvironment>,
) -> Result<Vec<ExecutableCandidate>, String> {
    let executable = executable_name(kind)?;
    let user_local = environment.map(|value| format!("{}/.local/bin", value.home));
    let cargo_bin = environment.map(|value| format!("{}/.cargo/bin", value.home));
    let mut directories = Vec::new();

    if let Some(environment) = environment {
        for entry in environment.path.split(':').take(MAX_PATH_ENTRIES) {
            if let Some(path) = normalize_posix_dir(entry) {
                directories.push(path);
            }
        }
        if let Some(path) = &user_local {
            directories.push(path.clone());
        }
        if let Some(path) = &cargo_bin {
            directories.push(path.clone());
        }
    }
    directories.extend(
        ["/usr/local/bin", "/usr/bin", "/bin"]
            .into_iter()
            .map(str::to_string),
    );

    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    for directory in directories {
        if candidates.len() >= MAX_EXECUTABLE_CANDIDATES {
            break;
        }
        let path = if directory == "/" {
            format!("/{executable}")
        } else {
            format!("{directory}/{executable}")
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let source = if user_local.as_deref() == Some(directory.as_str()) {
            MultiplexerSource::UserLocal
        } else if cargo_bin.as_deref() == Some(directory.as_str()) {
            MultiplexerSource::CargoBin
        } else if matches!(directory.as_str(), "/usr/local/bin" | "/usr/bin" | "/bin") {
            MultiplexerSource::System
        } else {
            MultiplexerSource::Path
        };
        candidates.push(ExecutableCandidate { path, source });
    }
    Ok(candidates)
}

pub fn build_environment_probe_argv(
    distro: &str,
    printenv_executable: &str,
) -> Result<Vec<String>, String> {
    if !PRINTENV_EXECUTABLES.contains(&printenv_executable) {
        return Err("멀티플렉서 환경 조회 실행 파일이 올바르지 않습니다".into());
    }
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 환경 조회 인자가 올바르지 않습니다".to_string())?;
    argv.extend([printenv_executable.into(), "HOME".into(), "PATH".into()]);
    Ok(argv)
}

pub fn session_name(distro: &str, pane_key: &str) -> Result<String, String> {
    devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 세션의 배포판 이름이 올바르지 않습니다".to_string())?;
    if pane_key.is_empty()
        || pane_key.len() > 128
        || !pane_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("멀티플렉서 팬 식별자가 올바르지 않습니다".into());
    }
    let source = format!("{distro}-{pane_key}");
    let distro_slug = slug(distro, MAX_SESSION_COMPONENT);
    let pane_slug = slug(pane_key, MAX_SESSION_COMPONENT);
    Ok(format!(
        "{SESSION_PREFIX}-{distro_slug}-{pane_slug}-{:08x}",
        stable_hash(&source) as u32
    ))
}

pub fn build_session_argv(
    distro: &str,
    cwd: Option<&str>,
    pane_key: &str,
    multiplexer: MultiplexerKind,
    resolved_executable: Option<&str>,
) -> Result<Vec<String>, String> {
    let cwd = cwd.filter(|value| !value.trim().is_empty());
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, cwd, "")
        .map_err(|_| "WSL 터미널 실행 인자가 올바르지 않습니다".to_string())?;
    match multiplexer {
        MultiplexerKind::Native => {}
        MultiplexerKind::Tmux => {
            let name = session_name(distro, pane_key)?;
            let executable = require_resolved_executable(multiplexer, resolved_executable)?;
            argv.extend([executable, "new-session".into(), "-A".into()]);
            argv.extend(["-s".into(), name.clone()]);
            if let Some(cwd) = cwd {
                argv.extend(["-c".into(), cwd.into()]);
            }
            // These are session-scoped options, not global user configuration changes.
            argv.extend([
                ";".into(),
                "set-option".into(),
                "-t".into(),
                name.clone(),
                "status".into(),
                "off".into(),
                ";".into(),
                "set-option".into(),
                "-t".into(),
                name,
                "mouse".into(),
                "off".into(),
            ]);
        }
        MultiplexerKind::Zellij => {
            let name = session_name(distro, pane_key)?;
            let executable = require_resolved_executable(multiplexer, resolved_executable)?;
            argv.extend([
                executable,
                "attach".into(),
                "--create".into(),
                name,
                "options".into(),
                // Official built-in layout with no tab/status plugin panes.
                "--default-layout".into(),
                "disable-status".into(),
                "--pane-frames".into(),
                "false".into(),
                "--mouse-mode".into(),
                "false".into(),
            ]);
        }
    }
    Ok(argv)
}

pub fn build_version_probe_argv(
    distro: &str,
    kind: MultiplexerKind,
    resolved_executable: &str,
) -> Result<Vec<String>, String> {
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 감지 인자가 올바르지 않습니다".to_string())?;
    let executable = require_resolved_executable(kind, Some(resolved_executable))?;
    match kind {
        MultiplexerKind::Native => return Err("native 모드는 외부 감지가 필요하지 않습니다".into()),
        MultiplexerKind::Tmux => argv.extend([executable, "-V".into()]),
        MultiplexerKind::Zellij => argv.extend([executable, "--version".into()]),
    }
    Ok(argv)
}

pub fn build_session_probe_argv(
    distro: &str,
    pane_key: &str,
    kind: MultiplexerKind,
    resolved_executable: &str,
) -> Result<Vec<String>, String> {
    let name = session_name(distro, pane_key)?;
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 세션 감지 인자가 올바르지 않습니다".to_string())?;
    let executable = require_resolved_executable(kind, Some(resolved_executable))?;
    match kind {
        MultiplexerKind::Native => return Err("native 세션은 재연결하지 않습니다".into()),
        MultiplexerKind::Tmux => {
            argv.extend([executable, "has-session".into(), "-t".into(), name]);
        }
        MultiplexerKind::Zellij => {
            argv.extend([
                executable,
                "list-sessions".into(),
                "--short".into(),
                "--no-formatting".into(),
            ]);
        }
    }
    Ok(argv)
}

fn executable_name(kind: MultiplexerKind) -> Result<&'static str, String> {
    match kind {
        MultiplexerKind::Native => Err("native 모드는 외부 실행 파일이 필요하지 않습니다".into()),
        MultiplexerKind::Tmux => Ok("tmux"),
        MultiplexerKind::Zellij => Ok("zellij"),
    }
}

fn require_resolved_executable(
    kind: MultiplexerKind,
    resolved_executable: Option<&str>,
) -> Result<String, String> {
    let expected = executable_name(kind)?;
    let executable = resolved_executable
        .and_then(normalize_posix_dir)
        .filter(|path| path.rsplit('/').next() == Some(expected))
        .ok_or_else(|| "멀티플렉서 실행 파일 경로가 올바르지 않습니다".to_string())?;
    Ok(executable)
}

fn normalize_posix_dir(value: &str) -> Option<String> {
    if !value.starts_with('/')
        || value.len() > MAX_POSIX_PATH_BYTES
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let mut segments = Vec::new();
    for segment in value.split('/') {
        if segment.is_empty() {
            continue;
        }
        if matches!(segment, "." | "..") {
            return None;
        }
        segments.push(segment);
    }
    if segments.is_empty() {
        Some("/".into())
    } else {
        Some(format!("/{}", segments.join("/")))
    }
}

/// Zellij short output may append `EXITED`; only an exact active name is resumable.
pub fn zellij_session_is_running(output: &str, expected: &str) -> bool {
    output.lines().any(|line| {
        let trimmed = line.trim().trim_matches('\'');
        if trimmed.is_empty() || trimmed.contains("EXITED") {
            return false;
        }
        trimmed
            .split_ascii_whitespace()
            .next()
            .is_some_and(|name| name == expected)
    })
}

pub fn normalize_version(kind: MultiplexerKind, output: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(output).ok()?.trim();
    if value.is_empty()
        || value.len() > 120
        || value.chars().any(|character| character.is_control())
    {
        return None;
    }
    let expected_prefix = match kind {
        MultiplexerKind::Native => return None,
        MultiplexerKind::Tmux => "tmux ",
        MultiplexerKind::Zellij => "zellij ",
    };
    let version = value.strip_prefix(expected_prefix)?;
    if version.is_empty()
        || version.len() > 80
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'))
    {
        return None;
    }
    Some(value.to_string())
}

fn slug(value: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for byte in value.bytes() {
        let next = if byte.is_ascii_alphanumeric() {
            last_dash = false;
            byte.to_ascii_lowercase() as char
        } else if !last_dash && !out.is_empty() {
            last_dash = true;
            '-'
        } else {
            continue;
        };
        if out.len() >= max {
            break;
        }
        out.push(next);
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "item".into()
    } else {
        out
    }
}

fn stable_hash(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_argv_keeps_cwd_separate() {
        assert_eq!(
            build_session_argv(
                "Ubuntu",
                Some("/mnt/e/path with 'quote'"),
                "pane-1",
                MultiplexerKind::Native,
                None,
            )
            .unwrap(),
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--cd",
                "/mnt/e/path with 'quote'",
                "--",
            ]
        );
    }

    #[test]
    fn tmux_argv_is_exact_and_options_are_not_global() {
        let argv = build_session_argv(
            "Ubuntu Dev",
            Some("/mnt/e/project"),
            "pane-1",
            MultiplexerKind::Tmux,
            Some("/home/jihoon/.local/bin/tmux"),
        )
        .unwrap();
        let name = session_name("Ubuntu Dev", "pane-1").unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu Dev",
                "--cd",
                "/mnt/e/project",
                "--",
                "/home/jihoon/.local/bin/tmux",
                "new-session",
                "-A",
                "-s",
                &name,
                "-c",
                "/mnt/e/project",
                ";",
                "set-option",
                "-t",
                &name,
                "status",
                "off",
                ";",
                "set-option",
                "-t",
                &name,
                "mouse",
                "off",
            ]
        );
        assert!(!argv.iter().any(|item| item == "-g"));
        assert!(!argv.iter().any(|item| item == "sh" || item == "-lc"));
    }

    #[test]
    fn zellij_argv_uses_no_status_layout_and_disables_frames_and_mouse() {
        let argv = build_session_argv(
            "Ubuntu",
            None,
            "pane-1",
            MultiplexerKind::Zellij,
            Some("/home/jihoon/.local/bin/zellij"),
        )
        .unwrap();
        let name = session_name("Ubuntu", "pane-1").unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "/home/jihoon/.local/bin/zellij",
                "attach",
                "--create",
                &name,
                "options",
                "--default-layout",
                "disable-status",
                "--pane-frames",
                "false",
                "--mouse-mode",
                "false",
            ]
        );
        assert!(!argv
            .iter()
            .any(|item| item == "tab-bar" || item == "status-bar"));
    }

    #[test]
    fn session_name_is_stable_safe_and_bounded() {
        let first = session_name("Ubuntu / Dev", "pane_123").unwrap();
        let second = session_name("Ubuntu / Dev", "pane_123").unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("wsld-ubuntu-dev-pane-123-"));
        assert!(first.len() <= 70);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    }

    #[test]
    fn parses_active_and_exited_zellij_fixtures() {
        let expected = "wsld-ubuntu-pane-1-12345678";
        let fixture =
            format!("other-session [Created 2h ago]\n{expected} [Created 1m ago]\nold EXITED\n");
        assert!(zellij_session_is_running(&fixture, expected));
        assert!(!zellij_session_is_running(
            &format!("{expected} EXITED\n"),
            expected
        ));
        assert!(!zellij_session_is_running(
            "No active zellij sessions found.",
            expected
        ));
    }

    #[test]
    fn environment_and_version_probe_argv_never_use_a_shell() {
        assert_eq!(
            build_environment_probe_argv("Ubuntu", "/usr/bin/printenv").unwrap(),
            [
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "/usr/bin/printenv",
                "HOME",
                "PATH",
            ]
        );
        assert_eq!(
            build_version_probe_argv(
                "Ubuntu",
                MultiplexerKind::Zellij,
                "/home/jihoon/.local/bin/zellij",
            )
            .unwrap(),
            [
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "/home/jihoon/.local/bin/zellij",
                "--version",
            ]
        );
    }

    #[test]
    fn user_environment_adds_bounded_home_candidates_and_skips_unsafe_path_entries() {
        let environment = parse_user_environment(
            b"/home/jihoon\nrelative:/opt/tools:/home/jihoon/.local/bin:/tmp/../bad:/usr/bin\n",
        )
        .unwrap();
        let candidates =
            executable_candidates(MultiplexerKind::Zellij, Some(&environment)).unwrap();
        assert_eq!(
            candidates,
            vec![
                ExecutableCandidate {
                    path: "/opt/tools/zellij".into(),
                    source: MultiplexerSource::Path,
                },
                ExecutableCandidate {
                    path: "/home/jihoon/.local/bin/zellij".into(),
                    source: MultiplexerSource::UserLocal,
                },
                ExecutableCandidate {
                    path: "/usr/bin/zellij".into(),
                    source: MultiplexerSource::System,
                },
                ExecutableCandidate {
                    path: "/home/jihoon/.cargo/bin/zellij".into(),
                    source: MultiplexerSource::CargoBin,
                },
                ExecutableCandidate {
                    path: "/usr/local/bin/zellij".into(),
                    source: MultiplexerSource::System,
                },
                ExecutableCandidate {
                    path: "/bin/zellij".into(),
                    source: MultiplexerSource::System,
                },
            ]
        );
    }

    #[test]
    fn resolver_preserves_safe_unicode_and_spaces_as_one_argv_item() {
        let environment =
            parse_user_environment("/home/개발자\n/opt/개발 도구:/usr/bin\n".as_bytes()).unwrap();
        let candidates = executable_candidates(MultiplexerKind::Tmux, Some(&environment)).unwrap();
        assert_eq!(candidates[0].path, "/opt/개발 도구/tmux");
        assert_eq!(
            build_version_probe_argv("Ubuntu Dev", MultiplexerKind::Tmux, &candidates[0].path)
                .unwrap(),
            [
                "wsl.exe",
                "-d",
                "Ubuntu Dev",
                "--",
                "/opt/개발 도구/tmux",
                "-V",
            ]
        );
    }

    #[test]
    fn external_session_requires_the_expected_absolute_executable() {
        assert!(
            build_session_argv("Ubuntu", None, "pane-1", MultiplexerKind::Zellij, None,).is_err()
        );
        assert!(build_session_argv(
            "Ubuntu",
            None,
            "pane-1",
            MultiplexerKind::Zellij,
            Some("zellij"),
        )
        .is_err());
        assert!(build_session_argv(
            "Ubuntu",
            None,
            "pane-1",
            MultiplexerKind::Zellij,
            Some("/usr/bin/tmux"),
        )
        .is_err());
    }

    #[test]
    fn version_normalization_accepts_known_shapes_without_reflecting_paths_or_secrets() {
        assert_eq!(
            normalize_version(MultiplexerKind::Tmux, b"tmux 3.4\n"),
            Some("tmux 3.4".into())
        );
        assert_eq!(
            normalize_version(MultiplexerKind::Zellij, b"zellij 0.41.2\n"),
            Some("zellij 0.41.2".into())
        );
        assert_eq!(
            normalize_version(MultiplexerKind::Zellij, b"/home/user/.local/bin/zellij\n"),
            None
        );
        assert_eq!(
            normalize_version(MultiplexerKind::Tmux, b"tmux token=private\n"),
            None
        );
    }
}
