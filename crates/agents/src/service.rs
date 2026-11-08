pub(crate) use crate::events::{apply_event, changed};
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
    pub(crate) exports: Arc<crate::usage_plugins::ExportCache>,
    /// Compiled waiting patterns by profile id, published once by the output watcher so a send
    /// reads the pane with the same patterns. Unset while no watcher runs.
    pub(crate) waiting_patterns: OnceLock<Arc<HashMap<String, crate::watch::WaitingPatterns>>>,
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
/// What shows that a Codex turn ended (see `AgentsService::complete_codex`).
pub(crate) enum CodexTurnEnd {
    /// Codex's `notify` named this thread and turn.
    Reported(crate::hooks::CodexCompletionCandidate),
    /// A launch that resumed Codex's session, before any turn, sat at Codex's prompt (see
    /// `watch::shows_prompt`) while the task and binding were as the output watcher saw them.
    /// Codex waits for the next instruction and has no message of its own.
    Ready(Box<crate::watch::Snapshot>),
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
        self.update_bound(id, None, false, move |task, _| f(task))
            .await
    }
    /// `launch`, when given, must be the task's current launch: `f` then also gets that launch's
    /// binding, which is saved with the task when `f` changes it. `announce_later` holds back the
    /// notice of a wait the update enters (see `announce_later`); the task still changes at once.
    async fn update_bound<F>(
        &self,
        id: &str,
        launch: Option<String>,
        announce_later: bool,
        f: F,
    ) -> Result<Done>
    where
        F: FnOnce(&mut AgentTask, Option<&mut Binding>) -> bool + Send + 'static,
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
                    let mut binding = None;
                    if let Some(launch) = launch {
                        binding = store::binding(tx, &id)?;
                        if binding.as_ref().and_then(|b| b.launch_id.as_deref())
                            != Some(launch.as_str())
                        {
                            return Ok(Err(None));
                        }
                    }
                    let stored = binding.clone();
                    let previous_state = task.state;
                    if !f(&mut task, binding.as_mut()) {
                        return Ok(Err(None));
                    }
                    if let Some(binding) = binding.filter(|b| Some(b) != stored.as_ref()) {
                        store::put_binding(tx, &id, &binding)?;
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
            let Some(launch) = p.launch_id.clone().filter(|id| !id.is_empty()) else {
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
            return self
                .complete_codex(&task.id, CodexTurnEnd::Reported(candidate))
                .await;
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
        }
        // A finished turn may have added tokens, so the usage read from its export is dropped,
        // but only for a report of the current launch (see below): a late one from the launch
        // before a resume says nothing about this one.
        let turn_done = matches!(
            p.source,
            crate::hooks::HookSource::Opencode | crate::hooks::HookSource::Pi
        ) && events
            .iter()
            .any(|event| matches!(event, AgentEvent::TurnComplete { .. }));
        let exports = turn_done.then(|| self.exports.clone());
        // pi asks about trust even in a folder whose trust is decided, and goes on at once.
        let trust = p.source == crate::hooks::HookSource::Pi
            && events.iter().any(|event| {
                matches!(event, AgentEvent::NeedsInput { message }
                    if message == crate::plugin_event::PI_TRUST_COPY)
            });
        // A report changes the task only while the launch it names is current: a plugin's
        // report can arrive after a resume. Only Claude's go through `approvals`; the plugins
        // pair their own requests and replies. A plugin report naming no launch applies as
        // before, so a wait is never lost for it.
        let bound = match p.source {
            crate::hooks::HookSource::Opencode | crate::hooks::HookSource::Pi => {
                p.launch_id.filter(|id| !id.is_empty())
            }
            _ => launch.clone(),
        };
        // A launch that resumed Claude's session begins at Claude's prompt with no turn (see
        // `Binding::resumed_session`). Claude says it is there by starting the resumed session.
        let resumed = launch.is_some()
            && crate::hooks::claude_session_source(&p.payload).as_deref() == Some("resume");
        let approvals = self.approvals.clone();
        self.update_bound(&p.agent_id, bound, trust, move |t, binding| {
            if let Some(exports) = exports {
                exports.turn_completed(&t.id);
            }
            if let Some(launch) = &launch {
                let mut events = events;
                // The launch's first prompt ends its wait for one. Before it, the resumed session's
                // start ends the wait as a finished turn does, once: Claude waits for the next
                // instruction.
                if let Some(binding) = binding {
                    if events.contains(&AgentEvent::PromptSubmitted) {
                        binding.resumed_session = false;
                    } else if resumed && binding.awaits_first_instruction(t) {
                        binding.resumed_session = false;
                        events.push(AgentEvent::TurnComplete { message: None });
                    }
                }
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
    /// Ends a Codex turn as `end` shows it, checked against the task and its binding in the writer.
    pub(crate) async fn complete_codex(&self, id: &str, end: CodexTurnEnd) -> Result<Done> {
        let id = id.to_owned();
        let hub = self.hub.clone();
        let result = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(Some(AgentsError::NotFound { id })));
                    };
                    let binding = store::binding(tx, &id)?;
                    if task.state == AgentState::Closed {
                        return Ok(Err(None));
                    }
                    let message = match end {
                        CodexTurnEnd::Reported(candidate) => {
                            if !binding.as_ref().is_some_and(|b| {
                                b.matches(&candidate.thread_id, &candidate.turn_id)
                            }) || candidate.cwd != task.workdir
                            {
                                return Ok(Err(None));
                            }
                            candidate.message
                        }
                        // Judged once: the launch no longer waits for its first instruction.
                        CodexTurnEnd::Ready(seen) => {
                            if !seen.current(&task, &binding) || !crate::watch::eligible(&task) {
                                return Ok(Err(None));
                            }
                            let Some(mut binding) =
                                binding.filter(|b| b.awaits_first_instruction(&task))
                            else {
                                return Ok(Err(None));
                            };
                            binding.resumed_session = false;
                            store::put_binding(tx, &id, &binding)?;
                            None
                        }
                    };
                    let previous_state = task.state;
                    if !apply_event(&mut task, &AgentEvent::TurnComplete { message }, now_ms()) {
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
