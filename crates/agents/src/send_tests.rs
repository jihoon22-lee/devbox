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
        assert!(finish_row(
            &mut task,
            &reservation,
            &launch,
            Some(outcome),
            None
        ));
        assert!(first_instruction::is(&task, outcome));
        assert_eq!(task.follow_ups, []);
    }
    // Nothing left devbox and nothing touched the task: the watcher tries again.
    let (mut task, reservation) = reserved();
    assert!(finish_row(&mut task, &reservation, &launch, None, None));
    assert!(first_instruction::is(&task, InstructionDelivery::Queued));
    // A hook moved the task on meanwhile: agy has an instruction, so this one never goes.
    let (mut task, reservation) = reserved();
    apply_event(&mut task, &AgentEvent::PromptSubmitted, 3);
    assert!(finish_row(&mut task, &reservation, &launch, None, None));
    assert!(first_instruction::is(&task, InstructionDelivery::NotSent));
    // Another launch, or an instruction no longer being sent, is left as it is.
    let (mut task, reservation) = reserved();
    let before = task.clone();
    let other = Some("new-launch".to_owned());
    assert!(!finish_row(&mut task, &reservation, &other, None, None));
    first_instruction::set(&mut task, InstructionDelivery::Unknown, 2);
    let settled = task.clone();
    let delivered = Some(InstructionDelivery::Delivered);
    assert!(!finish_row(
        &mut task,
        &reservation,
        &launch,
        delivered,
        None
    ));
    assert_eq!(task, settled);
    assert_ne!(settled, before);
}
#[test]
fn a_first_instruction_left_without_enter_ends_the_wait_only_at_the_prompt_with_nothing_since() {
    let reserved = || {
        let (mut task, mut reservation) = provisional();
        task.follow_ups.clear();
        first_instruction::set(&mut task, InstructionDelivery::Pending, 2);
        reservation.first = true;
        (task, reservation)
    };
    let (launch, unknown) = (
        Some("launch".to_owned()),
        Some(InstructionDelivery::Unknown),
    );
    // Left at agy's prompt: agy began no turn, so the task waits for the user, told whether the
    // last read of the pane showed the box starting with the instruction.
    for (ours, notice) in [
        (true, first_instruction::LEFT_IN_BOX),
        (false, first_instruction::NOT_SEEN),
    ] {
        let (mut task, reservation) = reserved();
        assert_eq!(task.state, AgentState::Running);
        assert!(finish_row(
            &mut task,
            &reservation,
            &launch,
            unknown,
            Some(Entry::AtPrompt { ours })
        ));
        assert!(first_instruction::is(&task, InstructionDelivery::Unknown));
        assert_eq!(task.state, AgentState::Idle);
        let shown = task.attention.as_ref().map(|a| a.message.as_str());
        assert_eq!(shown, Some(notice));
        // A hook since, which every move of agy's is: its state stays, with no notice.
        let (mut task, reservation) = reserved();
        apply_event(&mut task, &AgentEvent::PromptSubmitted, 3);
        let moved = task.clone();
        assert!(finish_row(
            &mut task,
            &reservation,
            &launch,
            unknown,
            Some(Entry::AtPrompt { ours })
        ));
        assert_eq!(task.state, AgentState::Running);
        assert_eq!(task.attention, moved.attention);
    }
    // A question, a turn or an unread pane, and an Enter tmux may or may not have sent: the
    // state stays too.
    for entry in [Entry::Elsewhere, Entry::Unconfirmed] {
        let (mut task, reservation) = reserved();
        assert!(finish_row(
            &mut task,
            &reservation,
            &launch,
            unknown,
            Some(entry)
        ));
        assert_eq!(task.state, AgentState::Running, "{entry:?}");
        assert_eq!(task.attention, None, "{entry:?}");
        assert!(first_instruction::is(&task, InstructionDelivery::Unknown));
    }
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
        None,
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
        None,
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
        Some(InstructionDelivery::Delivered),
        None
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
        None,
        None
    ));
    assert_eq!(task.state, reservation.before.state);
    assert_eq!(task.attention, reservation.before.attention);
    assert!(task.activity_seq > reservation.epoch);
}
