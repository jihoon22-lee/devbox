//! A task whose turn ended is Idle, and the output watcher reads the panes of Running tasks only
//! (`watch::eligible`): a view that opens over an Idle task's prompt box is never stored as
//! Waiting. The pane a send reads itself is then all that keeps its paste and Enter out of that
//! view. The fake `tmux` prints `$live.screen` for `capture-pane` and appends the tmux command of
//! every call to `$live.calls`.
use super::*;
use crate::{hooks::CodexCompletionCandidate, service::CodexTurnEnd};

/// /feedback's list of kinds, open over the prompt box of an Idle task (real capture): Enter
/// would pick "1. bug".
const FEEDBACK_LIST: &str = include_str!("../tests/fixtures/codex-0.172.0/feedback-list.txt");
/// Codex at its prompt with nothing running (real capture).
const READY: &str = include_str!("../tests/fixtures/codex-0.167.0/resume-ready.txt");

/// The fake `tmux`'s file `kind`: `screen` or `calls`.
fn fake(kind: &str) -> PathBuf {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    PathBuf::from(format!("{live}.{kind}"))
}

/// The tmux command of every call since the task became Idle, in order.
fn calls() -> Vec<String> {
    let calls = fs::read_to_string(fake("calls")).unwrap_or_default();
    calls.lines().map(str::to_owned).collect()
}

/// Task `a` of the builtin Codex profile, Idle after its turn ended, in a live terminal whose
/// tool runs and whose pane shows the fixture `shown`. The service published its waiting
/// patterns, as a started daemon has (`watch::spawn`).
async fn idle_showing(shown: &str) -> Ended {
    let f = Ended::new("codex", Some("primary")).await;
    crate::watch::Watcher::new(&f.svc).expect("builtin waiting patterns");
    f.svc.resume("a").await.expect("resume");
    f.tool_started().await;
    // The tracker saw the launch's turn begin, and Codex's notify reported its end.
    let mut binding = f.binding().await;
    binding.turn_id = Some("owned-turn".into());
    binding.turn_started_ms = Some(binding.launch_ms + 1);
    f.svc
        .save_binding("a", binding, false)
        .await
        .expect("tracker save");
    let reported = CodexCompletionCandidate {
        thread_id: "primary".into(),
        turn_id: "owned-turn".into(),
        cwd: f.task().await.workdir,
        message: Some("Paused.".into()),
    };
    f.svc
        .complete_codex("a", CodexTurnEnd::Reported(reported))
        .await
        .expect("reported turn end");
    let idle = f.task().await;
    assert_eq!(idle.state, AgentState::Idle);
    assert_eq!(idle.attention.as_ref().expect("attention").kind, "idle");
    assert!(idle.terminal_id.is_some() && idle.session_id.is_some() && !idle.tool_exited);
    // The output watcher does not read this task's pane, so nothing turns it to Waiting.
    let watched = f.svc.watched_tasks().await.expect("watched tasks");
    assert_eq!(watched, Vec::<AgentTask>::new());
    let pane = shown
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1;
    fs::write(fake("screen"), pane).expect("owned screen");
    fs::remove_file(fake("calls")).expect("the resume's tmux calls");
    f
}

#[test]
fn a_send_to_an_idle_codex_task_cannot_answer_the_feedback_list_on_its_pane() {
    with_fake_tmux(
        "idle_pane::a_send_to_an_idle_codex_task_cannot_answer_the_feedback_list_on_its_pane",
        || async {
            let f = idle_showing(FEEDBACK_LIST).await;
            let idle = (f.task().await, f.binding().await);
            let refused = f.try_send().await;
            assert!(
                matches!(
                    refused,
                    Err(devbox_protocol::Fail::Domain(AgentsError::AwaitingInput))
                ),
                "{refused:?}"
            );
            // The send read the pane and stopped there: tmux loaded and pasted no buffer and
            // sent no key.
            assert_eq!(calls(), ["list-sessions", "capture-pane"]);
            // Nothing was reserved either: the task is Idle as before, with no instruction.
            assert_eq!((f.task().await, f.binding().await), idle);
            assert_eq!(idle.0.follow_ups, []);
            f.close().await;
        },
    );
}

#[test]
fn a_send_to_an_idle_codex_task_at_its_ready_prompt_is_delivered() {
    with_fake_tmux(
        "idle_pane::a_send_to_an_idle_codex_task_at_its_ready_prompt_is_delivered",
        || async {
            // The same Idle task: only what its pane shows differs.
            let f = idle_showing(READY).await;
            let sent = f.send().await;
            assert_eq!(sent.state, AgentState::Running);
            assert_eq!(sent.follow_ups.len(), 1);
            assert_eq!(
                sent.follow_ups[0].delivery,
                Some(InstructionDelivery::Delivered)
            );
            assert_eq!(
                calls(),
                [
                    "list-sessions",
                    "capture-pane",
                    "load-buffer",
                    "paste-buffer",
                    "send-keys"
                ]
            );
            f.close().await;
        },
    );
}
