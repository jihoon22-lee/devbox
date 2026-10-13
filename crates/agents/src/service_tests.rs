use super::*;
#[test]
fn allocate_port_skips_blocks_with_a_listening_port() {
    let base = allocate_port(&[]).expect("available owned block");
    let listener = std::net::TcpListener::bind(("127.0.0.1", base + 3)).expect("owned test port");
    assert!(allocate_port_from(&[], base).unwrap() >= base + 10);
    drop(listener);
}
#[test]
fn same_state_session_metadata_and_closed_hooks() {
    let mut t = task();
    t.state = crate::state::AgentState::Running;
    assert!(apply_event(
        &mut t,
        &crate::state::AgentEvent::SessionStarted {
            session_id: "session".into()
        },
        1
    ));
    assert_eq!(t.session_id.as_deref(), Some("session"));
    t.state = crate::state::AgentState::Closed;
    assert!(!apply_event(
        &mut t,
        &crate::state::AgentEvent::SessionStarted {
            session_id: "other".into()
        },
        2
    ));
    assert_eq!(t.session_id.as_deref(), Some("session"));
}
fn task() -> crate::model::AgentTask {
    serde_json::from_str(r#"{"id":"a","projectId":"p","title":"x","profile":"claude","baseBranch":"main","branch":null,"worktreePath":null,"workdir":"/tmp","sessionId":null,"terminalId":null,"state":"preparing","reason":null,"attention":null,"port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null}"#).unwrap()
}

#[test]
fn session_metadata_preserves_existing_permission_attention() {
    let mut task = task();
    task.state = AgentState::Running;
    apply_event(
        &mut task,
        &AgentEvent::NeedsInput {
            message: "approve Bash".into(),
        },
        1,
    );
    let original = serde_json::to_string(&task.attention).unwrap();
    assert!(apply_event(
        &mut task,
        &AgentEvent::SessionStarted {
            session_id: "session".into()
        },
        2
    ));
    assert_eq!(task.state, AgentState::Waiting);
    assert_eq!(task.session_id.as_deref(), Some("session"));
    assert_eq!(serde_json::to_string(&task.attention).unwrap(), original);
}
#[test]
fn rejected_live_hooks_preserve_failure_attention_and_revision_timestamp() {
    let mut task = task();
    apply_event(&mut task, &AgentEvent::SetupFailed { code: 3 }, 1);
    let before = serde_json::to_string(&task).unwrap();
    for event in [
        AgentEvent::NeedsInput {
            message: "irrelevant".into(),
        },
        AgentEvent::TurnComplete {
            message: Some("irrelevant".into()),
        },
    ] {
        assert!(!apply_event(&mut task, &event, 2));
        assert_eq!(serde_json::to_string(&task).unwrap(), before);
    }
}

#[test]
fn tool_exit_is_recorded_independently_of_user_review_state() {
    let mut task = task();
    task.state = AgentState::Idle;
    apply_event(&mut task, &AgentEvent::UserReview, 1);
    assert_eq!(task.state, AgentState::Review);
    assert!(!task.tool_exited, "user review preserves the live tool");
    assert!(apply_event(&mut task, &AgentEvent::Exited { code: 0 }, 2));
    assert!(task.tool_exited);
    assert_eq!(task.state, AgentState::Review);
    assert!(!apply_event(&mut task, &AgentEvent::UserInstructed, 3));
    assert!(!apply_event(
        &mut task,
        &AgentEvent::SessionStarted {
            session_id: "stale".into()
        },
        3
    ));
}

#[test]
fn output_attention_has_provenance_and_hook_promotes_same_state() {
    let mut t = task();
    t.state = AgentState::Running;
    assert!(apply_event(&mut t, &AgentEvent::OutputWaiting, 1));
    assert_eq!(t.attention.as_ref().unwrap().kind, "output");
    let message = t.attention.as_ref().unwrap().message.clone();
    assert!(apply_event(&mut t, &AgentEvent::NeedsInput { message }, 2));
    assert_eq!(t.attention.as_ref().unwrap().kind, "input");
    assert!(!apply_event(&mut t, &AgentEvent::OutputWaiting, 3));
    assert!(!apply_event(&mut t, &AgentEvent::OutputResumed, 3));
    assert_eq!(t.state, AgentState::Waiting);
}
