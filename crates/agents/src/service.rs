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
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
pub(crate) type Result<T> = std::result::Result<T, Fail<AgentsError>>;
pub struct AgentRuntime {
    pub devbox_bin: String,
    pub use_scope: bool,
    pub cleanup_after_close: bool,
    /// Finds agent CLIs the way the user's terminal does.
    pub shell: Arc<dyn crate::command_path::ShellProbe>,
    /// Where those lookups are kept. The daemon passes the shared cache, which its environment
    /// check refreshes.
    pub lookups: Arc<crate::command_path::LookupCache>,
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
    pub(crate) shell: Arc<dyn crate::command_path::ShellProbe>,
    pub(crate) lookups: Arc<crate::command_path::LookupCache>,
    pub(crate) background: TaskTracker,
    pub(crate) stop: CancellationToken,
    pub(crate) resource_samples: Mutex<crate::resources::ResourceSampler>,
    pub(crate) approvals: Arc<crate::approvals::Approvals>,
    /// opencode usage already exported, per task.
    pub(crate) exports: crate::usage_plugins::ExportCache,
    /// Compiled waiting patterns by profile id, published once by the output watcher so a send
    /// reads the pane with the same patterns. Unset while no watcher runs.
    pub(crate) waiting_patterns: OnceLock<Arc<HashMap<String, Vec<regex::Regex>>>>,
    /// What this daemon has seen of each task's latest pi launch (see `ready`).
    pub(crate) launches: Arc<crate::ready::Launches>,
    /// When this daemon started: it may have missed the reports of launches from before.
    pub(crate) started_ms: i64,
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
            shell: runtime.shell,
            lookups: runtime.lookups,
            background: TaskTracker::new(),
            stop: CancellationToken::new(),
            resource_samples: Mutex::new(crate::resources::ResourceSampler::default()),
            approvals: Arc::default(),
            exports: Default::default(),
            waiting_patterns: OnceLock::new(),
            launches: Arc::default(),
            started_ms: now_ms(),
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
        self.update_bound(id, None, false, f).await
    }
    /// `announce_later` holds back the notice of a wait the update enters (see
    /// `announce_later`); the task still changes at once.
    async fn update_bound<F>(
        &self,
        id: &str,
        launch: Option<String>,
        announce_later: bool,
        f: F,
    ) -> Result<Done>
    where
        F: FnOnce(&mut AgentTask) -> bool + Send + 'static,
    {
        let id = id.to_owned();
        let hub = self.hub.clone();
        let approvals = self.approvals.clone();
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
                    if task.state != AgentState::Waiting {
                        approvals.forget(&task.id);
                    }
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous_state)))
                },
                move |(task, previous), rev| {
                    changed(&hub, task, rev, !announce_later && task.state != *previous)
                },
            )
            .await
            .internal("update agent")?;
        match result {
            Ok(((task, previous), rev)) => {
                if announce_later && task.state != previous && task.state == AgentState::Waiting {
                    self.announce_later(task);
                }
                Ok(Done {
                    rev: rev.to_string(),
                })
            }
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
        let binding = if lifecycle {
            Some(self.launch_binding(&task.id).await?)
        } else {
            None
        };
        let launch_id = binding.as_ref().and_then(|b| b.launch_id.as_deref());
        if launch_id.is_some_and(|launch| launch != p.payload) {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        }
        if task.state == AgentState::Closed {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        }
        // The launch's setup is over and its tool starts now.
        if p.source == crate::hooks::HookSource::Setup
            && p.code == Some(0)
            && launch_id == Some(p.payload.as_str())
            && self.says_ready(&task)
        {
            self.launches.tool_started(&task.id, &p.payload, now_ms());
        }
        if crate::plugin_event::is_ready_report(p.source, &p.payload) {
            return self.record_ready(&task.id, p.launch_id).await;
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
        // Claude ends every tool call with a hook. Without a recorded approval the ending
        // changes nothing, so skip it before keying its input or opening a write.
        if launch.is_some()
            && !self.approvals.has_pending(&task.id)
            && crate::hooks::claude_event(&p.payload)
                .is_some_and(|event| crate::hooks::ends_tool_call(&event))
        {
            return Ok(Done {
                rev: self.db.rev().to_string(),
            });
        }
        let approval = launch
            .as_ref()
            .and_then(|_| crate::hooks::approval(&p.payload));
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
        if matches!(
            p.source,
            crate::hooks::HookSource::Opencode | crate::hooks::HookSource::Pi
        ) {
            // The plugin reports a resolved request even when nothing waits for it. Such a
            // report, or one that means nothing, changes nothing, so no write is opened.
            if events.is_empty()
                || (events == [AgentEvent::OutputResumed] && task.state != AgentState::Waiting)
            {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            }
            if events
                .iter()
                .any(|event| matches!(event, AgentEvent::TurnComplete { .. }))
            {
                self.exports.turn_completed(&task.id);
            }
        }
        // pi asks about trust even in a folder whose trust is decided, and goes on at once.
        let trust = p.source == crate::hooks::HookSource::Pi
            && events.iter().any(|event| {
                matches!(event, AgentEvent::NeedsInput { message }
                    if message == crate::plugin_event::PI_TRUST_COPY)
            });
        let approvals = self.approvals.clone();
        self.update_bound(&p.agent_id, launch.clone(), trust, move |t| {
            if let Some(launch) = &launch {
                return approvals.apply(t, launch, approval, events);
            }
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
