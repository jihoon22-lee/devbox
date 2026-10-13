use crate::{
    model::*,
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState},
    store,
    tracker::{self, Binding},
};
use devbox_protocol::{Fail, Internal};
use std::sync::Arc;
impl AgentsService {
    pub(crate) async fn refresh_binding(&self, task: &AgentTask, retry: bool) -> Result<Binding> {
        let id = task.id.clone();
        let old = self
            .db
            .read(move |tx| store::binding(tx, &id))
            .await
            .internal("read agent binding")?
            .unwrap_or_else(|| Binding::new(task.created_ms));
        let root = self.paths.home.join(".codex/sessions");
        let mut last = tracker::Diagnostic::Missing;
        for attempt in 0..if retry { 3 } else { 1 } {
            let (root, workdir, binding) = (root.clone(), task.workdir.clone(), old.clone());
            let result =
                tokio::task::spawn_blocking(move || tracker::refresh(&root, &workdir, &binding))
                    .await
                    .internal("read Codex metadata")?;
            match result {
                Ok(binding) => {
                    let id = task.id.clone();
                    let saved = binding.clone();
                    let hub = self.hub.clone();
                    let _ = self
                        .db
                        .write_checked_with_commit(
                            move |tx| {
                                let Some(mut current) = store::get(tx, &id)? else {
                                    return Ok(Err(()));
                                };
                                if current.state == AgentState::Closed {
                                    return Ok(Err(()));
                                }
                                let previous_state = current.state;
                                let current_binding = store::binding(tx, &id)?;
                                // A poll started before resume must not replace the new launch or ownership.
                                if current_binding.as_ref().is_some_and(|b| {
                                    b.launch_ms != saved.launch_ms || b.launch_id != saved.launch_id
                                }) {
                                    return Ok(Err(()));
                                }
                                let mut saved = saved;
                                if let Some(binding) = &current_binding {
                                    saved.resources = binding.resources.clone();
                                    saved.owned_rebase = binding.owned_rebase;
                                }
                                if current_binding.as_ref().and_then(|b| b.turn_started_ms)
                                    > saved.turn_started_ms
                                {
                                    return Ok(Err(()));
                                }
                                let previous =
                                    current_binding.as_ref().and_then(|b| b.turn_id.clone());
                                if current_binding.as_ref() == Some(&saved)
                                    && current.session_id == saved.thread_id
                                {
                                    return Ok(Err(()));
                                }
                                store::put_binding(tx, &id, &saved)?;
                                let mut changed = false;
                                if let Some(session) = &saved.thread_id {
                                    changed |= apply_event(
                                        &mut current,
                                        &AgentEvent::SessionStarted {
                                            session_id: session.clone(),
                                        },
                                        now_ms(),
                                    );
                                }
                                if !retry && previous != saved.turn_id {
                                    changed |= apply_event(
                                        &mut current,
                                        &AgentEvent::PromptSubmitted,
                                        now_ms(),
                                    );
                                }
                                if changed {
                                    store::put(tx, &current)?;
                                }
                                Ok(Ok((current, changed, previous_state)))
                            },
                            move |(current, did_change, previous), rev| {
                                if *did_change {
                                    changed(&hub, current, rev, current.state != *previous)
                                }
                            },
                        )
                        .await
                        .internal("save Codex binding")?;
                    return Ok(binding);
                }
                Err(d) => last = d,
            }
            if attempt < 2 && retry {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        Err(AgentsError::SessionMetadata { diagnostic: last }.into())
    }
    pub async fn reconcile(&self) -> Result<()> {
        let sessions = self
            .terminal
            .tmux()
            .list()
            .await
            .map_err(|e| Fail::Internal(format!("agent reconcile tmux: {e}")))?;
        for task in self.list(ListAgents::default()).await?.items {
            if self.recover_tool_exit(&task.id).await? {
                continue;
            }
            if matches!(
                task.state,
                AgentState::Preparing
                    | AgentState::Running
                    | AgentState::Waiting
                    | AgentState::Idle
            ) && !task
                .terminal_id
                .as_ref()
                .is_some_and(|id| sessions.iter().any(|s| &s.name == id))
            {
                if task.state == AgentState::Preparing {
                    self.reconcile_resources(&task).await?;
                }
                self.update(&task.id, |t| {
                    apply_event(t, &AgentEvent::SessionLost, now_ms())
                })
                .await?;
            }
        }
        Ok(())
    }
    pub async fn initialize(self: &Arc<Self>) -> Result<()> {
        self.reconcile().await?;
        crate::watch::spawn(self).internal("compile output waiting patterns")?;
        let svc = self.clone();
        self.background.spawn(async move {
            let mut interval=tokio::time::interval(std::time::Duration::from_millis(250));
            loop {
                tokio::select! {
                    _=svc.stop.cancelled()=>break,
                    _=interval.tick()=>{}
                }
                let Ok(list)=svc.list(ListAgents::default()).await else {continue};
                for task in list.items {
                    if !task.tool_exited && svc.recover_tool_exit(&task.id).await.unwrap_or(false) {
                        continue;
                    }
                    let codex=crate::profile::resolve(&svc.profiles,&task.profile)
                        .is_some_and(|p|p.hooks==crate::profile::Hooks::Codex);
                    if codex && !task.tool_exited && matches!(task.state,AgentState::Running|AgentState::Waiting|AgentState::Idle) {
                        if let Err(error)=svc.refresh_binding(&task,false).await {
                            tracing::debug!(?error,agent=%task.id,"Codex lifecycle metadata pending");
                        }
                    }
                }
            }
        });
        Ok(())
    }
    pub async fn shutdown(&self) {
        self.stop.cancel();
        self.background.close();
        self.background.wait().await;
    }
}
