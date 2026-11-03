mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_projects::{AddProject, ListProjects, ProjectsAdd, ProjectsList};
use devbox_protocol::message::ClientKind;
use devbox_protocol::system::{Ping, PingParams};
use support::TestDaemon;

#[tokio::test]
async fn ping_and_project_round_trip_with_events() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let pong = c
        .call::<Ping>(&PingParams {})
        .await
        .expect("complete fixture RPC");
    assert_eq!(pong.version, devbox_cli::VERSION);

    let mut sub = c
        .subscribe("projects.changed")
        .await
        .expect("subscribe to fixture topic");
    let dir = d.home.path().join("projects/alpha");
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    let p = c
        .call::<ProjectsAdd>(&AddProject {
            path: dir.display().to_string(),
        })
        .await
        .expect("complete fixture RPC");
    let ev = sub.next_event().await.expect("receive fixture event");
    assert!(ev.rev.parse::<u64>().expect("parse fixture value") >= 1);
    let list = c
        .call::<ProjectsList>(&ListProjects {})
        .await
        .expect("complete fixture RPC");
    assert_eq!(list.items[0].id, p.id);
}

#[tokio::test]
async fn domain_errors_and_unknown_methods_are_typed() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let e = c
        .call::<ProjectsAdd>(&AddProject {
            path: "relative".into(),
        })
        .await
        .expect_err("fixture operation must fail");
    assert!(
        matches!(e, ClientError::Rpc { ref code, .. } if code == "projects.path_invalid"),
        "{e:?}"
    );
    let e = c
        .raw_call("nope.method", serde_json::json!({}), None)
        .await
        .expect_err("fixture operation must fail");
    assert!(matches!(e, ClientError::Rpc { ref code, .. } if code == "method_not_found"));
}

#[tokio::test]
async fn requests_before_hello_are_refused() {
    let d = TestDaemon::start();
    let c = Client::connect_without_hello(&d.socket)
        .await
        .expect("connect fixture client");
    let e = c
        .raw_call("system.ping", serde_json::json!({}), None)
        .await
        .expect_err("fixture operation must fail");
    assert!(matches!(e, ClientError::Rpc { ref code, .. } if code == "hello_required"));
}

#[tokio::test]
async fn same_client_request_id_runs_a_mutation_once() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let dir = d.home.path().join("projects/beta");
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    let params = serde_json::json!({ "path": dir.display().to_string() });
    let a = c
        .raw_call("projects.add", params.clone(), Some("req-1".into()))
        .await
        .expect("complete fixture RPC");
    let b = c
        .raw_call("projects.add", params, Some("req-1".into()))
        .await
        .expect("complete fixture RPC");
    assert_eq!(
        a, b,
        "second call must return the remembered result, not already_exists"
    );
}

#[tokio::test]
async fn closing_a_channel_stops_its_subscriptions() {
    let d = TestDaemon::start();
    let c = Client::connect(&d.socket, ClientKind::Test, &d.instance)
        .await
        .expect("connect fixture client");
    let mut sub = c
        .subscribe("projects.changed")
        .await
        .expect("subscribe to fixture topic");
    c.close_channel().await.expect("close fixture channel");
    let other = d.client().await;
    let dir = d.home.path().join("projects/gamma");
    std::fs::create_dir_all(&dir).expect("create fixture directory");
    other
        .call::<ProjectsAdd>(&AddProject {
            path: dir.display().to_string(),
        })
        .await
        .expect("complete fixture RPC");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), sub.next_event())
            .await
            .expect("receive fixture event")
            .is_none()
    );
}

#[path = "daemon_cases/channels.rs"]
mod channel_cases;
#[path = "daemon_cases/client.rs"]
mod client_cases;
#[path = "daemon_cases/socket.rs"]
mod socket_cases;

