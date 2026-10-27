//! The environment check's look at the four agent CLIs.
use super::{OsProbe, runner};
use devbox_agents::command_path::{self, CommandSource, InteractiveShell, LookupCache, ShellProbe};
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
        // The shared cache is the one the daemon's task starts read.
        let look = || look_up(path, &InteractiveShell, &LookupCache::shared());
        match self.agents.get_or_init(look) {
            Ok(all) => Ok(all.get(index).cloned().flatten()),
            Err(issue) => Err(*issue),
        }
    }
}

/// Looks the four agent CLIs up afresh and asks each one found for its version. The shell's fresh
/// answers replace the ones in `cache`, so the next task start uses a CLI installed since; a
/// lookup the shell did not answer leaves the cached one as it was.
pub(super) fn look_up(
    path: &str,
    shell: &impl ShellProbe,
    cache: &LookupCache,
) -> Probed<Vec<Option<AgentCommand>>> {
    let path = OsString::from(path);
    runner::block_on(async {
        let all = AgentTool::ALL.map(|tool| one(tool, &path, shell, cache));
        Ok(futures::future::join_all(all).await)
    })
}

async fn one(
    tool: AgentTool,
    path: &OsStr,
    shell: &impl ShellProbe,
    cache: &LookupCache,
) -> Option<AgentCommand> {
    let found = cache.refresh(tool.binary(), path, shell).await?;
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
