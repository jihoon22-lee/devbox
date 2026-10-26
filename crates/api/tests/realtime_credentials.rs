//! Owned loopback credential boundaries; ordinary literal response text stays readable.
mod support;
use devbox_api::*;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use support::api::*;
use support::realtime::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{
    Message,
    handshake::server::{Request, Response},
};

const T: Duration = Duration::from_secs(5);

#[tokio::test]
async fn ws_handshake_cookie_is_seeded_before_messages_and_close_reason() {
    let h = Harness::new().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let url = format!("ws://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("connection");
        // tungstenite `Callback`의 오류 응답 형식이 정해져 있다.
        #[allow(clippy::result_large_err)]
        let callback = |_: &Request, mut response: Response| {
            response.headers_mut().append(
                "set-cookie",
                "session=ws-issued-value; Path=/; HttpOnly"
                    .parse()
                    .expect("header"),
            );
            Ok(response)
        };
        let mut socket = tokio_tungstenite::accept_hdr_async(tcp, callback)
            .await
            .expect("handshake");
        socket
            .send(Message::text("ws-issued-value ordinary-literal"))
            .await
            .expect("message");
        socket
            .close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                reason: "ws-issued-value".into(),
            }))
            .await
            .expect("close");
        while socket.next().await.is_some() {}
    });
    let mut io = open_ws(&h.api, ws(&url)).await.expect("open");
    let events = io.events(T).await;
    tokio::time::timeout(T, server)
        .await
        .expect("server ended")
        .expect("server");
    let text = format!("{events:?}");
    assert!(text.contains("ordinary-literal"), "ordinary body preserved");
    assert!(
        !text.contains("ws-issued-value"),
        "handshake cookie leaked: {text}"
    );
    assert!(text.contains("[REDACTED]"));
    assert!(events.iter().any(|e| matches!(e, RealtimeEvent::Closed { reason: Some(reason), .. } if reason == "[REDACTED]")));
}

#[tokio::test]
async fn sse_every_reconnect_seeds_new_cookie_before_that_connections_events() {
    let h = Harness::new().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        for token in ["sse-first-issued", "sse-second-issued", ""] {
            let (mut tcp, _) = listener.accept().await.expect("connection");
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                tcp.read_exact(&mut byte).await.expect("request");
                request.push(byte[0]);
                assert!(request.len() < 16384);
            }
            let response = if token.is_empty() {
                "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n".to_owned()
            } else {
                let body = format!("retry: 100\ndata: {token} ordinary-literal\n\n");
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nSet-Cookie: session={token}; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
            };
            tcp.write_all(response.as_bytes()).await.expect("response");
            tcp.shutdown().await.expect("close");
        }
    });
    let mut io = open_sse(
        &h.api,
        SseOpen {
            url,
            headers: vec![],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            last_event_id: None,
            origin: None,
        },
    )
    .await
    .expect("open");
    let events = io.events(T).await;
    tokio::time::timeout(T, server)
        .await
        .expect("server ended")
        .expect("server");
    let data: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let RealtimeEvent::SseEvent { data, .. } = e {
                Some(data.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        data,
        ["[REDACTED] ordinary-literal", "[REDACTED] ordinary-literal"]
    );
}

fn full_budget() -> EnvSnapshot {
    EnvSnapshot {
        variables: (0..1024)
            .map(|i| EnvVar {
                name: format!("V{i}"),
                value: format!("fixture-seed-{i:04}"),
                sensitive: true,
                origin: VarOrigin::Environment,
            })
            .collect(),
    }
}
fn failed_before_open(events: &[RealtimeEvent]) {
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(
        matches!(&events[0], RealtimeEvent::Failed { error } if error.code=="api.redaction_failed"),
        "{events:?}"
    );
}

#[tokio::test]
async fn ws_response_cookie_capacity_failure_emits_no_open_or_message() {
    let h = Harness::new().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let url = format!("ws://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.expect("connection");
        // tungstenite `Callback`의 오류 응답 형식이 정해져 있다.
        #[allow(clippy::result_large_err)]
        let callback = |_: &Request, mut response: Response| {
            response.headers_mut().append(
                "set-cookie",
                "session=must-not-leak".parse().expect("header"),
            );
            Ok(response)
        };
        let mut socket = tokio_tungstenite::accept_hdr_async(tcp, callback)
            .await
            .expect("handshake");
        let _ = socket.send(Message::text("must-not-leak")).await;
        let _ = socket.next().await;
    });
    let mut io = open_ws(
        &h.api,
        WsOpen {
            env: full_budget(),
            ..ws(&url)
        },
    )
    .await
    .expect("open");
    failed_before_open(&io.events(T).await);
    tokio::time::timeout(T, server)
        .await
        .expect("server ended")
        .expect("server");
}

#[tokio::test]
async fn sse_response_cookie_capacity_failure_emits_no_open_or_message_or_reconnect() {
    let h = Harness::new().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let url = format!("http://{}/", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.expect("connection");
        let mut request = [0; 4096];
        let _ = tcp.read(&mut request).await.expect("request");
        tcp.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nSet-Cookie: session=must-not-leak\r\nContent-Length: 21\r\nConnection: close\r\n\r\ndata: must-not-leak\n\n").await.expect("response");
        tcp.shutdown().await.expect("close");
        assert!(
            tokio::time::timeout(Duration::from_millis(300), listener.accept())
                .await
                .is_err(),
            "capacity failure must not reconnect"
        );
    });
    let mut io = open_sse(
        &h.api,
        SseOpen {
            url,
            headers: vec![],
            tls: TlsOptions::default(),
            env: full_budget(),
            last_event_id: None,
            origin: None,
        },
    )
    .await
    .expect("open");
    failed_before_open(&io.events(T).await);
    tokio::time::timeout(T, server)
        .await
        .expect("server ended")
        .expect("server");
}
