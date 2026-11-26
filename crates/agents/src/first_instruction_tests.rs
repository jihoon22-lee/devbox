//! An Antigravity task's first instruction: agy gets no prompt argument, so devbox sends the
//! prompt once, through the guarded send path, when agy shows its empty prompt box. The fake
//! `tmux` of the parent module prints `$live.screen` for `capture-pane`, shows `$live.pasted` once
//! something is pasted, and notes every call.
use super::antigravity_pane::{after_paste, calls, ended, hook, show};
use super::*;
use crate::{model::FirstInstruction, service::apply_event, state::AgentEvent};
use std::time::{Duration, Instant};

const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";
const QUIET: Duration = Duration::from_secs(2);
/// The calls of a send from the task's screen: the pane is read, then the text is loaded, pasted
/// and entered.
const ONE_SEND: [&str; 5] = [
    "list-sessions",
    "capture-pane",
    "load-buffer",
    "paste-buffer",
    "send-keys",
];
/// The calls of the first instruction: the pane is read, the text is loaded and pasted without
/// Enter, the pane is read again, and only then Enter goes in.
const FIRST_SEND: [&str; 6] = [
    "list-sessions",
    "capture-pane",
    "load-buffer",
    "paste-buffer",
    "capture-pane",
    "send-keys",
];
/// The prompt pasted for draft.txt, which recorded how agy's box draws it.
const DRAFT: &str = "Reply with OK and stop.\nThis is line two of the same pasted prompt.\nLine three: also say how many lines this prompt had.";

macro_rules! capture {
    ($name:literal) => {
        include_str!(concat!(
            "../tests/fixtures/antigravity-1.5.17/",
            $name,
            ".txt"
        ))
    };
}

fn payload(name: &str) -> &'static str {
    let fixture = match name {
        "pre" => include_str!("../tests/fixtures/antigravity-1.5.17/hook-pre-invocation.json"),
        "start" => include_str!("../tests/fixtures/antigravity-1.5.17/hook-session-start.json"),
        _ => include_str!("../tests/fixtures/antigravity-1.5.17/hook-stop.json"),
    };
    fixture.split_once("\n---\n").expect("fixture header").1
}