#[tokio::test]
async fn concurrent_connections_share_mutation_result_and_reject_changed_params() {
    let d = TestDaemon::start();
    let (a, b) = (d.client().await, d.client().await);
    let dir = d.home.path().join("concurrent");
    std::fs::create_dir(&dir).expect("create fixture directory");
    let params = serde_json::json!({ "path": dir.display().to_string() });
    let (first, second) = support::bounded(async {
        tokio::join!(
            a.raw_call("projects.add", &params, Some("concurrent".into())),
            b.raw_call("projects.add", &params, Some("concurrent".into()))
        )
    })
    .await;
    assert_eq!(
        first.expect("complete deduplicated fixture mutation"),
        second.expect("complete deduplicated fixture mutation")
    );
    let changed = a
        .raw_call(
            "projects.add",
            serde_json::json!({ "path": "/different" }),
            Some("concurrent".into()),
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(matches!(changed, ClientError::Rpc { code, .. } if code == "invalid_params"));
    let list = a
        .call::<ProjectsList>(&ListProjects {})
        .await
        .expect("complete fixture RPC");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.rev, "1");
}

#[tokio::test]
async fn disconnect_closes_subscriptions_and_calls_promptly_and_restart_recovers_owned_socket() {
    let mut d = TestDaemon::start();
    let client = d.client().await;
    let mut sub = client
        .subscribe("projects.changed")
        .await
        .expect("subscribe to fixture topic");
    d.stop();
    assert!(support::bounded(sub.next_event()).await.is_none());
    assert!(matches!(
        support::bounded(client.call::<Ping>(&PingParams {})).await,
        Err(ClientError::ConnectionLost)
    ));
    d.launch();
    assert_eq!(
        d.client()
            .await
            .call::<Ping>(&PingParams {})
            .await
            .expect("complete fixture RPC")
            .version,
        devbox_cli::VERSION
    );
}

#[tokio::test]
async fn system_info_reports_daemon_os_hostname_when_available() {
    let d = TestDaemon::start();
    let c = d.client().await;
    let info = c
        .call::<devbox_protocol::system::Info>(&PingParams {})
        .await
        .expect("daemon metadata");
    #[cfg(target_os = "linux")]
    assert_eq!(
        info.hostname.as_deref(),
        Some(
            std::fs::read_to_string("/proc/sys/kernel/hostname")
                .expect("OS hostname")
                .trim()
        )
    );
    #[cfg(not(target_os = "linux"))]
    assert!(info.hostname.is_none());
}

