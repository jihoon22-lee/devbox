use crate::{
    model::*,
    service::{AgentsService, Result, changed, now_ms},
    state::AgentState,
};
use devbox_protocol::Internal;
use std::{path::Path, sync::Arc};
impl AgentsService {
    /// Track the launch independently of the RPC waiter so cancellation cannot orphan tmux.
    pub async fn resume(self: &Arc<Self>, id: &str) -> Result<AgentTask> {
        let service = self.clone();
        let id = id.to_owned();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.background.spawn(async move {
            let result = service.resume_owned(&id).await;
            let _ = tx.send(result);
        });
        rx.await.internal("resume task completion")?
    }
    async fn resume_owned(&self, id: &str) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        crate::actions::open(&task)?;
        if task.terminal_id.as_ref().is_some_and(|id| !id.is_empty())
            && self
                .terminal
                .tmux()
                .list()
                .await
                .internal("resume session list")?
                .iter()
                .any(|s| Some(&s.name) == task.terminal_id.as_ref())
            && !matches!(
                self.tool_phase(&task).await?,
                Some(crate::tool_lifecycle::Phase::Ended(_))
            )
        {
            return Err(AgentsError::BadState {
                state: "session-present".into(),
            }
            .into());
        }
        let project = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| devbox_protocol::Fail::Internal(format!("resume project: {e:?}")))?
            .ok_or(AgentsError::ProjectNotFound {
                id: task.project_id.clone(),
            })?;
        if task.worktree_path.is_some() {
            let (_, wt, branch) = self.owned_worktree(&task).await?;
            if devbox_git::current_branch(&wt)
                .await
                .map_err(git_failure)?
                .as_deref()
                != Some(&branch)
            {
                return Err(AgentsError::UnsafeWorktree.into());
            }
        } else if task.workdir != project.path {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let profile = crate::profile::resolve(&self.profiles, &task.profile).ok_or(
            AgentsError::ProfileNotFound {
                id: task.profile.clone(),
            },
        )?;
        let session_id = if profile.hooks == crate::profile::Hooks::Codex {
            self.refresh_binding(&task, true).await?.thread_id
        } else {
            task.session_id.clone()
        };
        let data = self.paths.data_dir.join("agents").join(&task.id);
        crate::prepare::private_dir(&data).internal("resume data directory")?;
        let generation = uuid::Uuid::new_v4().simple().to_string();
        let phase = self.phase_path(&task, &generation);
        crate::prepare::private_file(&phase, "starting\n", false).internal("resume phase")?;
        let script = data.join(format!("resume-{generation}.sh"));
        let prompt = data.join(format!("resume-{generation}.md"));
        crate::prepare::private_file(&prompt, &task.prompt, false).internal("resume prompt")?;
        let settings = if profile.hooks == crate::profile::Hooks::Claude {
            let path = data.join(format!("claude-{generation}.json"));
            crate::prepare::private_file(
                &path,
                &crate::hooks::claude_settings(&self.devbox_bin),
                false,
            )
            .internal("resume hooks")?;
            Some(path.to_string_lossy().into_owned())
        } else {
            None
        };
        let plan = crate::start::StartPlan {
            devbox: self.devbox_bin.clone(),
            instance: self.paths.instance.as_str().into(),
            agent_id: task.id.clone(),
            launch_id: generation.clone(),
            phase_file: phase.to_string_lossy().into_owned(),
            workdir: task.workdir.clone(),
            setup: vec![],
            tool_command: profile.command.clone(),
            hooks: profile.hooks,
            claude_settings: settings,
            prompt_file: Some(prompt.to_string_lossy().into_owned()),
            resume_session: session_id.clone(),
            use_scope: self.use_scope,
            port: task.port,
        };
        crate::prepare::private_file(&script, &crate::start::render_start_script(&plan), true)
            .internal("resume script")?;
        // Preserve the ended launch's evidence until its holder is gone. A
        // failed close must leave a subsequent resume able to identify it.
        if let Some(old) = &task.terminal_id {
            self.terminal.close_internal(old).await.map_err(|e| {
                devbox_protocol::Fail::Internal(format!("remove lost terminal record: {e:?}"))
            })?;
        }
        let rollback_generation = generation.clone();
        let terminal_uuid = uuid::Uuid::new_v4();
        let new_terminal = format!("t{}", terminal_uuid.simple());
        let key = task.id.clone();
        let launch = now_ms();
        let initial_session = session_id.clone();
        let hub = self.hub.clone();
        let previous_binding = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let mut binding = crate::store::binding(tx, &key)?
                        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let mut current =
                        crate::store::get(tx, &key)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let previous = current.state;
                    let previous_binding = binding.clone();
                    binding.launch_ms = launch;
                    binding.launch_id = Some(generation);
                    binding.turn_id = None;
                    binding.turn_started_ms = None;
                    current.session_id = initial_session;
                    current.terminal_id = None;
                    current.state = AgentState::Running;
                    current.tool_exited = false;
                    current.reason = None;
                    current.attention = None;
                    current.updated_ms = launch;
                    crate::store::put_binding(tx, &key, &binding)?;
                    crate::store::put(tx, &current)?;
                    Ok(Ok::<_, std::convert::Infallible>((
                        current,
                        previous,
                        previous_binding,
                    )))
                },
                move |(current, previous, _), rev| {
                    changed(&hub, current, rev, current.state != *previous)
                },
            )
            .await
            .internal("resume launch state")?
            .unwrap_or_else(|never| match never {})
            .0
            .2;
        let launched = self
            .terminal
            .create_internal_with_id(
                (devbox_terminal::SessionKind::Agent, terminal_uuid),
                Some(task.project_id.clone()),
                Path::new(&task.workdir).into(),
                task.title.clone(),
                format!("bash {}", crate::plan::sh_quote(&script.to_string_lossy())),
                &[],
            )
            .await;
        let session = match launched {
            Ok(session) => session,
            Err(error) => {
                self.terminal.close_internal(&new_terminal).await.map_err(|cleanup| {
                    devbox_protocol::Fail::Internal(format!("resume creation failed; retained launch {rollback_generation} terminal {new_terminal} after cleanup failure: {error:?}; {cleanup:?}"))
                })?;
                self.rollback_resume(id, &rollback_generation, previous_binding)
                    .await?;
                return Err(devbox_protocol::Fail::Internal(format!(
                    "resume terminal: {error:?}"
                )));
            }
        };
        let persisted = self
            .update(id, move |current| {
                current.terminal_id = Some(session.id);
                current.updated_ms = now_ms();
                true
            })
            .await;
        if let Err(error) = persisted {
            self.terminal
                .close_internal(&new_terminal)
                .await
                .map_err(|e| {
                    devbox_protocol::Fail::Internal(format!(
                        "remove unrecorded resumed session {new_terminal} failed; retained launch {rollback_generation}: {e:?}"
                    ))
                })?;
            self.rollback_resume(id, &rollback_generation, previous_binding)
                .await?;
            return Err(error);
        }
        self.get(id).await
    }
    /// Restore correlation only after the replacement's exact terminal is gone.
    async fn rollback_resume(
        &self,
        id: &str,
        generation: &str,
        mut previous: crate::tracker::Binding,
    ) -> Result<()> {
        let id = id.to_owned();
        let generation = generation.to_owned();
        let hub = self.hub.clone();
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut current) = crate::store::get(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    let Some(binding) = crate::store::binding(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    if binding.launch_id.as_deref() != Some(&generation)
                        || current.state == AgentState::Closed
                    {
                        return Ok(Err(()));
                    }
                    previous.resources = binding.resources;
                    previous.owned_rebase = binding.owned_rebase;
                    let old_state = current.state;
                    current.state = AgentState::Failed;
                    current.reason = Some("resume_failed".into());
                    current.attention = None;
                    current.tool_exited = true;
                    current.terminal_id = None;
                    current.activity_seq = current.activity_seq.wrapping_add(1);
                    current.updated_ms = now_ms();
                    crate::store::put_binding(tx, &id, &previous)?;
                    crate::store::put(tx, &current)?;
                    Ok(Ok((current, old_state)))
                },
                move |(current, old_state), rev| {
                    changed(&hub, current, rev, current.state != *old_state)
                },
            )
            .await
            .internal("restore failed resume correlation")?;
        Ok(())
    }
}
