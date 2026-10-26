//! Credential inference applies only to actual accepted metadata names, never arbitrary JSON.
mod support;
use devbox_api::*;
use std::time::Duration;
use support::api::*;
use support::grpc::*;
use support::lines::open;

#[tokio::test]
async fn grpc_request_headers_and_response_cookie_precede_body_and_metadata_output() {
    check("demo.Echo/Credentials", "grpc-header-issued", "message").await;
}

#[tokio::test]
async fn grpc_trailer_cookie_is_seeded_before_that_status_and_metadata_output() {
    check(
        "demo.Echo/CredentialTrailers",
        "grpc-trailer-issued",
        "trailers",
    )
    .await;
}

#[tokio::test]
async fn grpc_stream_trailer_cookie_is_seeded_when_trailers_arrive_after_ordinary_messages() {
    check(
        "demo.Echo/CredentialStream",
        "grpc-trailer-issued",
        "message",
    )
    .await;
}

async fn check(method: &str, issued: &str, event_type: &str) {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned proto");
    let (proto, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let p = GrpcCall {
        target: server.target(),
        method: method.into(),
        json: r#"{"name":"ordinary-literal"}"#.into(),
        metadata: vec![header(0, "Authorization", "Bearer grpc-direct-credential")],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        timeout_ms: 5000,
        proto_files: Some(vec![proto]),
        origin: None,
    };
    let mut lines = open(|ctx| h.api.grpc_call(ctx, p)).await.expect("call");
    let events = lines.all(Duration::from_secs(5)).await;
    assert!(events.iter().any(|e| e["type"] == event_type), "{events:?}");
    let text = serde_json::to_string(&events).expect("events");
    for secret in ["grpc-direct-credential", issued, "response-only-key"] {
        assert!(!text.contains(secret), "credential leaked: {text}");
    }
    assert!(text.contains("ordinary-literal"));
    assert!(text.contains("[REDACTED]"));
    assert!(
        server
            .state
            .metadata
            .lock()
            .expect("metadata")
            .iter()
            .any(|(key, value)| key == "authorization" && value == "Bearer grpc-direct-credential"),
        "request sends original credential"
    );
}

#[tokio::test]
async fn grpc_reflection_catalog_refuses_credential_echo_without_rewriting_dispatch_names() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned proto");
    let (_, set) = proto_file(dir.path());
    let server = GrpcServer::start(Some(set)).await;
    let target = GrpcTarget {
        target: server.target(),
        metadata: vec![header(0, "Authorization", "Bearer demo")],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        proto_files: None,
    };
    let error = h
        .api
        .grpc_describe(target)
        .await
        .expect_err("catalog contains credential");
    assert!(matches!(
        error,
        devbox_protocol::Fail::Domain(ApiError::RedactionFailed { .. })
    ));
    // Internal descriptor identities remain intact for an explicitly specified method.
    let p = GrpcCall {
        target: server.target(),
        method: "demo.Echo/Unary".into(),
        json: r#"{"name":"ordinary-literal"}"#.into(),
        metadata: vec![header(0, "Authorization", "Bearer demo")],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        timeout_ms: 5000,
        proto_files: None,
        origin: None,
    };
    let mut io = open(|ctx| h.api.grpc_call(ctx, p))
        .await
        .expect("original dispatch identity");
    let events = io.all(Duration::from_secs(5)).await;
    assert!(
        events.iter().any(|e| e["type"] == "message"
            && e["json"]
                .as_str()
                .is_some_and(|s| s.contains("ordinary-literal"))),
        "{events:?}"
    );
}

#[tokio::test]
async fn grpc_reflection_response_cookies_are_seeded_before_v1_and_alpha_catalog_display() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned proto");
    let (_, set) = proto_file(dir.path());
    for alpha in [false, true] {
        let server =
            GrpcServer::start_cookie(Some(set.clone()), alpha, Some("session=demo; Path=/")).await;
        let target = GrpcTarget {
            target: server.target(),
            metadata: vec![],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            proto_files: None,
        };
        let error = h
            .api
            .grpc_describe(target)
            .await
            .expect_err("cookie in catalog");
        assert!(
            matches!(
                error,
                devbox_protocol::Fail::Domain(ApiError::RedactionFailed { .. })
            ),
            "{error:?}"
        );
    }
}

#[tokio::test]
async fn grpc_reflection_first_answer_does_not_require_the_server_to_end_its_stream() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned proto");
    let (_, set) = proto_file(dir.path());
    let server = GrpcServer::held_reflection(set).await;
    let target = GrpcTarget {
        target: server.target(),
        metadata: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        proto_files: None,
    };
    let catalog = tokio::time::timeout(Duration::from_secs(2), h.api.grpc_describe(target))
        .await
        .expect("first answer without EOF")
        .expect("catalog");
    assert_eq!(catalog.services[0].name, "demo.Echo");
}

#[tokio::test]
async fn grpc_cookie_capacity_failure_stops_unpublished_head_and_trailer_fields() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned proto");
    let (proto, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    for method in ["Credentials", "CredentialTrailers", "CredentialStream"] {
        let env = EnvSnapshot {
            variables: (0..1024)
                .map(|i| EnvVar {
                    name: format!("V{i}"),
                    value: format!("fixture-seed-{i:04}"),
                    sensitive: true,
                    origin: VarOrigin::Environment,
                })
                .collect(),
        };
        let p = GrpcCall {
            target: server.target(),
            method: format!("demo.Echo/{method}"),
            json: "{}".into(),
            metadata: vec![],
            tls: TlsOptions::default(),
            env,
            timeout_ms: 5000,
            proto_files: Some(vec![proto.clone()]),
            origin: None,
        };
        let mut io = open(|ctx| h.api.grpc_call(ctx, p)).await.expect("call");
        let events = io.all(Duration::from_secs(5)).await;
        let types: Vec<_> = events
            .iter()
            .map(|e| e["type"].as_str().expect("type"))
            .collect();
        let expected = if method == "CredentialStream" {
            vec!["headers", "message", "failed"]
        } else {
            vec!["failed"]
        };
        assert_eq!(types, expected, "{events:?}");
        assert_eq!(
            events.last().expect("failed")["error"]["code"],
            "api.redaction_failed"
        );
        let text = serde_json::to_string(&events).expect("events");
        assert!(
            !text.contains("grpc-header-issued") && !text.contains("grpc-trailer-issued"),
            "{text}"
        );
    }
}
