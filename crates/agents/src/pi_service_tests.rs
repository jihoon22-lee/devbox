//! Reports from the pi extension through the service, without a pane or a real CLI.
use super::*;
use devbox_protocol::Topic;
use std::time::Duration;

/// The session devbox assigns a pi task before its first launch.
const ASSIGNED: &str = "6f1c2d3e-4b5a-4c6d-8e7f-001122334455";
const TRUST: &str = r#"{"v":1,"event":"needsInput","message":"project_trust"}"#;
const RESOLVED: &str = r#"{"v":1,"event":"inputResolved"}"#;
const READY: &str = r#"{"v":1,"event":"ready"}"#;
const TRUST_COPY: &str = "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다.";

/// One report the way `devbox agent event --from <source> <payload>` delivers it, from the
/// launch it names (`DEVBOX_AGENT_LAUNCH` in the pane).
fn params(id: &str, source: HookSource, launch: Option<&str>, payload: &str) -> AgentEventParams {
    AgentEventParams {
        agent_id: id.into(),
        launch_id: launch.map(Into::into),
        source,
        payload: payload.into(),
        code: None,
    }
}

/// One extension report to task `id`. The CLI forwards the launch it runs in; only Claude
/// reports are matched against it.
async fn report_to(f: &Fixture, id: &str, payload: &str) -> Done {
    f.svc
        .event(params(id, HookSource::Pi, Some("any-launch"), payload))
        .await
        .expect("owned extension report")
}

async fn report(f: &Fixture, payload: &str) -> Done {
    report_to(f, "a", payload).await
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

/// Makes `launch`, started at `launched_ms`, task `a`'s current launch, as a start or a resume
/// does; the app times it until pi says it is ready.
async fn launch(f: &Fixture, launch: &str, launched_ms: i64) {
    let mut binding = Binding::new(launched_ms);
    binding.launch_id = Some(launch.into());
    f.svc
        .db
        .write(move |tx| store::put_binding(tx, "a", &binding))
        .await
        .expect("owned launch");
    f.svc
        .update("a", move |t| {
            t.starting_since_ms = Some(launched_ms);
            true
        })
        .await
        .expect("owned launch timing");
}

/// Whether a send to task `a` at `now_ms` gets past the check for pi being ready.
async fn may_send(f: &Fixture, now_ms: i64) -> bool {
    let task = f.svc.get("a").await.expect("task");
    match f.svc.check_ready(&task, now_ms).await {
        Ok(()) => true,
        Err(Fail::Domain(AgentsError::ToolStarting)) => false,
        Err(other) => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_pi_trust_question_waits_until_the_user_decides() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    report(&f, TRUST).await;
    let waiting = f.svc.get("a").await.expect("waiting task");
    assert_eq!(waiting.state, AgentState::Waiting);
    let shown = waiting.attention.expect("attention");
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        ("input", TRUST_COPY)
    );
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
    let bad = params("a", HookSource::Pi, Some("any"), &working("../other"));
    assert!(f.svc.event(bad).await.is_err());
    assert_eq!(session().await.as_deref(), Some(moved));
}

#[tokio::test]
async fn a_pi_send_waits_until_its_current_launch_says_pi_is_ready() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    let launched = now_ms().max(f.svc.started_ms);
    launch(&f, "L2", launched).await;
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let starting = || async { f.svc.get("a").await.expect("task").starting_since_ms };
    // Nothing heard from this launch yet.
    assert!(!may_send(&f, launched + 1).await);
    // A report from an earlier launch, or from no launch, opens nothing and changes nothing.
    for launch in [Some("L1"), Some(""), None] {
        let stale = params("a", HookSource::Pi, launch, READY);
        f.svc.event(stale).await.expect("stale report");
        assert!(!may_send(&f, launched + 1).await, "{launch:?}");
        assert_eq!(starting().await, Some(launched), "{launch:?}");
    }
    // This launch's report: sends are taken, and the app stops timing the launch.
    let ready = params("a", HookSource::Pi, Some("L2"), READY);
    f.svc.event(ready).await.expect("ready report");
    assert!(may_send(&f, launched + 1).await);
    assert_eq!(starting().await, None);
    let published = tokio::time::timeout(Duration::from_secs(2), changes.recv()).await;
    assert!(
        matches!(published, Ok(Some(devbox_core::HubMessage::Event { .. }))),
        "{published:?}"
    );
    // The next launch starts closed again, whatever the last one said.
    let relaunched = launched + 5;
    launch(&f, "L3", relaunched).await;
    assert!(!may_send(&f, relaunched + 1).await);
}

