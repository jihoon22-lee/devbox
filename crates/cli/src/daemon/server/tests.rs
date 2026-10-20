use super::*;
use devbox_projects::{ProjectsScan, ScanResult};
use devbox_protocol::message::{ClientKind, control_frame};
use devbox_protocol::system::{Ping, Pong};
use std::time::Duration;

type Wire = Framed<UnixStream, FrameCodec>;
fn hello() -> ClientMessage {
    ClientMessage::Hello {
        version: crate::VERSION.into(),
        protocol: PROTOCOL_VERSION,
        client: ClientKind::Test,
        instance: "test-r".into(),
    }
}
fn request(id: u32, method: &str) -> ClientMessage {
    ClientMessage::Request {
        id,
        method: method.into(),
        params: RawValue::from_string("{}".into()).expect("encode fixture JSON"),
        client_request_id: None,
    }
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test deadline")
}
async fn receive(wire: &mut Wire) -> ServerMessage {
    parse_json(
        &bounded(wire.next())
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame"),
    )
    .expect("receive fixture frame")
}
fn daemon(temp: &tempfile::TempDir, started: Arc<tokio::sync::Notify>) -> Arc<Daemon> {
    let services = super::super::build_services_for_test(temp.path());
    let router = Router::new(services.clone())
        .register::<Ping, _, _>(|s, _, _| async move {
            Ok(Pong {
                daemon_id: s.daemon_id.clone(),
                version: s.version.into(),
                uptime_ms: 0,
            })
        })
        .register::<ProjectsScan, _, _>(move |_, _, _| {
            let started = started.clone();
            async move {
                started.notify_one();
                std::future::pending::<
                    Result<ScanResult, devbox_protocol::Fail<devbox_projects::ProjectsError>>,
                >()
                .await
            }
        });
    Arc::new(Daemon {
        router,
        services,
        topics: routes::declared().topics.into_iter().collect(),
        dedupe: Dedupe::default(),
    })
}

