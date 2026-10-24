//! gRPC(S5-5): reflection·protox 서술, unary·server streaming, 연결 전 메타데이터 검사, 비밀 가림, 취소, 상한.
mod support;
use devbox_api::*;
use serde_json::{Value, json};
use std::time::Duration;
use support::api::*;
use support::grpc::*;
use support::lines::{Lines, open};

const T: Duration = Duration::from_secs(10);

fn target(server: &GrpcServer, proto: Option<&str>) -> GrpcTarget {
    GrpcTarget {
        target: server.target(),
        metadata: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        proto_files: proto.map(|p| vec![p.to_owned()]),
    }
}

fn call(server: &GrpcServer, method: &str, json: &str, proto: Option<&str>) -> GrpcCall {
    GrpcCall {
        target: server.target(),
        method: method.into(),
        json: json.into(),
        metadata: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        timeout_ms: 5_000,
        proto_files: proto.map(|p| vec![p.to_owned()]),
        origin: None,
    }
}

fn env(vars: &[(&str, &str)]) -> EnvSnapshot {
    EnvSnapshot {
        variables: vars
            .iter()
            .map(|(n, v)| EnvVar {
                name: (*n).into(),
                value: (*v).into(),
                sensitive: false,
                origin: VarOrigin::Environment,
            })
            .collect(),
    }
}

async fn describe(h: &Harness, p: GrpcTarget) -> Result<GrpcCatalog, Rpc> {
    h.api.grpc_describe(p).await.map_err(|e| match e {
        devbox_protocol::Fail::Domain(e) => Rpc(devbox_protocol::to_wire(&e)),
        devbox_protocol::Fail::Internal(m) => panic!("internal: {m}"),
    })
}

async fn start_call(h: &Harness, p: GrpcCall) -> Result<Lines, Rpc> {
    open(|ctx| h.api.grpc_call(ctx, p)).await
}

fn messages(events: &[Value]) -> Vec<Value> {
    events
        .iter()
        .filter(|e| e["type"] == "message")
        .map(|e| serde_json::from_str(e["json"].as_str().expect("json text")).expect("message"))
        .collect()
}

fn trailers(events: &[Value]) -> &Value {
    events
        .iter()
        .find(|e| e["type"] == "trailers")
        .unwrap_or_else(|| panic!("trailers in {events:?}"))
}

#[tokio::test]
async fn grpc_reflection_describe_and_unary_round_trip() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (_, set) = proto_file(dir.path());
    let server = GrpcServer::start(Some(set)).await;
    let catalog = describe(&h, target(&server, None)).await.expect("describe");
    assert_eq!(catalog.source, CatalogSource::Reflection);
    let names: Vec<_> = catalog.services.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["demo.Echo"], "reflection services are hidden");
    let unary = catalog.services[0]
        .methods
        .iter()
        .find(|m| m.name == "Unary")
        .expect("unary method");
    assert_eq!(unary.full_name, "demo.Echo/Unary");
    assert!(!unary.client_streaming && !unary.server_streaming);
    let default: Value = serde_json::from_str(&unary.input_default).expect("default json");
    assert!(default.get("name").is_some(), "{default}");
    let server_stream = catalog.services[0]
        .methods
        .iter()
        .find(|m| m.name == "Server")
        .expect("server method");
    assert!(server_stream.server_streaming);

    let mut lines = start_call(
        &h,
        call(
            &server,
            "demo.Echo/Unary",
            r#"{"name":"a","count":"2"}"#,
            None,
        ),
    )
    .await
    .expect("call");
    let events = lines.all(T).await;
    assert_eq!(events[0]["type"], "headers", "{events:?}");
    assert_eq!(messages(&events), vec![json!({"name":"a","count":"2"})]);
    assert_eq!(trailers(&events)["code"], 0);
}

#[tokio::test]
async fn grpc_reflection_falls_back_to_v1alpha() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (_, set) = proto_file(dir.path());
    let server = GrpcServer::start_with(Some(set), true).await;
    let catalog = describe(&h, target(&server, None)).await.expect("describe");
    assert_eq!(catalog.services.len(), 1);
}

