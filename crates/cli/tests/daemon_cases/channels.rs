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
        UnixStream::connect(&d.socket).await.unwrap(),
        FrameCodec::default(),
    )
}
pub async fn receive(wire: &mut Framed<UnixStream, FrameCodec>) -> ServerMessage {
    parse_json(&bounded(wire.next()).await.unwrap().unwrap()).unwrap()
}

#[tokio::test]
async fn invalid_hello_and_malformed_frames_close_without_authorizing() {
    let d = TestDaemon::start();
    for field in ["version", "protocol", "instance"] {
        let mut wire = wire(&d).await;
        let ClientMessage::Hello {
            mut version,
            mut protocol,
            mut instance,
            client,
        } = hello(&d.instance)
        else {
            unreachable!()
        };
        match field {
            "version" => version = "wrong".into(),
            "protocol" => protocol += 1,
            _ => instance = "wrong".into(),
        }
        wire.send(json_frame(
            0,
            &ClientMessage::Hello {
                version,
                protocol,
                instance,
                client,
            },
        ))
        .await
        .unwrap();
        assert!(bounded(wire.next()).await.is_none(), "{field}");
    }
    let mut wire = wire(&d).await;
    wire.send(Frame {
        kind: FrameKind::Json,
        channel: 0,
        body: bytes::Bytes::from_static(b"{"),
    })
    .await
    .unwrap();
    assert!(bounded(wire.next()).await.is_none());
}

#[tokio::test]
async fn reopen_requires_hello_and_independent_channel_remains_usable() {
    let d = TestDaemon::start();
    let mut wire = wire(&d).await;
    for ch in [7, 8] {
        wire.send(json_frame(ch, &hello(&d.instance)))
            .await
            .unwrap();
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
    .unwrap();
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Subscribed { .. }
    ));
    wire.send(control_frame(7, &Control::ChannelClose))
        .await
        .unwrap();
    wire.send(control_frame(7, &Control::ChannelOpen))
        .await
        .unwrap();
    wire.send(json_frame(
        7,
        &ClientMessage::Request {
            id: 2,
            method: "system.ping".into(),
            params: serde_json::value::RawValue::from_string("{}".into()).unwrap(),
            client_request_id: None,
        },
    ))
    .await
    .unwrap();
    assert!(
        matches!(receive(&mut wire).await, ServerMessage::Response { error: Some(error), .. } if error.code == "hello_required")
    );
    wire.send(json_frame(
        8,
        &ClientMessage::Request {
            id: 3,
            method: "system.ping".into(),
            params: serde_json::value::RawValue::from_string("{}".into()).unwrap(),
            client_request_id: None,
        },
    ))
    .await
    .unwrap();
    assert!(matches!(
        receive(&mut wire).await,
        ServerMessage::Response {
            result: Some(_),
            id: 3,
            ..
        }
    ));
}
