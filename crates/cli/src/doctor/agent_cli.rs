//! The environment check's look at the four agent CLIs.
use super::{OsProbe, runner};
use devbox_agents::command_path::{self, CommandSource, InteractiveShell, ShellProbe};
use devbox_tools::doctor::{AgentCommand, AgentTool, ProbeIssue, Probed};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
};

impl OsProbe {
    /// One agent CLI from this check's look, which is made once, on first use. One check uses
    /// one `PATH`.
    pub(super) fn agent_cli(&self, agent: AgentTool, path: &str) -> Probed<Option<AgentCommand>> {
        let index = AgentTool::ALL
            .iter()
            .position(|tool| *tool == agent)
            .ok_or(ProbeIssue::Unavailable)?;
        match self.agents.get_or_init(|| look_up(path, &InteractiveShell)) {
            Ok(all) => Ok(all.get(index).cloned().flatten()),
            Err(issue) => Err(*issue),
        }
    }
}

/// Looks the four agent CLIs up afresh and asks each one found for its version. The fresh
/// answers replace the cached ones, so the next task start uses a CLI installed since.
pub(super) fn look_up(path: &str, shell: &impl ShellProbe) -> Probed<Vec<Option<AgentCommand>>> {
    let path = OsString::from(path);
    runner::block_on(async {
        Ok(futures::future::join_all(AgentTool::ALL.map(|tool| one(tool, &path, shell))).await)
    })
}

async fn one(tool: AgentTool, path: &OsStr, shell: &impl ShellProbe) -> Option<AgentCommand> {
    let found = command_path::refresh(tool.binary(), path, shell).await?;
    let version = command_path::version(&found.program, path).await;
    let login = command_path::search_path(tool.binary(), path)
        .filter(|login| !same_file(login, &found.program));
    Some(AgentCommand {
        path: found.program.display().to_string(),
        interactive: found.source == CommandSource::InteractiveShell,
        version,
        login_path: login.map(|login| login.display().to_string()),
    })
}

/// Two names for one installed CLI (a symlink, say) are not two different CLIs.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
#[path = "agent_cli_tests.rs"]
mod tests;
