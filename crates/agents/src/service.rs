pub(crate) use crate::events::{apply_event, changed};
#[cfg(test)]
use crate::tracker::Binding;
use crate::{
    model::*,
    profile::ToolProfile,
    state::{AgentEvent, AgentState},
    store,
};
use devbox_core::{Db, Hub, Paths};
use devbox_projects::ProjectsService;
use devbox_protocol::{Done, Fail, Internal};
use devbox_terminal::TerminalService;
#[cfg(test)]
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
pub(crate) type Result<T> = std::result::Result<T, Fail<AgentsError>>;
pub struct AgentRuntime {
    pub devbox_bin: String,
    pub use_scope: bool,
    pub cleanup_after_close: bool,
}
pub struct AgentsService {
    pub(crate) db: Db,
    pub(crate) hub: Arc<Hub>,
    pub(crate) paths: Paths,
    pub(crate) projects: Arc<ProjectsService>,
    pub(crate) terminal: Arc<TerminalService>,
    pub(crate) profiles: Vec<ToolProfile>,
    pub(crate) devbox_bin: String,
    pub(crate) use_scope: bool,
    pub(crate) reservations: Mutex<()>,
    pub(crate) actions: Mutex<()>,
    pub(crate) cleanup_after_close: bool,
    pub(crate) hook_files: Mutex<()>,
    pub(crate) background: TaskTracker,
    pub(crate) stop: CancellationToken,
    pub(crate) resource_samples: Mutex<crate::resources::ResourceSampler>,
}
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
impl AgentsService {
    pub fn new(
        db: Db,
        hub: Arc<Hub>,
        paths: Paths,
        projects: Arc<ProjectsService>,
        terminal: Arc<TerminalService>,
        profiles: Vec<ToolProfile>,
        runtime: AgentRuntime,
    ) -> Self {
        Self {
            db,
            hub,
            paths,
            projects,
            terminal,
            profiles,
            devbox_bin: runtime.devbox_bin,
            use_scope: runtime.use_scope,
            reservations: Mutex::new(()),
            actions: Mutex::new(()),
            cleanup_after_close: runtime.cleanup_after_close,
            hook_files: Mutex::new(()),
            background: TaskTracker::new(),
            stop: CancellationToken::new(),
            resource_samples: Mutex::new(crate::resources::ResourceSampler::default()),
        }
    }
    pub fn profiles(&self) -> AgentProfiles {
        let mut items = self.profiles.clone();
        for profile in crate::profile::builtin() {
            if !items.iter().any(|p| p.id == profile.id) {
                items.push(profile)
            }
        }
        AgentProfiles { items }
    }
    pub fn cleanup_after_close_enabled(&self) -> bool {
        self.cleanup_after_close
    }
    pub async fn get(&self, id: &str) -> Result<AgentTask> {
        let missing = id.to_owned();
        let id = id.to_owned();
        let task = self
            .db
            .read(move |tx| store::get(tx, &id))
            .await
            .internal("read agent")?
            .ok_or(AgentsError::NotFound { id: missing })?;
        crate::limits::serialized(&task, crate::limits::STORED_TASK_BYTES)?;
        Ok(task)
    }
    pub(crate) async fn update<F>(&self, id: &str, f: F) -> Result<Done>
    where
        F: FnOnce(&mut AgentTask) -> bool + Send + 'static,
    {
        self.update_bound(id, None, f).await
    }
    async fn update_bound<F>(&self, id: &str, launch: Option<String>, f: F) -> Result<Done>
    where
        F: FnOnce(&mut AgentTask) -> bool + Send + 'static,
    {
        let id = id.to_owned();
        let hub = self.hub.clone();
        let result = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(Some(AgentsError::NotFound { id })));
                    };
                    if let Some(launch) = launch
                        && store::binding(tx, &id)?
                            .and_then(|b| b.launch_id)
                            .as_deref()
                            != Some(launch.as_str())
                    {
                        return Ok(Err(None));
                    }
                    let previous_state = task.state;
                    if !f(&mut task) {
                        return Ok(Err(None));
                    }
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous_state)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .internal("update agent")?;
        match result {
            Ok((_, rev)) => Ok(Done {
                rev: rev.to_string(),
            }),
            Err(Some(e)) => Err(e.into()),
            Err(None) => Ok(Done {
                rev: self.db.rev().to_string(),
            }),
        }
    }
    pub async fn event(&self, p: AgentEventParams) -> Result<Done> {
        let lifecycle = matches!(
            p.source,
            crate::hooks::HookSource::Setup | crate::hooks::HookSource::Exit
        );
        let _guard = if lifecycle {
            Some(self.actions.lock().await)
        } else {
            None
        };
        let task = self.get(&p.agent_id).await?;
        if lifecycle
            && self
                .launch_binding(&task.id)
                .await?
                .launch_id
                .as_deref()
                .is_some_and(|launch| launch != p.payload)
        {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        }
        if task.state == AgentState::Closed {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        }
        let launch = if p.source == crate::hooks::HookSource::Claude {
            let Some(launch) = p.launch_id.filter(|id| !id.is_empty()) else {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            };
            Some(launch)
        } else {
            None
        };
        let events = if p.source == crate::hooks::HookSource::Codex {
            let Some(candidate) = crate::hooks::parse_codex_candidate(&p.payload) else {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            };
            crate::limits::identity(&candidate.thread_id)?;
            crate::limits::identity(&candidate.turn_id)?;
            if candidate.cwd != task.workdir {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            }
            let binding = self.refresh_binding(&task, true).await?;
            if !binding.matches(&candidate.thread_id, &candidate.turn_id) {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            }
            return self.complete_codex(&task.id, candidate).await;
        } else {
            crate::hooks::parse(p.source, &p.payload, p.code)
        };
        for event in &events {
            if let AgentEvent::SessionStarted { session_id } = event {
                crate::limits::identity(session_id)?;
            }
        }
        self.update_bound(&p.agent_id, launch, move |t| {
            let mut changed = false;
            for event in events {
                changed |= apply_event(t, &event, now_ms())
            }
            changed
        })
        .await
    }
    async fn complete_codex(
        &self,
        id: &str,
        candidate: crate::hooks::CodexCompletionCandidate,
    ) -> Result<Done> {
        let id = id.to_owned();
        let hub = self.hub.clone();
        let result = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(Some(AgentsError::NotFound { id })));
                    };
                    let Some(binding) = store::binding(tx, &id)? else {
                        return Ok(Err(None));
                    };
                    if task.state == AgentState::Closed
                        || !binding.matches(&candidate.thread_id, &candidate.turn_id)
                        || candidate.cwd != task.workdir
                    {
                        return Ok(Err(None));
                    }
                    let previous_state = task.state;
                    if !apply_event(
                        &mut task,
                        &AgentEvent::TurnComplete {
                            message: candidate.message,
                        },
                        now_ms(),
                    ) {
                        return Ok(Err(None));
                    }
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous_state)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .internal("complete Codex turn")?;
        match result {
            Ok((_, rev)) => Ok(Done {
                rev: rev.to_string(),
            }),
            Err(Some(e)) => Err(e.into()),
            Err(None) => Ok(Done {
                rev: self.db.rev().to_string(),
            }),
        }
    }
}
#[cfg(test)]
#[path = "correlation_tests.rs"]
mod correlation_tests;
#[cfg(all(test, unix))]
#[path = "resource_tests.rs"]
mod resource_tests;
#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
