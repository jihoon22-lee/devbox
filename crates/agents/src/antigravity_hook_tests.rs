//! Any hook of the launch means agy took an instruction, so a first instruction still queued for
//! it is never sent, whatever else the hook's payload holds. The rest of a payload devbox does
//! not read is refused as before.
use super::*;

async fn queued(f: &Fixture) {
    f.svc
        .update("agy", |task| {
            crate::first_instruction::set(task, InstructionDelivery::Queued, 1);
            true
        })
        .await
        .expect("queued");
}

/// The task and its launch's binding.
async fn state(f: &Fixture) -> (AgentTask, Binding) {
    (
        task(f).await,
        f.svc.launch_binding("agy").await.expect("binding"),
    )
}

#[tokio::test]
async fn a_hook_of_the_launch_cancels_a_queued_first_instruction_whatever_its_payload() {
    let long = serde_json::json!({ "hookEventName": "Stop", "conversationId": "x".repeat(129) });
    let odd = serde_json::json!({ "hookEventName": "Stop", "conversationId": "../../etc" });
    for (payload, refused) in [
        // An event devbox does not register, and one agy does not have.
        (post_invocation().to_owned(), None),
        (
            serde_json::json!({ "hookEventName": "Bogus", "conversationId": CONVERSATION })
                .to_string(),
            None,
        ),
        // No conversation, or an empty one.
        (r#"{"hookEventName":"Stop"}"#.to_owned(), None),
        (
            r#"{"hookEventName":"Stop","conversationId":""}"#.to_owned(),
            None,
        ),
        // What `devbox agent event` sends for a payload it could not read or that was too long.
        (String::new(), None),
        ("not json".to_owned(), None),
        // A conversation id that is not one is still refused, after the cancel.
        (long.to_string(), Some("too large")),
        (odd.to_string(), Some("invalid")),
    ] {
        let f = Fixture::new().await;
        running(&f, None, true).await;
        queued(&f).await;
        let before = state(&f).await;
        let result = hook_from(&f, Some("launch"), &payload).await;
        match (refused, &result) {
            (None, Ok(_)) => {}
            (Some("too large"), Err(Fail::Domain(AgentsError::InputTooLarge { .. }))) => {}
            (Some("invalid"), Err(Fail::Domain(AgentsError::InvalidInput))) => {}
            other => panic!("{payload}: {other:?}"),
        }
        let (mut after, binding) = state(&f).await;
        assert!(
            crate::first_instruction::is(&after, InstructionDelivery::NotSent),
            "{payload}"
        );
        // Nothing else of the payload reached the task or the launch.
        after.first_instruction = before.0.first_instruction;
        assert_eq!((after, binding), before, "{payload}");
    }
}

#[tokio::test]
async fn a_hook_of_another_launch_or_of_none_cancels_nothing() {
    let f = Fixture::new().await;
    running(&f, None, true).await;
    queued(&f).await;
    let before = state(&f).await;
    for launch in [None, Some(""), Some("first")] {
        for payload in [session_start(), pre_invocation(), stop(), ""] {
            hook_from(&f, launch, payload).await.expect("ignored");
        }
    }
    assert_eq!(state(&f).await, before);
}
