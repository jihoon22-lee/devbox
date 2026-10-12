use crate::support::bounded;
use devbox_cli::client::{Client, ClientError};
use devbox_protocol::frame::FrameCodec;
use devbox_protocol::message::{ClientKind, ClientMessage, ServerMessage, json_frame, parse_json};
use futures::{SinkExt, StreamExt};
use tokio::net::UnixListener;
use tokio_util::codec::Framed;

#[tokio::test]
async fn pending_request_wakes_when_peer_disconnects() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).expect("bind fixture socket");
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture peer");
        let mut framed = Framed::new(stream, FrameCodec::default());
        framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        // Disconnect with a request registered and awaiting its reply.
    });
    let client = Client::connect_without_hello(&path)
        .await
        .expect("connect fixture client");
    assert!(matches!(
        bounded(client.raw_call("system.ping", (), None)).await,
        Err(ClientError::ConnectionLost)
    ));
    peer.await.expect("join fixture peer");
}

#[tokio::test]
async fn immediate_subscription_event_is_registered_and_overflow_reports_resync() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).expect("bind fixture socket");
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture peer");
        let mut framed = Framed::new(stream, FrameCodec::default());
        let hello = framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        assert!(matches!(
            parse_json::<ClientMessage>(&hello).expect("parse fixture message"),
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
            .expect("send fixture message");
        let subscribe = framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        let ClientMessage::Subscribe { id, .. } =
            parse_json(&subscribe).expect("parse fixture message")
        else {
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
            .expect("complete fixture subscription exchange");
        framed
            .feed(json_frame(
                0,
                &ServerMessage::Event {
                    sub_id: 1,
                    rev: "1".into(),
                    payload: serde_json::value::RawValue::from_string(
                        "{\"n\":9007199254740993}".into(),
                    )
                    .expect("encode fixture JSON"),
                },
            ))
            .await
            .expect("encode fixture JSON");
        framed.flush().await.expect("flush fixture messages");
        let barrier = framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        let ClientMessage::Request { id, .. } =
            parse_json(&barrier).expect("parse fixture message")
        else {
            panic!("request")
        };
        for n in 2..302 {
            framed
                .feed(json_frame(
                    0,
                    &ServerMessage::Event {
                        sub_id: 1,
                        rev: n.to_string(),
                        payload: serde_json::value::RawValue::from_string("null".into())
                            .expect("encode fixture JSON"),
                    },
                ))
                .await
                .expect("encode fixture JSON");
        }
        framed
            .feed(json_frame(
                0,
                &ServerMessage::Response {
                    id,
                    result: Some(
                        serde_json::value::RawValue::from_string("null".into())
                            .expect("encode fixture JSON"),
                    ),
                    error: None,
                    diagnostic_id: None,
                },
            ))
            .await
            .expect("encode fixture JSON");
        framed.flush().await.expect("flush fixture messages");
        // Hold the connection open until the caller has consumed Resync.
        framed.next().await;
    });
    let client = Client::connect(&path, ClientKind::Test, "test-fake")
        .await
        .expect("connect fixture client");
    let mut sub = client
        .subscribe("test.events")
        .await
        .expect("subscribe to fixture topic");
    let first = bounded(sub.next_event())
        .await
        .expect("receive fixture event");
    assert_eq!(first.rev, "1");
    assert_eq!(first.payload, "{\"n\":9007199254740993}");
    assert_eq!(
        bounded(client.raw_call("barrier", (), None))
            .await
            .expect("complete fixture RPC"),
        "null"
    );
    assert!(
        bounded(sub.next_event())
            .await
            .expect("receive fixture event")
            .rev
            .is_empty()
    );
    client.close_channel().await.expect("close fixture channel");
    assert!(bounded(sub.next_event()).await.is_none());
    peer.await.expect("complete fixture subscription exchange");
}

#[tokio::test]
async fn client_rejects_mismatched_welcome() {
    for (version, protocol) in [
        ("wrong", devbox_protocol::PROTOCOL_VERSION),
        (devbox_cli::VERSION, devbox_protocol::PROTOCOL_VERSION + 1),
    ] {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("peer.sock");
        let listener = UnixListener::bind(&path).expect("bind fixture socket");
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept fixture peer");
            let mut framed = Framed::new(stream, FrameCodec::default());
            framed
                .next()
                .await
                .expect("receive fixture frame")
                .expect("receive fixture frame");
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
                .expect("send fixture message");
            framed.next().await;
        });
        assert!(matches!(
            bounded(Client::connect(&path, ClientKind::Test, "test-fake")).await,
            Err(ClientError::Protocol("welcome mismatch"))
        ));
        peer.await.expect("join mismatched fixture peer");
    }
}

#[tokio::test]
async fn dropping_subscription_sends_unsubscribe() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let path = temp.path().join("peer.sock");
    let listener = UnixListener::bind(&path).expect("bind fixture socket");
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept fixture peer");
        let mut framed = Framed::new(stream, FrameCodec::default());
        framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
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
            .expect("send fixture message");
        let frame = framed
            .next()
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        let ClientMessage::Subscribe { id, .. } =
            parse_json(&frame).expect("parse fixture message")
        else {
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
            .expect("send fixture message");
        let frame = bounded(framed.next())
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame");
        assert!(matches!(
            parse_json::<ClientMessage>(&frame).expect("parse fixture message"),
            ClientMessage::Unsubscribe { sub_id: 19 }
        ));
    });
    let client = Client::connect(&path, ClientKind::Test, "test-fake")
        .await
        .expect("connect fixture client");
    let sub = client
        .subscribe("test.events")
        .await
        .expect("subscribe to fixture topic");
    drop(sub);
    bounded(peer).await.expect("join fixture peer");
}