#[tokio::test]
async fn grpc_server_streaming_messages_arrive_in_order() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (_, set) = proto_file(dir.path());
    let server = GrpcServer::start(Some(set)).await;
    let mut lines = start_call(
        &h,
        call(
            &server,
            "demo.Echo/Server",
            r#"{"name":"s","count":3}"#,
            None,
        ),
    )
    .await
    .expect("call");
    let events = lines.all(T).await;
    let names: Vec<_> = messages(&events)
        .iter()
        .map(|m| m["name"].as_str().unwrap_or("").to_owned())
        .collect();
    assert_eq!(names, vec!["s-0", "s-1", "s-2"]);
    assert_eq!(trailers(&events)["code"], 0);
}

#[tokio::test]
async fn grpc_proto_files_describe_without_reflection() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let err = describe(&h, target(&server, None))
        .await
        .expect_err("no reflection");
    assert_eq!(err.code(), "api.grpc");
    assert_eq!(err.detail()["reason"], "reflection_unavailable");
    let catalog = describe(&h, target(&server, Some(&path)))
        .await
        .expect("describe from proto files");
    assert_eq!(catalog.source, CatalogSource::ProtoFiles);
    assert_eq!(catalog.services[0].name, "demo.Echo");
    let mut lines = start_call(
        &h,
        call(&server, "demo.Echo/Unary", r#"{"name":"p"}"#, Some(&path)),
    )
    .await
    .expect("call with proto files");
    let events = lines.all(T).await;
    assert_eq!(messages(&events), vec![json!({"name":"p"})]);
}

#[tokio::test]
async fn grpc_invalid_metadata_is_rejected_before_connecting() {
    let h = Harness::new().await;
    h.secret("NL", "a\nb").await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    type Expected<'a> = (u32, &'a str, &'a str);
    let cases: Vec<(&str, HeaderRow, EnvSnapshot, Expected)> = vec![
        (
            "space",
            header(0, "X Bad", "1"),
            env(&[]),
            (0, "name", "invalid_token"),
        ),
        (
            "newline",
            header(1, "x-a", "a\nb"),
            env(&[]),
            (1, "value", "control_character"),
        ),
        (
            "env injection",
            header(2, "x-a", "{{bad}}"),
            env(&[("bad", "x\r\nInjected: 1")]),
            (2, "value", "control_character"),
        ),
        (
            "secret newline",
            header(3, "authorization", "Bearer {{secret:NL}}"),
            env(&[]),
            (3, "value", "control_character"),
        ),
        (
            "te",
            header(4, "te", "trailers"),
            env(&[]),
            (4, "name", "reserved"),
        ),
        (
            "grpc-",
            header(5, "grpc-timeout", "1S"),
            env(&[]),
            (5, "name", "reserved"),
        ),
        (
            "content-type",
            header(6, "Content-Type", "x"),
            env(&[]),
            (6, "name", "reserved"),
        ),
        (
            "bin",
            header(7, "x-id-bin", "!!not base64"),
            env(&[]),
            (7, "value", "base64"),
        ),
        (
            "empty name",
            header(8, "", "1"),
            env(&[]),
            (8, "name", "empty"),
        ),
    ];
    for (case, row, env, expect) in cases {
        let mut p = call(&server, "demo.Echo/Unary", "{}", Some(&path));
        p.metadata = vec![row.clone()];
        p.env = env.clone();
        let err = start_call(&h, p)
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}"));
        assert_eq!(err.code(), "api.invalid_header", "{case}");
        let d = err.detail();
        assert_eq!(
            (
                d["index"].as_u64(),
                d["field"].as_str(),
                d["reason"].as_str()
            ),
            (Some(expect.0 as u64), Some(expect.1), Some(expect.2)),
            "{case}"
        );
        assert!(!err.detail_text().contains("Injected"), "{case}");
        let mut t = target(&server, None);
        t.metadata = vec![row];
        t.env = env;
        let err = describe(&h, t).await.expect_err(case);
        assert_eq!(err.code(), "api.invalid_header", "{case}");
    }
    assert_eq!(server.connections(), 0, "nothing reached the server");
}