fn pane(fixture: &'static str) -> &'static str {
    fixture.split_once("\n---\n").expect("fixture header").1
}

/// Task `a` on a live launch of agy that waits for its first instruction, as a new task with the
/// prompt [`DRAFT`] is after its tool started: no conversation yet, the prompt queued. A paste
/// leaves its pane as agy drew that prompt in its box (draft.txt). `first` is the delivery state
/// stored for it, or none for a task started without a prompt.
async fn launched(first: Option<InstructionDelivery>) -> Ended {
    let (f, _) = ended(None).await;
    f.svc.resume("a").await.expect("launch");
    f.tool_started().await;
    f.svc
        .update("a", move |task| {
            task.first_instruction = first.map(|delivery| FirstInstruction {
                delivery,
                at_ms: 1,
                reason: None,
            });
            task.prompt = DRAFT.into();
            true
        })
        .await
        .expect("owned first instruction");
    after_paste(capture!("draft"));
    assert!(f.binding().await.resumed_session);
    let _ = fs::remove_file(format!(
        "{}.calls",
        std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake")
    ));
    f
}

/// One pass of the output watcher at `at` over the task's pane, which shows `shown`.
async fn pass(f: &Ended, watcher: &mut crate::watch::Watcher, shown: &'static str, at: Instant) {
    let snapshot = crate::watch::Snapshot {
        task: f.task().await,
        binding: Some(f.binding().await),
    };
    if !crate::watch::eligible(&snapshot.task) {
        return;
    }
    watcher.read(&f.svc, snapshot, pane(shown).into(), at).await;
}

/// Two passes a quiet interval apart over the same pane, which is also what tmux captures.
async fn settle(f: &Ended, watcher: &mut crate::watch::Watcher, shown: &'static str, at: Instant) {
    show(shown);
    pass(f, watcher, shown, at).await;
    pass(f, watcher, shown, at + QUIET).await;
}

fn delivery(task: &AgentTask) -> Option<InstructionDelivery> {
    task.first_instruction.as_ref().map(|first| first.delivery)
}

#[test]
fn the_first_instruction_is_sent_once_when_agy_shows_its_empty_prompt_box() {
    with_fake_tmux(
        "first_instruction::the_first_instruction_is_sent_once_when_agy_shows_its_empty_prompt_box",
        || async {
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            // The trust screen first: the task waits for the user, and nothing is sent.
            settle(&f, &mut watcher, capture!("trust"), start).await;
            let waiting = f.task().await;
            assert_eq!(waiting.state, AgentState::Waiting);
            assert_eq!(delivery(&waiting), Some(InstructionDelivery::Queued));
            assert_eq!(calls(), Vec::<String>::new());
            // The user trusts the folder: agy shows its prompt box. One pass sees it change and
            // resumes the task; only once it has sat still is the prompt sent.
            show(capture!("idle-after-trust"));
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 2,
            )
            .await;
            assert_eq!(calls(), Vec::<String>::new());
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 3,
            )
            .await;
            assert_eq!(f.task().await.state, AgentState::Running);
            assert_eq!(calls(), Vec::<String>::new());
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 4,
            )
            .await;
            assert_eq!(calls(), FIRST_SEND);
            let sent = f.task().await;
            assert_eq!(delivery(&sent), Some(InstructionDelivery::Delivered));
            assert_eq!(sent.state, AgentState::Running);
            // It is the task's first instruction, not a follow-up, and the launch has it now.
            assert_eq!(sent.follow_ups, []);
            assert!(!f.binding().await.resumed_session);
            // Never again: not while the box still shows, and not after the turn.
            for step in 5..8 {
                pass(
                    &f,
                    &mut watcher,
                    capture!("idle-after-trust"),
                    start + QUIET * step,
                )
                .await;
            }
            hook(&f, payload("start")).await;
            hook(&f, payload("pre")).await;
            hook(&f, payload("stop")).await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            assert_eq!(delivery(&idle), Some(InstructionDelivery::Delivered));
            assert_eq!(idle.session_id.as_deref(), Some(CONVERSATION));
            assert_eq!(calls(), FIRST_SEND);
            f.close().await;
        },
    );
}

#[test]
fn nothing_is_sent_at_a_prompt_or_while_the_pane_changes() {
    with_fake_tmux(
        "first_instruction::nothing_is_sent_at_a_prompt_or_while_the_pane_changes",
        || async {
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let start = Instant::now();
            for (index, (name, shown)) in [
                ("trust", capture!("trust")),
                ("selection-hooks", capture!("selection-hooks")),
                ("slash-menu", capture!("slash-menu")),
                ("slash-draft", capture!("slash-draft")),
                ("shell-mode", capture!("shell-mode")),
                ("login", capture!("login")),
                // No prompt, but not the empty prompt box either. (A turn in progress is
                // another matter: see the test of a turn seen before the send.)
                ("draft", capture!("draft")),
            ]
            .into_iter()
            .enumerate()
            {
                let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
                let at = start + QUIET * (10 * index as u32);
                settle(&f, &mut watcher, shown, at).await;
                pass(&f, &mut watcher, shown, at + QUIET * 2).await;
                assert_eq!(calls(), Vec::<String>::new(), "{name}");
                assert_eq!(
                    delivery(&f.task().await),
                    Some(InstructionDelivery::Queued),
                    "{name}"
                );
                // Back to a running task for the next screen.
                f.svc
                    .update("a", |task| {
                        apply_event(task, &AgentEvent::OutputResumed, now_ms());
                        true
                    })
                    .await
                    .expect("resumed");
            }
            // The box shows, but the pane changed since the pass before: not sent yet.
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let at = start + QUIET * 200;
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("draft"), at).await;
            pass(&f, &mut watcher, capture!("idle-after-trust"), at + QUIET).await;
            assert_eq!(calls(), Vec::<String>::new());
            // The watcher saw the box sit still, but a prompt came up before the send read the
            // pane: the send stops there, and the prompt stays queued.
            show(capture!("trust"));
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                at + QUIET * 2,
            )
            .await;
            assert_eq!(calls(), ["list-sessions", "capture-pane"]);
            let held = f.task().await;
            assert_eq!(delivery(&held), Some(InstructionDelivery::Queued));
            assert!(f.binding().await.resumed_session);
            // Likewise when the box is gone without a prompt in its place: agy began to work on
            // something typed into it.
            let at = at + QUIET * 10;
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            show(capture!("working"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), at + QUIET).await;
            assert_eq!(calls(), ["list-sessions", "capture-pane"]);
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            f.close().await;
            // An approval is asked in the middle of a turn, and these captures show the prompt
            // of that turn above it: not the queued one's turn, so it is never sent.
            for shown in [capture!("approval"), capture!("selection-model")] {
                let other = launched(Some(InstructionDelivery::Queued)).await;
                let mut watcher = crate::watch::Watcher::new(&other.svc).expect("patterns");
                settle(&other, &mut watcher, shown, start).await;
                assert_eq!(calls(), Vec::<String>::new());
                assert_eq!(
                    delivery(&other.task().await),
                    Some(InstructionDelivery::NotSent)
                );
                other.close().await;
            }
        },
    );
}

