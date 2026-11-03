//! Reports of the opencode plugin and the pi extension belong to the launch they come from. Their
//! `devbox agent event` children can finish seconds after the reporting process died, so after a
//! resume a late report of the old launch must not change the task of the new one.
use super::*;

const WORKING: &str = r#"{"v":1,"event":"working"}"#;
const ASKS: &str = r#"{"v":1,"event":"needsInput","kind":"permission","message":"bash"}"#;
const QUESTION: &str = r#"{"v":1,"event":"needsInput","kind":"question","message":"Which file?"}"#;
const RESOLVED: &str = r#"{"v":1,"event":"inputResolved"}"#;
const IDLE: &str = r#"{"v":1,"event":"idle"}"#;

/// Makes `launch` task `a`'s current launch, as a start or a resume does.
async fn bind(f: &Fixture, launch: &str) {
    let mut binding = Binding::new(now_ms());
    binding.launch_id = Some(launch.into());
    f.svc
        .db
        .write(move |tx| store::put_binding(tx, "a", &binding))
        .await
        .expect("owned launch");
}

/// One report of `source` from `launch`, as `devbox agent event` sends it from that pane.
async fn report(f: &Fixture, source: HookSource, launch: Option<&str>, payload: &str) {
    f.svc
        .event(AgentEventParams {
            agent_id: "a".into(),
            launch_id: launch.map(Into::into),
            source,
            payload: payload.into(),
            code: None,
        })
        .await
        .expect("owned report");
}

#[tokio::test]
async fn a_late_report_of_the_launch_before_a_resume_changes_nothing() {
    for (source, profile) in [(HookSource::Opencode, "opencode"), (HookSource::Pi, "pi")] {
        let f = Fixture::new().await;
        f.svc
            .update("a", move |t| {
                t.profile = profile.into();
                true
            })
            .await
            .expect("plugin task");
        // The user resumes the task while its first launch waits for an approval.
        bind(&f, "old").await;
        report(&f, source, Some("old"), ASKS).await;
        assert_eq!(f.state().await, AgentState::Waiting, "{profile}");
        bind(&f, "new").await;
        report(&f, source, Some("new"), WORKING).await;
        report(&f, source, Some("new"), ASKS).await;
        let waiting = f.svc.get("a").await.expect("waiting task");
        assert_eq!(waiting.state, AgentState::Waiting, "{profile}");
        // The old launch's last reports arrive now: they neither end the new launch's wait nor
        // change anything about the task.
        let revision = f.svc.db.rev();
        for stale in [RESOLVED, IDLE, WORKING, QUESTION] {
            report(&f, source, Some("old"), stale).await;
            let task = f.svc.get("a").await.expect("task");
            assert_eq!(task, waiting, "{profile}: {stale}");
        }
        assert_eq!(f.svc.db.rev(), revision, "{profile}");
        // The new launch's own reports still apply, and its wait ends on its resolution.
        report(&f, source, Some("new"), RESOLVED).await;
        assert_eq!(f.state().await, AgentState::Running, "{profile}");
        // A late request of the old launch starts no wait, and its late idle ends no turn.
        for stale in [ASKS, IDLE] {
            report(&f, source, Some("old"), stale).await;
            assert_eq!(f.state().await, AgentState::Running, "{profile}: {stale}");
        }
        report(&f, source, Some("new"), IDLE).await;
        assert_eq!(f.state().await, AgentState::Idle, "{profile}");
        // A report that names no launch is applied as before, so no wait is lost for it.
        for unnamed in [None, Some("")] {
            report(&f, source, Some("new"), IDLE).await;
            report(&f, source, unnamed, ASKS).await;
            assert_eq!(
                f.state().await,
                AgentState::Waiting,
                "{profile}: {unnamed:?}"
            );
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_late_idle_of_the_launch_before_a_resume_leaves_the_usage_read() {
    let f = Fixture::new().await;
    super::plugin_service_tests::fake_opencode(&f);
    super::plugin_service_tests::opencode_session(&f, "ses_taskroot").await;
    let calls = || {
        fs::read_to_string(f.root.path().join("calls"))
            .unwrap_or_default()
            .lines()
            .count()
    };
    bind(&f, "old").await;
    bind(&f, "new").await;
    f.svc.usage("a").await.expect("usage");
    assert_eq!(calls(), 1);
    // The old launch's late idle ends no turn of the new launch, so the usage read stands.
    report(&f, HookSource::Opencode, Some("old"), IDLE).await;
    f.svc.usage("a").await.expect("usage");
    assert_eq!(calls(), 1);
    // The new launch's idle may have added tokens: the next read exports again.
    report(&f, HookSource::Opencode, Some("new"), IDLE).await;
    f.svc.usage("a").await.expect("usage");
    assert_eq!(calls(), 2);
}
