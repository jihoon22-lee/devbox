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
    /// The CLI an agent task would run, found the way task starts find it, with `path` as the
    /// service `PATH`. `Ok(None)` when neither shell finds it.
    fn agent_command(&self, _agent: AgentTool, _path: &str) -> Probed<Option<AgentCommand>> {
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
    Opencode,
    Pi,
}
impl AgentTool {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Opencode, Self::Pi];
    pub fn binary(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
        }
    }
    /// The lowest version devbox has verified its connection with, where one matters.
    pub fn verified_minimum(self) -> Option<&'static str> {
        match self {
            Self::Claude => Some("2.1.300"),
            Self::Codex => None,
            Self::Opencode => Some("1.19.3"),
            Self::Pi => Some("1.3.0"),
        }
    }
}
/// Where an agent CLI was found: paths and the version token only, never other output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommand {
    /// The executable devbox runs.
    pub path: String,
    /// Found by the interactive shell rather than the login shell's (service) `PATH`.
    pub interactive: bool,
    pub version: Option<String>,
    /// The login shell's executable, when it is another file than `path`.
    pub login_path: Option<String>,
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