#[tokio::test]
async fn grpc_metadata_and_binary_values_are_sent() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let mut p = call(&server, "demo.Echo/Unary", "{}", Some(&path));
    p.metadata = vec![header(0, "X-Trace", "t1"), header(1, "x-id-bin", "AQID")];
    let mut lines = start_call(&h, p).await.expect("call");
    lines.all(T).await;
    assert_eq!(server.seen("x-trace"), vec!["t1"], "names are lowercased");
    assert_eq!(server.seen("x-id-bin"), vec!["AQID"]);
}

#[tokio::test]
async fn grpc_secret_echo_is_redacted_in_messages_and_metadata() {
    let h = Harness::new().await;
    h.secret("TOKEN", "fixture-grpc-secret").await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let mut p = call(
        &server,
        "demo.Echo/Unary",
        r#"{"name":"{{secret:TOKEN}}"}"#,
        Some(&path),
    );
    p.metadata = vec![header(0, "x-echo", "{{secret:TOKEN}}")];
    let mut lines = start_call(&h, p).await.expect("call");
    let events = lines.all(T).await;
    assert_eq!(
        server.seen("x-echo"),
        vec!["fixture-grpc-secret"],
        "server got the value"
    );
    let text = serde_json::to_string(&events).expect("events");
    assert!(!text.contains("fixture-grpc-secret"), "{text}");
    assert_eq!(messages(&events), vec![json!({"name": REDACTED})]);
    assert!(text.contains(REDACTED));
}

#[tokio::test]
async fn grpc_server_status_is_reported_as_trailers() {
    let h = Harness::new().await;
    h.secret("TOKEN", "fixture-status-secret").await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let p = call(
        &server,
        "demo.Echo/Fail",
        r#"{"name":"{{secret:TOKEN}}"}"#,
        Some(&path),
    );
    let mut lines = start_call(&h, p).await.expect("call");
    let events = lines.all(T).await;
    let t = trailers(&events);
    assert_eq!(t["code"], 7, "{events:?}");
    assert_eq!(t["message"], format!("denied:{REDACTED}"));
}

#[tokio::test]
async fn grpc_closing_the_stream_cancels_the_call() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let mut lines = start_call(&h, call(&server, "demo.Echo/Slow", "{}", Some(&path)))
        .await
        .expect("call");
    let (first, _) = lines.until("message", T).await;
    assert!(first.is_some(), "first message");
    lines.close();
    assert!(
        server.wait_cancelled(Duration::from_secs(1)).await,
        "server sees the call cancelled within 1s"
    );
    let rest = lines.all(T).await;
    assert!(
        rest.iter().all(|e| e["type"] != "trailers"),
        "a cancelled call has no trailers: {rest:?}"
    );
}

#[tokio::test]
async fn grpc_calls_are_bounded_and_fail_closed_before_streaming() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let reason = |e: &Rpc| {
        (
            e.code().to_owned(),
            e.detail()["reason"].as_str().unwrap_or("").to_owned(),
        )
    };
    // 클라이언트 스트리밍은 지원하지 않는다.
    let err = start_call(&h, call(&server, "demo.Echo/Client", "{}", Some(&path)))
        .await
        .err()
        .expect("client streaming");
    assert_eq!(reason(&err), ("api.grpc".into(), "client_streaming".into()));
    let err = start_call(&h, call(&server, "demo.Echo/Missing", "{}", Some(&path)))
        .await
        .err()
        .expect("unknown method");
    assert_eq!(reason(&err), ("api.grpc".into(), "method_not_found".into()));
    let err = start_call(
        &h,
        call(
            &server,
            "demo.Echo/Unary",
            r#"{"nope":"PRIVATE-VALUE"}"#,
            Some(&path),
        ),
    )
    .await
    .err()
    .expect("bad json");
    assert_eq!(reason(&err), ("api.grpc".into(), "message_json".into()));
    assert!(!err.detail_text().contains("PRIVATE-VALUE"));
    // 요청 메시지 크기 상한(4MiB): 보내기 전에 거절한다.
    let huge = format!(r#"{{"name":"{}"}}"#, "x".repeat(4 * 1024 * 1024 + 1));
    let err = start_call(&h, call(&server, "demo.Echo/Unary", &huge, Some(&path)))
        .await
        .err()
        .expect("too large");
    assert_eq!(
        (err.code(), err.detail()["limit"].as_u64()),
        ("api.too_large", Some(4 * 1024 * 1024))
    );
    // proto 파일: 상대 경로·너무 큼·너무 많음·문법 오류(문장에 파일 내용·경로가 없음).
    let big = dir.path().join("big.proto");
    std::fs::write(&big, vec![b' '; 1024 * 1024 + 1]).expect("big proto");
    let broken = dir.path().join("broken.proto");
    std::fs::write(&broken, "syntax = \"proto3\";\nmessage SECRET-CONTENT {").expect("broken");
    let many: Vec<String> = (0..33).map(|_| path.clone()).collect();
    for (case, files, expect) in [
        ("relative", vec!["echo.proto".to_owned()], "proto_path"),
        ("big", vec![big.display().to_string()], "proto_too_large"),
        ("many", many, "proto_too_many"),
        ("broken", vec![broken.display().to_string()], "proto_parse"),
    ] {
        let mut t = target(&server, None);
        t.proto_files = Some(files);
        let err = describe(&h, t).await.expect_err(case);
        assert_eq!(reason(&err), ("api.grpc".into(), expect.into()), "{case}");
        let text = err.detail_text();
        assert!(
            !text.contains("SECRET-CONTENT") && !text.contains("/tmp"),
            "{case}: {text}"
        );
    }
    assert_eq!(server.calls(), 0);
}

