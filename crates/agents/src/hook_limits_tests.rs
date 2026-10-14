//! Hook metadata must not roll back lifecycle transitions or change exact identities.
use super::*;

async fn near_full(f: &Fixture, preparing: bool) -> AgentTask {
    f.svc
        .db
        .write(move |tx| {
            let mut binding = store::binding(tx, "a")?.expect("owned binding");
            binding.launch_id = Some(uuid::Uuid::new_v4().simple().to_string());
            store::put_binding(tx, "a", &binding)?;
            let mut task = store::get(tx, "a")?.expect("owned task");
            task.state = if preparing {
                AgentState::Preparing
            } else {
                AgentState::Running
            };
            task.follow_ups.push(FollowUp {
                at_ms: 1,
                text: String::new(),
                instruction_id: None,
                delivery: None,
            });
            let size = serde_json::to_vec(&task).expect("owned task").len();
            task.follow_ups[0].text = "u".repeat(crate::limits::TASK_BYTES - size);
            store::put(tx, &task)?;
            Ok(task)
        })
        .await
        .expect("near-full existing admitted row")
        .0
}
async fn claude(f: &Fixture, payload: serde_json::Value) -> Result<Done> {
    let launch = f
        .svc
        .launch_binding("a")
        .await
        .expect("owned binding")
        .launch_id;
    f.svc
        .event(AgentEventParams {
            agent_id: "a".into(),
            launch_id: launch,
            source: HookSource::Claude,
            payload: payload.to_string(),
            code: None,
        })
        .await
}

#[tokio::test]
async fn large_claude_attention_preserves_transition_and_near_full_user_history() {
    for (event, state) in [
        (
            serde_json::json!({"hook_event_name":"Stop","last_assistant_message":"\u{0000}가".repeat(100_000)}),
            AgentState::Idle,
        ),
        (
            serde_json::json!({"hook_event_name":"Notification","notification_type":"agent_needs_input","message":"\u{0000}가".repeat(100_000)}),
            AgentState::Waiting,
        ),
        (
            serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"\u{0000}가".repeat(100_000)}),
            AgentState::Waiting,
        ),
    ] {
        let f = Fixture::new().await;
        f.svc
            .update("a", |task| {
                task.session_id = Some("legacy".repeat(2048));
                true
            })
            .await
            .expect("existing legacy identity");
        let before = near_full(&f, false).await;
        claude(&f, event)
            .await
            .expect("large hook persists lifecycle transition");
        let after = f
            .svc
            .get("a")
            .await
            .expect("bounded stored row remains readable");
        assert_eq!(after.state, state);
        assert_eq!(after.prompt, before.prompt);
        assert_eq!(
            after.session_id, before.session_id,
            "legacy identity is never truncated"
        );
        assert_eq!(after.follow_ups, before.follow_ups);
        let attention = after.attention.as_ref().expect("bounded attention");
        assert!(
            serde_json::to_vec(&attention.message)
                .expect("attention")
                .len()
                <= 1024
        );
        assert!(
            attention.message.contains("생략"),
            "explicit truncation marker"
        );
        assert!(after.activity_seq > before.activity_seq);
        assert!(
            serde_json::to_vec(&after).expect("stored row").len()
                <= crate::limits::STORED_TASK_BYTES
        );
        let page = f
            .svc
            .list(ListAgents::default())
            .await
            .expect("bounded page");
        assert_eq!(page.items[0], after);
        assert!(serde_json::to_vec(&page).expect("page").len() <= crate::limits::PAGE_BYTES);
    }
}

#[tokio::test]
async fn exact_claude_session_is_preserved_and_oversized_or_unsafe_identity_is_rejected() {
    let f = Fixture::new().await;
    let before = near_full(&f, true).await;
    let exact = "s".repeat(128);
    claude(
        &f,
        serde_json::json!({"hook_event_name":"SessionStart","session_id":exact}),
    )
    .await
    .expect("exact bounded identity");
    let after = f.svc.get("a").await.expect("read stored session");
    assert_eq!(after.state, AgentState::Running);
    assert_eq!(after.session_id.as_deref(), Some(exact.as_str()));
    assert_eq!(after.follow_ups, before.follow_ups);
    for invalid in ["s".repeat(129), "../other".into(), "bad\u{0000}id".into()] {
        let revision = f.svc.db.rev();
        let error = claude(
            &f,
            serde_json::json!({"hook_event_name":"SessionStart","session_id":invalid}),
        )
        .await
        .expect_err("typed invalid identity rejection");
        assert!(
            matches!(
                error,
                Fail::Domain(AgentsError::InputTooLarge { .. } | AgentsError::InvalidInput)
            ),
            "{error:?}"
        );
        assert_eq!(f.svc.db.rev(), revision);
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), after);
    }
}

