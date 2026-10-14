//! A provisional instruction precedes synchronous hooks; completion compensates only its own epoch.
use crate::{
    actions::open,
    model::*,
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState},
    store,
};
use devbox_protocol::{Fail, Internal};
use devbox_terminal::tmux::PasteDelivery;
fn after_delivery_failure(id: &str, error: Fail<AgentsError>) -> Fail<AgentsError> {
    // Keep the original diagnostic without logging the instruction body.
    tracing::warn!(agent_id = id, error = ?error, "agent instruction delivery needs verification");
    AgentsError::DeliveryUnknown.into()
}
struct Reservation {
    instruction: String,
    launch: Option<String>,
    epoch: u64,
    before: AgentTask,
}
impl AgentsService {
    pub async fn send(&self, p: SendToAgent) -> Result<AgentTask> {
        crate::limits::input(&p.text)?;
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        if task.tool_exited
            || !matches!(
                self.tool_phase(&task).await?,
                Some(crate::tool_lifecycle::Phase::Tool)
            )
        {
            return Err(AgentsError::BadState {
                state: "tool-ended".into(),
            }
            .into());
        }
        let session = task
            .terminal_id
            .as_deref()
            .ok_or(AgentsError::SessionMissing)?;
        if !self
            .terminal
            .tmux()
            .list()
            .await
            .internal("list agent sessions")?
            .iter()
            .any(|s| s.name == session)
        {
            return Err(AgentsError::SessionMissing.into());
        }
        let reservation = self.reserve_instruction(&p, &task).await?;
        match self
            .terminal
            .tmux()
            .paste_with_delivery(session, &p.text, true)
            .await
        {
            Ok(()) => self
                .finish_instruction(&p.id, reservation, Some(InstructionDelivery::Delivered))
                .await
                .map_err(|error| after_delivery_failure(&p.id, error))?,
            Err(failure) => {
                let unsent = failure.delivery == PasteDelivery::Unsent;
                let completion = self
                    .finish_instruction(
                        &p.id,
                        reservation,
                        if unsent {
                            None
                        } else {
                            Some(InstructionDelivery::Unknown)
                        },
                    )
                    .await;
                if !unsent {
                    completion.map_err(|error| after_delivery_failure(&p.id, error))?;
                    return Err(AgentsError::DeliveryUnknown.into());
                }
                completion?;
                return Err(Fail::Internal(format!(
                    "send agent instruction before delivery: {:?}",
                    failure.error
                )));
            }
        }
        self.get(&p.id)
            .await
            .map_err(|error| after_delivery_failure(&p.id, error))
    }
    async fn reserve_instruction(
        &self,
        p: &SendToAgent,
        checked: &AgentTask,
    ) -> Result<Reservation> {
        let id = p.id.clone();
        let text = p.text.clone();
        let terminal = checked.terminal_id.clone();
        let instruction = uuid::Uuid::new_v4().simple().to_string();
        let hub = self.hub.clone();
        let (reservation, _) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(AgentsError::NotFound { id }));
                    };
                    if task.tool_exited
                        || task.terminal_id != terminal
                        || matches!(task.state, AgentState::Closed | AgentState::Preparing)
                    {
                        return Ok(Err(AgentsError::BadState {
                            state: "changed".into(),
                        }));
                    }
                    let launch = store::binding(tx, &id)?.and_then(|binding| binding.launch_id);
                    let before = task.clone();
                    task.follow_ups.push(FollowUp {
                        at_ms: now_ms(),
                        text,
                        instruction_id: Some(instruction.clone()),
                        delivery: Some(InstructionDelivery::Pending),
                    });
                    apply_event(&mut task, &AgentEvent::UserInstructed, now_ms());
                    if let Err(error) =
                        crate::limits::serialized(&task, crate::limits::TASK_BYTES - 4096)
                    {
                        return Ok(Err(match error {
                            Fail::Domain(error) => error,
                            Fail::Internal(_) => AgentsError::InvalidInput,
                        }));
                    }
                    let reservation = Reservation {
                        instruction,
                        launch,
                        epoch: task.activity_seq,
                        before,
                    };
                    store::put(tx, &task)?;
                    Ok(Ok((reservation, task)))
                },
                move |(_, task), rev| changed(&hub, task, rev, false),
            )
            .await
            .internal("reserve agent instruction")?
            .map_err(Fail::Domain)?
            .0;
        Ok(reservation)
    }
    async fn finish_instruction(
        &self,
        id: &str,
        reservation: Reservation,
        delivery: Option<InstructionDelivery>,
    ) -> Result<()> {
        let id = id.to_owned();
        let hub = self.hub.clone();
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    let launch = store::binding(tx, &id)?.and_then(|binding| binding.launch_id);
                    let previous = task.state;
                    if !finish_row(&mut task, &reservation, &launch, delivery) {
                        return Ok(Err(()));
                    }
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .internal("finish agent instruction")?;
        Ok(())
    }
}

