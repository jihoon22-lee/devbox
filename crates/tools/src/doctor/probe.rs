pub trait Probe {
    fn pid1(&self) -> Probed<String>;
    fn user_manager_available(&self) -> Probed<bool>;
    fn linger(&self) -> Probed<String>;
    fn tmux_version(&self) -> Probed<Option<String>>;
    fn inotify_instances(&self) -> Probed<String>;
    fn env_path_valid(&self) -> Probed<bool>;
    fn which(&self, tool: &str, path: &str) -> Probed<bool>;
    fn daemon_path(&self) -> String;
    fn hook_executable(&self) -> Probed<bool>;
    fn wsl_config(&self) -> Probed<Option<String>> {
        Err(ProbeIssue::Unavailable)
    }
    fn rust_versions(&self) -> Probed<RustVersions> {
        Err(ProbeIssue::Unavailable)
    }
    fn docker(&self) -> Probed<DockerProbe> {
        Err(ProbeIssue::Unavailable)
    }
    fn gh_authenticated(&self) -> Probed<bool> {
        Err(ProbeIssue::Unavailable)
    }
    fn agent_auth(&self, _agent: AgentTool) -> Probed<AgentAuth> {
        Err(ProbeIssue::Unavailable)
    }
    fn agent_hook(&self, _agent: AgentTool) -> Probed<bool> {
        Err(ProbeIssue::Unavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeIssue {
    Missing,
    Timeout,
    Unavailable,
    InvalidData,
    UnsupportedCapability,
}
pub type Probed<T> = Result<T, ProbeIssue>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTool {
    Claude,
    Codex,
}
impl AgentTool {
    pub fn binary(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}
#[derive(Debug, Clone)]
pub struct RustVersions {
    pub cargo: Option<String>,
    pub rustc: Option<String>,
}
#[derive(Debug, Clone)]
pub struct DockerProbe {
    pub version: Option<String>,
    pub responsive: bool,
}
#[derive(Debug, Clone)]
pub struct AgentAuth {
    pub version: Option<String>,
    pub file_present: bool,
}
