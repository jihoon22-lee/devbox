use super::*;
use devbox_protocol::system::{Check, Status};
const NEW_IDS: [&str; 8] = [
    "wsl.idle",
    "tool.rust",
    "tool.docker",
    "auth.gh",
    "auth.claude",
    "auth.codex",
    "hook.claude",
    "hook.codex",
];
struct FakeProbe {
    issue: Option<ProbeIssue>,
    ready: bool,
    config: Option<&'static str>,
    version: &'static str,
}
impl Default for FakeProbe {
    fn default() -> Self {
        Self {
            issue: None,
            ready: true,
            config: Some("[general]\ninstanceIdleTimeout=-1\n[wsl2]\nvmIdleTimeout=60000"),
            version: "1.2.3",
        }
    }
}
impl FakeProbe {
    fn result<T>(&self, value: T) -> Probed<T> {
        self.issue.map_or(Ok(value), Err)
    }
}
impl Probe for FakeProbe {
    fn pid1(&self) -> Probed<String> {
        self.result("systemd".into())
    }
    fn user_manager_available(&self) -> Probed<bool> {
        self.result(self.ready)
    }
    fn linger(&self) -> Probed<String> {
        self.result("Linger=yes".into())
    }
    fn tmux_version(&self) -> Probed<Option<String>> {
        self.result(Some("tmux 3.3".into()))
    }
    fn inotify_instances(&self) -> Probed<String> {
        self.result("1024".into())
    }
    fn env_path_valid(&self) -> Probed<bool> {
        self.result(true)
    }
    fn which(&self, _: &str, _: &str) -> Probed<bool> {
        self.result(self.ready)
    }
    fn daemon_path(&self) -> String {
        "/fixture/bin".into()
    }
    fn hook_executable(&self) -> Probed<bool> {
        self.result(true)
    }
    fn wsl_config(&self) -> Probed<Option<String>> {
        self.result(self.config.map(Into::into))
    }
    fn rust_versions(&self) -> Probed<RustVersions> {
        self.result(RustVersions {
            cargo: self.ready.then(|| format!("cargo {}", self.version)),
            rustc: self.ready.then(|| format!("rustc {}", self.version)),
        })
    }
    fn docker(&self) -> Probed<DockerProbe> {
        self.result(DockerProbe {
            version: self
                .ready
                .then(|| format!("Docker version {}", self.version)),
            responsive: self.ready,
        })
    }
    fn gh_authenticated(&self) -> Probed<bool> {
        self.result(self.ready)
    }
    fn agent_auth(&self, agent: AgentTool) -> Probed<AgentAuth> {
        self.result(AgentAuth {
            version: Some(format!("{} {}", agent.binary(), self.version)),
            file_present: self.ready,
        })
    }
    fn agent_hook(&self, _: AgentTool) -> Probed<bool> {
        self.result(self.ready)
    }
}
fn find<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|c| c.id == id)
        .expect("required check exists")
}
#[test]
fn new_doctor_checks_are_present_and_healthy() {
    let all = checks(&FakeProbe::default());
    for id in NEW_IDS {
        assert_eq!(find(&all, id).status, Status::Ok, "{id}");
    }
    for id in ["auth.claude", "auth.codex"] {
        assert!(find(&all, id).message.contains("유효성"));
    }
}
#[test]
fn new_doctor_missing_evidence_warns_without_claiming_authentication() {
    let all = checks(&FakeProbe {
        ready: false,
        config: None,
        ..Default::default()
    });
    for id in NEW_IDS {
        assert_eq!(find(&all, id).status, Status::Warn, "{id}");
        assert!(find(&all, id).fix.is_some(), "{id}");
    }
    assert_eq!(find(&all, "systemd").status, Status::Fail);
    assert_eq!(find(&all, "tool.git").status, Status::Fail);
}
#[test]
fn new_doctor_timeouts_and_unavailable_probes_are_explicit_warnings() {
    for issue in [
        ProbeIssue::Timeout,
        ProbeIssue::Unavailable,
        ProbeIssue::InvalidData,
    ] {
        let all = checks(&FakeProbe {
            issue: Some(issue),
            ..Default::default()
        });
        for id in NEW_IDS {
            let check = find(&all, id);
            assert_eq!(check.status, Status::Warn);
            assert!(
                check.message.contains("확인하지 못함"),
                "{id}: {}",
                check.message
            );
        }
    }
}
#[test]
fn wsl_idle_uses_documented_sections_and_rejects_ambiguous_values() {
    for config in [
        "[wsl2]\ninstanceIdleTimeout=-1",
        "[general]\ninstanceIdleTimeout=no",
        "[general]\ninstanceIdleTimeout=-1\ninstanceIdleTimeout=1",
        "[general]\ninstanceIdleTimeout=-2",
        "[general]\ninstanceIdleTimeout=15000\n[wsl2]\nvmIdleTimeout=-1",
    ] {
        let all = checks(&FakeProbe {
            config: Some(config),
            ..Default::default()
        });
        assert_eq!(find(&all, "wsl.idle").status, Status::Warn);
    }
}
#[test]
fn doctor_never_reflects_raw_version_or_configuration_diagnostics() {
    let all = checks(&FakeProbe {
        version: "1.2.3 fixture-private-token /home/fixture-private",
        config: Some("[general]\ninstanceIdleTimeout=fixture-private-token"),
        ..Default::default()
    });
    for check in all {
        assert!(!check.message.contains("fixture-private"));
        assert!(!check.fix.unwrap_or_default().contains("fixture-private"));
    }
}

#[test]
fn existing_doctor_probes_also_report_timeouts_as_unknown_not_failures() {
    let all = checks(&FakeProbe {
        issue: Some(ProbeIssue::Timeout),
        ..Default::default()
    });
    for check in all {
        assert_eq!(check.status, Status::Warn, "{}", check.id);
        assert!(check.message.contains("확인하지 못함"), "{}", check.id);
    }
}

#[test]
fn unsupported_gh_capability_is_unknown_with_a_read_only_upgrade_hint() {
    let all = checks(&FakeProbe {
        issue: Some(ProbeIssue::UnsupportedCapability),
        ..Default::default()
    });
    let auth = find(&all, "auth.gh");
    assert_eq!(auth.status, Status::Warn);
    assert!(auth.message.contains("확인하지 못함"));
    assert!(
        auth.fix
            .as_deref()
            .unwrap_or_default()
            .contains("--no-refresh")
    );
}