#[test]
fn a_prompt_typed_in_the_terminal_first_cancels_the_send() {
    with_fake_tmux(
        "first_instruction::a_prompt_typed_in_the_terminal_first_cancels_the_send",
        || async {
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            // The user's own first prompt, typed into agy: its hooks arrive before the send.
            hook(&f, payload("start")).await;
            let cancelled = f.task().await;
            assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
            hook(&f, payload("pre")).await;
            hook(&f, payload("stop")).await;
            assert_eq!(f.task().await.state, AgentState::Idle);
            // Nothing is pasted later, whatever the pane shows and however long.
            for step in 1..5 {
                pass(
                    &f,
                    &mut watcher,
                    capture!("idle-after-trust"),
                    start + QUIET * step,
                )
                .await;
            }
            assert_eq!(calls(), Vec::<String>::new());
            let after = f.task().await;
            assert_eq!(delivery(&after), Some(InstructionDelivery::NotSent));
            assert_eq!(after.prompt, DRAFT);
            f.close().await;
        },
    );
}

#[test]
fn an_instruction_the_user_sends_first_takes_the_queued_ones_place() {
    with_fake_tmux(
        "first_instruction::an_instruction_the_user_sends_first_takes_the_queued_ones_place",
        || async {
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            show(capture!("idle-after-trust"));
            let sent = f.send().await;
            assert_eq!(delivery(&sent), Some(InstructionDelivery::NotSent));
            assert_eq!(sent.follow_ups.len(), 1);
            assert_eq!(calls(), ONE_SEND);
            // The watcher sends nothing more, however long the box shows.
            let start = Instant::now();
            settle(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 2,
            )
            .await;
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::NotSent)
            );
            f.close().await;
        },
    );
}

#[test]
fn a_restart_neither_loses_the_queued_instruction_nor_sends_one_twice() {
    with_fake_tmux(
        "first_instruction::a_restart_neither_loses_the_queued_instruction_nor_sends_one_twice",
        || async {
            let start = Instant::now();
            // Queued, and the daemon restarts while agy still asks about trust: nobody can have
            // typed a prompt yet, so it stays queued and is sent once after the question.
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            f.restart().await;
            f.svc.settle_first_instructions().await.expect("start");
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            settle(&f, &mut watcher, capture!("trust"), start).await;
            assert_eq!(f.task().await.state, AgentState::Waiting);
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            assert_eq!(calls(), Vec::<String>::new());
            show(capture!("idle-after-trust"));
            for step in 2..5 {
                let at = start + QUIET * step;
                pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            }
            assert_eq!(calls(), FIRST_SEND);
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            // Delivered: a daemon that restarts sends nothing.
            f.restart().await;
            f.svc.settle_first_instructions().await.expect("start");
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            settle(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 10,
            )
            .await;
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 12,
            )
            .await;
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            f.close().await;

            // Reserved, and the daemon ended before it recorded how the paste went: whether agy
            // got it is not known, so it is never sent again.
            let mut f = launched(Some(InstructionDelivery::Pending)).await;
            // The reserve cleared the launch's wait for an instruction.
            let mut binding = f.binding().await;
            binding.resumed_session = false;
            f.svc
                .save_binding("a", binding, true)
                .await
                .expect("reserved");
            f.restart().await;
            f.svc.settle_first_instructions().await.expect("start");
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Unknown)
            );
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            settle(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 20,
            )
            .await;
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 22,
            )
            .await;
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Unknown)
            );
            f.close().await;
        },
    );
}

