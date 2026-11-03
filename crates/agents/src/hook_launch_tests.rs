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
    let input = serde_json::json!({"command":format!("echo {secret}")});
    let tool_used = serde_json::json!({"hook_event_name":"PostToolUse","session_id":"sess-1","tool_name":"Bash","tool_input":input,"tool_use_id":"toolu_1","tool_response":{"stdout":format!("{secret}=sk-live-0123456789"),"stderr":""},"error":secret,"reason":secret});
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
    let event = |message: Option<devbox_core::HubMessage>| match message {
        Some(devbox_core::HubMessage::Event { payload, .. }) => payload,
        other => panic!("one publication: {other:?}"),
    };
    let stored = || {
        f.svc.db.read(|tx| {
            let body = "SELECT body FROM agents_tasks WHERE id='a'";
            tx.query_row(body, [], |row| row.get::<_, String>(0))
        })
    };
    // Each way the approved call ends resumes the wait; no payload content is kept.
    for ending in ["PostToolUse", "PostToolUseFailure", "PermissionDenied"] {
        set_state(AgentState::Running).await.expect("owned fixture");
        let request = serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":input,"permission_suggestions":[]});
        f.svc.event(hook(request)).await.expect("permission hook");
        assert_eq!(f.state().await, AgentState::Waiting, "{ending}");
        assert!(!event(changed.recv().await).contains(secret));
        assert!(!event(attention.recv().await).contains(secret));
        let row = stored().await.expect("waiting task");
        assert!(!row.contains(secret), "{ending}");
        let mut ended = tool_used.clone();
        ended["hook_event_name"] = ending.into();
        let revision = f.svc.db.rev();
        f.svc.event(hook(ended)).await.expect("ending hook");
        let task = f.svc.get("a").await.expect("resumed task");
        assert_eq!(
            (task.state, task.attention),
            (AgentState::Running, None),
            "{ending}"
        );
        assert_eq!(f.svc.db.rev(), revision + 1, "{ending}");
        assert!(!event(changed.recv().await).contains(secret));
        assert!(tokio::time::timeout(quiet, changed.recv()).await.is_err());
        assert!(tokio::time::timeout(quiet, attention.recv()).await.is_err());
        let row = stored().await.expect("resumed task");
        assert!(
            !row.contains(secret) && !row.contains("toolu_1"),
            "{ending}"
        );
    }
}
