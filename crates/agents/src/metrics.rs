use crate::{
    model::*,
    service::{AgentsService, Result},
    state::AgentState,
    store,
};
use devbox_protocol::{Fail, Internal};
use futures::{StreamExt, stream};
impl AgentsService {
    pub async fn usage(&self, id: &str) -> Result<AgentUsage> {
        self.read_usage(id, false).await
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
