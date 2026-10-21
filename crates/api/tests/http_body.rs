//! multipart·헤더 순서·Cookie 행·이진 응답·시한(v0.9.0 http-client-engine 이식). 로그 시험은 http_logs.rs.
mod support;
use devbox_api::*;
use std::time::Duration;
use support::api::*;
use support::{Reply, Seen, Server};

#[tokio::test]
async fn live_request_streams_text_and_file_multipart_with_derived_boundary() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let dir = tempfile::tempdir().expect("owned file");
    let file = dir.path().join("private-dir-report.txt");
    std::fs::write(&file, b"file-content-unique").expect("fixture file");
    api.complete(request(RequestSnapshot {
        headers: vec![header(0, "Content-Type", "text/plain")],
        body: Body::Multipart {
            parts: vec![
                Part::Text {
                    name: "note".into(),
                    value: "hello-multipart".into(),
                    content_type: Some("text/plain".into()),
                },
                Part::File {
                    name: "upload".into(),
                    path: file.display().to_string(),
                    file_name: Some("display.bin".into()),
                    content_type: Some("text/plain".into()),
                },
            ],
        },
        ..snapshot("POST", &server.url("/multipart"))
    }))
    .await;
    let seen = server.only();
    let content_type = seen.header("content-type");
    assert_eq!(content_type.len(), 1);
    assert!(content_type[0].starts_with("multipart/form-data; boundary="));
    let observed = seen.body_text();
    assert!(observed.contains("name=\"note\""));
    assert!(observed.contains("hello-multipart"));
    assert!(observed.contains("name=\"upload\""));
    assert!(observed.contains("filename=\"display.bin\""));
    assert!(observed.contains("file-content-unique"));
    assert!(
        observed.contains("content-type: text/plain")
            || observed.contains("Content-Type: text/plain")
    );
    assert!(!observed.contains("private-dir"));
}

#[tokio::test]
async fn missing_multipart_file_error_never_contains_path() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let missing = format!(
        "{}/devbox-missing-multipart-{}",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let err = api
        .send(request(RequestSnapshot {
            body: Body::Multipart {
                parts: vec![
                    Part::Text {
                        name: "a".into(),
                        value: "1".into(),
                        content_type: None,
                    },
                    Part::File {
                        name: "upload".into(),
                        path: missing.clone(),
                        file_name: None,
                        content_type: None,
                    },
                ],
            },
            ..snapshot("POST", &server.url("/upload"))
        }))
        .await
        .err()
        .expect("missing file");
    assert_eq!(err.code(), "api.file_missing");
    assert_eq!(err.detail()["part"], 1);
    assert!(!err.detail_text().contains(&missing));
    assert!(!err.detail_text().contains("devbox-missing"));
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn multipart_secret_resolution_ignores_disabled_parts() {
    let api = Harness::new().await;
    api.secret("MP", "multipart-secret").await;
    let server = Server::echo().await;
    let events = api
        .complete(request(RequestSnapshot {
            body: Body::Multipart {
                parts: vec![Part::Text {
                    name: "token".into(),
                    value: "{{secret:MP}}".into(),
                    content_type: None,
                }],
            },
            ..snapshot("POST", &server.url("/upload"))
        }))
        .await;
    assert!(server.only().body_text().contains("multipart-secret"));
    let echoed = String::from_utf8(body(&events)).expect("utf8");
    assert!(!echoed.contains("multipart-secret") && echoed.contains(REDACTED));
    // 비활성 부분은 snapshot에 없으므로 잠긴 저장소도 막지 않는다.
    let locked = Harness::locked().await;
    let events = locked
        .complete(request(RequestSnapshot {
            body: Body::Multipart {
                parts: vec![Part::Text {
                    name: "plain".into(),
                    value: "1".into(),
                    content_type: None,
                }],
            },
            ..snapshot("POST", &server.url("/upload"))
        }))
        .await;
    assert_eq!(head(&events).0, 200);
}

#[tokio::test]
async fn live_request_sends_enabled_duplicate_headers_in_order_and_skips_disabled() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    api.complete(request(RequestSnapshot {
        headers: vec![header(0, "X-Trace", "one"), header(1, "X-Trace", "two")],
        ..snapshot("GET", &server.url("/headers"))
    }))
    .await;
    let raw = server.only().raw();
    let lines: Vec<&str> = raw.lines().filter(|l| l.starts_with("x-trace:")).collect();
    assert_eq!(lines, ["x-trace: one", "x-trace: two"]);
    assert!(!raw.contains("x-skip"));
}

#[tokio::test]
async fn live_request_builds_one_ordered_cookie_header_and_skips_disabled_rows() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    api.complete(request(RequestSnapshot {
        cookies: vec![header(0, "session", "one"), header(2, "token", "two")],
        ..snapshot("GET", &server.url("/cookies"))
    }))
    .await;
    assert_eq!(server.only().header("cookie"), ["session=one; token=two"]);
}

