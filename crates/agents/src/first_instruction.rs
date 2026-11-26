//! The first instruction of a tool that takes no prompt argument (Antigravity). devbox holds
//! the task's prompt as queued (`AgentTask::first_instruction`) and sends it once, through the
//! guarded path of every send (`send::send_guarded`), when the output watcher sees the tool's
//! empty prompt box sit still on a launch that still waits for its first instruction
//! (`Binding::awaits_first_instruction`).
//!
//! It is pasted without the blank lines before it and the blanks after it ([`to_paste`]) and
//! without Enter, and Enter goes in only once agy's box starts with the pasted text, or with the
//! placeholder agy draws for a paste too large to show, of the text's count
//! (`enter_first_instruction`). Left without Enter, the launch waits for an instruction as before
//! the paste ([`Entry`]).
//!
//! `Queued` → `Pending` (reserved, being pasted) → `Delivered`, or `Unknown` when the paste may
//! or may not have arrived or got no Enter, or back to `Queued` when nothing left devbox.
//! `Queued` → `NotSent` when the tool gets another instruction first (a hook of the launch, a
//! send), when the tool ends or the task closes, and on a resume. The watcher also cancels it
//! when the pane shows a turn in progress or a prompt already echoed above the box: agy got one
//! whose hooks were lost.
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
    antigravity::{self, Pasted},
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
use std::time::Duration;

/// How long agy may take to draw a pasted first instruction in its box, and how often the pane is
/// read again meanwhile.
const DRAWN_WITHIN: Duration = Duration::from_secs(2);
const DRAWN_POLL: Duration = Duration::from_millis(50);

/// The notice of a task whose first instruction devbox left in agy's box without Enter, where the
/// last read of the pane showed the box starting with it ([`Entry::AtPrompt`]).
pub(crate) const LEFT_IN_BOX: &str = "처음 지시를 입력창에 붙여 넣었지만 Enter는 누르지 않았습니다. 터미널에서 확인한 뒤 보내거나 지우세요";
/// The notice of one pasted without Enter where the last read showed agy at rest without the box
/// starting with it: an empty box, or something else in front of it.
pub(crate) const NOT_SEEN: &str = "처음 지시를 붙여 넣었지만 입력창에서 확인하지 못해 Enter는 누르지 않았습니다. 터미널에서 입력창을 확인하세요";

/// What came of a first instruction devbox pasted without Enter
/// ([`AgentsService::enter_first_instruction`]). Without Enter the tool began no turn of it, so
/// where nothing reached the task meanwhile its launch waits for an instruction as before the
/// paste (`send::finish_row`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// Enter went in: delivered.
    Entered,
    /// No Enter, and the last read of the pane showed agy's prompt box with nothing asked or
    /// running. `ours`: the box started with the prompt, and Enter was held back only because a
    /// prompt agy took earlier is on screen. The launch's wait ends as at an empty box: the task is
    /// idle, with [`LEFT_IN_BOX`], or [`NOT_SEEN`] when the box did not start with the prompt.
    AtPrompt { ours: bool },
    /// No Enter, and the last read showed a question or a turn, no box, or the pane could not be
    /// read. The launch waits as before, and the output watcher judges its pane.
    Elsewhere,
    /// Enter was sent and tmux failed: it may or may not have gone in.
    Unconfirmed,
}
impl Entry {
    pub(crate) fn delivery(self) -> InstructionDelivery {
        match self {
            Self::Entered => InstructionDelivery::Delivered,
            Self::AtPrompt { .. } | Self::Elsewhere | Self::Unconfirmed => {
                InstructionDelivery::Unknown
            }
        }
    }
}

