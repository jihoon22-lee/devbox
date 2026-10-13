use crate::{
    AgentsAttentionTopic, AgentsChangedTopic,
    model::*,
    profile::ToolProfile,
    state::{self, AgentEvent, AgentState},
    store,
    tracker::Binding,
};
use devbox_core::{Db, Hub, Paths};
use devbox_projects::ProjectsService;
use devbox_protocol::{Done, Fail, Internal};
use devbox_terminal::TerminalService;
use std::{collections::HashSet, path::Path, sync::Arc};
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
    reservations: Mutex<()>,
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
pub(crate) fn changed(hub: &Hub, task: &AgentTask, rev: u64, entered_attention: bool) {
    hub.publish::<AgentsChangedTopic>(
        rev,
        &AgentsChanged {
            rev: rev.to_string(),
            id: task.id.clone(),
        },
    );
    if entered_attention && state::is_attention(task.state) {
        hub.publish::<AgentsAttentionTopic>(
            rev,
            &AgentsAttention {
                id: task.id.clone(),
                state: task.state,
                message: task
                    .attention
                    .as_ref()
                    .map(|a| a.message.clone())
                    .unwrap_or_default(),
            },
        );
    }
}
pub(crate) fn apply_event(t: &mut AgentTask, event: &AgentEvent, now: i64) -> bool {
    if matches!(event, AgentEvent::OutputWaiting) && t.state != AgentState::Running {
        return false;
    }
    if matches!(event, AgentEvent::OutputResumed)
        && t.attention.as_ref().is_none_or(|a| a.kind != "output")
    {
        return false;
    }
    if t.state == AgentState::Closed {
        return false;
    }
    let ended = matches!(
        event,
        AgentEvent::Exited { .. } | AgentEvent::SetupFailed { .. } | AgentEvent::SessionLost
    );
    if t.tool_exited
        && !ended
        && !matches!(event, AgentEvent::UserReview | AgentEvent::Closed { .. })
    {
        return false;
    }
    let exit_changed = ended && !t.tool_exited;
    t.tool_exited |= ended;
    let mut updated = exit_changed
        || matches!(
            event,
            AgentEvent::PromptSubmitted | AgentEvent::UserInstructed
        ) && matches!(
            t.state,
            AgentState::Running | AgentState::Waiting | AgentState::Idle
        );
    let mut transitioned = false;
    if let AgentEvent::SessionStarted { session_id } = event
        && t.session_id.as_ref() != Some(session_id)
    {
        t.session_id = Some(session_id.clone());
        updated = true;
    }
    if let Some(next) = state::next(t.state, event) {
        transitioned = true;
        t.state = next;
        t.reason = state::fail_reason(event);
        updated = true;
    }
    let message = match event {
        AgentEvent::NeedsInput { message } if t.state == AgentState::Waiting => {
            Some(message.clone())
        }
        AgentEvent::TurnComplete { message } if t.state == AgentState::Idle => Some(
            message
                .clone()
                .unwrap_or_else(|| "다음 지시를 기다립니다".into()),
        ),
        AgentEvent::OutputWaiting if t.state == AgentState::Waiting => {
            Some("입력을 기다립니다".into())
        }
        _ => None,
    };
    if state::is_attention(t.state) && (transitioned || message.is_some()) {
        let message = message.unwrap_or_else(|| {
            t.reason
                .clone()
                .unwrap_or_else(|| "다음 지시를 기다립니다".into())
        });
        let kind = match t.state {
            AgentState::Waiting if matches!(event, AgentEvent::OutputWaiting) => "output",
            AgentState::Waiting => "input",
            AgentState::Idle => "idle",
            _ => "failed",
        };
        let attention_changed = t
            .attention
            .as_ref()
            .is_none_or(|a| a.message != message || a.kind != kind);
        if attention_changed {
            updated = true;
        }
        if transitioned || attention_changed {
            t.attention = Some(Attention {
                kind: kind.into(),
                message,
                at_ms: now,
            });
        }
    } else if transitioned {
        t.attention = None;
    }
    if updated {
        t.activity_seq = t.activity_seq.wrapping_add(1);
        t.updated_ms = now;
    }
    updated
}
pub(crate) fn allocate_port(tasks: &[AgentTask]) -> std::result::Result<u16, AgentsError> {
    allocate_port_from(tasks, 20000)
}
fn allocate_port_from(tasks: &[AgentTask], start: u16) -> std::result::Result<u16, AgentsError> {
    let used: HashSet<_> = tasks
        .iter()
        .filter(|t| t.state != AgentState::Closed)
        .map(|t| t.port)
        .collect();
    for base in (u32::from(start)..=65520).step_by(10) {
        let base = base as u16;
        if used.contains(&base) {
            continue;
        }
        let listeners: std::io::Result<Vec<_>> = (base..=base + 9)
            .map(|p| std::net::TcpListener::bind(("127.0.0.1", p)))
            .collect();
        if listeners.is_ok() {
            return Ok(base);
        }
    }
    Err(AgentsError::InvalidInput)
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
    pub async fn get(&self, id: &str) -> Result<AgentTask> {
        let missing = id.to_owned();
        let id = id.to_owned();
        let task = self
            .db
            .read(move |tx| store::get(tx, &id))
            .await
            .internal("read agent")?
            .ok_or(AgentsError::NotFound { id: missing })?;
        crate::limits::serialized(&task, crate::limits::TASK_BYTES)?;
        Ok(task)
    }
    pub async fn create(self: &Arc<Self>, p: CreateAgentTask) -> Result<AgentTask> {
        crate::limits::input(&p.prompt)?;
        let title = crate::plan::checked_title(&p.title)?.to_owned();
        let _reservation = self.reservations.lock().await;
        let project = self
            .projects
            .get(&p.project_id)
            .await
            .map_err(|e| Fail::Internal(format!("agent project: {e:?}")))?
            .ok_or_else(|| AgentsError::ProjectNotFound {
                id: p.project_id.clone(),
            })?;
        let profile = crate::profile::resolve(&self.profiles, &p.profile).ok_or_else(|| {
            AgentsError::ProfileNotFound {
                id: p.profile.clone(),
            }
        })?;
        // Reject malformed project commands/copy configuration before reserving work.
        devbox_projects::config::load(Path::new(&project.path))
            .map_err(|e| Fail::Internal(format!("agent config: {e}")))?;
        let base = match p.base_branch {
            Some(b) => b,
            None => devbox_git::current_branch(Path::new(&project.path))
                .await
                .map_err(git_failure)?
                .unwrap_or_else(|| "main".into()),
        };
        let mut taken: HashSet<String> = devbox_git::branches(Path::new(&project.path))
            .await
            .map_err(git_failure)?
            .into_iter()
            .filter_map(|b| b.strip_prefix("agent/").map(str::to_owned))
            .collect();
        let now = now_ms();
        let id = format!("a{}", uuid::Uuid::new_v4().simple());
        let repo = std::path::PathBuf::from(&project.path);
        let svc = self.clone();
        let hub = self.hub.clone();
        let handle = tokio::runtime::Handle::current();
        let prep_project = project.clone();
        let committed_reservation = self.background.token();
        let committed = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let project_exists: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM projects_projects WHERE id=?1)",
                        [&project.id],
                        |row| row.get(0),
                    )?;
                    if !project_exists {
                        return Ok(Err(AgentsError::ProjectNotFound { id: project.id }));
                    }
                    let existing = store::list(tx, i64::MIN)?;
                    for task in &existing {
                        if task.project_id == project.id
                            && let Some(branch) = &task.branch
                            && let Some(slug) = branch.strip_prefix("agent/")
                        {
                            taken.insert(slug.into());
                        }
                    }
                    let (branch, worktree) = if p.worktree {
                        let base_slug = crate::plan::slug(&title, now as u64);
                        let selected = loop {
                            let slug = match crate::plan::unique_slug(&base_slug, &taken) {
                                Ok(v) => v,
                                Err(e) => return Ok(Err(e)),
                            };
                            let path = match crate::plan::worktree_dir(&repo, &slug) {
                                Ok(v) => v,
                                Err(e) => return Ok(Err(e)),
                            };
                            if std::fs::symlink_metadata(&path).is_ok() {
                                taken.insert(slug);
                                continue;
                            }
                            break (slug, path);
                        };
                        (
                            Some(
                                crate::plan::branch_for(&selected.0)
                                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                            ),
                            Some(selected.1.to_string_lossy().into_owned()),
                        )
                    } else {
                        (None, None)
                    };
                    let port = match allocate_port(&existing) {
                        Ok(v) => v,
                        Err(e) => return Ok(Err(e)),
                    };
                    let task = AgentTask {
                        id,
                        project_id: project.id,
                        title,
                        profile: profile.id.clone(),
                        base_branch: base,
                        branch,
                        worktree_path: worktree.clone(),
                        workdir: worktree.unwrap_or(project.path),
                        session_id: None,
                        terminal_id: None,
                        tool_exited: false,
                        state: AgentState::Preparing,
                        reason: None,
                        attention: None,
                        port,
                        prompt: p.prompt,
                        follow_ups: vec![],
                        tokens: None,
                        pr_url: None,
                        closed_how: None,
                        created_ms: now,
                        updated_ms: now,
                        closed_ms: None,
                        closed_commits: None,
                        closed_head_oid: None,
                        cleanup_error: None,
                        tokens_truncated: false,
                        activity_seq: 0,
                    };
                    if let Err(error) = crate::limits::serialized(&task, crate::limits::TASK_BYTES)
                    {
                        return Ok(Err(match error {
                            Fail::Domain(error) => error,
                            Fail::Internal(_) => AgentsError::InvalidInput,
                        }));
                    }
                    store::put(tx, &task)?;
                    let mut binding = Binding::new(now);
                    binding.launch_id = Some(uuid::Uuid::new_v4().simple().to_string());
                    store::put_binding(tx, &task.id, &binding)?;
                    Ok(Ok((task, profile)))
                },
                move |(task, profile), rev| {
                    let _tracked_reservation = committed_reservation;
                    changed(&hub, task, rev, false);
                    let task = task.clone();
                    let profile = profile.clone();
                    let background = svc.background.clone();
                    let svc = svc.clone();
                    let _entered = handle.enter();
                    background.spawn(async move {
                        if let Err(error) = svc.prepare(&task, &profile, &prep_project).await {
                            tracing::error!(?error,agent=%task.id,"agent preparation failed");
                            let _ = svc.preparation_failed(&task.id, &error).await;
                        }
                    });
                },
            )
            .await
            .internal("reserve agent")?
            .map_err(Fail::Domain)?;
        Ok(committed.0.0)
    }
    pub(crate) async fn update<F>(&self, id: &str, f: F) -> Result<Done>
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
        let events = if p.source == crate::hooks::HookSource::Codex {
            let Some(candidate) = crate::hooks::parse_codex_candidate(&p.payload) else {
                return Ok(Done {
                    rev: self.db.rev().to_string(),
                });
            };
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
        self.update(&p.agent_id, move |t| {
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
