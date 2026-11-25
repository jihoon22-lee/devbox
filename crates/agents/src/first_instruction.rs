//! The first instruction of a tool that takes no prompt argument (Antigravity). devbox holds
//! the task's prompt as queued (`AgentTask::first_instruction`) and sends it once, through the
//! guarded path of every send (`send::send_guarded`), when the output watcher sees the tool's
//! empty prompt box sit still on a launch that still waits for its first instruction
//! (`Binding::awaits_first_instruction`).
//!
//! `Queued` → `Pending` (reserved, being pasted) → `Delivered`, or `Unknown` when the paste may
//! or may not have arrived, or back to `Queued` when nothing left devbox. `Queued` → `NotSent`
//! when the tool gets another instruction first (a hook of the launch, a send), when the tool
//! ends or the task closes, and on a resume. The watcher also cancels it when the pane shows a
//! turn in progress or a prompt already echoed above the box: agy got one whose hooks were lost.
//!
//! Only `Queued` is ever sent, so a daemon that restarts does not send one twice: a `Pending` it
//! finds is one whose outcome was never recorded, and becomes `Unknown`. And it does not send one
//! after a prompt it never saw. Whatever happened in the pane while no daemon ran left no hook
//! and no screen to judge, so an instruction still `Queued` at the daemon's start stays queued
//! only if the watcher's first sight of the pane is a screen agy draws before it takes any
//! prompt (its trust question, its sign-in choice). Any other first sight, or none, makes it
//! `NotSent` with the reason `Unobserved` (`AgentsService::first_sight`). A task still
//! `Preparing` at the start is not held to this: its tool had not started.
use crate::{
    model::{
        AgentTask, AgentsError, FirstInstruction, FirstInstructionReason, InstructionDelivery,
        SendToAgent,
    },
    profile::{Hooks, ToolProfile},
    service::{AgentsService, Result, now_ms},
    state::AgentState,
    watch::Snapshot,
};
use devbox_protocol::Internal;

/// Whether `profile`'s tool gets its first instruction from devbox instead of an argument.
fn sends_first(profile: &ToolProfile) -> bool {
    profile.hooks == Hooks::Antigravity
}

/// A prompt devbox will paste is checked before anything is reserved. Like every pasted text it
/// may hold no control character, which would act as a key in the tool's terminal. And it may
/// not start with `!` or `/`: the tool would run it as a shell or tool command, where the other
/// tools get the same prompt as an argument and read it as an instruction. Leading blanks and
/// line breaks do not hide the first character, as for an instruction sent over MCP.
pub(crate) fn admit(profile: &ToolProfile, prompt: &str) -> Result<()> {
    if !sends_first(profile) {
        return Ok(());
    }
    crate::limits::pasted(prompt)?;
    let text = prompt.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if text.starts_with(['!', '/']) {
        return Err(AgentsError::FirstInstructionCommand {
            tool: profile.label.clone(),
        }
        .into());
    }
    Ok(())
}

/// What a new task of `profile` records for `prompt`: queued, when devbox sends it and there is
/// one to send.
pub(crate) fn queued(profile: &ToolProfile, prompt: &str, now: i64) -> Option<FirstInstruction> {
    (sends_first(profile) && !prompt.trim().is_empty()).then_some(FirstInstruction {
        delivery: InstructionDelivery::Queued,
        at_ms: now,
        reason: None,
    })
}

pub(crate) fn is(task: &AgentTask, delivery: InstructionDelivery) -> bool {
    task.first_instruction
        .is_some_and(|first| first.delivery == delivery)
}

pub(crate) fn is_queued(task: &AgentTask) -> bool {
    is(task, InstructionDelivery::Queued)
}

pub(crate) fn set(task: &mut AgentTask, delivery: InstructionDelivery, now: i64) {
    task.first_instruction = Some(FirstInstruction {
        delivery,
        at_ms: now,
        reason: None,
    });
}

/// Why a queued first instruction was cancelled, for the log: the tool ended or the task closed.
const ENDED: &str = "the tool ended or the task closed";

