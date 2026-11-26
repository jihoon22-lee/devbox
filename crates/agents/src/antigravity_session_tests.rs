//! Which conversation an Antigravity task follows: the one a resume named, a new one started
//! inside agy, and the transcript usage is read from.
use super::*;

#[tokio::test]
async fn a_resumed_conversation_is_followed_and_another_one_fails_the_task() {
    // The resume named the task's conversation and agy continued it: PreInvocation alone.
    let f = Fixture::new().await;
    running(&f, Some(CONVERSATION), true).await;
    assert!(
        f.svc
            .launch_binding("agy")
            .await
            .expect("b")
            .unconfirmed_session
    );
    hook(&f, pre_invocation()).await;
    assert_eq!(task(&f).await.state, AgentState::Running);
    let binding = f.svc.launch_binding("agy").await.expect("binding");
    assert!(!binding.resumed_session && !binding.unconfirmed_session);
    hook(&f, stop()).await;
    assert_eq!(task(&f).await.state, AgentState::Idle);
    assert_eq!(task(&f).await.session_id.as_deref(), Some(CONVERSATION));

    // agy did not know the id and opened a new conversation without a word.
    let f = Fixture::new().await;
    running(&f, Some(OTHER), true).await;
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    hook(&f, session_start()).await;
    let failed = task(&f).await;
    assert_eq!(failed.state, AgentState::Failed);
    assert_eq!(failed.reason.as_deref(), Some("session_mismatch"));
    // Never rebound: the task keeps the conversation it was resumed for, and the tool, which
    // still runs in its pane, is not marked as ended.
    assert_eq!(failed.session_id.as_deref(), Some(OTHER));
    assert!(!failed.tool_exited);
    let shown = failed.attention.clone().expect("attention");
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        (
            "failed",
            "도구가 이 작업의 대화가 아닌 다른 대화를 열었습니다"
        )
    );
    use futures::FutureExt;
    assert!(matches!(attention.recv().now_or_never(), Some(Some(_))));
    // The rest of that conversation's turn changes nothing: not running, not idle, no transcript.
    for payload in [pre_invocation(), stop(), session_start()] {
        hook(&f, payload).await;
    }
    assert_eq!(task(&f).await, failed);
    let binding = f.svc.launch_binding("agy").await.expect("binding");
    assert_eq!(binding.transcript, None);
    assert!(
        binding.unconfirmed_session,
        "still not the task's conversation"
    );
    // An instruction sent afterwards runs in that other conversation too.
    f.svc
        .update("agy", |task| {
            apply_event(task, &AgentEvent::UserInstructed, now_ms())
        })
        .await
        .expect("instructed");
    assert_eq!(task(&f).await.state, AgentState::Running);
    hook(&f, pre_invocation()).await;
    let again = task(&f).await;
    assert_eq!(
        (
            again.state,
            again.reason.as_deref(),
            again.session_id.as_deref()
        ),
        (AgentState::Failed, Some("session_mismatch"), Some(OTHER))
    );
}

/// A hook payload of `event` for the conversation `id`, whose transcript is in the fixture's home.
fn payload_of(f: &Fixture, event: &str, id: &str) -> (String, PathBuf) {
    let transcript = f
        .root
        .path()
        .join(".gemini/antigravity-cli/brain")
        .join(id)
        .join(".system_generated/logs/transcript_full.jsonl");
    let payload = serde_json::json!({
        "hookEventName": event,
        "conversationId": id,
        "transcriptPath": transcript,
    });
    (payload.to_string(), transcript)
}

