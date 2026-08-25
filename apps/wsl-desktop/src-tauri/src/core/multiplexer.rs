//! Optional tmux/zellij adapter policy.
//!
//! This module owns only exact argv and stable-output parsing. It never installs or downloads an
//! external tool. Native WSL remains the complete default path.

use super::workspace::MultiplexerKind;

const SESSION_PREFIX: &str = "wsld";
const MAX_SESSION_COMPONENT: usize = 24;

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
) -> Result<Vec<String>, String> {
    let cwd = cwd.filter(|value| !value.trim().is_empty());
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, cwd, "")
        .map_err(|_| "WSL 터미널 실행 인자가 올바르지 않습니다".to_string())?;
    match multiplexer {
        MultiplexerKind::Native => {}
        MultiplexerKind::Tmux => {
            let name = session_name(distro, pane_key)?;
            argv.extend(["tmux".into(), "new-session".into(), "-A".into()]);
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
            argv.extend([
                "zellij".into(),
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

pub fn build_probe_argv(distro: &str, kind: MultiplexerKind) -> Result<Vec<String>, String> {
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 감지 인자가 올바르지 않습니다".to_string())?;
    match kind {
        MultiplexerKind::Native => return Err("native 모드는 외부 감지가 필요하지 않습니다".into()),
        MultiplexerKind::Tmux => argv.extend(["tmux".into(), "-V".into()]),
        MultiplexerKind::Zellij => argv.extend(["zellij".into(), "--version".into()]),
    }
    Ok(argv)
}

pub fn build_session_probe_argv(
    distro: &str,
    pane_key: &str,
    kind: MultiplexerKind,
) -> Result<Vec<String>, String> {
    let name = session_name(distro, pane_key)?;
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, None, "")
        .map_err(|_| "멀티플렉서 세션 감지 인자가 올바르지 않습니다".to_string())?;
    match kind {
        MultiplexerKind::Native => return Err("native 세션은 재연결하지 않습니다".into()),
        MultiplexerKind::Tmux => {
            argv.extend(["tmux".into(), "has-session".into(), "-t".into(), name]);
        }
        MultiplexerKind::Zellij => {
            argv.extend([
                "zellij".into(),
                "list-sessions".into(),
                "--short".into(),
                "--no-formatting".into(),
            ]);
        }
    }
    Ok(argv)
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

pub fn normalize_version(output: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(output).ok()?.trim();
    if value.is_empty()
        || value.len() > 120
        || value.chars().any(|character| character.is_control())
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
                "tmux",
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
        let argv = build_session_argv("Ubuntu", None, "pane-1", MultiplexerKind::Zellij).unwrap();
        let name = session_name("Ubuntu", "pane-1").unwrap();
        assert_eq!(
            argv,
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "zellij",
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
    fn probe_argv_never_uses_a_shell() {
        assert_eq!(
            build_probe_argv("Ubuntu", MultiplexerKind::Tmux).unwrap(),
            ["wsl.exe", "-d", "Ubuntu", "--", "tmux", "-V"]
        );
        assert_eq!(
            build_probe_argv("Ubuntu", MultiplexerKind::Zellij).unwrap(),
            ["wsl.exe", "-d", "Ubuntu", "--", "zellij", "--version"]
        );
    }
}
