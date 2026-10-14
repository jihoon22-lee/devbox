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