/// A queued first instruction will not be sent. Says whether there was one. The write that holds
/// this says so in the log once it has committed ([`log_cancelled`]).
pub(crate) fn cancel(task: &mut AgentTask, now: i64) -> bool {
    let queued = is(task, InstructionDelivery::Queued);
    if queued {
        set(task, InstructionDelivery::NotSent, now);
    }
    queued
}

/// For the log, once a write has committed: the first instruction that `was_queued` before it
/// will not be sent, as `task` now says, because of `why`. The line names the task and never the
/// instruction. A write that fails, or one whose cancel a later write takes back before it is
/// final (a send that never left devbox), logs nothing.
pub(crate) fn log_cancelled(was_queued: bool, task: &AgentTask, why: &'static str) {
    if was_queued && is(task, InstructionDelivery::NotSent) {
        tracing::info!(agent = %task.id, why, "first instruction cancelled");
    }
}

/// [`log_cancelled`] for a write that ended the tool or closed the task.
pub(crate) fn log_ended(was_queued: bool, task: &AgentTask) {
    log_cancelled(was_queued, task, ENDED);
}

/// [`log_cancelled`]'s `why` for a write of `AgentsService::update_bound`, read from what the
/// write left: the restart rule, an ended tool or a closed task, a `hook` of the launch, or else
/// the watcher's sight of a turn or an earlier prompt in the pane.
pub(crate) fn why(task: &AgentTask, hook: bool) -> &'static str {
    let unobserved = Some(FirstInstructionReason::Unobserved);
    if task.first_instruction.and_then(|first| first.reason) == unobserved {
        "the daemon was down and the pane may hold a prompt"
    } else if task.tool_exited || task.state == AgentState::Closed {
        ENDED
    } else if hook {
        "a hook of the launch arrived"
    } else {
        "the pane shows a turn or an earlier prompt"
    }
}

/// [`cancel`] for the restart rule, which the task's own record does not explain: the reason is
/// kept with it.
fn cancel_unobserved(task: &mut AgentTask, now: i64) -> bool {
    let queued = cancel(task, now);
    if let (true, Some(first)) = (queued, &mut task.first_instruction) {
        first.reason = Some(FirstInstructionReason::Unobserved);
    }
    queued
}

impl AgentsService {
    /// Sends the queued first instruction of the task the output watcher saw at its empty prompt
    /// box in `seen`. The send reads the pane again and checks the task again in its writer, so
    /// anything that changed since that pass ends it with nothing pasted.
    pub(crate) async fn send_first_instruction(&self, seen: Snapshot) -> Result<()> {
        let send = SendToAgent {
            id: seen.task.id.clone(),
            text: seen.task.prompt.clone(),
        };
        crate::limits::input(&send.text)?;
        crate::limits::pasted(&send.text)?;
        // The watcher reads every task's pane in turn: it never waits for an action on another
        // task to end. The instruction stays queued, and the next pass tries again.
        let Ok(_guard) = self.actions.try_lock() else {
            return Ok(());
        };
        let sent = self.send_guarded(&send, Some(&seen)).await;
        let launch = seen.binding.as_ref().and_then(|b| b.launch_id.as_deref());
        let outcome = match &sent {
            Ok(_) => "sent",
            Err(devbox_protocol::Fail::Domain(AgentsError::DeliveryUnknown)) => "unknown",
            Err(_) => "not sent",
        };
        tracing::info!(agent = %send.id, launch = launch.unwrap_or(""), outcome, "first instruction");
        sent.map(|_| ())
    }

    /// The watcher saw, on a launch whose first instruction is still queued, that agy got
    /// another one first, and its hooks did not reach the daemon. As when they do, the queued one
    /// is never sent.
    pub(crate) async fn first_instruction_superseded(&self, id: &str) -> Result<()> {
        self.update(id, |task| cancel(task, now_ms())).await?;
        Ok(())
    }

    /// The restart rule, at the watcher's first sight of the pane of task `id` since this daemon
    /// started: `before_any_prompt` when the pane was read and shows a screen agy draws before
    /// it takes any prompt. Says whether the task's first instruction may still be sent. One
    /// that was queued when the daemon started is cancelled at any other first sight; later
    /// sights, and tasks the daemon saw from their tool's start, are not judged.
    pub(crate) async fn first_sight(&self, id: &str, before_any_prompt: bool) -> Result<bool> {
        let noted = || {
            self.unobserved
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        };
        if !noted().contains(id) {
            return Ok(true);
        }
        if !before_any_prompt {
            // The note is dropped only once the task says so: a write that fails is judged again.
            self.update(id, |task| cancel_unobserved(task, now_ms()))
                .await?;
        }
        noted().remove(id);
        Ok(before_any_prompt)
    }

