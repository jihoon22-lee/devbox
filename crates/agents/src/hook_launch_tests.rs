use super::*;

#[tokio::test]
async fn managed_claude_requires_matching_binding_without_waiting_for_send_actions() {
    let f = Fixture::new().await;
    let launch = uuid::Uuid::new_v4().simple().to_string();
    let mut binding = Binding::new(1);
    binding.launch_id = Some(launch.clone());
    f.seed(binding).await;
    // Send owns this lock while tmux may synchronously wait for a tool hook.
    let _send = f.svc.actions.lock().await;
    let before = f.svc.get("a").await.expect("task");
    let revision = f.svc.db.rev();
    for generation in [None, Some(String::new()), Some("previous".into())] {
        let done = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            f.svc.event(AgentEventParams {
                agent_id: "a".into(),
                launch_id: generation,
                source: HookSource::Claude,
                payload:
                    serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"})
                        .to_string(),
                code: None,
            }),
        )
        .await
        .expect("hook does not wait for actions")
        .expect("ignored hook");
        assert_eq!(done.rev, revision.to_string());
        assert_eq!(f.svc.db.rev(), revision);
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), before);
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        f.svc.event(AgentEventParams {
            agent_id: "a".into(),
            launch_id: Some(launch),
            source: HookSource::Claude,
            payload: serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"})
                .to_string(),
            code: None,
        }),
    )
    .await
    .expect("synchronous hook while send owns actions")
    .expect("current hook");
    assert_eq!(f.state().await, AgentState::Waiting);
}

#[tokio::test]
async fn missing_runtime_generation_never_admits_an_unbound_claude_hook() {
    let f = Fixture::new().await;
    let before = f.svc.get("a").await.expect("task");
    let revision = f.svc.db.rev();
    for generation in [None, Some("unbound".into())] {
        f.svc
            .event(AgentEventParams {
                agent_id: "a".into(),
                launch_id: generation,
                source: HookSource::Claude,
                payload: serde_json::json!({"hook_event_name":"Stop"}).to_string(),
                code: None,
            })
            .await
            .expect("unbound hook ignored");
        assert_eq!(f.svc.db.rev(), revision);
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), before);
    }
}

#[tokio::test]
async fn post_tool_use_outside_waiting_state_writes_and_publishes_nothing() {
    use devbox_protocol::Topic;
    let f = Fixture::new().await;
    let launch = uuid::Uuid::new_v4().simple().to_string();
    let mut binding = Binding::new(1);
    binding.launch_id = Some(launch.clone());
    f.seed(binding).await;
    let hook = |payload: serde_json::Value| AgentEventParams {
        agent_id: "a".into(),
        launch_id: Some(launch.clone()),
        source: HookSource::Claude,
        payload: payload.to_string(),
        code: None,
    };
    let secret = "OWNED_SECRET_MARKER";
    let tool_used = serde_json::json!({"hook_event_name":"PostToolUse","session_id":"sess-1","tool_name":"Bash","tool_input":{"command":format!("echo {secret}")},"tool_response":{"stdout":format!("{secret}=sk-live-0123456789"),"stderr":""}});
    let set_state = |state: AgentState| {
        f.svc.db.write(move |tx| {
            let mut task = store::get(tx, "a")?.expect("owned task");
            task.state = state;
            store::put(tx, &task)
        })
    };
    let mut changed = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    let quiet = std::time::Duration::from_millis(20);
    for state in [
        AgentState::Preparing,
        AgentState::Running,
        AgentState::Idle,
        AgentState::Failed,
        AgentState::Review,
    ] {
        set_state(state).await.expect("owned fixture");
        let before = f.svc.get("a").await.expect("task");
        let revision = f.svc.db.rev();
        let done = f.svc.event(hook(tool_used.clone())).await.expect("hook");
        assert_eq!(done.rev, revision.to_string(), "{state:?}");
        assert_eq!(f.svc.db.rev(), revision, "{state:?}");
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), before);
    }
    // Publication happens before the reply, so an empty queue now stays empty.
    assert!(tokio::time::timeout(quiet, changed.recv()).await.is_err());
    assert!(tokio::time::timeout(quiet, attention.recv()).await.is_err());
    set_state(AgentState::Running).await.expect("owned fixture");
    f.svc
        .event(hook(
            serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"}),
        ))
        .await
        .expect("permission hook");
    assert_eq!(f.state().await, AgentState::Waiting);
    changed.recv().await.expect("waiting change");
    attention.recv().await.expect("waiting attention");
    let revision = f.svc.db.rev();
    f.svc.event(hook(tool_used)).await.expect("resumed hook");
    let task = f.svc.get("a").await.expect("resumed task");
    assert_eq!(task.state, AgentState::Running);
    assert_eq!(task.attention, None);
    assert_eq!(f.svc.db.rev(), revision + 1);
    let Some(devbox_core::HubMessage::Event { payload, .. }) = changed.recv().await else {
        panic!("one change for the resumed task");
    };
    assert!(!payload.contains(secret));
    assert!(tokio::time::timeout(quiet, changed.recv()).await.is_err());
    assert!(tokio::time::timeout(quiet, attention.recv()).await.is_err());
    let row: String = f
        .svc
        .db
        .read(|tx| {
            tx.query_row("SELECT body FROM agents_tasks WHERE id='a'", [], |row| {
                row.get(0)
            })
        })
        .await
        .expect("stored task");
    assert!(!row.contains(secret) && !row.contains("tool_response"));
}