#[tokio::test]
async fn only_a_new_conversation_started_inside_agy_becomes_the_tasks_session() {
    // A first launch: its conversation is recorded by its first hooks.
    let f = Fixture::new().await;
    running(&f, None, true).await;
    let (first, first_transcript) = payload_of(&f, "PreInvocation", CONVERSATION);
    hook(&f, &first).await;
    hook(&f, &payload_of(&f, "Stop", CONVERSATION).0).await;
    let idle = task(&f).await;
    assert_eq!(
        (idle.state, idle.session_id.as_deref()),
        (AgentState::Idle, Some(CONVERSATION))
    );
    let binding = f.svc.launch_binding("agy").await.expect("binding");
    assert_eq!(binding.transcript, Some(first_transcript.clone()));
    // A report of another conversation that does not start one (a sub-agent may report its own,
    // never seen with agy 1.5.17) changes nothing: not the session, the transcript or the state.
    let before = (task(&f).await, binding);
    for event in ["PreInvocation", "Stop"] {
        hook(&f, &payload_of(&f, event, OTHER).0).await;
        let after = (
            task(&f).await,
            f.svc.launch_binding("agy").await.expect("binding"),
        );
        assert_eq!(after, before, "{event}");
    }
    // The user starts a new conversation inside agy (`/new`): its `SessionStart` names it. The
    // task follows it, with its transcript, and is not failed.
    let (second, second_transcript) = payload_of(&f, "SessionStart", OTHER);
    hook(&f, &second).await;
    hook(&f, &payload_of(&f, "PreInvocation", OTHER).0).await;
    let moved = task(&f).await;
    assert_eq!(
        (
            moved.state,
            moved.session_id.as_deref(),
            moved.reason.as_deref()
        ),
        (AgentState::Running, Some(OTHER), None)
    );
    let binding = f.svc.launch_binding("agy").await.expect("binding");
    assert_eq!(binding.transcript, Some(second_transcript.clone()));
    hook(&f, &payload_of(&f, "Stop", OTHER).0).await;
    assert_eq!(task(&f).await.state, AgentState::Idle);
    // The first conversation's reports are another conversation's now.
    hook(&f, &first).await;
    assert_eq!(task(&f).await.state, AgentState::Idle);
    assert_eq!(task(&f).await.session_id.as_deref(), Some(OTHER));
    // A new conversation whose payload names no usable transcript drops the old one: usage never
    // reads another conversation's file.
    let bare =
        serde_json::json!({ "hookEventName": "SessionStart", "conversationId": CONVERSATION });
    hook(&f, &bare.to_string()).await;
    assert_eq!(task(&f).await.session_id.as_deref(), Some(CONVERSATION));
    assert_eq!(
        f.svc.launch_binding("agy").await.expect("b").transcript,
        None
    );

    // A resumed launch, once its first hook confirmed the task's conversation, ignores another
    // conversation's reports and follows a new one the same way.
    let f = Fixture::new().await;
    running(&f, Some(CONVERSATION), true).await;
    hook(&f, pre_invocation()).await;
    hook(&f, stop()).await;
    hook(&f, &payload_of(&f, "PreInvocation", OTHER).0).await;
    let kept = task(&f).await;
    assert_eq!(
        (kept.state, kept.session_id.as_deref()),
        (AgentState::Idle, Some(CONVERSATION))
    );
    hook(&f, &payload_of(&f, "SessionStart", OTHER).0).await;
    assert_eq!(task(&f).await.session_id.as_deref(), Some(OTHER));
}

#[tokio::test]
async fn only_a_transcript_in_the_users_agy_data_is_kept_for_usage() {
    let f = Fixture::new().await;
    running(&f, None, true).await;
    // The fixture's path is another home's: not kept.
    hook(&f, session_start()).await;
    assert_eq!(
        f.svc.launch_binding("agy").await.expect("b").transcript,
        None
    );
    let named = |path: &Path| {
        serde_json::json!({
            "hookEventName": "PreInvocation",
            "conversationId": CONVERSATION,
            "transcriptPath": path,
        })
        .to_string()
    };
    for outside in [
        f.root.path().join("elsewhere/transcript_full.jsonl"),
        f.root
            .path()
            .join(".gemini/antigravity-cli/../../.ssh/id_ed25519"),
        PathBuf::from("/etc/passwd"),
    ] {
        hook(&f, &named(&outside)).await;
        assert_eq!(
            f.svc.launch_binding("agy").await.expect("b").transcript,
            None,
            "{outside:?}"
        );
    }
    let logs = f
        .root
        .path()
        .join(".gemini/antigravity-cli/brain")
        .join(CONVERSATION)
        .join(".system_generated/logs");
    fs::create_dir_all(&logs).expect("owned agy data");
    let transcript = logs.join("transcript_full.jsonl");
    fs::write(
        &transcript,
        fixture(include_str!(
            "../tests/fixtures/antigravity-1.5.17/transcript_full.jsonl"
        )),
    )
    .expect("owned transcript");
    hook(&f, &named(&transcript)).await;
    assert_eq!(
        f.svc.launch_binding("agy").await.expect("b").transcript,
        Some(transcript)
    );
    // The total the list shows is read from it, as `agents.usage` reads it.
    let usage = f.svc.usage("agy").await.expect("usage");
    let tokens = usage.tokens.expect("tokens");
    assert_eq!(
        (
            tokens.input,
            tokens.output,
            tokens.cache_read,
            tokens.cache_write
        ),
        (42_546, 2_582, 180_479, 0)
    );
    assert!(!usage.truncated);
}
