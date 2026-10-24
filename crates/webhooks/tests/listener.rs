use devbox_webhooks::*;
#[test]
fn lan_address_selection_is_pure() {
    assert_eq!(address::bind_address(false).to_string(), "127.0.0.1");
    assert_eq!(address::bind_address(true).to_string(), "0.0.0.0");
}

async fn fixture() -> (tempfile::TempDir, std::sync::Arc<WebhooksService>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir = tempfile::tempdir().unwrap();
    let db = devbox_core::Db::open(
        &dir.path().join("db"),
        &dir.path().join("backups"),
        "1",
        MIGRATIONS,
    )
    .unwrap();
    let svc = std::sync::Arc::new(WebhooksService::new(
        db,
        devbox_core::Hub::new(),
        dir.path().join("webhooks"),
    ));
    (dir, svc)
}
#[tokio::test]
async fn lan_false_binds_loopback_only_and_records_binary() {
    let (dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    assert!(l.addresses.iter().all(|a| a.starts_with("127.0.0.1:")));
    let body = vec![0, 255, 127, 128];
    let response = reqwest::Client::new()
        .post(format!("http://{}/hook?q=1", l.addresses[0]))
        .body(body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let rows = svc
        .history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(rows.items.len(), 1);
    let detail = svc
        .history_get(ReceivedId {
            id: rows.items[0].id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(detail.query, "q=1");
    assert_eq!(
        data_encoding::BASE64
            .decode(detail.body_base64.as_bytes())
            .unwrap(),
        body
    );
    assert_eq!(
        std::fs::read(
            dir.path()
                .join("webhooks")
                .join(&l.id)
                .join(format!("{}.bin", detail.received.id))
        )
        .unwrap(),
        body
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn sequenced_responses_follow_order_and_reset_on_upsert() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let rule = ResponseRule {
        id: "r".into(),
        priority: 0,
        method: None,
        path: "/*".into(),
        status: 500,
        headers: vec![],
        body: "base".into(),
        delay_ms: 0,
        sequence: vec![ResponseSequenceStep {
            status: 200,
            headers: vec![],
            body: "ok".into(),
            delay_ms: 0,
        }],
    };
    svc.rule_upsert(UpsertRule {
        listener_id: l.id.clone(),
        rule: rule.clone(),
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    for status in [500, 200, 200] {
        assert_eq!(
            client
                .get(format!("http://{}/x", l.addresses[0]))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            status
        );
    }
    svc.rule_upsert(UpsertRule {
        listener_id: l.id.clone(),
        rule,
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .get(format!("http://{}/x", l.addresses[0]))
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn listeners_are_not_reopened_after_restart() {
    let (dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    svc.stop(ListenerId { id: l.id.clone() }).await.unwrap();
    drop(svc);
    let db = devbox_core::Db::open(
        &dir.path().join("db"),
        &dir.path().join("backups"),
        "1",
        MIGRATIONS,
    )
    .unwrap();
    let svc = WebhooksService::new(db, devbox_core::Hub::new(), dir.path().join("webhooks"));
    let list = svc.listeners().await.unwrap();
    assert_eq!(list.items.len(), 1);
    assert!(!list.items[0].running);
    assert!(
        tokio::net::TcpStream::connect(&l.addresses[0])
            .await
            .is_err()
    );
}

async fn raw(addr: &str, request: &[u8]) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(12),
        stream.read_to_end(&mut out),
    )
    .await
    .unwrap()
    .unwrap();
    out
}
#[tokio::test]
async fn chunked_and_100_continue_are_recorded() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let bytes=raw(&l.addresses[0],b"POST /chunk HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n3;foo=bar\r\nabc\r\n2\r\nde\r\n0\r\nX-Trailer: value\r\n\r\n").await;
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    let mut stream = tokio::net::TcpStream::connect(&l.addresses[0])
        .await
        .unwrap();
    stream.write_all(b"POST /expect HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 2\r\nExpect: 100-continue\r\n\r\n").await.unwrap();
    let mut first = [0; 25];
    stream.read_exact(&mut first).await.unwrap();
    assert_eq!(&first, b"HTTP/1.1 100 Continue\r\n\r\n");
    stream.write_all(&[0, 255]).await.unwrap();
    let mut final_response = Vec::new();
    stream.read_to_end(&mut final_response).await.unwrap();
    assert!(final_response.starts_with(b"HTTP/1.1 200"));
    let records = svc
        .history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(records.items.len(), 2);
    for item in records.items {
        let d = svc.history_get(ReceivedId { id: item.id }).await.unwrap();
        let bytes = data_encoding::BASE64
            .decode(d.body_base64.as_bytes())
            .unwrap();
        assert_eq!(
            bytes,
            if d.received.path == "/chunk" {
                b"abcde".to_vec()
            } else {
                vec![0, 255]
            }
        );
    }
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn oversized_body_and_headers_are_rejected() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://{}/big", l.addresses[0]))
        .body(vec![b'x'; 16 * 1024 * 1024 + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
    let mut req = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nX-Huge: ".to_vec();
    req.extend(vec![b'a'; 65536]);
    req.extend(b"\r\n\r\n");
    let reply = raw(&l.addresses[0], &req).await;
    assert!(
        reply.starts_with(b"HTTP/1.1 431"),
        "{}",
        String::from_utf8_lossy(&reply)
    );
    let rows = svc
        .history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(rows.items.len(), 1);
    assert!(rows.items[0].truncated);
    let detail = svc
        .history_get(ReceivedId {
            id: rows.items[0].id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(detail.body_base64, "");
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn rejected_rule_does_not_mutate_storage() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let mut rule = ResponseRule {
        id: "r".into(),
        priority: 0,
        method: None,
        path: "/*".into(),
        status: 200,
        headers: vec![],
        body: "ok".into(),
        delay_ms: 0,
        sequence: vec![],
    };
    svc.rule_upsert(UpsertRule {
        listener_id: l.id.clone(),
        rule: rule.clone(),
    })
    .await
    .unwrap();
    rule.headers.push(("Content-Length".into(), "123".into()));
    assert!(
        svc.rule_upsert(UpsertRule {
            listener_id: l.id.clone(),
            rule
        })
        .await
        .is_err()
    );
    assert!(
        svc.rules(ListenerId { id: l.id.clone() })
            .await
            .unwrap()
            .items[0]
            .headers
            .is_empty()
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}

#[tokio::test]
async fn unknown_expectation_is_rejected_without_history() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    assert!(raw(&l.addresses[0],b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nExpect: magic\r\nContent-Length: 0\r\n\r\n").await.starts_with(b"HTTP/1.1 417"));
    assert!(
        svc.history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20
        })
        .await
        .unwrap()
        .items
        .is_empty()
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn header_count_limit_has_no_partial_history() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let mut request = String::from("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for i in 0..101 {
        request.push_str(&format!("X-{i}: a\r\n"));
    }
    request.push_str("\r\n");
    assert!(
        raw(&l.addresses[0], request.as_bytes())
            .await
            .starts_with(b"HTTP/1.1 431")
    );
    assert!(
        svc.history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20
        })
        .await
        .unwrap()
        .items
        .is_empty()
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn head_and_bodyless_statuses_own_framing() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let client = reqwest::Client::new();
    for (method, status) in [
        (reqwest::Method::HEAD, 200),
        (reqwest::Method::GET, 204),
        (reqwest::Method::GET, 304),
    ] {
        svc.rule_upsert(UpsertRule {
            listener_id: l.id.clone(),
            rule: ResponseRule {
                id: "r".into(),
                priority: 0,
                method: None,
                path: "/*".into(),
                status,
                headers: vec![],
                body: "must not be on wire".into(),
                delay_ms: 0,
                sequence: vec![],
            },
        })
        .await
        .unwrap();
        let reply = client
            .request(method, format!("http://{}/", l.addresses[0]))
            .send()
            .await
            .unwrap();
        assert_eq!(reply.status().as_u16(), status);
        assert!(reply.bytes().await.unwrap().is_empty());
    }
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
#[tokio::test]
async fn stop_cancels_incomplete_request_and_releases_port() {
    use tokio::io::AsyncWriteExt;
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    let mut stream = tokio::net::TcpStream::connect(&l.addresses[0])
        .await
        .unwrap();
    stream
        .write_all(b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 100\r\n\r\nx")
        .await
        .unwrap();
    svc.stop(ListenerId { id: l.id.clone() }).await.unwrap();
    assert!(tokio::net::TcpListener::bind(&l.addresses[0]).await.is_ok());
    assert!(
        svc.history(HistoryQuery {
            listener_id: l.id,
            before: None,
            limit: 20
        })
        .await
        .unwrap()
        .items
        .is_empty()
    );
}
#[tokio::test]
async fn slow_body_times_out_without_partial_history() {
    let (_dir, svc) = fixture().await;
    let l = svc
        .start(StartListener {
            port: 0,
            lan: false,
        })
        .await
        .unwrap();
    assert!(raw(&l.addresses[0],b"POST / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: 2\r\n\r\nx").await.starts_with(b"HTTP/1.1 408"));
    assert!(
        svc.history(HistoryQuery {
            listener_id: l.id.clone(),
            before: None,
            limit: 20
        })
        .await
        .unwrap()
        .items
        .is_empty()
    );
    svc.stop(ListenerId { id: l.id }).await.unwrap();
}
