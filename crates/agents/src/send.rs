//! A provisional instruction precedes synchronous hooks; completion compensates only its own epoch.
use crate::{
    actions::open,
    first_instruction::{self, Entry},
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
        // A first instruction is pasted without Enter, which follows only once the pane shows
        // agy's box starting with it, or with agy's placeholder of it (`enter_first_instruction`).
        let enter = first.is_none();
        match self
            .terminal
            .tmux()
            .paste_with_delivery(session, &p.text, enter)
            .await
        {
            Ok(()) => {
                let entry = match enter {
                    true => Entry::Entered,
                    false => self.enter_first_instruction(&task, session, &p.text).await,
                };
                self.finish_instruction(&p.id, reservation, Some(entry.delivery()), Some(entry))
                    .await
                    .map_err(|error| after_delivery_failure(&p.id, error))?;
                if entry != Entry::Entered {
                    return Err(AgentsError::DeliveryUnknown.into());
                }
            }
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
                        None,
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
                        || task.reason.as_deref() == Some("session_mismatch")
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
    /// Records how the reserved instruction went: `delivery`, or none when nothing left devbox.
    /// `entry` is what came of a first instruction pasted without Enter.
    async fn finish_instruction(
        &self,
        id: &str,
        reservation: Reservation,
        delivery: Option<InstructionDelivery>,
        entry: Option<Entry>,
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
                    // The tool still waits for its first instruction: nothing left devbox, or
                    // devbox pressed no Enter where the pane showed something else than agy at
                    // rest in its box. The watcher then ends the wait at an empty box, as for any
                    // launch that got none.
                    let waits = reservation.untouched(&task)
                        && (delivery.is_none() || entry == Some(Entry::Elsewhere));
                    if !finish_row(&mut task, &reservation, &launch, delivery, entry) {
                        return Ok(Err(()));
                    }
                    if let Some(binding) = restore_marker(&reservation, waits, binding) {
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
    entry: Option<Entry>,
) -> bool {
    if launch != &reservation.launch {
        return false;
    }
    if reservation.first {
        if !first_instruction::is(task, InstructionDelivery::Pending) {
            return false;
        }
        let untouched = reservation.untouched(task);
        // Nothing left devbox: the instruction is queued again, for the watcher's next pass,
        // unless a hook or a launch moved the task on meanwhile.
        let delivery = delivery.unwrap_or(if untouched {
            InstructionDelivery::Queued
        } else {
            InstructionDelivery::NotSent
        });
        first_instruction::set(task, delivery, now_ms());
        // Left without Enter at agy's prompt, and no hook since: agy began no turn, so the launch's
        // wait ends as at an empty box (`complete_codex`), with a notice of what the last read
        // showed in the box. A hook since (every one of agy's moves the task's activity on) leaves
        // its state be.
        if untouched && let Some(Entry::AtPrompt { ours }) = entry {
            let notice = match ours {
                true => first_instruction::LEFT_IN_BOX,
                false => first_instruction::NOT_SEEN,
            };
            let left = AgentEvent::TurnComplete {
                message: Some(notice.into()),
            };
            apply_event(task, &left, now_ms());
        }
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
/// The binding to store once the tool `waits` for its first instruction after all (an unsent
/// instruction `finish_row` restored, or a first one left without Enter): the ready marker its
/// reserve cleared goes back on while the launch is the reserve's and began no turn.
fn restore_marker(
    reservation: &Reservation,
    waits: bool,
    binding: Option<Binding>,
) -> Option<Binding> {
    let mut binding = binding.filter(|binding| {
        waits
            && reservation.cleared_marker
            && binding.launch_id == reservation.launch
            && binding.turn_id.is_none()
    })?;
    binding.resumed_session = true;
    Some(binding)
}
#[cfg(test)]
#[path = "send_tests.rs"]
mod tests;
