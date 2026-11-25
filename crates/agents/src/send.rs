//! A provisional instruction precedes synchronous hooks; completion compensates only its own epoch.
use crate::{
    actions::open,
    first_instruction,
    model::*,
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState},
    store,
    tracker::Binding,
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
    /// The reserve cleared the launch's ready marker (`Binding::resumed_session`).
    cleared_marker: bool,
    /// The instruction is the task's queued first one (`first_instruction`), not a follow-up.
    first: bool,
}
impl Reservation {
    /// Whether nothing reached `task` since the reserve: no hook, turn or launch moved it on.
    fn untouched(&self, task: &AgentTask) -> bool {
        task.activity_seq == self.epoch
            && task.terminal_id == self.before.terminal_id
            && task.session_id == self.before.session_id
    }
}
impl AgentsService {
    pub async fn send(&self, p: SendToAgent) -> Result<AgentTask> {
        crate::limits::input(&p.text)?;
        // Before the task, its pane or tmux is touched: the app and MCP alike.
        crate::limits::pasted(&p.text)?;
        let _guard = self.actions.lock().await;
        self.send_guarded(&p, None).await
    }
    /// The guarded path of every paste into a task's pane, under the action lock. With `first`,
    /// the text is the task's queued first instruction and `first` is the watcher's pass that
    /// saw the tool ready for it (`first_instruction`): the same checks, and stricter ones.
    pub(crate) async fn send_guarded(
        &self,
        p: &SendToAgent,
        first: Option<&crate::watch::Snapshot>,
    ) -> Result<AgentTask> {
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
        // Until pi reads its input, Enter could answer its folder-trust question, which is not
        // on screen yet for the pane check below to see.
        self.check_ready(&task, now_ms()).await?;
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
        // The stored state below only turns to Waiting once the watcher has seen the prompt sit
        // still. Read the pane now, so Enter cannot answer a prompt that is up but not yet
        // reported (Codex's trust prompt rests on "Trust and continue"). Nothing is reserved yet.
        // A first instruction goes in only at the empty prompt box the watcher saw, read again.
        let blocked = match first {
            Some(_) => !self.shows_ready_prompt(&task, session).await?,
            None => self.pane_is_waiting(&task, session).await?,
        };
        if blocked {
            return Err(AgentsError::AwaitingInput.into());
        }
        let reservation = self.reserve_instruction(p, &task, first.cloned()).await?;
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
        first: Option<crate::watch::Snapshot>,
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
                    // The last state read before the paste, in the single writer: pasted
                    // text and Enter never answer an approval or question committed first.
                    if task.state == AgentState::Waiting {
                        return Ok(Err(AgentsError::AwaitingInput));
                    }
                    let binding = store::binding(tx, &id)?;
                    // Still queued, on the launch and at the moment the watcher judged: a hook,
                    // a send or a resume since then ends it here.
                    if first.as_ref().is_some_and(|seen| {
                        !seen.current(&task, &binding)
                            || !first_instruction::is(&task, InstructionDelivery::Queued)
                            || !binding
                                .as_ref()
                                .is_some_and(|b| b.awaits_first_instruction(&task))
                    }) {
                        return Ok(Err(AgentsError::BadState {
                            state: "changed".into(),
                        }));
                    }
                    let launch = binding
                        .as_ref()
                        .and_then(|binding| binding.launch_id.clone());
                    let before = task.clone();
                    if first.is_some() {
                        first_instruction::set(&mut task, InstructionDelivery::Pending, now_ms());
                    } else {
                        task.follow_ups.push(FollowUp {
                            at_ms: now_ms(),
                            text,
                            instruction_id: Some(instruction.clone()),
                            delivery: Some(InstructionDelivery::Pending),
                        });
                        // The user's own instruction takes the queued one's place.
                        first_instruction::cancel(&mut task, now_ms());
                    }
                    apply_event(&mut task, &AgentEvent::UserInstructed, now_ms());
                    if let Err(error) =
                        crate::limits::serialized(&task, crate::limits::TASK_BYTES - 4096)
                    {
                        return Ok(Err(match error {
                            Fail::Domain(error) => error,
                            Fail::Internal(_) => AgentsError::InvalidInput,
                        }));
                    }
                    // A resumed launch gets its first instruction: the tool's turn says when it
                    // waits again, not its prompt.
                    let cleared_marker = match binding.filter(|binding| binding.resumed_session) {
                        Some(mut binding) => {
                            binding.resumed_session = false;
                            store::put_binding(tx, &id, &binding)?;
                            true
                        }
                        None => false,
                    };
                    let reservation = Reservation {
                        instruction,
                        launch,
                        epoch: task.activity_seq,
                        before,
                        cleared_marker,
                        first: first.is_some(),
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
        // For the log: a send that got through took the queued first instruction's place, and a
        // send of the first instruction itself that the task outran leaves it unsent.
        let queued = reservation.first || first_instruction::is_queued(&reservation.before);
        let why = match reservation.first {
            true => "the task changed during the send",
            false => "an instruction was sent",
        };
        let _ = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    let binding = store::binding(tx, &id)?;
                    let launch = binding.as_ref().and_then(|b| b.launch_id.clone());
                    let previous = task.state;
                    let undone = delivery.is_none() && reservation.untouched(&task);
                    if !finish_row(&mut task, &reservation, &launch, delivery) {
                        return Ok(Err(()));
                    }
                    if let Some(binding) = restore_marker(&reservation, undone, binding) {
                        store::put_binding(tx, &id, &binding)?;
                    }
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous)))
                },
                move |(task, previous), rev| {
                    first_instruction::log_cancelled(queued, task, why);
                    changed(&hub, task, rev, task.state != *previous)
                },
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
    if reservation.first {
        if !first_instruction::is(task, InstructionDelivery::Pending) {
            return false;
        }
        // Nothing left devbox: the instruction is queued again, for the watcher's next pass,
        // unless a hook or a launch moved the task on meanwhile.
        let delivery = delivery.unwrap_or(if reservation.untouched(task) {
            InstructionDelivery::Queued
        } else {
            InstructionDelivery::NotSent
        });
        first_instruction::set(task, delivery, now_ms());
        task.updated_ms = now_ms();
        return true;
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
        if reservation.untouched(task) {
            task.state = reservation.before.state;
            task.reason = reservation.before.reason.clone();
            task.attention = reservation.before.attention.clone();
            task.first_instruction = reservation.before.first_instruction;
            task.activity_seq = task.activity_seq.wrapping_add(1);
        }
    }
    task.updated_ms = now_ms();
    true
}
/// The binding to store once an unsent instruction is `undone` (`finish_row` restored the task):
/// the ready marker its reserve cleared goes back on while the launch is the reserve's and began no
/// turn. The tool never got the instruction, so it still waits at its prompt for its first one.
fn restore_marker(
    reservation: &Reservation,
    undone: bool,
    binding: Option<Binding>,
) -> Option<Binding> {
    let mut binding = binding.filter(|binding| {
        undone
            && reservation.cleared_marker
            && binding.launch_id == reservation.launch
            && binding.turn_id.is_none()
    })?;
    binding.resumed_session = true;
    Some(binding)
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
            cleared_marker: false,
            first: false,
        };
        (task, reservation)
    }
    #[test]
    fn an_undone_instruction_restores_only_the_marker_its_reserve_cleared() {
        let (_, mut reservation) = provisional();
        reservation.cleared_marker = true;
        let mut binding = Binding::new(1);
        binding.launch_id = Some("launch".into());
        let restored = restore_marker(&reservation, true, Some(binding.clone()));
        assert!(restored.expect("restored").resumed_session);
        // A hook, a turn or a launch moved the task on since the reserve.
        assert_eq!(
            restore_marker(&reservation, false, Some(binding.clone())),
            None
        );
        // The launch began a turn, or is not the reserve's.
        let mut turned = binding.clone();
        turned.turn_id = Some("turn".into());
        assert_eq!(restore_marker(&reservation, true, Some(turned)), None);
        let mut other = binding.clone();
        other.launch_id = Some("new-launch".into());
        assert_eq!(restore_marker(&reservation, true, Some(other)), None);
        assert_eq!(restore_marker(&reservation, true, None), None);
        // A send that cleared no marker never sets one.
        reservation.cleared_marker = false;
        assert_eq!(restore_marker(&reservation, true, Some(binding)), None);
    }
    #[test]
    fn a_first_instruction_is_finished_on_the_task_and_queued_again_only_when_nothing_moved() {
        let reserved = || {
            let (mut task, mut reservation) = provisional();
            task.follow_ups.clear();
            first_instruction::set(&mut task, InstructionDelivery::Pending, 2);
            reservation.first = true;
            (task, reservation)
        };
        let launch = Some("launch".to_owned());
        for outcome in [InstructionDelivery::Delivered, InstructionDelivery::Unknown] {
            let (mut task, reservation) = reserved();
            assert!(finish_row(&mut task, &reservation, &launch, Some(outcome)));
            assert!(first_instruction::is(&task, outcome));
            assert_eq!(task.follow_ups, []);
        }
        // Nothing left devbox and nothing touched the task: the watcher tries again.
        let (mut task, reservation) = reserved();
        assert!(finish_row(&mut task, &reservation, &launch, None));
        assert!(first_instruction::is(&task, InstructionDelivery::Queued));
        // A hook moved the task on meanwhile: agy has an instruction, so this one never goes.
        let (mut task, reservation) = reserved();
        apply_event(&mut task, &AgentEvent::PromptSubmitted, 3);
        assert!(finish_row(&mut task, &reservation, &launch, None));
        assert!(first_instruction::is(&task, InstructionDelivery::NotSent));
        // Another launch, or an instruction no longer being sent, is left as it is.
        let (mut task, reservation) = reserved();
        let before = task.clone();
        let other = Some("new-launch".to_owned());
        assert!(!finish_row(&mut task, &reservation, &other, None));
        first_instruction::set(&mut task, InstructionDelivery::Unknown, 2);
        let settled = task.clone();
        let delivered = Some(InstructionDelivery::Delivered);
        assert!(!finish_row(&mut task, &reservation, &launch, delivered));
        assert_eq!(task, settled);
        assert_ne!(settled, before);
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