#[tokio::test]
async fn verified_codex_large_attention_preserves_identity_turn_and_near_full_history() {
    let f = Fixture::new().await;
    f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    let before = near_full(&f, false).await;
    let payload = serde_json::json!({"type":"agent-turn-complete","thread-id":"primary","turn-id":"one","cwd":f.workdir,"last-assistant-message":"\u{0000}가".repeat(100_000)});
    f.svc
        .event(AgentEventParams {
            agent_id: "a".into(),
            source: HookSource::Codex,
            launch_id: None,
            payload: payload.to_string(),
            code: None,
        })
        .await
        .expect("verified bounded completion");
    let after = f.svc.get("a").await.expect("read completed task");
    assert_eq!(after.state, AgentState::Idle);
    assert_eq!(after.session_id.as_deref(), Some("primary"));
    assert_eq!(after.follow_ups, before.follow_ups);
    let message = &after.attention.as_ref().expect("attention").message;
    assert!(serde_json::to_vec(message).expect("message").len() <= 1024);
    assert!(message.contains("생략"));
    let binding = f.svc.launch_binding("a").await.expect("binding");
    assert!(binding.matches("primary", "one"));
    let rev = f.svc.db.rev();
    assert_eq!(
        f.complete("primary", "stale")
            .await
            .expect("stale no-op")
            .rev,
        rev.to_string()
    );
    assert_eq!(f.svc.get("a").await.expect("preserved completion"), after);
}

#[tokio::test]
async fn oversized_codex_candidate_and_metadata_ids_are_rejected_without_truncation() {
    let f = Fixture::new().await;
    for (thread, turn) in [
        ("x".repeat(129), "one".into()),
        ("primary".into(), "x".repeat(129)),
    ] {
        let rev = f.svc.db.rev();
        let error = f
            .complete(&thread, &turn)
            .await
            .expect_err("reject oversized candidate before reading metadata");
        assert!(
            matches!(error, Fail::Domain(AgentsError::InputTooLarge { .. })),
            "{error:?}"
        );
        assert_eq!(f.svc.db.rev(), rev);
    }
    let path = f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    for field in ["id", "turn_id"] {
        f.rollout(
            "primary",
            "user",
            "2026-10-13T00:00:01Z",
            "one",
            "2026-10-13T00:00:02Z",
        );
        let text = fs::read_to_string(&path).expect("owned rollout");
        let mut lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("owned record"))
            .collect();
        lines[usize::from(field == "turn_id")]["payload"][field] = "x".repeat(129).into();
        fs::write(
            &path,
            lines
                .into_iter()
                .map(|line| line.to_string() + "\n")
                .collect::<String>(),
        )
        .expect("owned metadata");
        let task = f.svc.get("a").await.expect("task");
        let revision = f.svc.db.rev();
        let error = f
            .svc
            .refresh_binding(&task, false)
            .await
            .expect_err("reject oversized discovered identity");
        assert!(
            matches!(
                error,
                Fail::Domain(AgentsError::SessionMetadata {
                    diagnostic: Diagnostic::LimitExceeded
                })
            ),
            "{error:?}"
        );
        assert_eq!(f.svc.db.rev(), revision);
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), task);
    }
}

#[tokio::test]
async fn metadata_growth_keeps_large_rows_paged_and_the_stored_ceiling_finite() {
    let f = Fixture::new().await;
    near_full(&f, false).await;
    claude(&f, serde_json::json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"large".repeat(1000)})).await.expect("bounded metadata growth");
    let original = f.svc.get("a").await.expect("read metadata row");
    assert!(serde_json::to_vec(&original).expect("row").len() > crate::limits::TASK_BYTES);
    f.svc
        .db
        .write(move |tx| {
            let mut second = original.clone();
            second.id = "b".into();
            store::put(tx, &second)?;
            second.follow_ups[0].text.push_str(&"u".repeat(32 * 1024));
            assert!(
                store::put(tx, &second).is_err(),
                "stored ceiling remains finite"
            );
            Ok(())
        })
        .await
        .expect("second owned row");
    let first = f.svc.list(ListAgents::default()).await.expect("first page");
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.next_after_id.as_deref(), Some("a"));
    let second = f
        .svc
        .list(ListAgents {
            include_closed_hours: None,
            after_id: first.next_after_id.clone(),
        })
        .await
        .expect("second page");
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].id, "b");
    assert!(second.next_after_id.is_none());
    for page in [first, second] {
        assert!(serde_json::to_vec(&page).expect("page").len() <= crate::limits::PAGE_BYTES);
    }
}
