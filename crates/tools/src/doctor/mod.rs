mod hooks;
pub use hooks::hook_present;
mod checks;
mod extended;
mod probe;
pub mod wsl;

pub use checks::{checks, tmux_at_least};
pub use probe::{AgentAuth, AgentTool, DockerProbe, Probe, ProbeIssue, Probed, RustVersions};

#[cfg(test)]
mod checks_tests;

#[cfg(test)]
mod extension_tests;
