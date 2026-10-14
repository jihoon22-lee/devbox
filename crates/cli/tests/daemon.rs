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
