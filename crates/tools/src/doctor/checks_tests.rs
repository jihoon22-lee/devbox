use super::*;
use devbox_protocol::system::Status;

struct Fake {
    available: bool,
    env_path_valid: bool,
}
impl Probe for Fake {
    fn daemon_path(&self) -> String {
        "/daemon/bin".into()
    }
    fn hook_executable(&self) -> Probed<bool> {
        Ok(false)
    }
    fn pid1(&self) -> Probed<String> {
        Ok("systemd".into())
    }
    fn user_manager_available(&self) -> Probed<bool> {
        Ok(self.available)
    }
    fn linger(&self) -> Probed<String> {
        Ok("Linger=no".into())
    }
    fn tmux_version(&self) -> Probed<Option<String>> {
        Ok(Some("tmux 3.3a".into()))
    }
    fn inotify_instances(&self) -> Probed<String> {
        Ok("1024".into())
    }
    fn env_path_valid(&self) -> Probed<bool> {
        Ok(self.env_path_valid)
    }
    fn which(&self, tool: &str, path: &str) -> Probed<bool> {
        assert_eq!(path, "/daemon/bin");
        Ok(!matches!(tool, "claude" | "codex" | "gh"))
    }
}

#[test]
fn versions_are_validated() {
    for v in ["tmux 3.2", "tmux 3.3a", "tmux next-3.6"] {
        assert!(tmux_at_least(v, (3, 2)));
    }
    for v in ["tmux 3", "garbage 9.9", "tmux 2.9", "tmux 3.x"] {
        assert!(!tmux_at_least(v, (3, 2)));
    }
}

#[test]
fn unavailable_manager_fails_and_responsive_manager_allows_degraded_units() {
    for available in [true, false] {
        let checks = checks(&Fake {
            available,
            env_path_valid: true,
        });
        assert_eq!(
            checks[0].status,
            if available { Status::Ok } else { Status::Fail }
        );
        assert_eq!(
            checks
                .iter()
                .find(|c| c.id == "linger")
                .expect("expected check exists")
                .status,
            Status::Warn
        );
    }
}

#[test]
fn saved_environment_diagnostic_is_independent_of_current_tool_checks() {
    let checks = checks(&Fake {
        available: true,
        env_path_valid: false,
    });
    assert_eq!(
        checks
            .iter()
            .find(|c| c.id == "env")
            .expect("expected check exists")
            .status,
        Status::Fail
    );
    assert!(
        checks
            .iter()
            .any(|c| c.id == "tool.gh" && c.status == Status::Warn)
    );
}

#[test]
fn agent_checks_use_daemon_path_even_without_saved_environment() {
    let checks = checks(&Fake {
        available: true,
        env_path_valid: false,
    });
    for id in [
        "tool.claude",
        "tool.codex",
        "tool.gh",
        "tool.tmux",
        "hook.binary",
    ] {
        assert!(checks.iter().any(|check| check.id == id), "missing {id}");
    }
    for id in ["tool.claude", "tool.codex", "tool.gh"] {
        assert_eq!(
            checks
                .iter()
                .find(|c| c.id == id)
                .expect("owned fixture")
                .status,
            Status::Warn
        );
    }
    assert_eq!(
        checks
            .iter()
            .find(|c| c.id == "tool.tmux")
            .expect("owned fixture")
            .status,
        Status::Ok
    );
    assert_eq!(
        checks
            .iter()
            .find(|c| c.id == "hook.binary")
            .expect("owned fixture")
            .status,
        Status::Fail
    );
}
