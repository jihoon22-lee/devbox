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
        params: RawValue::from_string("{}".into()).unwrap(),
        client_request_id: None,
    }
}
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test deadline")
}
async fn receive(wire: &mut Wire) -> ServerMessage {
    parse_json(&bounded(wire.next()).await.unwrap().unwrap()).unwrap()
}
fn daemon(temp: &tempfile::TempDir, started: Arc<tokio::sync::Notify>) -> Arc<Daemon> {
    let services = super::super::build_services_for_test(temp.path());
    let router = Router::new(services.clone())
        .add::<Ping, _, _>(|s, _, _| async move {
            Ok(Pong {
                daemon_id: s.daemon_id.clone(),
                version: s.version.into(),
                uptime_ms: 0,
            })
        })
        .add::<ProjectsScan, _, _>(move |_, _, _| {
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
    let temp = tempfile::tempdir().unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let d = daemon(&temp, started.clone());
    let (client, server) = UnixStream::pair().unwrap();
    let serving = tokio::spawn(connection(server, d.clone(), CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    wire.send(json_frame(7, &hello())).await.unwrap();
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
    .unwrap();
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Subscribed { .. }
    ));
    assert_eq!(d.services.hub.subscriber_count("projects.changed"), 1);
    wire.send(json_frame(7, &request(1, "projects.scan")))
        .await
        .unwrap();
    bounded(started.notified()).await;
    wire.send(control_frame(7, &Control::ChannelClose))
        .await
        .unwrap();
    wire.send(control_frame(7, &Control::ChannelOpen))
        .await
        .unwrap();
    wire.send(json_frame(7, &hello())).await.unwrap();
    // The cancelled old request must not deliver its id=1 reply into this epoch.
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Welcome { .. }
    ));
    wire.send(json_frame(7, &request(1, "system.ping")))
        .await
        .unwrap();
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
    bounded(serving).await.unwrap().unwrap();
    d.dedupe.tracker.close();
    bounded(d.dedupe.tracker.wait()).await;
}

#[tokio::test]
async fn inflight_request_id_collision_closes_connection_and_cleans_state() {
    let temp = tempfile::tempdir().unwrap();
    let started = Arc::new(tokio::sync::Notify::new());
    let d = daemon(&temp, started.clone());
    let (client, server) = UnixStream::pair().unwrap();
    let serving = tokio::spawn(connection(server, d.clone(), CancellationToken::new()));
    let mut wire = Wire::new(client, FrameCodec::default());
    wire.send(json_frame(0, &hello())).await.unwrap();
    receive(&mut wire).await;
    wire.send(json_frame(0, &request(4, "projects.scan")))
        .await
        .unwrap();
    bounded(started.notified()).await;
    wire.send(json_frame(0, &request(4, "system.ping")))
        .await
        .unwrap();
    assert!(bounded(wire.next()).await.is_none());
    assert!(bounded(serving).await.unwrap().is_err());
    d.dedupe.tracker.close();
    bounded(d.dedupe.tracker.wait()).await;
}