    /// Whether the pane of `session` shows, right now, the tool's empty prompt box and nothing
    /// a send must wait for: what the watcher saw, read again where the paste is made.
    pub(crate) async fn shows_ready_prompt(&self, task: &AgentTask, session: &str) -> Result<bool> {
        let Some(patterns) = self
            .waiting_patterns
            .get()
            .and_then(|all| all.get(&task.profile))
        else {
            return Ok(false);
        };
        let tail = self
            .terminal
            .tmux()
            .capture_tail(session, crate::watch::TAIL_ROWS)
            .await
            .internal("capture agent pane before first instruction")?;
        Ok(!crate::watch::matches_waiting(patterns, &tail)
            && crate::antigravity::shows_prompt(&tail)
            && !crate::antigravity::shows_earlier_prompt(&tail))
    }

    /// At the daemon's start, before its watcher reads a pane: the watcher reads none until this
    /// has succeeded, and tries it again on each pass (`Watcher::settled`).
    ///
    /// A first instruction still queued for a tool that has started is noted for the restart rule
    /// (`first_sight`). All are noted before anything is written, so a write that fails leaves
    /// none of them out. The start's reconciliation ran before this and made `Running` every task
    /// whose launch says its tool started, so one still `Preparing` here has no tool yet. And a
    /// task made after this daemon started is one it saw from the start, however late this runs
    /// (a start that failed once is tried again while the daemon serves).
    ///
    /// A first instruction left as being pasted is one whose outcome the daemon before never
    /// recorded: it may have arrived, so it is not sent again.
    pub(crate) async fn settle_first_instructions(&self) -> Result<()> {
        let tasks = self.all_tasks(None).await?;
        let noted = tasks.iter().filter(|task| {
            is(task, InstructionDelivery::Queued)
                && task.state != AgentState::Preparing
                && task.created_ms < self.started_ms
        });
        self.unobserved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(noted.map(|task| task.id.clone()));
        for task in tasks {
            if !is(&task, InstructionDelivery::Pending) {
                continue;
            }
            let id = task.id;
            self.update(&id, |task| {
                let pending = is(task, InstructionDelivery::Pending);
                if pending {
                    set(task, InstructionDelivery::Unknown, now_ms());
                }
                pending
            })
            .await?;
            tracing::info!(agent = %id, outcome = "unknown", "first instruction");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        service::apply_event,
        state::{AgentEvent, CloseHow},
    };
    use devbox_protocol::Fail;

    fn task(first: Option<InstructionDelivery>) -> AgentTask {
        let mut task: AgentTask = serde_json::from_value(serde_json::json!({
            "id":"a","projectId":"p","title":"t","profile":"antigravity","baseBranch":"main",
            "branch":"agent/fix","worktreePath":"/w/fix","workdir":"/w/fix","sessionId":null,
            "terminalId":"term","state":"running","reason":null,"attention":null,"port":20000,
            "prompt":"owned prompt","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
            "createdMs":0,"updatedMs":0,"closedMs":null
        }))
        .expect("owned task");
        task.first_instruction = first.map(|delivery| FirstInstruction {
            delivery,
            at_ms: 1,
            reason: None,
        });
        task
    }

    #[test]
    fn only_a_tool_without_a_prompt_argument_queues_a_prompt_that_is_one() {
        let builtin = |id: &str| crate::profile::resolve(&[], id).expect("builtin profile");
        let agy = builtin("antigravity");
        assert_eq!(
            queued(&agy, "fix it", 7),
            Some(FirstInstruction {
                delivery: InstructionDelivery::Queued,
                at_ms: 7,
                reason: None,
            })
        );
        // Nothing to send.
        for empty in ["", " ", "\n\t"] {
            assert_eq!(queued(&agy, empty, 7), None, "{empty:?}");
        }
        // The other tools get the prompt as an argument, as before.
        for id in ["claude", "codex", "opencode", "pi"] {
            assert_eq!(queued(&builtin(id), "fix it", 7), None, "{id}");
            // Their prompt is never pasted, so a control character in it is not refused.
            assert!(admit(&builtin(id), "esc \u{1b}[201~ here").is_ok(), "{id}");
        }
        // A prompt devbox pastes may not start with what the tool runs as a command, whatever
        // blanks, line breaks or byte-order mark come before it.
        for text in [
            "!rm -rf build",
            "/model",
            "  /help",
            "\n\t !ls",
            "\u{feff}/quit",
            "!",
            "/",
        ] {
            assert!(
                matches!(
                    admit(&agy, text),
                    Err(Fail::Domain(AgentsError::FirstInstructionCommand { ref tool }))
                        if tool == "Antigravity"
                ),
                "{text:?}"
            );
            // The other tools get it as an argument and read it as an instruction.
            assert!(admit(&builtin("claude"), text).is_ok(), "{text:?}");
        }
        for text in [
            "fix /src/main.rs",
            "see !important",
            "",
            " ",
            "- /x",
            "1. !x",
        ] {
            assert!(admit(&agy, text).is_ok(), "{text:?}");
        }
        // A prompt devbox will paste follows the rule of every pasted text.
        assert!(admit(&agy, "two\nlines\tand a tab").is_ok());
        for text in ["esc \u{1b}[201~ here", "ctrl-u \u{15}", "nul \0"] {
            assert!(
                matches!(
                    admit(&agy, text),
                    Err(Fail::Domain(AgentsError::ControlCharacters))
                ),
                "{text:?}"
            );
        }
    }

    #[test]
    fn only_a_queued_instruction_is_cancelled() {
        let mut queued = task(Some(InstructionDelivery::Queued));
        assert!(cancel(&mut queued, 9));
        assert_eq!(
            queued.first_instruction,
            Some(FirstInstruction {
                delivery: InstructionDelivery::NotSent,
                at_ms: 9,
                reason: None,
            })
        );
        assert!(!cancel(&mut queued, 10));
        for kept in [
            InstructionDelivery::Pending,
            InstructionDelivery::Delivered,
            InstructionDelivery::Unknown,
            InstructionDelivery::NotSent,
        ] {
            let mut other = task(Some(kept));
            let before = other.clone();
            assert!(!cancel(&mut other, 9), "{kept:?}");
            assert_eq!(other, before, "{kept:?}");
        }
        assert!(!cancel(&mut task(None), 9));
    }

    #[test]
    fn a_tool_that_ends_or_a_task_that_closes_leaves_its_queued_instruction_unsent() {
        // The user declined agy's trust question and agy exited, the terminal was lost, or the
        // task was discarded before agy showed its prompt.
        for (event, state) in [
            (AgentEvent::Exited { code: 0 }, AgentState::Review),
            (AgentEvent::Exited { code: 1 }, AgentState::Failed),
            (AgentEvent::SessionLost, AgentState::Failed),
            (
                AgentEvent::Closed {
                    how: CloseHow::Discarded,
                },
                AgentState::Closed,
            ),
        ] {
            let mut ended = task(Some(InstructionDelivery::Queued));
            assert!(apply_event(&mut ended, &event, 5), "{event:?}");
            assert_eq!(ended.state, state, "{event:?}");
            assert!(is(&ended, InstructionDelivery::NotSent), "{event:?}");
            assert_eq!(ended.prompt, "owned prompt");
        }
        // What only moves the task between its live states leaves it queued: the trust screen,
        // and its end.
        let mut waiting = task(Some(InstructionDelivery::Queued));
        for event in [AgentEvent::OutputWaiting, AgentEvent::OutputResumed] {
            assert!(apply_event(&mut waiting, &event, 5));
            assert!(is(&waiting, InstructionDelivery::Queued), "{event:?}");
        }
        // One that was sent stays as it was recorded.
        let mut sent = task(Some(InstructionDelivery::Delivered));
        apply_event(&mut sent, &AgentEvent::Exited { code: 0 }, 5);
        assert!(is(&sent, InstructionDelivery::Delivered));
    }
}
