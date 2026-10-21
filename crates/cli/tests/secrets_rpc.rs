mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_protocol::{
    frame::FrameCodec,
    message::{ClientKind, ClientMessage, ServerMessage, json_frame, parse_json},
};
use devbox_secrets::{Empty, SetSecret, crypto::Key};
use futures::{SinkExt, StreamExt};
use support::{TestDaemon, bounded};
async fn client(f: &TestDaemon, kind: ClientKind) -> Client {
    bounded(Client::connect(&f.socket, kind, &f.instance))
        .await
        .expect("fixture client")
}
fn rejected(result: Result<String, ClientError>, code: &str) {
    assert!(matches!(result,Err(ClientError::Rpc{code:actual,..}) if actual==code));
}
#[tokio::test]
async fn secret_rpc_keeps_native_keys_off_renderer_mcp_and_public_lists() {
    let f = TestDaemon::start();
    let app = client(&f, ClientKind::App).await;
    let id = Key::from_bytes([7; 32]).id();
    let blob = b"synthetic-sealed-fixture".to_vec();
    app.raw_call(
        "secrets.bootstrap",
        &devbox_protocol::secrets::Bootstrap {
            key_id: id.clone(),
            blob: blob.clone(),
        },
        None,
    )
    .await
    .expect("bootstrap");
    rejected(
        app.raw_call(
            "secrets.bootstrap",
            &devbox_protocol::secrets::Bootstrap {
                key_id: id.clone(),
                blob: vec![99],
            },
            None,
        )
        .await,
        "secrets.key_mismatch",
    );
    rejected(
        app.raw_call(
            "secrets.unlock",
            &devbox_protocol::secrets::Unlock { key: vec![8; 32] },
            None,
        )
        .await,
        "secrets.key_mismatch",
    );
    app.raw_call(
        "secrets.unlock",
        &devbox_protocol::secrets::Unlock { key: vec![7; 32] },
        None,
    )
    .await
    .expect("unlock");
    app.call::<devbox_secrets::Set>(&SetSecret {
        name: "TOKEN".into(),
        value: "owned-secret-value".into(),
    })
    .await
    .expect("set");
    let status: devbox_protocol::secrets::KeyStatus = serde_json::from_str(
        &app.raw_call("secrets.key_status", Empty {}, None)
            .await
            .expect("status"),
    )
    .expect("typed status");
    assert_eq!(status.blob, Some(blob));
    assert_eq!(status.key_id, Some(id));
    for kind in [ClientKind::Test, ClientKind::Cli, ClientKind::Mcp] {
        let other = client(&f, kind).await;
        for method in ["secrets.key_status", "secrets.bootstrap", "secrets.unlock"] {
            rejected(
                other.raw_call(method, Empty {}, None).await,
                "method_not_found",
            );
        }
    }
    for kind in [ClientKind::App, ClientKind::Mcp, ClientKind::Test] {
        let other = client(&f, kind).await;
        rejected(
            other.raw_call("secrets.export_key", Empty {}, None).await,
            "method_not_found",
        );
        rejected(
            other.raw_call("secrets.run", Empty {}, None).await,
            "method_not_found",
        );
    }
    let list = app
        .call::<devbox_secrets::List>(&Empty {})
        .await
        .expect("names only");
    let json = serde_json::to_string(&list).expect("list JSON");
    assert!(json.contains("TOKEN"));
    assert!(!json.contains("owned-secret-value"));
    let cli = client(&f, ClientKind::Cli).await;
    let recovery = cli
        .call::<devbox_secrets::ExportKey>(&Empty {})
        .await
        .expect("explicit export");
    assert_eq!(
        Key::recover(&recovery.recovery).expect("recovery").id(),
        Key::from_bytes([7; 32]).id()
    );
    let output = f
        .environment(env!("CARGO_BIN_EXE_devbox"))
        .args(["secrets", "export-key"])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("explicit CLI export");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout)
            .expect("CLI recovery")
            .trim(),
        recovery.recovery
    );
    let stream = tokio::net::UnixStream::connect(&f.socket)
        .await
        .expect("renderer socket");
    let mut wire = tokio_util::codec::Framed::new(stream, FrameCodec::default());
    wire.send(json_frame(
        1,
        &ClientMessage::Hello {
            version: devbox_cli::VERSION.into(),
            protocol: devbox_protocol::PROTOCOL_VERSION,
            client: ClientKind::App,
            instance: f.instance.clone(),
        },
    ))
    .await
    .expect("renderer hello");
    let _ = bounded(wire.next()).await.expect("welcome").expect("frame");
    wire.send(json_frame(
        1,
        &ClientMessage::Request {
            id: 1,
            method: "secrets.key_status".into(),
            params: serde_json::value::to_raw_value(&Empty {}).expect("params"),
            client_request_id: None,
        },
    ))
    .await
    .expect("renderer request");
    let frame = bounded(wire.next())
        .await
        .expect("response")
        .expect("frame");
    let message = parse_json::<ServerMessage>(&frame).expect("reply");
    assert!(
        matches!(message,ServerMessage::Response{error:Some(e),result:None,..}if e.code=="method_not_found")
    );
    for (request_id, method) in [(2, "secrets.status"), (3, "secrets.export_key")] {
        wire.send(json_frame(
            1,
            &ClientMessage::Request {
                id: request_id,
                method: method.into(),
                params: serde_json::value::to_raw_value(&Empty {}).expect("params"),
                client_request_id: None,
            },
        ))
        .await
        .expect("explicit renderer request");
        let frame = bounded(wire.next())
            .await
            .expect("response")
            .expect("frame");
        let message = parse_json::<ServerMessage>(&frame).expect("reply");
        let ServerMessage::Response {
            result: Some(result),
            error: None,
            ..
        } = message
        else {
            panic!("renderer metadata and explicit export must be allowed");
        };
        if method == "secrets.status" {
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(result.get()).expect("metadata"),
                serde_json::json!({"locked":false,"keyMismatch":false})
            );
        } else {
            let explicit: devbox_secrets::Recovery =
                serde_json::from_str(result.get()).expect("explicit recovery");
            assert_eq!(explicit.recovery, recovery.recovery);
        }
    }
    assert_eq!(
        <devbox_secrets::Reset as devbox_protocol::Method>::CONFIRM,
        devbox_protocol::Confirm::Always
    );
    let logs = std::fs::read_to_string(f.home.path().join("stderr.log")).expect("daemon logs");
    assert!(!logs.contains("owned-secret-value"));
    assert!(!logs.contains(&recovery.recovery));
}