#[tokio::test]
async fn a_pi_launch_that_never_says_it_is_ready_takes_sends_after_30_seconds() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    let launched = now_ms().max(f.svc.started_ms);
    launch(&f, "L1", launched).await;
    // Timed with explicit clocks, not by waiting.
    assert!(!may_send(&f, launched + 29_999).await);
    assert!(may_send(&f, launched + 30_000).await);
    // From the moment its setup ends and pi starts, which can be long after the launch.
    let setup = AgentEventParams {
        code: Some(0),
        ..params("a", HookSource::Setup, Some("L1"), "L1")
    };
    f.svc.event(setup).await.expect("setup report");
    let started = f
        .svc
        .launches
        .get("a")
        .and_then(|seen| seen.tool_started_ms)
        .expect("tool start");
    assert!(started >= launched);
    assert!(!may_send(&f, started + 29_999).await);
    assert!(may_send(&f, started + 30_000).await);
    // A launch from before this daemon started counts as ready: its reports may be lost.
    launch(&f, "L0", f.svc.started_ms - 1).await;
    assert!(may_send(&f, f.svc.started_ms).await);
    // Other tools never say they are ready, and are never held for it.
    launch(&f, "L4", launched).await;
    for profile in ["claude", "codex", "opencode"] {
        f.svc
            .update("a", move |t| {
                t.profile = profile.into();
                true
            })
            .await
            .expect("other tool");
        assert!(may_send(&f, launched + 1).await, "{profile}");
    }
}

#[tokio::test]
async fn a_pi_trust_question_is_announced_only_if_still_open_after_two_seconds() {
    let f = Fixture::new().await;
    pi_task(&f).await;
    // Task b is a second pi task; task c uses opencode.
    let task = f.svc.get("a").await.expect("task");
    let (mut b, mut c) = (task.clone(), task);
    b.id = "b".into();
    c.id = "c".into();
    c.profile = "opencode".into();
    f.svc
        .db
        .write(move |tx| {
            for task in [&b, &c] {
                store::put(tx, task)?;
                store::put_binding(tx, &task.id, &Binding::new(1))?;
            }
            Ok(())
        })
        .await
        .expect("owned tasks");
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let notice = |message: Option<devbox_core::HubMessage>| match message {
        Some(devbox_core::HubMessage::Event { payload, .. }) => payload.to_string(),
        other => panic!("{other:?}"),
    };
    // Other tools' waits, and pi's own dialogs, are announced at once as before.
    let permission = r#"{"v":1,"event":"needsInput","kind":"permission","message":"bash"}"#;
    f.svc
        .event(params("c", HookSource::Opencode, None, permission))
        .await
        .expect("opencode report");
    let at_once = tokio::time::timeout(Duration::from_millis(500), attention.recv()).await;
    assert!(notice(at_once.expect("opencode notice")).contains(r#""id":"c""#));
    let dialog = r#"{"v":1,"event":"needsInput","kind":"question","message":"Proceed?"}"#;
    report_to(&f, "b", dialog).await;
    let at_once = tokio::time::timeout(Duration::from_millis(500), attention.recv()).await;
    assert!(notice(at_once.expect("dialog notice")).contains("pi 질문: Proceed?"));
    report_to(&f, "b", RESOLVED).await;
    while tokio::time::timeout(Duration::from_millis(50), changes.recv())
        .await
        .is_ok()
    {}
    // Both tasks wait at once, and the list hears of it at once.
    let asked = std::time::Instant::now();
    report_to(&f, "a", TRUST).await;
    report_to(&f, "b", TRUST).await;
    for id in ["a", "b"] {
        let task = f.svc.get(id).await.expect("task");
        assert_eq!(task.state, AgentState::Waiting, "{id}");
        let listed = tokio::time::timeout(Duration::from_millis(500), changes.recv()).await;
        assert!(notice(listed.expect("list change")).contains(&format!(r#""id":"{id}""#)));
    }
    // a's folder was decided already, so pi went on at once.
    report_to(&f, "a", RESOLVED).await;
    assert_eq!(f.state().await, AgentState::Running);
    // Only b's question is announced, and only once it has waited two seconds.
    let announced = tokio::time::timeout(Duration::from_secs(5), attention.recv()).await;
    let announced = notice(announced.expect("b's notice"));
    assert!(asked.elapsed() >= Duration::from_secs(2), "{announced}");
    assert!(announced.contains(r#""id":"b""#), "{announced}");
    assert!(announced.contains(TRUST_COPY), "{announced}");
    let more = tokio::time::timeout(Duration::from_millis(700), attention.recv()).await;
    assert!(more.is_err(), "{more:?}");
}
