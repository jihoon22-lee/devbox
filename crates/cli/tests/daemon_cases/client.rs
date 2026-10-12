use crate::support::bounded;
use devbox_cli::client::{Client, ClientError};
use devbox_protocol::frame::FrameCodec;
use devbox_protocol::message::{ClientKind, ClientMessage, ServerMessage, json_frame, parse_json};
use futures::{SinkExt, StreamExt};
use tokio::net::UnixListener;
use tokio_util::codec::Framed;

#[tokio::test]
async fn pending_request_wakes_when_peer_disconnects() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut framed = Framed::new(stream, FrameCodec::default());
        framed.next().await.unwrap().unwrap();
        // Disconnect with a request registered and awaiting its reply.
    });
    let client = Client::connect_without_hello(&path).await.unwrap();
    assert!(matches!(
        bounded(client.raw_call("system.ping", (), None)).await,
        Err(ClientError::ConnectionLost)
    ));
    peer.await.unwrap();
}

#[tokio::test]
async fn immediate_subscription_event_is_registered_and_overflow_reports_resync() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut framed = Framed::new(stream, FrameCodec::default());
        let hello = framed.next().await.unwrap().unwrap();
        assert!(matches!(
            parse_json::<ClientMessage>(&hello).unwrap(),
            ClientMessage::Hello { .. }
        ));
        framed
            .send(json_frame(
                0,
                &ServerMessage::Welcome {
                    version: devbox_cli::VERSION.into(),
                    protocol: devbox_protocol::PROTOCOL_VERSION,
                    daemon_id: "fake".into(),
                },
            ))
            .await
            .unwrap();
        let subscribe = framed.next().await.unwrap().unwrap();
        let ClientMessage::Subscribe { id, .. } = parse_json(&subscribe).unwrap() else {
            panic!("subscribe")
        };
        framed
            .feed(json_frame(
                0,
                &ServerMessage::Subscribed {
                    id,
                    sub_id: 1,
                    rev: "0".into(),
                },
            ))
            .await
            .unwrap();
        framed
            .feed(json_frame(
                0,
                &ServerMessage::Event {
                    sub_id: 1,
                    rev: "1".into(),
                    payload: serde_json::value::RawValue::from_string(
                        "{\"n\":9007199254740993}".into(),
                    )
                    .unwrap(),
                },
            ))
            .await
            .unwrap();
        framed.flush().await.unwrap();
        let barrier = framed.next().await.unwrap().unwrap();
        let ClientMessage::Request { id, .. } = parse_json(&barrier).unwrap() else {
            panic!("request")
        };
        for n in 2..302 {
            framed
                .feed(json_frame(
                    0,
                    &ServerMessage::Event {
                        sub_id: 1,
                        rev: n.to_string(),
                        payload: serde_json::value::RawValue::from_string("null".into()).unwrap(),
                    },
                ))
                .await
                .unwrap();
        }
        framed
            .feed(json_frame(
                0,
                &ServerMessage::Response {
                    id,
                    result: Some(serde_json::value::RawValue::from_string("null".into()).unwrap()),
                    error: None,
                    diagnostic_id: None,
                },
            ))
            .await
            .unwrap();
        framed.flush().await.unwrap();
        // Hold the connection open until the caller has consumed Resync.
        framed.next().await;
    });
    let client = Client::connect(&path, ClientKind::Test, "test-fake")
        .await
        .unwrap();
    let mut sub = client.subscribe("test.events").await.unwrap();
    let first = bounded(sub.next_event()).await.unwrap();
    assert_eq!(first.rev, "1");
    assert_eq!(first.payload, "{\"n\":9007199254740993}");
    assert_eq!(
        bounded(client.raw_call("barrier", (), None)).await.unwrap(),
        "null"
    );
    assert!(bounded(sub.next_event()).await.unwrap().rev.is_empty());
    client.close_channel().await.unwrap();
    assert!(bounded(sub.next_event()).await.is_none());
    peer.await.unwrap();
}

#[tokio::test]
async fn client_rejects_mismatched_welcome() {
    for (version, protocol) in [
        ("wrong", devbox_protocol::PROTOCOL_VERSION),
        (devbox_cli::VERSION, devbox_protocol::PROTOCOL_VERSION + 1),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("peer.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut framed = Framed::new(stream, FrameCodec::default());
            framed.next().await.unwrap().unwrap();
            framed
                .send(json_frame(
                    0,
                    &ServerMessage::Welcome {
                        version: version.into(),
                        protocol,
                        daemon_id: "fake".into(),
                    },
                ))
                .await
                .unwrap();
            framed.next().await;
        });
        assert!(matches!(
            bounded(Client::connect(&path, ClientKind::Test, "test-fake")).await,
            Err(ClientError::Protocol("welcome mismatch"))
        ));
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn dropping_subscription_sends_unsubscribe() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut framed = Framed::new(stream, FrameCodec::default());
        framed.next().await.unwrap().unwrap();
        framed
            .send(json_frame(
                0,
                &ServerMessage::Welcome {
                    version: devbox_cli::VERSION.into(),
                    protocol: devbox_protocol::PROTOCOL_VERSION,
                    daemon_id: "fake".into(),
                },
            ))
            .await
            .unwrap();
        let frame = framed.next().await.unwrap().unwrap();
        let ClientMessage::Subscribe { id, .. } = parse_json(&frame).unwrap() else {
            panic!("subscribe")
        };
        framed
            .send(json_frame(
                0,
                &ServerMessage::Subscribed {
                    id,
                    sub_id: 19,
                    rev: "0".into(),
                },
            ))
            .await
            .unwrap();
        let frame = bounded(framed.next()).await.unwrap().unwrap();
        assert!(matches!(
            parse_json::<ClientMessage>(&frame).unwrap(),
            ClientMessage::Unsubscribe { sub_id: 19 }
        ));
    });
    let client = Client::connect(&path, ClientKind::Test, "test-fake")
        .await
        .unwrap();
    let sub = client.subscribe("test.events").await.unwrap();
    drop(sub);
    bounded(peer).await.unwrap();
}
