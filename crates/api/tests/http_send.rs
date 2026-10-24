//! 송신 전 검사(CV-F05)·본문·인코딩·비밀·취소.
mod support;
use devbox_api::*;
use support::Server;
use support::api::*;

fn graphql() -> Body {
    Body::Graphql {
        query: "{ viewer { id } }".into(),
        variables: String::new(),
        operation_name: None,
        method: GraphqlMethod::Post,
    }
}

type Case = (
    &'static str,
    RequestSnapshot,
    Vec<(&'static str, &'static str, bool)>,
    (Option<u64>, Option<&'static str>),
);

/// 회귀 표 CV-F05 ①–⑥: (사례, snapshot, 환경, 기대 (index, field)).
fn cv_f05_cases(server: &Server) -> Vec<Case> {
    let base = || snapshot("GET", &server.url("/cv"));
    let with = |headers: Vec<HeaderRow>| RequestSnapshot { headers, ..base() };
    vec![
        (
            "① name with spaces",
            with(vec![header(0, "X API Key", "1")]),
            vec![],
            (Some(0), Some("name")),
        ),
        (
            "② newline in value",
            with(vec![header(0, "X-Ok", "a\nb")]),
            vec![],
            (Some(0), Some("value")),
        ),
        (
            "③ substituted value",
            with(vec![header(0, "X-Ok", "{{bad}}")]),
            vec![("bad", "x\r\nInjected: 1", false)],
            (Some(0), Some("value")),
        ),
        (
            "④ resolved secret value",
            with(vec![header(0, "Authorization", "Bearer {{secret:NL}}")]),
            vec![],
            (Some(0), Some("value")),
        ),
        (
            "⑤ screen row index",
            with(vec![header(1, "X API Key", "1")]),
            vec![],
            (Some(1), Some("name")),
        ),
        (
            "⑥ GraphQL user header",
            RequestSnapshot {
                method: "POST".into(),
                body: graphql(),
                headers: vec![header(0, "X Bad", "1")],
                ..base()
            },
            vec![],
            (Some(0), Some("name")),
        ),
    ]
}

#[tokio::test]
async fn invalid_header_name_is_rejected_before_sending() {
    let api = Harness::new().await;
    api.secret("NL", "fixture-secret\nInjected: 1").await;
    let server = Server::ok().await;
    for (case, snapshot, env, expect) in cv_f05_cases(&server) {
        let err = match api.send(with_env(snapshot, &env)).await {
            Ok(_) => panic!("{case}: must be rejected before sending"),
            Err(err) => err,
        };
        assert_eq!(err.code(), "api.invalid_header", "{case}");
        assert_eq!(
            (
                err.detail()["index"].as_u64(),
                err.detail()["field"].as_str()
            ),
            expect,
            "{case}"
        );
        assert!(
            !err.detail_text().contains("Injected")
                && !err.detail_text().contains("fixture-secret"),
            "{case}: {}",
            err.detail_text()
        );
    }
    assert_eq!(server.accepted_connections(), 0);
    assert_eq!(api.history_rows().await, 0);
    // ⑦ 앞뒤 공백(OWS)만 있는 정상 값은 보낸다.
    let events = api
        .complete(request(RequestSnapshot {
            headers: vec![header(0, "X-Ok", "  ok\t ")],
            ..snapshot("GET", &server.url("/ows"))
        }))
        .await;
    assert_eq!(head(&events).0, 200);
    assert_eq!(server.only().header("x-ok"), ["ok"]);
}

#[tokio::test]
async fn disabled_body_and_auth_are_not_sent() {
    let api = Harness::locked().await;
    let server = Server::ok().await;
    let events = api
        .complete(request(snapshot("POST", &server.url("/none"))))
        .await;
    assert_eq!(head(&events).0, 200);
    let seen = server.only();
    assert!(seen.body.is_empty());
    assert!(seen.header("content-type").is_empty());
    assert!(seen.header("authorization").is_empty());
}

#[tokio::test]
async fn none_body_ignores_hidden_draft_and_rejects_unknown_kind() {
    let wire = |body: &str| {
        format!(
            r#"{{"snapshot":{{"method":"POST","url":"http://127.0.0.1:1/","query":[],"headers":[],"cookies":[],"body":{body},"auth":{{"kind":"none"}},"tls":{{"verify":true,"caPath":null,"clientCertPath":null,"clientKeyPath":null}},"timeoutMs":1000,"followRedirects":true}},"env":{{"variables":[]}},"runId":null,"origin":null,"cookieJar":null}}"#
        )
    };
    // 화면에 남은 초안 필드는 `none`과 함께 와도 snapshot에 남지 않는다(v0.9.0 "hidden draft").
    let hidden: SendRequest = serde_json::from_str(&wire(r#"{"kind":"none","text":"{{hidden}}"}"#))
        .expect("hidden draft is ignored");
    assert_eq!(hidden.snapshot.body, Body::None);
    assert!(serde_json::from_str::<SendRequest>(&wire(r#"{"kind":"invalid"}"#)).is_err());
    let api = Harness::new().await;
    let server = Server::ok().await;
    let mut snapshot = hidden.snapshot;
    snapshot.url = server.url("/");
    // `{{hidden}}`은 정의되지 않았지만 활성 필드가 아니므로 api.variable_missing이 아니다.
    let events = api.complete(request(snapshot)).await;
    assert_eq!(head(&events).0, 200);
    let seen = server.only();
    assert!(seen.body.is_empty());
    assert!(seen.header("content-type").is_empty());
}

#[tokio::test]
async fn active_auth_ignores_inactive_secret_references() {
    let api = Harness::locked().await;
    let server = Server::ok().await;
    for (auth, expected) in [
        (Auth::None, vec![]),
        (
            Auth::Basic {
                username: "{{user}}".into(),
                password: "{{password}}".into(),
            },
            vec!["password", "user"],
        ),
        (
            Auth::Bearer {
                token: "{{token}}".into(),
            },
            vec!["token"],
        ),
        (
            Auth::ApiKey {
                name: "{{key}}".into(),
                value: "{{value}}".into(),
                placement: KeyPlacement::Header,
            },
            vec!["key", "value"],
        ),
    ] {
        let result = api
            .send(request(RequestSnapshot {
                auth,
                ..snapshot("GET", &server.url("/auth"))
            }))
            .await;
        match result {
            Ok(sent) if expected.is_empty() => assert_eq!(head(&sent.events().await).0, 200),
            Ok(_) => panic!("references must be rejected: {expected:?}"),
            Err(err) => {
                assert_eq!(err.code(), "api.variable_missing");
                assert_eq!(err.detail()["names"], serde_json::json!(expected));
            }
        }
    }
    assert_eq!(server.requests().len(), 1);
    // 비활성 쪽(화면의 다른 인증·본문 종류 초안)에만 `{{secret:…}}`가 있고 저장소가 잠겨 있어도
    // 활성 필드에 비밀 참조가 없으므로 비밀을 풀지 않고 보낸다.
    let wire = format!(
        r#"{{"snapshot":{{"method":"POST","url":"{}","query":[],"headers":[],"cookies":[],"body":{{"kind":"none","text":"{{{{secret:INACTIVE_BODY}}}}"}},"auth":{{"kind":"none","token":"{{{{secret:INACTIVE_AUTH}}}}"}},"tls":{{"verify":true,"caPath":null,"clientCertPath":null,"clientKeyPath":null}},"timeoutMs":5000,"followRedirects":true}},"env":{{"variables":[]}},"runId":null,"origin":null,"cookieJar":null}}"#,
        server.url("/inactive")
    );
    let inactive: SendRequest = serde_json::from_str(&wire).expect("inactive drafts");
    let events = api.complete(inactive).await;
    assert_eq!(head(&events).0, 200, "locked store is not consulted");
    let seen = server.requests().pop().expect("inactive request");
    assert_eq!(seen.path(), "/inactive");
    assert!(seen.header("authorization").is_empty() && seen.body.is_empty());
}

#[tokio::test]
async fn query_and_form_are_url_encoded() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let pairs = vec![
        pair("q", "a b+c&d=é한글"),
        pair("q", "2"),
        pair("empty", ""),
    ];
    api.complete(request(RequestSnapshot {
        query: pairs.clone(),
        body: Body::Form {
            fields: pairs.clone(),
        },
        ..snapshot("POST", &format!("{}?x=1#frag", server.url("/p")))
    }))
    .await;
    let seen = server.only();
    let expected: Vec<(String, String)> = pairs
        .iter()
        .map(|p| (p.name.clone(), p.value.clone()))
        .collect();
    let mut with_existing = vec![("x".to_string(), "1".to_string())];
    with_existing.extend(expected.clone());
    assert_eq!(seen.query_pairs(), with_existing);
    assert!(seen.target.contains("%2B"), "{}", seen.target);
    assert!(!seen.target.contains("frag"));
    assert_eq!(
        seen.header("content-type"),
        ["application/x-www-form-urlencoded"]
    );
    assert!(seen.body_text().contains("%2B"));
    assert_eq!(
        url::form_urlencoded::parse(&seen.body)
            .into_owned()
            .collect::<Vec<_>>(),
        expected
    );
}

#[tokio::test]
async fn query_form_preserves_reserved_characters_duplicates_and_fragment() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let pairs = [
        ("q", "a&admin=true"),
        ("q", "a#b"),
        ("plus", "a+b"),
        ("한글", " 한 글 "),
        ("empty", ""),
    ];
    let rows: Vec<Pair> = pairs.iter().map(|(n, v)| pair(n, v)).collect();
    api.complete(request(RequestSnapshot {
        query: rows.clone(),
        body: Body::Form { fields: rows },
        ..snapshot("POST", &format!("{}?existing=ok#fragment", server.url("/")))
    }))
    .await;
    let seen = server.only();
    let mut expected = vec![("existing".to_string(), "ok".to_string())];
    expected.extend(pairs.iter().map(|(n, v)| (n.to_string(), v.to_string())));
    assert_eq!(seen.query_pairs(), expected);
    // fragment는 URL serializer가 보존하지만 HTTP 요청 대상으로는 보내지 않는다.
    assert!(!seen.target.contains("fragment"));
    assert_eq!(
        url::form_urlencoded::parse(&seen.body)
            .into_owned()
            .collect::<Vec<_>>(),
        expected[1..]
    );
}

#[tokio::test]
async fn append_query_and_form_keep_existing_behavior() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    api.complete(request(RequestSnapshot {
        query: vec![pair("b", "2")],
        body: Body::Form {
            fields: vec![pair("name", "John Doe"), pair("age", "30")],
        },
        ..snapshot("POST", &format!("{}?a=1", server.url("")))
    }))
    .await;
    let seen = server.only();
    assert_eq!(seen.target, "/?a=1&b=2");
    assert_eq!(seen.body_text(), "name=John+Doe&age=30");
}

#[tokio::test]
async fn secret_reference_is_resolved_on_send_and_kept_as_reference_in_history() {
    let api = Harness::new().await;
    api.secret("TOKEN", "fixture-token-value").await;
    let server = Server::echo().await;
    let events = api
        .complete(request(RequestSnapshot {
            headers: vec![header(0, "Authorization", "Bearer {{secret:TOKEN}}")],
            body: Body::Raw {
                encoding: None,
                text: "echo {{secret:TOKEN}}".into(),
                content_type: Some("text/plain".into()),
            },
            ..snapshot("POST", &server.url("/echo"))
        }))
        .await;
    let seen = server.only();
    assert_eq!(seen.header("authorization"), ["Bearer fixture-token-value"]);
    assert_eq!(seen.body_text(), "echo fixture-token-value");
    let (_, headers, _) = head(&events);
    let echoed = headers
        .iter()
        .find(|h| h.name == "x-echo-authorization")
        .expect("echo header");
    assert_eq!(echoed.value, "Bearer [REDACTED]");
    assert_eq!(body(&events), b"echo [REDACTED]");
    let stream_text = serde_json::to_string(&events).expect("events json");
    assert!(!stream_text.contains("fixture-token-value"));
    let bytes = api.history_bytes();
    assert!(!bytes.is_empty());
    use std::os::unix::fs::PermissionsExt;
    for file in api.history_files() {
        let mode = std::fs::metadata(&file)
            .expect("history file")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "history files are private");
    }
    assert!(!contains(&bytes, "fixture-token-value"));
    assert!(!contains(
        &bytes,
        &data_encoding::BASE64.encode(b"echo fixture-token-value")
    ));
    let entry = api.only_history().await;
    assert_eq!(entry.snapshot.headers[0].name, "Authorization");
    assert_eq!(entry.snapshot.headers[0].value, "Bearer {{secret:TOKEN}}");
    let Body::Raw { text, .. } = &entry.snapshot.body else {
        panic!("raw body")
    };
    assert_eq!(text, "echo {{secret:TOKEN}}");
    assert_eq!(
        data_encoding::BASE64
            .decode(entry.body_base64.as_bytes())
            .expect("body"),
        b"echo [REDACTED]"
    );
}

#[tokio::test]
async fn secrets_locked_or_missing_sends_nothing() {
    let server = Server::ok().await;
    let snap = || RequestSnapshot {
        headers: vec![header(0, "Authorization", "Bearer {{secret:TOKEN}}")],
        ..snapshot("GET", &server.url("/"))
    };
    let locked = Harness::locked().await;
    let err = locked.send(request(snap())).await.err().expect("locked");
    assert_eq!(err.code(), "secrets_locked");
    let open = Harness::new().await;
    let err = open.send(request(snap())).await.err().expect("missing");
    assert_eq!(err.code(), "api.secret_missing");
    assert_eq!(err.detail()["names"], serde_json::json!(["TOKEN"]));
    assert_eq!(server.accepted_connections(), 0);
    assert_eq!(open.history_rows().await, 0);
}

#[tokio::test]
async fn cancel_aborts_the_in_flight_request() {
    let api = Harness::new().await;
    let server = Server::start(support::handler(|_| async {
        Reply::ok("late").delayed(std::time::Duration::from_secs(30))
    }))
    .await;
    let mut sent = api
        .send(request(snapshot("GET", &server.url("/slow"))))
        .await
        .expect("open");
    assert!(matches!(
        sent.next(std::time::Duration::from_secs(5)).await,
        Some(SendEvent::Started { .. })
    ));
    assert!(
        server
            .wait(
                |s| s.requests().len() == 1,
                std::time::Duration::from_secs(5)
            )
            .await
    );
    let closed_at = std::time::Instant::now();
    sent.close();
    assert!(
        server
            .wait(
                |s| s.closed_connections() == 1,
                std::time::Duration::from_secs(1)
            )
            .await
    );
    assert!(closed_at.elapsed() < std::time::Duration::from_secs(1));
    let mut rest = vec![];
    while let Some(event) = sent.next(std::time::Duration::from_millis(200)).await {
        rest.push(event);
    }
    assert!(!rest.iter().any(|e| matches!(e, SendEvent::End { .. })));
    let entry = wait_history(&api).await;
    assert_eq!(entry.outcome, HistoryOutcome::Cancelled);
}

async fn wait_history(api: &Harness) -> HistoryEntry {
    for _ in 0..100 {
        if api.history_rows().await == 1 {
            return api.only_history().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("history row was not written");
}

#[tokio::test]
async fn two_streams_cancel_independently() {
    let api = Harness::new().await;
    let server = Server::start(support::handler(|seen: Seen| async move {
        let delay = if seen.target == "/a" { 30 } else { 0 };
        Reply::ok("b-body").delayed(std::time::Duration::from_millis(delay * 1000))
    }))
    .await;
    let a = api
        .send(request(snapshot("GET", &server.url("/a"))))
        .await
        .expect("a");
    let b = api
        .send(request(snapshot("GET", &server.url("/b"))))
        .await
        .expect("b");
    assert!(
        server
            .wait(
                |s| s.requests().iter().any(|r| r.target == "/a"),
                std::time::Duration::from_secs(5)
            )
            .await
    );
    a.close();
    let events = b.events().await;
    assert_eq!(body(&events), b"b-body");
    assert!(events.iter().any(|e| matches!(e, SendEvent::End { .. })));
}
use support::{Reply, Seen};

/// 리뷰 3b-B1: 변수 치환으로 `{{secret:` 참조를 조립하면 저장소 파일의 재검토 판정(글자 그대로의 참조)을 우회한다.
/// 원본 활성 필드에 글자 그대로 있거나, 값 전체가 참조인 환경 변수의 비밀만 해석한다.
#[tokio::test]
async fn composed_secret_reference_is_rejected_before_connecting() {
    let api = Harness::new().await;
    api.secret("GH", "gh-secret-value").await;
    let server = Server::echo().await;
    for send in [
        with_env(
            snapshot(
                "GET",
                &format!("{}?t={{{{o}}}}secret:GH}}}}", server.url("/p")),
            ),
            &[("o", "{{", false)],
        ),
        with_env(
            RequestSnapshot {
                headers: vec![header(0, "X-Token", "{{a}}{{b}}")],
                ..snapshot("GET", &server.url("/p"))
            },
            &[("a", "{{secret:", false), ("b", "GH}}", false)],
        ),
        with_env(
            RequestSnapshot {
                headers: vec![header(0, "X-Token", "{{partial}}")],
                ..snapshot("GET", &server.url("/p"))
            },
            &[("partial", "x {{secret:GH}}", false)],
        ),
    ] {
        let err = api.send(send).await.err().expect("composed reference");
        assert_eq!(
            (err.code(), err.detail()["reason"].as_str()),
            ("api.invalid_input", Some("composed_reference"))
        );
        assert!(!err.detail_text().contains("gh-secret-value"));
    }
    assert_eq!(server.accepted_connections(), 0);
    // 값 전체가 참조인 환경 변수와 글자 그대로의 참조는 그대로 쓴다.
    api.complete(with_env(
        RequestSnapshot {
            headers: vec![
                header(0, "Authorization", "Bearer {{token}}"),
                header(1, "X-Literal", "{{secret:GH}}"),
            ],
            ..snapshot("GET", &server.url("/p"))
        },
        &[("token", "{{secret:GH}}", true)],
    ))
    .await;
    let seen = server.only();
    assert_eq!(seen.header("authorization"), ["Bearer gh-secret-value"]);
    assert_eq!(seen.header("x-literal"), ["gh-secret-value"]);
}

#[tokio::test]
async fn raw_base64_replay_sends_original_bytes_with_byte_content_length() {
    let api = Harness::new().await;
    let server = Server::echo().await;
    let encoded: Body = serde_json::from_str(r#"{"kind":"raw","text":"AP+A","encoding":"base64","contentType":"application/octet-stream"}"#).unwrap();
    api.complete(request(RequestSnapshot {
        body: encoded,
        ..snapshot("POST", &server.url("/binary"))
    }))
    .await;
    let seen = server.only();
    assert_eq!(seen.body, [0, 255, 128]);
    assert_eq!(seen.header("content-length"), ["3"]);
    assert_eq!(seen.header("content-type"), ["application/octet-stream"]);
}