#[test]
fn a_paste_that_never_left_is_tried_again_and_one_that_may_have_is_not() {
    with_fake_tmux(
        "first_instruction::a_paste_that_never_left_is_tried_again_and_one_that_may_have_is_not",
        || async {
            let start = Instant::now();
            // tmux refuses the text before anything reaches the pane: still queued.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            fail_tmux(Some("load-buffer"));
            settle(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            // tmux's own buffer is dropped after a failed paste.
            assert_eq!(
                calls(),
                [
                    "list-sessions",
                    "capture-pane",
                    "load-buffer",
                    "delete-buffer"
                ]
            );
            let held = f.task().await;
            assert_eq!(delivery(&held), Some(InstructionDelivery::Queued));
            assert!(f.binding().await.resumed_session);
            fail_tmux(None);
            show(capture!("idle-after-trust"));
            for step in 2..6 {
                pass(
                    &f,
                    &mut watcher,
                    capture!("idle-after-trust"),
                    start + QUIET * step,
                )
                .await;
            }
            assert_eq!(calls(), FIRST_SEND);
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            f.close().await;

            // The text was pasted, the box showed it, and Enter failed: agy may hold it. Not sent
            // again.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            fail_tmux(Some("send-keys"));
            settle(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 10,
            )
            .await;
            assert_eq!(calls(), FIRST_SEND);
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Unknown)
            );
            fail_tmux(None);
            show(capture!("idle-after-trust"));
            for step in 12..15 {
                pass(
                    &f,
                    &mut watcher,
                    capture!("idle-after-trust"),
                    start + QUIET * step,
                )
                .await;
            }
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Unknown)
            );
            f.close().await;
        },
    );
}

#[test]
fn a_task_without_a_prompt_and_a_resume_send_nothing() {
    with_fake_tmux(
        "first_instruction::a_task_without_a_prompt_and_a_resume_send_nothing",
        || async {
            let start = Instant::now();
            // Started without a prompt: the empty prompt box means it waits for an instruction.
            let f = launched(None).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            settle(&f, &mut watcher, capture!("idle-default-statusline"), start).await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            assert_eq!(idle.first_instruction, None);
            assert_eq!(calls(), Vec::<String>::new());
            f.close().await;

            // The first launch ended with its prompt still queued (the user declined trust, and
            // agy exited). A resume never sends it: the launch waits for an instruction instead.
            let (f, _) = ended(Some(CONVERSATION)).await;
            f.svc
                .update("a", |task| {
                    task.first_instruction = Some(FirstInstruction {
                        delivery: InstructionDelivery::Queued,
                        at_ms: 1,
                        reason: None,
                    });
                    true
                })
                .await
                .expect("queued");
            let resumed = f.svc.resume("a").await.expect("resume");
            assert_eq!(delivery(&resumed), Some(InstructionDelivery::NotSent));
            f.tool_started().await;
            let _ = fs::remove_file(format!(
                "{}.calls",
                std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake")
            ));
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            settle(
                &f,
                &mut watcher,
                capture!("idle-resumed"),
                start + QUIET * 10,
            )
            .await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            assert_eq!(delivery(&idle), Some(InstructionDelivery::NotSent));
            assert_eq!(calls(), Vec::<String>::new());
            f.close().await;
        },
    );
}

#[path = "first_instruction_enter_tests.rs"]
mod entering;
#[path = "first_instruction_watch_tests.rs"]
mod watching;
