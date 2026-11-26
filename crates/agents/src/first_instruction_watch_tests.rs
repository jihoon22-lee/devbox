//! What the watcher does about a queued first instruction besides sending it: a turn it sees
//! first, and an action lock another task holds.
use super::*;

#[test]
fn a_turn_seen_in_the_pane_before_the_send_cancels_it() {
    with_fake_tmux(
        "first_instruction::watching::a_turn_seen_in_the_pane_before_the_send_cancels_it",
        || async {
            // The user typed a prompt into agy, and its hooks never reached the daemon: the
            // pane's spinner row is all that shows it.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            show(capture!("working"));
            pass(&f, &mut watcher, capture!("working"), start + QUIET / 2).await;
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::NotSent)
            );
            // The turn ends and the box is empty again: nothing is pasted, then or later. With
            // no hook to say so, the still pane is what ends the launch's wait.
            show(capture!("idle-after-trust"));
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
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            assert_eq!(delivery(&idle), Some(InstructionDelivery::NotSent));
            f.close().await;
            // A prompt queued behind a turn (the user's own) is a turn too. And so is a turn
            // whose prompt a long answer has scrolled out of the rows read: its spinner row alone
            // shows it (NOT a real capture: the recorded turn without its echoed prompt).
            let scrolled = capture!("working").replacen("\n> Reply", "\n  Reply", 1);
            let scrolled: &'static str = Box::leak(scrolled.into_boxed_str());
            assert!(!crate::antigravity::shows_earlier_prompt(pane(scrolled)));
            for shown in [capture!("queued"), scrolled] {
                let f = launched(Some(InstructionDelivery::Queued)).await;
                let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
                pass(&f, &mut watcher, shown, start).await;
                assert_eq!(
                    delivery(&f.task().await),
                    Some(InstructionDelivery::NotSent)
                );
                f.close().await;
            }
        },
    );
}

#[test]
fn the_watcher_never_waits_for_an_action_on_another_task() {
    with_fake_tmux(
        "first_instruction::watching::the_watcher_never_waits_for_an_action_on_another_task",
        || async {
            let f = launched(Some(InstructionDelivery::Queued)).await;
            // A second task of the same family, at its trust screen.
            let mut other = f.task().await;
            other.id = "b".into();
            other.terminal_id = Some("term-b".into());
            other.first_instruction = None;
            let binding = f.binding().await;
            f.svc
                .db
                .write(move |tx| {
                    crate::store::put(tx, &other)?;
                    crate::store::put_binding(tx, "b", &binding)
                })
                .await
                .expect("owned second task");
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            // An action on some task holds the action lock for as long as it takes: a merge, a
            // discard, a slow teardown.
            let held = f.svc.actions.lock().await;
            let passes = async {
                show(capture!("idle-after-trust"));
                for step in 0..3 {
                    let at = start + QUIET * step;
                    pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
                    let second = crate::watch::Snapshot {
                        task: f.svc.get("b").await.expect("task"),
                        binding: Some(f.svc.launch_binding("b").await.expect("binding")),
                    };
                    if crate::watch::eligible(&second.task) {
                        let tail = pane(capture!("trust")).to_owned();
                        watcher.read(&f.svc, second, tail, at).await;
                    }
                }
            };
            tokio::time::timeout(Duration::from_secs(10), passes)
                .await
                .expect("the watcher does not wait for the action lock");
            // The other task's trust screen was still found, and the first one's prompt waits.
            assert_eq!(
                f.svc.get("b").await.expect("task").state,
                AgentState::Waiting
            );
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            assert_eq!(calls(), Vec::<String>::new());
            // The action ends: the next pass sends it.
            drop(held);
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 3,
            )
            .await;
            assert_eq!(calls(), FIRST_SEND);
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            f.close().await;
        },
    );
}

fn reason(task: &AgentTask) -> Option<crate::model::FirstInstructionReason> {
    task.first_instruction.and_then(|first| first.reason)
}

