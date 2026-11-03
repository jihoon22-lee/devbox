mod agent_tools;
mod checks;
mod extended;
mod probe;
pub mod wsl;

pub use checks::{Scope, checks, checks_for, tmux_at_least};
pub use probe::{
    AgentAuth, AgentCommand, AgentTool, DockerProbe, Probe, ProbeIssue, Probed, RustVersions,
};

#[cfg(test)]
mod checks_tests;

#[cfg(test)]
mod extension_tests;