/// What devbox pastes for the first instruction `prompt`: the prompt from its first line that is
/// not blank, with that line's own indentation (an indented block of code stays aligned) but
/// without a byte-order mark in front of it, and without the blanks and line breaks at its end.
/// A paste that would end with a backslash gets a space after it: some tools read a backslash
/// and then Enter as a line break, and Enter would not send it. agy's box then starts with the
/// first line, and the count of a paste agy shows as a placeholder is that of this text
/// (`antigravity::pasted`).
pub(crate) fn to_paste(prompt: &str) -> String {
    let blank = |c: char| c.is_whitespace() || c == '\u{feff}';
    let start = prompt
        .find(|c: char| !blank(c))
        .map_or(prompt.len(), |first| {
            prompt[..first].rfind(['\n', '\r']).map_or(0, |at| at + 1)
        });
    let mut text = prompt[start..]
        .trim_start_matches('\u{feff}')
        .trim_end()
        .to_owned();
    if text.ends_with('\\') {
        text.push(' ');
    }
    text
}

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
    /// Notes that this daemon process makes the task `id`, before anything of it is written: the
    /// restart rule never holds it to a first sight of its pane (`settle_first_instructions`).
    pub(crate) fn note_created(&self, id: &str) {
        self.created
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_owned());
    }

    /// Whether this daemon process made the task `id` ([`Self::note_created`]).
    pub(crate) fn made_here(&self, id: &str) -> bool {
        self.created
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(id)
    }

    /// Sends the queued first instruction of the task the output watcher saw at its empty prompt
    /// box in `seen`. The send reads the pane again and checks the task again in its writer, so
    /// anything that changed since that pass ends it with nothing pasted.
    pub(crate) async fn send_first_instruction(&self, seen: Snapshot) -> Result<()> {
        let send = SendToAgent {
            id: seen.task.id.clone(),
            text: to_paste(&seen.task.prompt),
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
            && antigravity::shows_prompt(&tail)
            && !antigravity::shows_earlier_prompt(&tail))
    }

    /// After `prompt`, the first instruction of `task`, was pasted into the pane of `session`
    /// without Enter. The paste follows the user's answer to agy's trust question in that pane,
    /// where a key typed before it landed (a `!`) would make Enter run the box as a shell command.
    /// So Enter goes in only once the pane shows agy's box starting with the prompt, or with agy's
    /// placeholder of it (`antigravity::pasted`), with no waiting pattern, turn or earlier prompt
    /// on screen. A first instruction left without Enter is never sent again: the user sees it in
    /// the box and decides, told whether the last read of the pane showed it there.
    pub(crate) async fn enter_first_instruction(
        &self,
        task: &AgentTask,
        session: &str,
        prompt: &str,
    ) -> Entry {
        if let Err(left) = self.shows_pasted(task, session, prompt).await {
            tracing::info!(agent = %task.id, ?left, "first instruction pasted, Enter not pressed");
            return left;
        }
        match self.terminal.tmux().press_enter(session).await {
            Ok(()) => Entry::Entered,
            Err(error) => {
                tracing::warn!(agent = %task.id, ?error, "first instruction pasted, Enter not confirmed");
                Entry::Unconfirmed
            }
        }
    }

    /// Whether the pane of `session` shows the prompt box starting with `prompt`, or with agy's
    /// placeholder of it, and nothing a send waits for, or else what its last read showed
    /// ([`Entry::AtPrompt`] or [`Entry::Elsewhere`]). agy takes a moment to draw a paste: while no
    /// box is drawn, or the box shows less of the prompt than `antigravity::pasted` needs, the
    /// pane is read again, for at most [`DRAWN_WITHIN`]. A prompt agy took earlier, on screen,
    /// ends it at once.
    async fn shows_pasted(
        &self,
        task: &AgentTask,
        session: &str,
        prompt: &str,
    ) -> std::result::Result<(), Entry> {
        let Some(patterns) = self
            .waiting_patterns
            .get()
            .and_then(|all| all.get(&task.profile))
        else {
            return Err(Entry::Elsewhere);
        };
        let deadline = tokio::time::Instant::now() + DRAWN_WITHIN;
        loop {
            let tmux = self.terminal.tmux();
            let Ok(tail) = tmux.capture_tail(session, crate::watch::TAIL_ROWS).await else {
                return Err(Entry::Elsewhere);
            };
            if crate::watch::matches_waiting(patterns, &tail) || antigravity::shows_turn(&tail) {
                return Err(Entry::Elsewhere);
            }
            let earlier = antigravity::shows_earlier_prompt(&tail);
            match antigravity::pasted(&tail, prompt) {
                Pasted::Leads if !earlier => return Ok(()),
                Pasted::Leads => return Err(Entry::AtPrompt { ours: true }),
                Pasted::Drawing | Pasted::NoBox
                    if !earlier && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(DRAWN_POLL).await;
                }
                Pasted::NoBox => return Err(Entry::Elsewhere),
                Pasted::Drawing | Pasted::Other => return Err(Entry::AtPrompt { ours: false }),
            }
        }
    }

    /// At the daemon's start, before its watcher reads a pane: the watcher reads none until this
    /// has succeeded, and tries it again on each pass (`Watcher::settled`).
    ///
    /// A first instruction still queued for a tool that has started is noted for the restart rule
    /// (`first_sight`). All are noted before anything is written, so a write that fails leaves
    /// none of them out. The start's reconciliation ran before this and made `Running` every task
    /// whose launch says its tool started, so one still `Preparing` here has no tool yet. And a
    /// task this daemon process made is one it saw from the start, however late this runs (a
    /// start that failed once is tried again while the daemon serves): it is told by the
    /// process's own note of it ([`Self::note_created`]), never by the clock, which a WSL clock
    /// step could move.
    ///
    /// A first instruction left as being pasted is one whose outcome the daemon before never
    /// recorded: it may have arrived, so it is not sent again.
    pub(crate) async fn settle_first_instructions(&self) -> Result<()> {
        let tasks = self.all_tasks(None).await?;
        let noted = tasks.iter().filter(|task| {
            is(task, InstructionDelivery::Queued)
                && task.state != AgentState::Preparing
                && !self.made_here(&task.id)
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