#[tokio::test]
async fn close_reopen_cancels_old_requests_and_subscriptions_without_old_replies() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let started = Arc::new(tokio::sync::Notify::new());
    let d = daemon(&temp, started.clone());
    let (client, server) = UnixStream::pair().expect("create fixture socket pair");
    let serving = tokio::spawn(connection(server, d.clone(), CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    wire.send(json_frame(7, &hello()))
        .await
        .expect("send fixture message");
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Welcome { .. }
    ));
    wire.send(json_frame(
        7,
        &ClientMessage::Subscribe {
            id: 2,
            topic: "projects.changed".into(),
            params: None,
        },
    ))
    .await
    .expect("send fixture message");
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Subscribed { .. }
    ));
    assert_eq!(d.services.hub.subscriber_count("projects.changed"), 1);
    wire.send(json_frame(7, &request(1, "projects.scan")))
        .await
        .expect("send fixture message");
    bounded(started.notified()).await;
    wire.send(control_frame(7, &Control::ChannelClose))
        .await
        .expect("send fixture message");
    wire.send(control_frame(7, &Control::ChannelOpen))
        .await
        .expect("send fixture message");
    wire.send(json_frame(7, &hello()))
        .await
        .expect("send fixture message");
    // The cancelled old request must not deliver its id=1 reply into this epoch.
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Welcome { .. }
    ));
    wire.send(json_frame(7, &request(1, "system.ping")))
        .await
        .expect("send fixture message");
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Response {
            id: 1,
            result: Some(_),
            error: None,
            ..
        }
    ));
    bounded(async {
        while d.services.hub.subscriber_count("projects.changed") != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    drop(wire);
    bounded(serving)
        .await
        .expect("join fixture connection")
        .expect("serve fixture connection");
    d.dedupe.tracker.close();
    bounded(d.dedupe.tracker.wait()).await;
}

#[tokio::test]
async fn inflight_request_id_collision_closes_connection_and_cleans_state() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let started = Arc::new(tokio::sync::Notify::new());
    let d = daemon(&temp, started.clone());
    let (client, server) = UnixStream::pair().expect("create fixture socket pair");
    let serving = tokio::spawn(connection(server, d.clone(), CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    wire.send(json_frame(0, &hello()))
        .await
        .expect("send fixture message");
    receive(&mut wire).await;
    wire.send(json_frame(0, &request(4, "projects.scan")))
        .await
        .expect("send fixture message");
    bounded(started.notified()).await;
    wire.send(json_frame(0, &request(4, "system.ping")))
        .await
        .expect("send fixture message");
    assert!(bounded(wire.next()).await.is_none());
    assert!(
        bounded(serving)
            .await
            .expect("join rejected fixture connection")
            .is_err()
    );
    d.dedupe.tracker.close();
    bounded(d.dedupe.tracker.wait()).await;
}

#[tokio::test]
async fn problems_subscription_ack_uses_memory_rev_even_when_db_rev_is_larger() {
    use devbox_observe::{Problem, ProblemRun, ProblemSeverity};
    let temp = tempfile::tempdir().unwrap();
    let d = daemon(&temp, Arc::new(tokio::sync::Notify::new()));
    for _ in 0..8 {
        d.services.db.write(|_| Ok(())).await.unwrap();
    }
    let (client, server) = UnixStream::pair().unwrap();
    let serving = tokio::spawn(connection(server, d.clone(), CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    wire.send(json_frame(0, &hello())).await.unwrap();
    receive(&mut wire).await;
    wire.send(json_frame(
        0,
        &ClientMessage::Subscribe {
            id: 1,
            topic: "observe.problems.changed".into(),
            params: None,
        },
    ))
    .await
    .unwrap();
    assert!(matches!(receive(&mut wire).await,ServerMessage::Subscribed{rev,..} if rev=="0"));
    d.services
        .observe
        .apply_problem_batch(
            ProblemRun {
                run_id: "one".into(),
                project_id: "p".into(),
                root: temp.path().into(),
                path: temp.path().join("one.log"),
                attempt_id: Some("a".into()),
                started_ms: 1,
            },
            vec![Problem {
                file: "x.ts".into(),
                line: 1,
                col: None,
                severity: ProblemSeverity::Error,
                message: "example".into(),
                source: String::new(),
            }],
            false,
            &d.services.hub,
        )
        .unwrap();
    assert!(matches!(receive(&mut wire).await,ServerMessage::Event{rev,..} if rev=="1"));
    assert!(d.services.db.rev() > d.services.observe.problem_rev());
    drop(wire);
    bounded(serving).await.unwrap().unwrap();
    d.dedupe.tracker.close();
    bounded(d.dedupe.tracker.wait()).await;
}

#[tokio::test]
async fn windows_native_long_poll_does_not_block_renderer_ping_and_routes_are_private() {
    let temp = tempfile::tempdir().unwrap();
    let d = daemon(&temp, Arc::new(tokio::sync::Notify::new()));
    let (client, server) = UnixStream::pair().unwrap();
    let serving = tokio::spawn(connection(server, d, CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    let mut native = hello();
    if let ClientMessage::Hello { client, .. } = &mut native {
        *client = ClientKind::App;
    }
    wire.send(json_frame(0, &native)).await.unwrap();
    receive(&mut wire).await;
    wire.send(json_frame(0, &request(100, "windows.next")))
        .await
        .unwrap();
    wire.send(json_frame(7, &native)).await.unwrap();
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Welcome { .. }
    ));
    for (id, method) in [
        (1, "system.ping"),
        (2, "windows.next"),
        (3, "windows.prepare"),
        (4, "windows.run"),
        (5, "windows.output"),
    ] {
        wire.send(json_frame(7, &request(id, method)))
            .await
            .unwrap();
        let response = receive(&mut wire).await;
        assert!(
            matches!(response, ServerMessage::Response { error: None, .. }) == (id == 1),
            "{method}"
        );
    }
    drop(wire);
    bounded(serving).await.unwrap().unwrap();
}