/// The daemon restarts on the task's data, in its start's order before its watcher reads a pane.
async fn restarted(f: &mut Ended) -> crate::watch::Watcher {
    f.restart().await;
    f.svc.reconcile().await.expect("reconcile");
    f.svc.settle_first_instructions().await.expect("start");
    crate::watch::Watcher::new(&f.svc).expect("patterns")
}

#[test]
fn a_prompt_typed_while_the_daemon_was_down_is_never_followed_by_the_queued_one() {
    with_fake_tmux(
        "first_instruction::watching::a_prompt_typed_while_the_daemon_was_down_is_never_followed_by_the_queued_one",
        || async {
            use crate::model::FirstInstructionReason::Unobserved;
            // Found with the real tool: the daemon was down for seven seconds. Meanwhile the user
            // trusted the folder, typed a prompt, and its turn ended. No hook reached a daemon and
            // no spinner was seen: the daemon comes back to a pane at the prompt box.
            for shown in [
                capture!("idle-after-user-turn"),
                // The same without anything to tell it by: the box alone.
                capture!("idle-after-trust"),
                capture!("idle-default-statusline"),
                // And anything else that is not a screen before any prompt.
                capture!("working"),
                capture!("slash-menu"),
                capture!("approval"),
            ] {
                let mut f = launched(Some(InstructionDelivery::Queued)).await;
                let mut watcher = restarted(&mut f).await;
                let start = Instant::now();
                show(shown);
                // The first sight of the pane decides, without waiting for it to sit still.
                pass(&f, &mut watcher, shown, start).await;
                let cancelled = f.task().await;
                assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
                assert_eq!(reason(&cancelled), Some(Unobserved));
                // Nothing is pasted then or later, whatever the pane shows from here on.
                show(capture!("idle-after-trust"));
                for step in 1..5 {
                    let at = start + QUIET * step;
                    pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
                }
                assert_eq!(calls(), Vec::<String>::new());
                // The decision is the task's: another restart finds it made.
                let mut watcher = restarted(&mut f).await;
                show(capture!("idle-after-trust"));
                for step in 10..14 {
                    let at = start + QUIET * step;
                    pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
                }
                let after = f.task().await;
                assert_eq!(delivery(&after), Some(InstructionDelivery::NotSent));
                assert_eq!(reason(&after), Some(Unobserved));
                assert_eq!(calls(), Vec::<String>::new());
                f.close().await;
            }
        },
    );
}

#[test]
fn a_restart_at_a_screen_before_any_prompt_keeps_the_instruction_queued() {
    with_fake_tmux(
        "first_instruction::watching::a_restart_at_a_screen_before_any_prompt_keeps_the_instruction_queued",
        || async {
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            let start = Instant::now();
            // Two restarts while agy asks about trust: each finds the question still up.
            for restart in 0..2 {
                let mut watcher = restarted(&mut f).await;
                let at = start + QUIET * (restart * 10);
                settle(&f, &mut watcher, capture!("trust"), at).await;
                let waiting = f.task().await;
                assert_eq!(delivery(&waiting), Some(InstructionDelivery::Queued));
                assert_eq!(reason(&waiting), None);
                assert_eq!(calls(), Vec::<String>::new());
            }
            // The user answers: the daemon that saw the question sends the prompt, once.
            let mut watcher = restarted(&mut f).await;
            settle(&f, &mut watcher, capture!("trust"), start + QUIET * 30).await;
            show(capture!("idle-after-trust"));
            for step in 32..36 {
                let at = start + QUIET * step;
                pass(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            }
            assert_eq!(calls(), FIRST_SEND);
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            f.close().await;

            // A task whose tool had not started when the daemon came back is watched from the
            // tool's first screen on: its prompt box is the launch's first.
            let phase = |f: &Ended, text: &'static str| {
                let (svc, at) = (f.svc.clone(), f.root.path().to_owned());
                async move {
                    let task = svc.get("a").await.expect("task");
                    let launch = svc.launch_binding("a").await.expect("binding").launch_id;
                    let path = svc.phase_path(&task, &launch.expect("launch"));
                    assert!(path.starts_with(at));
                    fs::write(path, text).expect("phase");
                    svc.update("a", |task| {
                        task.state = AgentState::Preparing;
                        true
                    })
                    .await
                    .expect("still preparing");
                }
            };
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            phase(&f, "starting\n").await;
            let mut watcher = restarted(&mut f).await;
            assert_eq!(f.task().await.state, AgentState::Preparing);
            f.tool_started().await;
            f.svc
                .update("a", |task| {
                    task.state = AgentState::Running;
                    true
                })
                .await
                .expect("tool started");
            let at = start + QUIET * 40;
            settle(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            assert_eq!(calls(), FIRST_SEND);
            f.close().await;

            // One whose tool was up, with the report of it lost while no daemon ran, is held to
            // the rule: the start finds the tool started before it notes the instruction.
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            phase(&f, "tool\n").await;
            let mut watcher = restarted(&mut f).await;
            assert_eq!(f.task().await.state, AgentState::Running);
            let at = start + QUIET * 50;
            settle(&f, &mut watcher, capture!("idle-after-trust"), at).await;
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::NotSent)
            );
            assert_eq!(calls(), Vec::<String>::new());
            f.close().await;
        },
    );
}

