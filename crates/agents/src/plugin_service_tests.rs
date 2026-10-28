//! Reports from the opencode plugin through the service, without a pane or a real CLI.
use super::*;
use devbox_protocol::Topic;

/// One plugin report, the way `devbox agent event --from opencode <payload>` delivers it.
async fn report(f: &Fixture, payload: &str) -> Done {
    let params = AgentEventParams {
        agent_id: "a".into(),
        launch_id: Some("any-launch".into()),
        source: HookSource::Opencode,
        payload: payload.into(),
        code: None,
    };
    f.svc.event(params).await.expect("owned plugin report")
}
const NEEDS_BASH: &str = r#"{"v":1,"event":"needsInput","message":"bash"}"#;
const RESOLVED: &str = r#"{"v":1,"event":"inputResolved"}"#;

#[tokio::test]
async fn an_opencode_permission_wait_ends_when_the_plugin_resolves_it() {
    let f = Fixture::new().await;
    report(&f, r#"{"v":1,"event":"working","sessionId":"ses_root"}"#).await;
    report(&f, NEEDS_BASH).await;
    let waiting = f.svc.get("a").await.expect("waiting task");
    assert_eq!(waiting.state, AgentState::Waiting);
    assert_eq!(waiting.session_id.as_deref(), Some("ses_root"));
    assert_eq!(
        waiting.attention.expect("attention").message,
        "opencode 승인 요청: bash"
    );
    // The plugin pairs requests and replies itself: no Claude approval is recorded.
    assert!(!f.svc.approvals.has_pending("a"));
    report(&f, RESOLVED).await;
    assert_eq!(f.state().await, AgentState::Running);
    report(&f, r#"{"v":1,"event":"idle"}"#).await;
    assert_eq!(f.state().await, AgentState::Idle);
}

#[tokio::test]
async fn a_plugin_resume_outside_a_wait_neither_writes_nor_publishes() {
    let f = Fixture::new().await;
    let mut changed = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    for idle in [false, true] {
        if idle {
            report(&f, r#"{"v":1,"event":"idle"}"#).await;
            assert_eq!(f.state().await, AgentState::Idle);
            assert!(changed.recv().await.is_some() && attention.recv().await.is_some());
        }
        let before = f.svc.get("a").await.expect("task");
        let revision = f.svc.db.rev();
        // Another connection holds the write lock, so a write transaction would wait 5 s.
        let lock = rusqlite::Connection::open(&f.svc.paths.db_path).expect("owned database");
        lock.execute_batch("BEGIN IMMEDIATE")
            .expect("owned write lock");
        let resumed = tokio::time::timeout(std::time::Duration::from_secs(1), report(&f, RESOLVED))
            .await
            .expect("no write transaction");
        lock.execute_batch("ROLLBACK").expect("owned write lock");
        assert_eq!(resumed.rev, revision.to_string(), "idle={idle}");
        assert_eq!(f.svc.db.rev(), revision);
        assert_eq!(f.svc.get("a").await.expect("unchanged task"), before);
        let quiet = std::time::Duration::from_millis(20);
        assert!(tokio::time::timeout(quiet, changed.recv()).await.is_err());
        assert!(tokio::time::timeout(quiet, attention.recv()).await.is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn opencode_usage_exports_once_per_session_and_finished_turn() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new().await;
    fs::create_dir_all(&f.workdir).expect("owned workdir");
    // Every lookup in this fixture finds `fixture-bin/agent-cli`; here it plays opencode.
    let home = f.root.path();
    let export = include_str!("../tests/fixtures/opencode-1.19.3/export.json")
        .replace("/tmp/devbox-opencode-fixture/repo", &f.workdir);
    fs::write(home.join("export.json"), export).expect("owned export");
    let cli = home.join("fixture-bin/agent-cli");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$3\" >> {calls}\ncat {export}\n",
        calls = crate::plan::sh_quote(&home.join("calls").to_string_lossy()),
        export = crate::plan::sh_quote(&home.join("export.json").to_string_lossy()),
    );
    fs::write(&cli, script).expect("owned fake opencode");
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).expect("owned mode");
    let calls = || fs::read_to_string(home.join("calls")).unwrap_or_default();
    let session = |id: &'static str| {
        f.svc.update("a", move |t| {
            t.profile = "opencode".into();
            t.session_id = Some(id.into());
            true
        })
    };
    session("ses_taskroot").await.expect("opencode task");
    let summed = Some(Tokens {
        input: 2500,
        output: 24,
        cache_read: 500,
        cache_write: 0,
    });
    for _ in 0..3 {
        let usage = f.svc.usage("a").await.expect("usage");
        assert_eq!(
            (usage.tokens.clone(), usage.truncated),
            (summed.clone(), false)
        );
    }
    assert_eq!(calls(), "ses_taskroot\n");
    // A finished turn may have added tokens: the next read exports again, once.
    report(&f, r#"{"v":1,"event":"idle"}"#).await;
    for _ in 0..2 {
        f.svc.usage("a").await.expect("usage");
    }
    assert_eq!(calls(), "ses_taskroot\nses_taskroot\n");
    // Another session is another export; it is not this fixture's, so it is reported.
    session("ses_taskchild").await.expect("other session");
    let other = f.svc.usage("a").await.expect("usage");
    assert_eq!((other.tokens, other.truncated), (None, true));
    // A session id that could be read as an option never reaches the CLI.
    session("-h").await.expect("odd session");
    let odd = f.svc.usage("a").await.expect("usage");
    assert_eq!((odd.tokens, odd.truncated), (None, false));
    assert_eq!(calls(), "ses_taskroot\nses_taskroot\nses_taskchild\n");
}
