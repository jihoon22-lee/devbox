use crate::{
    model::*,
    service::{AgentsService, Result, now_ms},
};
use devbox_protocol::{Fail, Internal};
use std::{path::Path, time::Duration};
impl AgentsService {
    pub(crate) async fn quiesce(&self, task: &AgentTask) -> Result<()> {
        if let Some(session) = &task.terminal_id {
            self.terminal.close_internal(session).await.map_err(|e| {
                devbox_protocol::Fail::Internal(format!("close owned agent terminal: {e:?}"))
            })?;
        }
        if self.use_scope {
            let unit = format!(
                "devbox-{}-agent-{}.scope",
                self.paths.instance.as_str(),
                task.id
            );
            if let Err(error) = bounded_command("systemctl", &["--user", "stop", &unit], None).await
            {
                // Completed scopes are unloaded by systemd. Verify this exact owned unit;
                // permission, bus and timeout failures must still block cleanup.
                if !scope_absent(&unit).await.unwrap_or(false) {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn teardown_commands(
        &self,
        task: &AgentTask,
        project: &devbox_projects::Project,
    ) -> Result<()> {
        let config =
            devbox_projects::config::load(Path::new(&project.path)).internal("teardown config")?;
        for command in config.agent.teardown {
            bounded_command("bash", &["-lc", &command], Some(Path::new(&task.workdir))).await?;
        }
        Ok(())
    }
    pub(crate) async fn record_cleanup_error(&self, id: &str, error: &Fail<AgentsError>) {
        tracing::error!(?error, agent = id, "closed task cleanup retained resources");
        // Domain errors contain safe details; process causes remain in diagnostics/logs.
        let message = match error {
            Fail::Domain(error) => {
                serde_json::to_string(error).unwrap_or_else(|_| "agents.cleanup_failed".into())
            }
            Fail::Internal(_) => {
                "정리 중 오류가 발생했습니다. 작업 폴더와 브랜치를 확인해 주세요".into()
            }
        };
        let _ = self
            .update(id, move |t| {
                t.cleanup_error = Some(message);
                t.updated_ms = now_ms();
                true
            })
            .await;
    }
    pub(crate) async fn cleanup_closed(&self, id: &str) {
        if let Err(error) = self.cleanup_closed_inner(id).await {
            self.record_cleanup_error(id, &error).await;
        }
    }
    async fn cleanup_closed_inner(&self, id: &str) -> Result<()> {
        let task = self.get(id).await?;
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        // Stop only the task's own resources, then inspect again before any removal.
        self.quiesce(&task).await?;
        self.teardown_commands(&task, &project).await?;
        self.owned_worktree(&task).await?;
        if task.closed_head_oid.as_deref()
            != Some(
                devbox_git::run::resolve(&wt, "HEAD")
                    .await
                    .map_err(git_failure)?
                    .as_str(),
            )
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        if devbox_git::current_branch(&wt)
            .await
            .map_err(git_failure)?
            .as_deref()
            != Some(&branch)
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        // Automatic cleanup never removes dirty files, even for a successful merge/PR.
        devbox_git::mutate::worktree_remove(Path::new(&project.path), &wt, false)
            .await
            .map_err(git_failure)?;
        Ok(())
    }
}
async fn scope_absent(unit: &str) -> Result<bool> {
    let mut command = tokio::process::Command::new("systemctl");
    command
        .args(["--user", "show", "--property=LoadState", "--value", unit])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn().internal("inspect owned agent scope")?;
    #[cfg(unix)]
    let _group = child.id().map(OwnedGroup);
    let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .internal("inspect owned scope deadline")?
        .internal("inspect owned scope output")?;
    // systemctl show can return 1 for a nonexistent unit while exposing LoadState.
    Ok(matches!(output.status.code(), Some(0 | 1)) && output.stdout == b"not-found\n")
}

/// Commands run only for an explicitly owned task. Timeout kills and reaps our process group.
async fn bounded_command(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    bounded_command_for(program, args, cwd, Duration::from_secs(60)).await
}
#[cfg(unix)]
struct OwnedGroup(u32);
#[cfg(unix)]
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(self.0 as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}
async fn bounded_command_for(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<()> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().internal("spawn agent cleanup")?;
    #[cfg(unix)]
    let _group = child.id().map(OwnedGroup);
    let result = tokio::time::timeout(timeout, child.wait()).await;
    match result {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(Fail::Internal(format!("agent cleanup exit: {status}"))),
        Ok(Err(error)) => Err(Fail::Internal(format!("agent cleanup wait: {error}"))),
        Err(_) => {
            #[cfg(unix)]
            if let Some(id) = child.id()
                && let Err(error) = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(id as i32),
                    nix::sys::signal::Signal::SIGKILL,
                )
                && error != nix::errno::Errno::ESRCH
            {
                tracing::error!(?error, "kill timed out cleanup group failed");
            }
            child
                .kill()
                .await
                .internal("kill timed out cleanup child")?;
            child
                .wait()
                .await
                .internal("reap timed out cleanup child")?;
            Err(Fail::Internal("agent cleanup timed out".into()))
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn teardown_timeout_kills_owned_descendants_and_reaps_the_shell() {
        let root = tempfile::tempdir().expect("owned fixture");
        let result = bounded_command_for(
            "bash",
            &["-c", "(sleep .2; printf leaked > sentinel) & wait"],
            Some(root.path()),
            Duration::from_millis(30),
        )
        .await;
        assert!(matches!(result, Err(Fail::Internal(_))));
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(!root.path().join("sentinel").exists());
    }
}