/// `system.info.startupProblems` as the app reads it on the wire.
async fn startup_problems(client: &Client) -> Vec<serde_json::Value> {
    let info: serde_json::Value = serde_json::from_str(
        &client
            .raw_call("system.info", serde_json::json!({}), None)
            .await
            .expect("system.info"),
    )
    .expect("system.info JSON");
    info.get("startupProblems")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn config_file(d: &TestDaemon) -> std::path::PathBuf {
    d.paths.config_dir.join("config.toml")
}

#[tokio::test]
async fn daemon_starts_with_a_broken_config_and_reports_problems() {
    let mut d = TestDaemon::fixture();
    d.write_config(
        "[[agents.profiles]]\nid = 'broken'\nlabel = 'Broken'\ncommand = 'broken'\nhooks = 'nonsense'\n",
    );
    d.launch();
    let c = d.client().await;
    c.call::<Ping>(&PingParams {})
        .await
        .expect("daemon answers");
    let problems = startup_problems(&c).await;
    let file = config_file(&d).display().to_string();
    // Only the broken entry is skipped, so the banner must not say profiles were turned off.
    assert!(
        problems.iter().any(|p| p["kind"] == "configSection"
            && p["file"] == file.as_str()
            && p["section"] == "agents.profiles"
            && p["message"].as_str().is_some_and(|m| m.contains("broken"))
            && p["effect"] == "ignored"),
        "{problems:?}"
    );
    let profiles = c
        .call::<devbox_agents::AgentsProfiles>(&PingParams {})
        .await
        .expect("agent profiles");
    assert_eq!(profiles.items, devbox_agents::profile::builtin());
}

#[tokio::test]
async fn broken_config_bytes_are_unchanged_after_a_settings_write_attempt() {
    use sha2::{Digest, Sha256};
    const BROKEN: &str = "[secrets]\nnames = ['KEEP']\n[notes\nvault = '~/kept'\n";
    let mut d = TestDaemon::fixture();
    d.write_config(BROKEN);
    d.launch();
    let app = support::bounded(Client::connect(&d.socket, ClientKind::App, &d.instance))
        .await
        .expect("app client");
    let problems = startup_problems(&app).await;
    let file = config_file(&d);
    assert!(
        matches!(problems.as_slice(), [p] if p["kind"] == "configFile"
            && p["file"] == file.display().to_string().as_str()),
        "{problems:?}"
    );
    // A syntax error turns notes off; the daemon must not fall back to a default vault.
    assert!(!d.home.path().join("notes").exists());
    // Nor can activity read its privacy rules, so it records nothing; the app is shown
    // rules that mask every title.
    let rules = app
        .call::<devbox_activity::ActivityRules>(&devbox_activity::NoParams {})
        .await
        .expect("activity rules");
    assert!(rules.rules.mask_all_titles);
    let key = devbox_secrets::crypto::Key::from_bytes([7; 32]);
    app.raw_call(
        "secrets.bootstrap",
        &devbox_protocol::secrets::Bootstrap {
            key_id: key.id(),
            blob: b"synthetic-sealed-fixture".to_vec(),
        },
        None,
    )
    .await
    .expect("bootstrap fixture key");
    app.raw_call(
        "secrets.unlock",
        &devbox_protocol::secrets::Unlock { key: vec![7; 32] },
        None,
    )
    .await
    .expect("unlock fixture key");
    let refused = app
        .call::<devbox_secrets::Set>(&devbox_secrets::SetSecret {
            name: "TOKEN".into(),
            value: "owned-secret-value".into(),
        })
        .await
        .expect_err("a secret name cannot be added to a broken config.toml");
    let ClientError::Rpc { code, detail, .. } = &refused else {
        panic!("{refused:?}")
    };
    assert_eq!(code, "secrets.config_invalid");
    let detail: serde_json::Value =
        serde_json::from_str(detail.as_deref().expect("refusal detail").get())
            .expect("refusal detail JSON");
    assert_eq!(detail["file"], file.display().to_string().as_str());
    // The activity rules writer refuses the same file, so the rules shown still mask titles.
    let refused = app
        .call::<devbox_activity::ActivitySetRules>(&devbox_activity::SetRules {
            rules: Default::default(),
            idle_threshold_ms: 300_000,
        })
        .await
        .expect_err("activity rules cannot be written into a broken config.toml");
    assert!(
        matches!(&refused, ClientError::Rpc { code, .. } if code == "activity.config_unavailable"),
        "{refused:?}"
    );
    let rules = app
        .call::<devbox_activity::ActivityRules>(&devbox_activity::NoParams {})
        .await
        .expect("activity rules after the refused write");
    assert!(rules.rules.mask_all_titles);
    // Compared with the bytes written before launch: startup left the file alone too.
    assert_eq!(
        Sha256::digest(std::fs::read(&file).expect("broken config")),
        Sha256::digest(BROKEN.as_bytes())
    );
}

#[tokio::test]
async fn valid_toml_with_an_invalid_section_survives_an_activity_rule_write() {
    let mut d = TestDaemon::fixture();
    let block = "# owned profile with a typo\n[[agents.profiles]]\nid = 'typo' # keep\nlable = 'Typo'\ncommand = 'typo'\nhooks = 'none'\n";
    let original = format!("[pc]\nname = 'desk'\n\n{block}");
    d.write_config(&original);
    d.launch();
    let c = d.client().await;
    c.call::<devbox_activity::ActivitySetRules>(&devbox_activity::SetRules {
        rules: devbox_activity::model::privacy::PrivacyRules {
            redact_title_patterns: vec!["secret".into()],
            ..Default::default()
        },
        idle_threshold_ms: 300_000,
    })
    .await
    .expect("rules are written next to the invalid section");
    let after = std::fs::read_to_string(config_file(&d)).expect("written config");
    assert!(after.starts_with(&original), "{after}");
    assert!(after[original.len()..].contains("[activity]"), "{after}");
}

#[tokio::test]
async fn activity_rules_that_do_not_compile_are_reported_until_a_save_that_compiles() {
    use devbox_activity::{ActivityRules, ActivitySetRules, NoParams, SetRules};
    for name in ["config.toml", "config.local.toml"] {
        let mut d = TestDaemon::fixture();
        let file = d.paths.config_dir.join(name);
        std::fs::create_dir_all(&d.paths.config_dir).expect("owned config folder");
        std::fs::write(
            &file,
            "[activity]\nexcluded_title_patterns = ['(unclosed']\n",
        )
        .expect("owned config");
        d.launch();
        let c = d.client().await;
        // Activity records nothing now, so the banner names the file that holds the rule.
        let problems = startup_problems(&c).await;
        assert!(
            matches!(problems.as_slice(), [p] if p["kind"] == "activityRules"
                && p["file"] == file.display().to_string().as_str()
                && p["local"] == (name == "config.local.toml")
                && p["message"].as_str().is_some_and(|m| m.contains("excluded_title_patterns[0]"))),
            "{name}: {problems:?}"
        );
        // The rules stay readable, so 설정 → 활동 shows them to fix.
        let rules = c.call::<ActivityRules>(&NoParams {}).await.expect("rules");
        assert_eq!(rules.rules.excluded_title_patterns, ["(unclosed"]);
        c.call::<ActivitySetRules>(&SetRules {
            rules: devbox_activity::model::privacy::PrivacyRules {
                excluded_title_patterns: vec!["fixed".into()],
                ..Default::default()
            },
            idle_threshold_ms: 300_000,
        })
        .await
        .expect("rules that compile are saved");
        // Recording is back on, so the problem is gone without a restart.
        assert_eq!(
            startup_problems(&c).await,
            [] as [serde_json::Value; 0],
            "{name}"
        );
    }
}

#[tokio::test]
async fn agents_initialize_failure_does_not_stop_the_daemon() {
    use std::os::unix::fs::PermissionsExt;
    let mut d = TestDaemon::fixture();
    // Only this daemon sees the shim; it never starts or contacts a real tmux server.
    let bin = d.home.path().join("fake-tmux-bin");
    std::fs::create_dir(&bin).expect("owned shim folder");
    let failing = d.home.path().join("tmux-list-fails");
    std::fs::write(&failing, b"").expect("owned failure marker");
    std::fs::write(
        bin.join("tmux"),
        format!(
            "#!/bin/sh\nif [ \"$5\" = list-sessions ] && [ -e '{}' ]; then\n  echo 'fake tmux: list failed' >&2\n  exit 1\nfi\nexit 0\n",
            failing.display()
        ),
    )
    .expect("owned shim");
    std::fs::set_permissions(bin.join("tmux"), std::fs::Permissions::from_mode(0o700))
        .expect("owned shim mode");
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").expect("test PATH"),
    )))
    .expect("shim PATH");
    d.extra_env
        .push(("PATH".into(), path.into_string().expect("UTF-8 PATH")));
    d.launch();
    let c = d.client().await;
    c.call::<Ping>(&PingParams {})
        .await
        .expect("daemon answers while tmux fails");
    let problems = startup_problems(&c).await;
    assert!(
        problems.iter().any(|p| p["kind"] == "agentsReconcile"
            && p["message"]
                .as_str()
                .is_some_and(|m| m.contains("fake tmux: list failed"))),
        "{problems:?}"
    );
    std::fs::remove_file(&failing).expect("tmux recovers");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while startup_problems(&c)
        .await
        .iter()
        .any(|p| p["kind"] == "agentsReconcile")
    {
        assert!(
            std::time::Instant::now() < deadline,
            "a background retry clears the problem"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    c.call::<Ping>(&PingParams {})
        .await
        .expect("daemon still answers");
}