#[test]
fn a_start_that_cannot_settle_reads_no_pane_until_it_can() {
    with_fake_tmux(
        "first_instruction::watching::a_start_that_cannot_settle_reads_no_pane_until_it_can",
        || async {
            use crate::model::FirstInstructionReason::Unobserved;
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            // Another task's first instruction was being pasted when the daemon before stopped.
            // It is the first the start comes to, and the write that settles it fails.
            let mut other = f.task().await;
            other.id = "p".into();
            other.terminal_id = None;
            other.state = AgentState::Review;
            other.updated_ms += 1;
            crate::first_instruction::set(&mut other, InstructionDelivery::Pending, 1);
            let broken = "CREATE TRIGGER broken BEFORE UPDATE ON agents_tasks WHEN NEW.id = 'p' \
                          BEGIN SELECT RAISE(ABORT, 'owned failure'); END;";
            f.svc
                .db
                .write(move |tx| {
                    crate::store::put(tx, &other)?;
                    tx.execute_batch(broken)
                })
                .await
                .expect("a write that fails");
            f.restart().await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            // The pass ends where the start could not settle: no pane is read, nothing is sent.
            for step in 0..3 {
                assert!(watcher.pass(&f.svc, || start + QUIET * step).await);
            }
            assert_eq!(calls(), Vec::<String>::new());
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            // The queued one was noted before the write that failed.
            assert!(f.svc.unobserved.lock().expect("notes").contains("a"));
            // The next pass settles, and the restart rule holds for the pane at its prompt box.
            f.svc
                .db
                .write(|tx| tx.execute_batch("DROP TRIGGER broken"))
                .await
                .expect("the write works again");
            for step in 3..7 {
                assert!(watcher.pass(&f.svc, || start + QUIET * step).await);
            }
            let cancelled = f.task().await;
            assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
            assert_eq!(reason(&cancelled), Some(Unobserved));
            assert!(
                calls().iter().all(|call| call == "capture-pane"),
                "{:?}",
                calls()
            );
            let other = f.svc.get("p").await.expect("the other task");
            assert_eq!(delivery(&other), Some(InstructionDelivery::Unknown));
            f.close().await;
        },
    );
}

#[test]
fn a_task_made_after_the_daemon_started_is_not_held_to_the_restart_rule() {
    with_fake_tmux(
        "first_instruction::watching::a_task_made_after_the_daemon_started_is_not_held_to_the_restart_rule",
        || async {
            // A start that failed once is tried again while the daemon serves, so it can find a
            // task that was made, and whose tool started, under this daemon's eyes. Whatever the
            // clock says: here it says the task is older than the daemon.
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            f.restart().await;
            f.svc.note_created("a");
            let before = f.svc.started_ms - 3_600_000;
            f.svc
                .update("a", move |task| {
                    task.created_ms = before;
                    true
                })
                .await
                .expect("made by this daemon");
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            for step in 0..3 {
                assert!(watcher.pass(&f.svc, || start + QUIET * step).await);
            }
            assert_eq!(
                delivery(&f.task().await),
                Some(InstructionDelivery::Delivered)
            );
            assert!(calls().iter().any(|call| call == "paste-buffer"));
            f.close().await;
        },
    );
}