fn finish_row(
    task: &mut AgentTask,
    reservation: &Reservation,
    launch: &Option<String>,
    delivery: Option<InstructionDelivery>,
) -> bool {
    if launch != &reservation.launch {
        return false;
    }
    let Some(index) = task
        .follow_ups
        .iter()
        .position(|entry| entry.instruction_id.as_deref() == Some(&reservation.instruction))
    else {
        return false;
    };
    if let Some(delivery) = delivery {
        task.follow_ups[index].delivery = Some(delivery);
    } else {
        task.follow_ups.remove(index);
        if task.activity_seq == reservation.epoch
            && task.terminal_id == reservation.before.terminal_id
            && task.session_id == reservation.before.session_id
        {
            task.state = reservation.before.state;
            task.reason = reservation.before.reason.clone();
            task.attention = reservation.before.attention.clone();
            task.activity_seq = task.activity_seq.wrapping_add(1);
        }
    }
    task.updated_ms = now_ms();
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    fn provisional() -> (AgentTask, Reservation) {
        let mut task: AgentTask = serde_json::from_value(serde_json::json!({
            "id":"owned","projectId":"p","title":"fixture","profile":"fixture","baseBranch":"main",
            "branch":null,"worktreePath":null,"workdir":"/owned","sessionId":"session","terminalId":"terminal",
            "state":"idle","reason":null,"attention":{"kind":"idle","message":"old attention","atMs":1},
            "port":20000,"prompt":"prompt","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
            "createdMs":0,"updatedMs":1,"closedMs":null,"activitySeq":4
        })).expect("fixture task");
        let before = task.clone();
        task.follow_ups.push(FollowUp {
            at_ms: 2,
            text: "instruction".into(),
            instruction_id: Some("owned-instruction".into()),
            delivery: Some(InstructionDelivery::Pending),
        });
        apply_event(&mut task, &AgentEvent::UserInstructed, 2);
        let reservation = Reservation {
            instruction: "owned-instruction".into(),
            launch: Some("launch".into()),
            epoch: task.activity_seq,
            before,
        };
        (task, reservation)
    }
    #[test]
    fn unsent_compensation_preserves_a_newer_permission_hook() {
        let (mut task, reservation) = provisional();
        apply_event(
            &mut task,
            &AgentEvent::NeedsInput {
                message: "new permission".into(),
            },
            3,
        );
        let attention = task.attention.clone();
        let epoch = task.activity_seq;
        assert!(finish_row(
            &mut task,
            &reservation,
            &Some("launch".into()),
            None
        ));
        assert!(task.follow_ups.is_empty());
        assert_eq!(task.state, AgentState::Waiting);
        assert_eq!(task.attention, attention);
        assert_eq!(task.activity_seq, epoch);
    }
    #[test]
    fn completion_never_overwrites_a_new_launch_or_synchronous_completion() {
        let (mut task, reservation) = provisional();
        let before = task.clone();
        assert!(!finish_row(
            &mut task,
            &reservation,
            &Some("new-launch".into()),
            None
        ));
        assert_eq!(task, before);
        apply_event(
            &mut task,
            &AgentEvent::TurnComplete {
                message: Some("synchronous done".into()),
            },
            3,
        );
        assert!(finish_row(
            &mut task,
            &reservation,
            &Some("launch".into()),
            Some(InstructionDelivery::Delivered)
        ));
        assert_eq!(task.state, AgentState::Idle);
        assert_eq!(
            task.attention.as_ref().expect("attention").message,
            "synchronous done"
        );
        assert_eq!(
            task.follow_ups[0].delivery,
            Some(InstructionDelivery::Delivered)
        );
    }
    #[test]
    fn unchanged_unsent_epoch_restores_attention_with_a_monotonic_epoch() {
        let (mut task, reservation) = provisional();
        assert!(finish_row(
            &mut task,
            &reservation,
            &Some("launch".into()),
            None
        ));
        assert_eq!(task.state, reservation.before.state);
        assert_eq!(task.attention, reservation.before.attention);
        assert!(task.activity_seq > reservation.epoch);
    }
}
