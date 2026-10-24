//! 실제 데몬 `api.send` 스트림 왕복(127.0.0.1 소유 fixture 서버).
mod support;
use devbox_protocol::stream::STREAM_WINDOW;
use devbox_protocol::system::{Ping, PingParams};
use std::time::Duration;
use support::{TestDaemon, bounded};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 요청 하나를 받아 1MiB 본문으로 답하는 소유 서버.
async fn fixture() -> (u16, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind owned fixture");
    let port = listener.local_addr().expect("address").port();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.expect("request head");
            head.push(byte[0]);
        }
        let body = vec![b'a'; 1024 * 1024];
        let reply = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(reply.as_bytes())
            .await
            .expect("write head");
        stream.write_all(&body).await.expect("write body");
        head
    });
    (port, task)
}

fn kinds(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let event: serde_json::Value = serde_json::from_slice(line).expect("NDJSON event");
            event["type"].as_str().expect("event type").to_owned()
        })
        .collect()
}

#[tokio::test]
async fn api_send_streams_started_head_body_end_and_other_rpcs_answer_under_backpressure() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let (port, server) = fixture().await;
    let params = serde_json::json!({
        "snapshot": {
            "method": "GET",
            "url": format!("http://127.0.0.1:{port}/data"),
            "query": [], "headers": [{"row": 0, "name": "X-Trace", "value": "daemon"}], "cookies": [],
            "body": {"kind": "none"}, "auth": {"kind": "none"},
            "tls": {"verify": true, "caPath": null, "clientCertPath": null, "clientKeyPath": null},
            "timeoutMs": 10000, "followRedirects": true
        },
        "env": {"variables": []}, "runId": null, "origin": null, "cookieJar": null
    });
    let stream = bounded(client.open_stream("api.send", params))
        .await
        .expect("open api.send");
    let initial = stream.read_available(Duration::from_millis(300)).await;
    assert_eq!(
        initial.len(),
        STREAM_WINDOW as usize,
        "the daemon stops at the credit window"
    );
    bounded(client.call::<Ping>(&PingParams {}))
        .await
        .expect("ping while credit is exhausted");
    let mut all = initial;
    loop {
        stream.credit(STREAM_WINDOW).await.expect("credit");
        let more = stream.read_available(Duration::from_millis(300)).await;
        if more.is_empty() {
            break;
        }
        all.extend(more);
    }
    assert!(stream.wait_end(Duration::from_secs(2)).await);
    let kinds = kinds(&all);
    assert_eq!(kinds.first().map(String::as_str), Some("started"));
    assert_eq!(kinds.get(1).map(String::as_str), Some("head"));
    assert_eq!(kinds.last().map(String::as_str), Some("end"));
    assert!(kinds[2..kinds.len() - 1].iter().all(|k| k == "body"));
    let head = String::from_utf8(bounded(server).await.expect("fixture")).expect("utf8");
    assert!(head.contains("x-trace: daemon"));
    let rejected = bounded(client.raw_call(
        "api.send",
        serde_json::json!({
            "snapshot": {
                "method": "GET", "url": format!("http://127.0.0.1:{port}/"),
                "query": [], "headers": [{"row": 2, "name": "X API Key", "value": "1"}], "cookies": [],
                "body": {"kind": "none"}, "auth": {"kind": "none"},
                "tls": {"verify": true, "caPath": null, "clientCertPath": null, "clientKeyPath": null},
                "timeoutMs": 10000, "followRedirects": true
            },
            "env": {"variables": []}, "runId": null, "origin": null, "cookieJar": null
        }),
        None,
    ))
    .await;
    match rejected {
        Err(devbox_cli::client::ClientError::Rpc { code, detail, .. }) => {
            assert_eq!(code, "api.invalid_header");
            assert_eq!(
                detail.expect("detail").get(),
                r#"{"index":2,"field":"name","reason":"invalid_token"}"#
            );
        }
        other => panic!("invalid header must be an RPC error before the stream opens: {other:?}"),
    }
    let page: serde_json::Value = serde_json::from_str(
        &bounded(client.raw_call(
            "api.history",
            serde_json::json!({"before": null, "limit": 10, "origin": null}),
            None,
        ))
        .await
        .unwrap_or_else(|e| panic!("api.history: {e:?}")),
    )
    .expect("page");
    assert_eq!(page["items"].as_array().map(Vec::len), Some(1));
}
