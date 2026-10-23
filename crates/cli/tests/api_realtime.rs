//! 실제 데몬 `api.ws_open`·`api.sse_open` 스트림 왕복(127.0.0.1 소유 fixture 서버).
//! 데몬 스트림 입력(화면 → 데몬 NDJSON)과 닫기 → 소켓 종료를 실제 연결 계층으로 확인한다.
mod support;
use devbox_protocol::system::{Ping, PingParams};
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use support::{TestDaemon, bounded};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;

const T: Duration = Duration::from_secs(5);

#[tokio::test]
async fn ws_open_round_trips_input_through_the_daemon_and_closing_ends_the_socket() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind owned fixture");
    let port = listener.local_addr().expect("address").port();
    let (ended_tx, ended) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("upgrade");
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message
                && socket
                    .send(Message::text(format!("pong:{text}")))
                    .await
                    .is_err()
            {
                break;
            }
        }
        let _ = ended_tx.send(());
    });
    let stream = bounded(client.open_stream(
        "api.ws_open",
        serde_json::json!({
            "url": format!("ws://127.0.0.1:{port}/echo"),
            "headers": [{"row": 0, "name": "X-Trace", "value": "daemon"}],
            "subprotocols": [],
            "tls": {"verify": true, "caPath": null, "clientCertPath": null, "clientKeyPath": null},
            "env": {"variables": []},
            "origin": null
        }),
    ))
    .await
    .expect("open api.ws_open");
    let open = stream.read_until(b"\"type\":\"open\"", T).await;
    assert!(String::from_utf8_lossy(&open).contains("\"open\""));
    stream
        .write(b"{\"type\":\"text\",\"data\":\"ping\"}\n")
        .await
        .expect("stream input");
    let got = stream.read_until(b"pong:ping", T).await;
    assert!(
        String::from_utf8_lossy(&got).contains("pong:ping"),
        "{}",
        String::from_utf8_lossy(&got)
    );
    bounded(client.call::<Ping>(&PingParams {}))
        .await
        .expect("other RPCs answer while the session is open");
    stream.close().await.expect("close stream");
    assert!(
        tokio::time::timeout(T, ended).await.is_ok(),
        "closing the stream ends the WebSocket connection"
    );
    // 잘못된 핸드셰이크 헤더는 스트림을 열기 전 RPC 오류다.
    let rejected = bounded(client.raw_call(
        "api.ws_open",
        serde_json::json!({
            "url": format!("ws://127.0.0.1:{port}/"),
            "headers": [{"row": 1, "name": "Sec-WebSocket-Key", "value": "x"}],
            "subprotocols": [],
            "tls": {"verify": true, "caPath": null, "clientCertPath": null, "clientKeyPath": null},
            "env": {"variables": []},
            "origin": null
        }),
        None,
    ))
    .await;
    match rejected {
        Err(devbox_cli::client::ClientError::Rpc { code, detail, .. }) => {
            assert_eq!(code, "api.invalid_header");
            assert_eq!(
                detail.expect("detail").get(),
                r#"{"index":1,"field":"name","reason":"reserved"}"#
            );
        }
        other => panic!("reserved header must be an RPC error: {other:?}"),
    }
}

#[tokio::test]
async fn sse_open_streams_events_through_the_daemon() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind owned fixture");
    let port = listener.local_addr().expect("address").port();
    let (ended_tx, ended) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.expect("request head");
            head.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\nid: 1\ndata: hello\n\n")
            .await
            .expect("write");
        let mut buf = [0u8; 16];
        while stream.read(&mut buf).await.unwrap_or(0) > 0 {}
        let _ = ended_tx.send(());
    });
    let stream = bounded(client.open_stream(
        "api.sse_open",
        serde_json::json!({
            "url": format!("http://127.0.0.1:{port}/events"),
            "headers": [],
            "tls": {"verify": true, "caPath": null, "clientCertPath": null, "clientKeyPath": null},
            "env": {"variables": []},
            "lastEventId": null,
            "origin": null
        }),
    ))
    .await
    .expect("open api.sse_open");
    let got = stream.read_until(b"hello", T).await;
    let text = String::from_utf8_lossy(&got);
    assert!(
        text.contains("\"sseEvent\"") && text.contains("\"id\":\"1\""),
        "{text}"
    );
    stream.close().await.expect("close stream");
    assert!(
        tokio::time::timeout(T, ended).await.is_ok(),
        "closing the stream ends the SSE connection"
    );
}