#[tokio::test]
async fn grpc_tls_follows_the_request_settings() {
    use support::tls::Pki;
    let h = Harness::new().await;
    let pki = Pki::new();
    let ca = pki.write("ca.pem", &pki.ca_pem);
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    // 소유 TLS 리스너: 핸드셰이크 결과와 협상한 ALPN만 기록한다(gRPC 응답은 하지 않음).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let mut tls = pki.server(false);
    tls.client_ca_pem = None;
    let acceptor = support::tls::acceptor_alpn(tls, &[b"h2"]);
    let (seen_tx, mut seen) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let seen_tx = seen_tx.clone();
            tokio::spawn(async move {
                let result = acceptor.accept(tcp).await;
                let alpn = result
                    .as_ref()
                    .ok()
                    .and_then(|s| s.get_ref().1.alpn_protocol().map(|p| p.to_vec()));
                let _ = seen_tx.send((result.is_ok(), alpn));
            });
        }
    });
    let base = |verify: bool, ca: Option<String>| GrpcCall {
        target: format!("https://127.0.0.1:{port}"),
        tls: TlsOptions {
            verify,
            ca_path: ca,
            client_cert_path: None,
            client_key_path: None,
        },
        timeout_ms: 2_000,
        ..GrpcCall {
            target: String::new(),
            method: "demo.Echo/Unary".into(),
            json: "{}".into(),
            metadata: vec![],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            timeout_ms: 0,
            proto_files: Some(vec![path.clone()]),
            origin: None,
        }
    };
    // 검증 켬 + 지정 CA → 핸드셰이크 성공, ALPN h2.
    let mut lines = start_call(&h, base(true, Some(ca.clone())))
        .await
        .expect("call");
    lines.all(T).await;
    let (ok, alpn) = tokio::time::timeout(T, seen.recv())
        .await
        .ok()
        .flatten()
        .expect("handshake");
    assert!(ok, "trusted CA handshake");
    assert_eq!(alpn.as_deref(), Some(&b"h2"[..]));
    // 검증 켬 + CA 없음 → 클라이언트가 인증서를 거절.
    let mut lines = start_call(&h, base(true, None)).await.expect("call");
    let events = lines.all(T).await;
    assert!(
        !tokio::time::timeout(T, seen.recv())
            .await
            .ok()
            .flatten()
            .expect("handshake")
            .0,
        "untrusted certificate is refused"
    );
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(failed["error"]["code"], "api.network", "{events:?}");
    // 검증 끔(요청 단위) → 핸드셰이크 성공.
    let mut lines = start_call(&h, base(false, None)).await.expect("call");
    lines.all(T).await;
    assert!(
        tokio::time::timeout(T, seen.recv())
            .await
            .ok()
            .flatten()
            .expect("handshake")
            .0,
        "verify off is honoured"
    );
}
