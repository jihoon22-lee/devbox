use super::*;
use devbox_protocol::system::Status;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Fake {
    available: bool,
    env_path_valid: bool,
    agents: Vec<(AgentTool, AgentCommand)>,
    /// Questions that would start an interactive shell or an agent CLI on a real system.
    agent_lookups: AtomicUsize,
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
    fn agent_auth(&self, _agent: AgentTool) -> Probed<AgentAuth> {
        self.agent_lookups.fetch_add(1, Ordering::SeqCst);
        Err(ProbeIssue::Unavailable)
    }
    fn agent_command(&self, agent: AgentTool, path: &str) -> Probed<Option<AgentCommand>> {
        assert_eq!(path, "/daemon/bin");
        self.agent_lookups.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .agents
            .iter()
            .find(|(tool, _)| *tool == agent)
            .map(|(_, command)| command.clone()))
    }
}
fn found(path: &str, interactive: bool, version: &str, login_path: Option<&str>) -> AgentCommand {
    AgentCommand {
        path: path.into(),
        interactive,
        version: Some(version.into()),
        login_path: login_path.map(Into::into),
    }
}
fn check<'a>(
    checks: &'a [devbox_protocol::system::Check],
    id: &str,
) -> &'a devbox_protocol::system::Check {
    checks
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
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
            ..Fake::default()
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
        ..Fake::default()
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
        ..Fake::default()
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

#[test]
fn reports_all_four_agent_tools_with_versions() {
    let checks = checks(&Fake {
        available: true,
        env_path_valid: true,
        agents: vec![
            (
                AgentTool::Claude,
                found("/home/u/.local/bin/claude", true, "2.1.308", None),
            ),
            (
                AgentTool::Codex,
                found(
                    "/home/u/.nvm/bin/codex",
                    true,
                    "0.164.0",
                    Some("/usr/local/bin/codex"),
                ),
            ),
            (
                AgentTool::Opencode,
                found("/usr/local/bin/opencode", false, "1.19.3", None),
            ),
            (
                AgentTool::Pi,
                found("/home/u/.npm/bin/pi", true, "1.3.0", None),
            ),
        ],
        ..Fake::default()
    });
    for (id, version, path, source) in [
        (
            "tool.claude",
            "2.1.308",
            "/home/u/.local/bin/claude",
            "대화형 셸",
        ),
        (
            "tool.codex",
            "0.164.0",
            "/home/u/.nvm/bin/codex",
            "대화형 셸",
        ),
        (
            "tool.opencode",
            "1.19.3",
            "/usr/local/bin/opencode",
            "로그인 셸",
        ),
        ("tool.pi", "1.3.0", "/home/u/.npm/bin/pi", "대화형 셸"),
    ] {
        let found = check(&checks, id);
        assert_eq!(found.status, Status::Ok, "{id}");
        for part in [version, path, source] {
            assert!(found.message.contains(part), "{id}: {}", found.message);
        }
        assert!(found.fix.is_none(), "{id}");
    }
    // The login and interactive shells disagree: the task uses the interactive one.
    assert_eq!(
        check(&checks, "tool.codex").message,
        "로그인 셸과 대화형 셸이 다른 codex 실행 파일을 가리킵니다. devbox는 대화형 셸의 /home/u/.nvm/bin/codex(0.164.0)를 씁니다."
    );
    let order: Vec<_> = checks.iter().map(|c| c.id.as_str()).collect();
    let at = |id: &str| order.iter().position(|c| *c == id).expect("listed");
    assert!(at("tool.git") < at("tool.claude"));
    assert!(at("tool.claude") < at("tool.codex"));
    assert!(at("tool.codex") < at("tool.opencode"));
    assert!(at("tool.opencode") < at("tool.pi"));
    assert!(at("tool.pi") < at("tool.gh"));
}

#[test]
fn low_opencode_or_pi_version_warns() {
    for (agent, version, warns) in [
        (AgentTool::Opencode, "1.19.2", true),
        (AgentTool::Opencode, "1.19.3", false),
        (AgentTool::Opencode, "1.20.0", false),
        (AgentTool::Pi, "1.2.9", true),
        (AgentTool::Pi, "1.3.0", false),
        (AgentTool::Pi, "1.10.0", false),
        (AgentTool::Claude, "2.1.299", true),
        (AgentTool::Claude, "2.1.300", false),
        (AgentTool::Codex, "0.1.0", false),
    ] {
        let checks = checks(&Fake {
            available: true,
            env_path_valid: true,
            agents: vec![(agent, found("/owned/bin/tool", true, version, None))],
            ..Fake::default()
        });
        let tool = check(&checks, &format!("tool.{}", agent.binary()));
        if warns {
            assert_eq!(tool.status, Status::Warn, "{agent:?} {version}");
            assert!(
                tool.message.contains("devbox가 확인한 가장 낮은 버전"),
                "{}",
                tool.message
            );
            let minimum = agent.verified_minimum().expect("minimum");
            assert!(tool.message.contains(minimum) && tool.message.contains(version));
            assert!(tool.fix.as_deref().is_some_and(|fix| fix.contains(minimum)));
        } else {
            assert_eq!(
                tool.status,
                Status::Ok,
                "{agent:?} {version}: {}",
                tool.message
            );
        }
    }
}

#[test]
fn the_startup_check_leaves_out_only_the_rows_that_look_up_an_agent_cli() {
    // The app runs this after every setup and acts only on failed rows; agent rows never fail.
    let probe = Fake {
        available: true,
        ..Fake::default()
    };
    let row = |c: &devbox_protocol::system::Check| {
        (c.id.clone(), c.status, c.message.clone(), c.fix.clone())
    };
    let full: Vec<_> = checks(&probe).iter().map(row).collect();
    let asked = probe.agent_lookups.load(Ordering::SeqCst);
    assert!(asked > 0);
    let startup: Vec<_> = checks_for(&probe, Scope::Startup).iter().map(row).collect();
    assert_eq!(probe.agent_lookups.load(Ordering::SeqCst), asked);
    let agent_rows = [
        "tool.claude",
        "tool.codex",
        "tool.opencode",
        "tool.pi",
        "auth.claude",
        "auth.codex",
    ];
    for id in agent_rows {
        assert!(
            full.iter().any(|(full, ..)| full == id),
            "the full check has {id}"
        );
    }
    let expected: Vec<_> = full
        .iter()
        .filter(|(id, ..)| !agent_rows.contains(&id.as_str()))
        .cloned()
        .collect();
    assert_eq!(startup, expected);
    // The failed rows that stop a launch are all still there.
    for id in ["systemd", "tool.tmux", "env", "tool.git", "hook.binary"] {
        assert!(startup.iter().any(|(startup, ..)| startup == id), "{id}");
    }
    assert!(
        startup
            .iter()
            .any(|(_, status, ..)| *status == Status::Fail)
    );
}