#[test]
fn a_task_from_before_the_restart_is_held_to_the_rule_whatever_the_clock_says() {
    with_fake_tmux(
        "first_instruction::watching::a_task_from_before_the_restart_is_held_to_the_rule_whatever_the_clock_says",
        || async {
            use crate::model::FirstInstructionReason::Unobserved;
            // The WSL clock stepped back between the two daemons: the task the daemon before
            // made looks an hour newer than this daemon's start.
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            f.restart().await;
            let later = f.svc.started_ms + 3_600_000;
            f.svc
                .update("a", move |task| {
                    task.created_ms = later;
                    true
                })
                .await
                .expect("a clock that stepped back");
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            for step in 0..4 {
                assert!(watcher.pass(&f.svc, || start + QUIET * step).await);
            }
            let cancelled = f.task().await;
            assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
            assert_eq!(reason(&cancelled), Some(Unobserved));
            assert!(
                calls().iter().all(|call| call == "capture-pane"),
                "{:?}",
                calls()
            );
            f.close().await;
        },
    );
}

#[test]
fn a_restart_that_cannot_read_the_pane_does_not_send() {
    with_fake_tmux(
        "first_instruction::watching::a_restart_that_cannot_read_the_pane_does_not_send",
        || async {
            use crate::model::FirstInstructionReason::Unobserved;
            let mut f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = restarted(&mut f).await;
            // tmux gave no capture of the pane on the watcher's first pass.
            watcher.unread(&f.svc, &f.task().await).await;
            let cancelled = f.task().await;
            assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
            assert_eq!(reason(&cancelled), Some(Unobserved));
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
            f.close().await;
            // A pane that cannot be read once in a daemon that saw the launch from its start
            // changes nothing: the next pass reads it.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            watcher.unread(&f.svc, &f.task().await).await;
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            settle(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET * 10,
            )
            .await;
            assert_eq!(calls(), FIRST_SEND);
            f.close().await;
        },
    );
}

#[test]
fn a_prompt_already_echoed_above_the_box_cancels_the_send_in_a_running_daemon() {
    with_fake_tmux(
        "first_instruction::watching::a_prompt_already_echoed_above_the_box_cancels_the_send_in_a_running_daemon",
        || async {
            // The user's own turn began and ended between two passes, and its hooks were lost:
            // no spinner was seen. The prompt it echoed above the box is what is left of it.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            let start = Instant::now();
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            show(capture!("idle-after-user-turn"));
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-user-turn"),
                start + QUIET / 2,
            )
            .await;
            let cancelled = f.task().await;
            assert_eq!(delivery(&cancelled), Some(InstructionDelivery::NotSent));
            assert_eq!(reason(&cancelled), None);
            for step in 1..5 {
                let at = start + QUIET * step;
                pass(&f, &mut watcher, capture!("idle-after-user-turn"), at).await;
            }
            assert_eq!(calls(), Vec::<String>::new());
            f.close().await;
            // The send reads the pane again itself: an echo that appeared since the watcher's
            // pass stops it there.
            let f = launched(Some(InstructionDelivery::Queued)).await;
            let mut watcher = crate::watch::Watcher::new(&f.svc).expect("patterns");
            show(capture!("idle-after-trust"));
            pass(&f, &mut watcher, capture!("idle-after-trust"), start).await;
            show(capture!("idle-after-user-turn"));
            pass(
                &f,
                &mut watcher,
                capture!("idle-after-trust"),
                start + QUIET,
            )
            .await;
            assert_eq!(calls(), ["list-sessions", "capture-pane"]);
            assert_eq!(delivery(&f.task().await), Some(InstructionDelivery::Queued));
            f.close().await;
        },
    );
}
