use crate::{
    model::*,
    service::{AgentsService, Result, changed},
    state::AgentState,
    store,
};
use devbox_protocol::{Fail, Internal};
use futures::{StreamExt, stream};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
/// The token refreshes that run, by task, each with whether a turn ended since its read began.
/// A task has one refresh at a time; a turn that ends during it has it read once more.
#[derive(Default)]
pub(crate) struct TokenRefreshes(Mutex<HashMap<String, bool>>);
impl TokenRefreshes {
    fn tasks(&self) -> MutexGuard<'_, HashMap<String, bool>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// Whether a refresh of `task` starts now. One that runs is asked to read once more instead.
    fn start(&self, task: &str) -> bool {
        let mut tasks = self.tasks();
        if let Some(again) = tasks.get_mut(task) {
            *again = true;
            return false;
        }
        tasks.insert(task.to_owned(), false);
        true
    }
    /// Whether the refresh of `task` reads once more: a turn ended during its read. When it does
    /// not, it has ended, and the next turn's end starts another.
    fn again(&self, task: &str) -> bool {
        let mut tasks = self.tasks();
        if tasks.get_mut(task).is_some_and(std::mem::take) {
            return true;
        }
        tasks.remove(task);
        false
    }
    fn end(&self, task: &str) {
        self.tasks().remove(task);
    }
}
/// The refresh of `task` while it runs. Dropped before the refresh ended itself (`again`), on
/// shutdown or a panic, it ends the refresh, so the task's next turn end starts another.
struct Running<'a> {
    refreshes: &'a TokenRefreshes,
    task: &'a str,
    ended: bool,
}
impl Running<'_> {
    /// Whether the refresh reads once more; when it does not, it has ended
    /// (see `TokenRefreshes::again`).
    fn again(&mut self) -> bool {
        let again = self.refreshes.again(self.task);
        self.ended = !again;
        again
    }
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.refreshes.end(self.task);
        }
    }
}
impl AgentsService {
    pub async fn usage(&self, id: &str) -> Result<AgentUsage> {
        self.read_usage(id, false).await
    }
    /// A turn of task `id` ended and may have added tokens: its stored total, which the list
    /// shows, is read again in the background as `agents.usage` reads it.
    pub(crate) fn refresh_tokens(self: &Arc<Self>, id: &str) {
        if self.stop.is_cancelled() || !self.token_refreshes.start(id) {
            return;
        }
        let (service, id) = (self.clone(), id.to_owned());
        self.background.spawn(async move {
            let mut running = Running {
                refreshes: &service.token_refreshes,
                task: &id,
                ended: false,
            };
            loop {
                tokio::select! {
                    _ = service.stop.cancelled() => return,
                    () = service.store_tokens(&id) => {}
                }
                if !running.again() {
                    return;
                }
            }
        });
    }
    /// Reads the usage of open task `id` and stores its total. A read without one leaves the
    /// stored total, and a closed task keeps the total read when it closed.
    async fn store_tokens(&self, id: &str) {
        let usage = match self.read_usage(id, false).await {
            Ok(usage) => usage,
            Err(error) => {
                tracing::debug!(?error, agent = id, "agent token total not read");
                return;
            }
        };
        let Some(tokens) = usage.tokens else {
            return;
        };
        let truncated = usage.truncated;
        let (key, hub, stop) = (id.to_owned(), self.hub.clone(), self.stop.clone());
        let stored = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    // Nothing is written once the daemon shuts down.
                    if stop.is_cancelled() {
                        return Ok(Err(()));
                    }
                    let Some(mut task) = store::get(tx, &key)? else {
                        return Ok(Err(()));
                    };
                    if task.state == AgentState::Closed
                        || (task.tokens.as_ref() == Some(&tokens)
                            && task.tokens_truncated == truncated)
                    {
                        return Ok(Err(()));
                    }
                    // Only the total: the state, its attention, the activity epoch and the update
                    // time, which orders the list, stay.
                    task.tokens = Some(tokens);
                    task.tokens_truncated = truncated;
                    store::put(tx, &task)?;
                    Ok(Ok(task))
                },
                move |task, rev| changed(&hub, task, rev, false),
            )
            .await;
        if let Err(error) = stored {
            tracing::warn!(?error, agent = id, "agent token total not stored");
        }
    }
    /// `closing` reads for a task about to close, which keeps that usage for good.
    async fn read_usage(&self, id: &str, closing: bool) -> Result<AgentUsage> {
        let key = id.to_owned();
        let (task, binding) = self
            .db
            .read(move |tx| Ok((store::get(tx, &key)?, store::binding(tx, &key)?)))
            .await
            .internal("read agent usage snapshot")?;
        let task = task.ok_or_else(|| AgentsError::NotFound { id: id.into() })?;
        if task.state == AgentState::Closed {
            return Ok(AgentUsage {
                tokens: task.tokens,
                truncated: task.tokens_truncated,
            });
        }
        let Some(profile) = crate::profile::resolve(&self.profiles, &task.profile) else {
            tracing::debug!(agent = %task.id, profile = %task.profile, "agent usage source unavailable");
            return Ok(AgentUsage {
                tokens: None,
                truncated: true,
            });
        };
        let hooks = profile.hooks;
        if hooks == crate::profile::Hooks::Opencode {
            return Ok(self.opencode_usage(&task, &profile, closing).await);
        }
        let home = self.paths.home.clone();
        if hooks == crate::profile::Hooks::Pi {
            // Assumed, not checked: the task's panes start with this daemon's `PI_*` variables.
            // The tmux server is its own unit and no `PI_*` variable is allowlisted.
            let agent_dir = std::env::var_os("PI_CODING_AGENT_DIR");
            let session_dir = std::env::var_os("PI_CODING_AGENT_SESSION_DIR");
            return tokio::task::spawn_blocking(move || {
                let (agent_dir, session_dir) = (agent_dir.as_deref(), session_dir.as_deref());
                crate::usage_plugins::pi_usage(&home, agent_dir, session_dir, &task)
            })
            .await
            .internal("read bounded pi tokens");
        }
        tokio::task::spawn_blocking(move || {
            crate::usage::tokens_for(&home, &task, hooks, binding.as_ref())
        })
        .await
        .internal("read bounded agent tokens")
    }
    // Optional telemetry must not undo a completed Git operation's logical close.
    pub(crate) async fn closure_usage(&self, id: &str) -> AgentUsage {
        match self.read_usage(id, true).await {
            Ok(usage) => usage,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    agent = id,
                    "agent usage snapshot unavailable at closure"
                );
                AgentUsage {
                    tokens: None,
                    truncated: true,
                }
            }
        }
    }
    pub async fn git_stats(&self) -> Result<AgentGitStats> {
        let tasks = self
            .all_tasks(Some(0))
            .await?
            .into_iter()
            .filter(|t| t.state != AgentState::Closed);
        let results = stream::iter(tasks.map(|task| async move {
            let id = task.id;
            let result = async {
                let changes = self.changes(&id).await?;
                let changed_files = u32::try_from(changes.files.len())
                    .map_err(|_| Fail::Internal("changed file count overflow".into()))?;
                Ok(AgentGitStat {
                    id: id.clone(),
                    changed_files,
                    ahead: changes.ahead,
                    behind: changes.behind,
                })
            }
            .await;
            (id, result)
        }))
        .buffered(4)
        .collect::<Vec<(String, Result<AgentGitStat>)>>()
        .await;
        let mut stats = AgentGitStats {
            items: vec![],
            failures: vec![],
        };
        for (id, result) in results {
            match result {
                Ok(item) => stats.items.push(item),
                Err(error) => {
                    tracing::warn!(?error, agent = %id, "agent Git stats unavailable");
                    stats.failures.push(AgentGitFailure {
                        id,
                        code: "agents.git_stats_unavailable".into(),
                    });
                }
            }
        }
        Ok(stats)
    }
    pub async fn resources(&self) -> Result<AgentResources> {
        let mut sampler = self.resource_samples.lock().await;
        if !self.use_scope {
            sampler.retain(&[]);
            return Ok(AgentResources { items: vec![] });
        }
        let sessions = tokio::select! {
            _ = self.stop.cancelled() => return Ok(AgentResources { items: vec![] }),
            sessions = self.terminal.tmux().list() => sessions.internal("list live agent resource sessions")?,
        };
        let tasks = self
            .db
            .read(|tx| {
                let tasks = store::list(tx, i64::MAX)?;
                tasks
                    .into_iter()
                    .map(|task| {
                        let binding = store::binding(tx, &task.id)?;
                        Ok((task, binding))
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .await
            .internal("read owned agent resource scopes")?;
        let mut items = Vec::new();
        let mut scopes = Vec::new();
        for (task, binding) in tasks {
            let Some(binding) = binding else { continue };
            if task.tool_exited
                || !matches!(
                    task.state,
                    AgentState::Running
                        | AgentState::Waiting
                        | AgentState::Idle
                        | AgentState::Review
                )
                || ((task.worktree_path.is_some() || task.branch.is_some())
                    && binding.resources != crate::tracker::Resources::Owned)
                || !task
                    .terminal_id
                    .as_ref()
                    .is_some_and(|id| sessions.iter().any(|s| &s.name == id))
            {
                continue;
            }
            let scope = format!(
                "devbox-{}-agent-{}.scope",
                self.paths.instance.as_str(),
                task.id
            );
            let (cpu_percent, memory_bytes) = sampler
                .sample(&task.id, &scope, binding.launch_ms, &self.stop)
                .await;
            scopes.push(scope);
            items.push(AgentResource {
                id: task.id,
                cpu_percent,
                memory_bytes,
            });
        }
        sampler.retain(&scopes);
        Ok(AgentResources { items })
    }
}
#[cfg(test)]
#[path = "token_refresh_tests.rs"]
mod token_refresh_tests;
