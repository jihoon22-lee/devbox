use crate::support::{TestDaemon, bounded};
use devbox_protocol::frame::{Frame, FrameCodec, FrameKind};
use devbox_protocol::message::{
    ClientKind, ClientMessage, Control, ServerMessage, control_frame, json_frame, parse_json,
};
use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio_util::codec::Framed;

pub fn hello(instance: &str) -> ClientMessage {
    ClientMessage::Hello {
        version: devbox_cli::VERSION.into(),
        protocol: devbox_protocol::PROTOCOL_VERSION,
        client: ClientKind::Test,
        instance: instance.into(),
    }
}
pub async fn wire(d: &TestDaemon) -> Framed<UnixStream, FrameCodec> {
    Framed::new(
        UnixStream::connect(&d.socket)
            .await
            .expect("connect fixture client"),
        FrameCodec::default(),
    )
}
pub async fn receive(wire: &mut Framed<UnixStream, FrameCodec>) -> ServerMessage {
    parse_json(
        &bounded(wire.next())
            .await
            .expect("receive fixture frame")
            .expect("receive fixture frame"),
    )
    .expect("receive fixture frame")
}

#[tokio::test]
async fn mismatched_hello_reports_daemon_identity_before_closing_without_authorizing() {
    let d = TestDaemon::start();
    let client = d.client().await;
    let identity = client
        .call::<devbox_protocol::system::Ping>(&devbox_protocol::system::PingParams {})
        .await
        .expect("read fixture daemon identity");
    for field in ["version", "protocol"] {
        let mut wire = wire(&d).await;
        let mut message = hello(&d.instance);
        let ClientMessage::Hello {
            version, protocol, ..
        } = &mut message
        else {
            unreachable!()
        };
        match field {
            "version" => *version = "wrong".into(),
            _ => *protocol += 1,
        }
        send_hello_and_mutation(&d, &mut wire, message).await;
        let frame = bounded(wire.next())
            .await
            .expect("mismatch must report Welcome before EOF")
            .expect("decode fixture Welcome");
        assert_eq!(frame.channel, 7);
        assert!(
            matches!(
                parse_json::<ServerMessage>(&frame).expect("parse fixture Welcome"),
                ServerMessage::Welcome { version, protocol, daemon_id }
                    if version == identity.version
                        && protocol == devbox_protocol::PROTOCOL_VERSION
                        && daemon_id == identity.daemon_id
            ),
            "{field}"
        );
        assert!(bounded(wire.next()).await.is_none(), "{field}");
    }
    assert_no_projects(&d).await;
}

async fn send_hello_and_mutation(
    d: &TestDaemon,
    wire: &mut Framed<UnixStream, FrameCodec>,
    message: ClientMessage,
) {
    let path = d.home.path().join("unauthorized-project");
    std::fs::create_dir_all(&path).expect("create fixture project");
    wire.feed(json_frame(7, &message))
        .await
        .expect("queue fixture Hello");
    wire.feed(json_frame(
        7,
        &ClientMessage::Request {
            id: 1,
            method: "projects.add".into(),
            params: serde_json::value::to_raw_value(&serde_json::json!({ "path": path }))
                .expect("encode fixture project"),
            client_request_id: None,
        },
    ))
    .await
    .expect("queue fixture mutation");
    wire.flush().await.expect("send pipelined fixture messages");
}

async fn assert_no_projects(d: &TestDaemon) {
    let list = d
        .client()
        .await
        .call::<devbox_projects::ProjectsList>(&devbox_projects::ListProjects {})
        .await
        .expect("list fixture projects");
    assert!(
        list.items.is_empty(),
        "rejected Hello must not authorize mutations"
    );
}

#[tokio::test]
async fn wrong_instance_and_duplicate_hello_close_without_welcome_or_authorizing() {
    let d = TestDaemon::start();
    for duplicate in [false, true] {
        let mut wire = wire(&d).await;
        if duplicate {
            wire.send(json_frame(7, &hello(&d.instance)))
                .await
                .expect("send fixture Hello");
            assert!(matches!(
                receive(&mut wire).await,
                ServerMessage::Welcome { .. }
            ));
        }
        // Identity/duplicate rejection must take precedence over version reporting.
        let mut message = hello(if duplicate { &d.instance } else { "wrong" });
        if let ClientMessage::Hello { version, .. } = &mut message {
            *version = "wrong".into();
        }
        send_hello_and_mutation(&d, &mut wire, message).await;
        assert!(bounded(wire.next()).await.is_none());
    }
    assert_no_projects(&d).await;
}

#[tokio::test]
async fn malformed_frames_close_without_authorizing() {
    let d = TestDaemon::start();
    let mut wire = wire(&d).await;
    wire.send(Frame {
        kind: FrameKind::Json,
        channel: 0,
        body: bytes::Bytes::from_static(b"{"),
    })
    .await
    .expect("send fixture message");
    assert!(bounded(wire.next()).await.is_none());
}

#[tokio::test]
async fn reopen_requires_hello_and_independent_channel_remains_usable() {
    let d = TestDaemon::start();
    let mut wire = wire(&d).await;
    for ch in [7, 8] {
        wire.send(json_frame(ch, &hello(&d.instance)))
            .await
            .expect("send fixture message");
        assert!(matches!(
            receive(&mut wire).await,
            ServerMessage::Welcome { .. }
        ));
    }
    wire.send(json_frame(
        7,
        &ClientMessage::Subscribe {
            id: 1,
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
    wire.send(control_frame(7, &Control::ChannelClose))
        .await
        .expect("send fixture message");
    wire.send(control_frame(7, &Control::ChannelOpen))
        .await
        .expect("send fixture message");
    wire.send(json_frame(
        7,
        &ClientMessage::Request {
            id: 2,
            method: "system.ping".into(),
            params: serde_json::value::RawValue::from_string("{}".into())
                .expect("encode fixture JSON"),
            client_request_id: None,
        },
    ))
    .await
    .expect("encode fixture JSON");
    assert!(
        matches!(receive(&mut wire).await, ServerMessage::Response { error: Some(error), .. } if error.code == "hello_required")
    );
    wire.send(json_frame(
        8,
        &ClientMessage::Request {
            id: 3,
            method: "system.ping".into(),
            params: serde_json::value::RawValue::from_string("{}".into())
                .expect("encode fixture JSON"),
            client_request_id: None,
        },
    ))
    .await
    .expect("encode fixture JSON");
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Response {
            result: Some(_),
            id: 3,
            ..
        }
    ));
}