#[tokio::test]
async fn invalid_or_ambiguous_cookie_configuration_fails_closed() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let send = |cookies: Vec<HeaderRow>, headers: Vec<HeaderRow>| {
        api.send(request(RequestSnapshot {
            cookies,
            headers,
            ..snapshot("GET", &server.url("/"))
        }))
    };
    let reason = |e: Rpc| {
        (
            e.code().to_owned(),
            e.detail()["index"].as_u64(),
            e.detail()["reason"].as_str().map(str::to_owned),
        )
    };
    // index는 화면 행 번호(비활성 행 포함)다.
    let e = send(vec![header(2, "bad name", "one")], vec![])
        .await
        .err()
        .expect("bad name");
    assert_eq!(
        reason(e),
        (
            "api.invalid_cookie".into(),
            Some(2),
            Some("invalid_name".into())
        )
    );
    let e = send(
        vec![header(0, "ok", "1"), header(4, "session", "bad;value")],
        vec![],
    )
    .await
    .err()
    .expect("bad value");
    assert_eq!(
        reason(e),
        (
            "api.invalid_cookie".into(),
            Some(4),
            Some("invalid_value".into())
        )
    );
    let e = send(
        vec![header(1, "session", "one")],
        vec![header(3, "Cookie", "legacy=two")],
    )
    .await
    .err()
    .expect("both");
    assert_eq!(
        reason(e),
        (
            "api.invalid_cookie".into(),
            Some(1),
            Some("duplicate_header".into())
        )
    );
    let many = (0..101).map(|i| header(i, &format!("c{i}"), "v")).collect();
    let e = send(many, vec![]).await.err().expect("too many");
    assert_eq!(
        (e.code(), e.detail()["limit"].as_u64()),
        ("api.too_large", Some(100))
    );
    assert_eq!(server.accepted_connections(), 0);
    let events = api
        .complete(request(RequestSnapshot {
            headers: vec![header(0, "Cookie", "legacy=two")],
            ..snapshot("GET", &server.url("/"))
        }))
        .await;
    assert_eq!(head(&events).0, 200);
    assert_eq!(server.only().header("cookie"), ["legacy=two"]);
}

#[tokio::test]
async fn cookie_defaults_and_backend_only_secret_resolution_are_safe() {
    let api = Harness::new().await;
    api.secret("CK", "cookie-secret").await;
    let server = Server::start(support::handler(|seen: Seen| async move {
        Reply::ok(format!(
            "{{\"echo\":\"{}\"}}",
            seen.header("cookie").join("")
        ))
        .header("content-type", "application/json")
    }))
    .await;
    let events = api
        .complete(with_env(
            RequestSnapshot {
                cookies: vec![header(0, "session", "${COOKIE_TOKEN}")],
                ..snapshot("GET", &server.url("/"))
            },
            &[("COOKIE_TOKEN", "{{secret:CK}}", false)],
        ))
        .await;
    assert_eq!(server.only().header("cookie"), ["session=cookie-secret"]);
    assert_eq!(body(&events), br#"{"echo":"session=[REDACTED]"}"#);
}

#[tokio::test]
async fn resolves_both_reference_styles_including_auth_only_in_backend() {
    let api = Harness::new().await;
    api.secret("TOK", "top-secret").await;
    let server = Server::ok().await;
    api.complete(with_env(
        RequestSnapshot {
            headers: vec![header(0, "X-Token", "Bearer ${TOKEN}")],
            body: Body::Json {
                text: r#"{"password":"${TOKEN}"}"#.into(),
            },
            auth: Auth::Bearer {
                token: "{{TOKEN}}".into(),
            },
            ..snapshot(
                "POST",
                &format!("{}?token={{{{ TOKEN }}}}", server.url("/api")),
            )
        },
        &[("TOKEN", "{{secret:TOK}}", false)],
    ))
    .await;
    let seen = server.only();
    assert_eq!(
        seen.query_pairs(),
        [("token".to_string(), "top-secret".to_string())]
    );
    assert_eq!(seen.header("x-token"), ["Bearer top-secret"]);
    assert_eq!(seen.header("authorization"), ["Bearer top-secret"]);
    assert_eq!(seen.header("content-type"), ["application/json"]);
    assert_eq!(seen.body_text(), r#"{"password":"top-secret"}"#);
}

#[tokio::test]
async fn binary_response_projection_is_bounded_and_masks_request_secrets() {
    let api = Harness::new().await;
    api.secret("BIN", "binary-secret").await;
    let payload: Vec<u8> = [
        &[0u8, 0xff, 0x10][..],
        b"prefix-binary-secret-",
        &vec![b'x'; 200 * 1024],
    ]
    .concat();
    let reply = payload.clone();
    let server = Server::start(support::handler(move |_| {
        let reply = reply.clone();
        async move { Reply::ok(reply).header("content-type", "application/octet-stream") }
    }))
    .await;
    let events = api
        .complete(request(RequestSnapshot {
            headers: vec![header(0, "X-Key", "{{secret:BIN}}")],
            ..snapshot("GET", &server.url("/bin"))
        }))
        .await;
    let streamed = body(&events);
    assert!(!contains(&streamed, "binary-secret"));
    assert!(contains(&streamed, REDACTED));
    assert!(events.iter().all(|e| match e {
        SendEvent::Body { data_base64 } =>
            data_base64.len() <= (48 * 1024_usize).div_ceil(3) * 4 + 16,
        _ => true,
    }));
    let end = events.iter().find_map(|e| match e {
        SendEvent::End {
            binary, size_bytes, ..
        } => Some((*binary, *size_bytes)),
        _ => None,
    });
    assert_eq!(end, Some((true, payload.len() as i64)));
}

#[tokio::test]
async fn request_timeout_is_reported_without_the_url() {
    let api = Harness::new().await;
    let server = Server::start(support::handler(|_| async {
        Reply::ok("late").delayed(Duration::from_secs(5))
    }))
    .await;
    let events = api
        .complete(request(RequestSnapshot {
            timeout_ms: 300,
            ..snapshot("GET", &server.url("/slow"))
        }))
        .await;
    assert_eq!(failure_kind(&events), "api.network:timeout");
    let err = api
        .send(request(RequestSnapshot {
            timeout_ms: 0,
            ..snapshot("GET", &server.url("/"))
        }))
        .await
        .err()
        .expect("timeout bound");
    assert_eq!(
        (err.code(), err.detail()["field"].as_str()),
        ("api.invalid_input", Some("timeoutMs"))
    );
}
