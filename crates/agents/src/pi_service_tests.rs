//! Reports from the pi extension through the service, without a pane or a real CLI.
use super::*;
use devbox_protocol::Topic;

/// The session devbox assigns a pi task before its first launch.
const ASSIGNED: &str = "6f1c2d3e-4b5a-4c6d-8e7f-001122334455";
const TRUST: &str = r#"{"v":1,"event":"needsInput","message":"project_trust"}"#;
const RESOLVED: &str = r#"{"v":1,"event":"inputResolved"}"#;

fn params(payload: &str) -> AgentEventParams {
    // The CLI forwards the launch it runs in; only Claude reports are matched against it.
    AgentEventParams {
        agent_id: "a".into(),
        launch_id: Some("any-launch".into()),
        source: HookSource::Pi,
        payload: payload.into(),
        code: None,
    }
}

/// One extension report, the way `devbox agent event --from pi <payload>` delivers it.
async fn report(f: &Fixture, payload: &str) -> Done {
    f.svc
        .event(params(payload))
        .await
        .expect("owned extension report")
}

fn working(session: &str) -> String {
    format!(r#"{{"v":1,"event":"working","sessionId":"{session}"}}"#)
}

/// Makes task `a` a pi task in the session devbox assigned it.
async fn pi_task(f: &Fixture) {
    f.svc
        .update("a", |t| {
            t.profile = "pi".into();
            t.session_id = Some(ASSIGNED.into());
            true
        })
        .await
        .expect("pi task");
}

#[tokio::test]
async fn a_pi_trust_question_waits_until_the_user_decides() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    report(&f, TRUST).await;
    let waiting = f.svc.get("a").await.expect("waiting task");
    assert_eq!(waiting.state, AgentState::Waiting);
    let shown = waiting.attention.expect("attention");
    let copy = "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다.";
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        ("input", copy)
    );
    match attention.recv().await {
        Some(devbox_core::HubMessage::Event { payload, .. }) => {
            assert!(payload.contains(copy), "{payload}");
        }
        other => panic!("{other:?}"),
    }
    // pi starts its session once the user has decided, and the extension reports that: the
    // wait ends at once, with no Claude approval involved.
    assert!(!f.svc.approvals.has_pending("a"));
    report(&f, RESOLVED).await;
    let decided = f.svc.get("a").await.expect("task");
    assert_eq!(decided.state, AgentState::Running);
    assert!(decided.attention.is_none());
    // A run that starts first ends the wait the same way.
    report(&f, TRUST).await;
    assert_eq!(f.state().await, AgentState::Waiting);
    report(&f, &working(ASSIGNED)).await;
    assert_eq!(f.state().await, AgentState::Running);
}

#[tokio::test]
async fn a_pi_dialog_waits_with_its_own_copy() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    for (payload, copy) in [
        (
            r#"{"v":1,"event":"needsInput","kind":"permission","message":"Allow the fixture command?"}"#,
            "pi 승인 요청: Allow the fixture command?",
        ),
        (
            r#"{"v":1,"event":"needsInput","kind":"question","message":"project_trust"}"#,
            "pi 질문: project_trust",
        ),
        (
            r#"{"v":1,"event":"needsInput","kind":"question"}"#,
            "pi 질문",
        ),
    ] {
        report(&f, payload).await;
        let waiting = f.svc.get("a").await.expect("waiting task");
        assert_eq!(waiting.state, AgentState::Waiting, "{payload}");
        assert_eq!(waiting.attention.expect("attention").message, copy);
        report(&f, RESOLVED).await;
        assert_eq!(f.state().await, AgentState::Running, "{payload}");
    }
    report(&f, r#"{"v":1,"event":"idle"}"#).await;
    assert_eq!(f.state().await, AgentState::Idle);
}

#[tokio::test]
async fn pi_keeps_the_assigned_session_until_a_turn_runs_in_another() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    let session = || async { f.svc.get("a").await.expect("task").session_id };
    // pi runs in the session devbox assigned, so its report names that one.
    report(&f, &working(ASSIGNED)).await;
    assert_eq!(session().await.as_deref(), Some(ASSIGNED));
    // Reports without a session never change it.
    for payload in [TRUST, RESOLVED, r#"{"v":1,"event":"idle"}"#] {
        report(&f, payload).await;
        assert_eq!(session().await.as_deref(), Some(ASSIGNED), "{payload}");
    }
    // The extension names a session only when a turn starts in it, so another id means the user
    // moved pi to another session (/new, /resume or /fork): resume and usage follow it there.
    let moved = "019a2b3c-7d8e-7f00-8000-000000000005";
    report(&f, &working(moved)).await;
    assert_eq!(session().await.as_deref(), Some(moved));
    // An id devbox could not hand back to pi is refused with its whole report.
    assert!(f.svc.event(params(&working("../other"))).await.is_err());
    assert_eq!(session().await.as_deref(), Some(moved));
}
