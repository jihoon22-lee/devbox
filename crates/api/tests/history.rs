//! 기록 정화·색인·30일 정리(v0.9.0 기록 정화 시험 적응).
mod support;
use devbox_api::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use support::api::*;
use support::{Reply, Seen, Server, handler};

async fn echo_all() -> Server {
    Server::start(handler(|seen: Seen| async move {
        let mut reply = Reply::ok(format!(
            "{} {} {}",
            seen.target,
            seen.header("authorization").join(","),
            seen.body_text()
        ));
        for (name, value) in &seen.headers {
            if name.starts_with("x-") || name == "authorization" {
                reply = reply.header(&format!("x-echo-{name}"), value);
            }
        }
        reply
    }))
    .await
}

#[tokio::test]
async fn history_never_contains_tokens_or_basic_credentials() {
    let api = Harness::new().await;
    api.secret("DEPLOY", "deploy-secret-value").await;
    let server = echo_all().await;
    let tokens = Server::start(handler(|_| async {
        Reply::ok(r#"{"access_token":"issued-access-token","refresh_token":"issued-refresh","expires_in":60}"#)
    }))
    .await;
    api.complete(request(RequestSnapshot {
        auth: Auth::Basic {
            username: "fixture-user".into(),
            password: "fixture-pass-word".into(),
        },
        ..snapshot("GET", &server.url("/basic"))
    }))
    .await;
    api.complete(request(RequestSnapshot {
        auth: Auth::Oauth2ClientCredentials {
            token_url: format!("{}?access_token=query-secret", tokens.url("/token")),
            client_id: "devbox".into(),
            client_secret: "plain-client-secret".into(),
            scope: None,
        },
        ..snapshot("GET", &server.url("/oauth"))
    }))
    .await;
    api.complete(with_env(
        RequestSnapshot {
            headers: vec![header(0, "X-Token", "{{token}}")],
            query: vec![pair("t", "{{token}}")],
            body: Body::Json {
                text: r#"{"password":"literal-private${UNUSED}","token":"${TOKEN}","secret":"{{KEY}}"}"#.into(),
            },
            ..snapshot("POST", &format!("{}/deploy/{{{{secret:DEPLOY}}}}/request", server.url("")))
        },
        &[("token", "captured-token-value", true), ("TOKEN", "t", false), ("KEY", "k", false)],
    ))
    .await;
    let bytes = api.history_bytes();
    let basic = data_encoding::BASE64.encode(b"fixture-user:fixture-pass-word");
    let leaks = [
        "fixture-pass-word",
        basic.as_str(),
        "issued-access-token",
        "plain-client-secret",
        "query-secret",
        "captured-token-value",
        "deploy-secret-value",
    ];
    for leaked in leaks {
        assert!(!contains(&bytes, leaked), "history leaked {leaked}");
    }
    // 요청 JSON의 민감 이름 리터럴은 snapshot 정화 대상이다. 응답 가림은 값 기준(계획 §기록 정화)이라
    // 서버가 되돌려 준 본문에는 남을 수 있으므로 디코딩하지 않은 파일 원문에서만 본다.
    let raw: Vec<u8> = api
        .history_files()
        .iter()
        .flat_map(|p| std::fs::read(p).expect("history file"))
        .collect();
    assert!(
        !contains(&raw, "literal-private"),
        "snapshot kept a sensitive literal"
    );
    assert!(contains(&bytes, "{{token}}"));
    let page = api
        .api
        .history(HistoryQuery {
            before: None,
            limit: 10,
            origin: None,
        })
        .await
        .unwrap_or_else(|_| panic!("page"));
    let mut entries = vec![];
    for item in page.items {
        entries.push(
            api.api
                .history_get(HistoryId { id: item.id })
                .await
                .unwrap_or_else(|_| panic!("entry")),
        );
    }
    for entry in &entries {
        let body = data_encoding::BASE64
            .decode(entry.body_base64.as_bytes())
            .expect("history body");
        assert!(!body.is_empty(), "echo body is recorded for {}", entry.url);
        for leaked in leaks {
            assert!(!contains(&body, leaked), "history body leaked {leaked}");
        }
    }
    let deploy = entries
        .iter()
        .find(|e| e.url.contains("/deploy/"))
        .expect("deploy entry");
    assert!(
        deploy.url.contains("/deploy/[REDACTED]/request"),
        "{}",
        deploy.url
    );
    assert!(deploy.snapshot.url.contains("{{secret:DEPLOY}}"));
    let Body::Json { text } = &deploy.snapshot.body else {
        panic!("json body")
    };
    let json: serde_json::Value = serde_json::from_str(text).expect("json");
    assert_eq!(json["password"], REDACTED);
    assert_eq!(json["token"], "${TOKEN}");
    assert_eq!(json["secret"], "{{KEY}}");
    let oauth = entries
        .iter()
        .find(|e| e.url.ends_with("/oauth"))
        .expect("oauth entry");
    let Auth::Oauth2ClientCredentials {
        token_url,
        client_secret,
        ..
    } = &oauth.snapshot.auth
    else {
        panic!("oauth")
    };
    assert_eq!(client_secret, REDACTED);
    assert!(
        token_url.contains("/token?access_token=[REDACTED]"),
        "{token_url}"
    );
    let basic_entry = entries
        .iter()
        .find(|e| e.url.ends_with("/basic"))
        .expect("basic entry");
    assert_eq!(
        basic_entry.snapshot.auth,
        Auth::Basic {
            username: "fixture-user".into(),
            password: REDACTED.into()
        }
    );
}

#[tokio::test]
async fn graphql_history_masks_query_literals() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let send = |query: &str, variables: &str| {
        with_env(
            RequestSnapshot {
                body: Body::Graphql {
                    query: query.into(),
                    variables: variables.into(),
                    operation_name: Some("Viewer".into()),
                    method: GraphqlMethod::Post,
                },
                ..snapshot("POST", &server.url("/graphql"))
            },
            &[("TOKEN", "t", false)],
        )
    };
    api.complete(send(
        "query Viewer { viewer(token: \"query-secret\") { id } }",
        r#"{"token":"variable-secret","mixed_token":"prefix-${TOKEN}","reference_token":"${TOKEN}","id":"42"}"#,
    ))
    .await;
    let entry = api.only_history().await;
    let Body::Graphql {
        query,
        variables,
        operation_name,
        ..
    } = &entry.snapshot.body
    else {
        panic!("graphql")
    };
    assert_eq!(
        query,
        "query Viewer { viewer(token: \"[REDACTED]\") { id } }"
    );
    assert_eq!(
        variables,
        r#"{"id":"42","mixed_token":"[REDACTED]","reference_token":"${TOKEN}","token":"[REDACTED]"}"#
    );
    assert_eq!(operation_name.as_deref(), Some("Viewer"));
    let api = Harness::new().await;
    api.complete(send("query Viewer { viewer(token: \"unterminated", ""))
        .await;
    let bytes = api.history_bytes();
    assert!(!contains(&bytes, "unterminated"));
    assert!(contains(&bytes, REDACTED));
    assert!(!contains(&bytes, "query-secret") && !contains(&bytes, "variable-secret"));
}

#[tokio::test]
async fn multipart_history_keeps_no_file_paths_or_text_secrets() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let dir = tempfile::tempdir().expect("owned dir");
    std::fs::create_dir(dir.path().join("private-dir")).expect("dir");
    let file = dir.path().join("private-dir/report.txt");
    std::fs::write(&file, b"raw-bytes").expect("file");
    api.complete(with_env(
        RequestSnapshot {
            body: Body::Multipart {
                parts: vec![
                    Part::File {
                        name: "upload".into(),
                        path: file.display().to_string(),
                        file_name: None,
                        content_type: Some("text/plain".into()),
                    },
                    Part::Text {
                        name: "token".into(),
                        value: "direct-secret".into(),
                        content_type: None,
                    },
                    Part::Text {
                        name: "token".into(),
                        value: "${TOKEN}".into(),
                        content_type: None,
                    },
                ],
            },
            ..snapshot("POST", &server.url("/upload"))
        },
        &[("TOKEN", "t", false)],
    ))
    .await;
    assert!(server.only().body_text().contains("raw-bytes"));
    let entry = api.only_history().await;
    let Body::Multipart { parts } = &entry.snapshot.body else {
        panic!("multipart")
    };
    assert!(
        matches!(&parts[0], Part::File { path, file_name, .. } if path.is_empty() && file_name.as_deref() == Some("report.txt"))
    );
    assert!(matches!(&parts[1], Part::Text { value, .. } if value == REDACTED));
    assert!(matches!(&parts[2], Part::Text { value, .. } if value == "${TOKEN}"));
    let bytes = api.history_bytes();
    for leaked in ["private-dir", "direct-secret", "raw-bytes"] {
        assert!(!contains(&bytes, leaked), "history leaked {leaked}");
    }
}

const DAY: i64 = 24 * 60 * 60 * 1000;
const NOW: i64 = 1_790_000_000_000;

#[tokio::test]
async fn history_is_indexed_and_pruned_after_30_days() {
    let clock = Arc::new(AtomicI64::new(NOW - 31 * DAY));
    let at = clock.clone();
    let api =
        Harness::with_options(ApiOptions::default().with_clock(move || at.load(Ordering::SeqCst)))
            .await;
    api.api.wait_ready().await;
    let server = Server::ok().await;
    let origin = RequestOrigin {
        root: CollectionRoot::Personal,
        path: "a.request.json".into(),
    };
    for (offset, path) in [
        (31, "/old"),
        (1, "/yesterday"),
        (0, "/a"),
        (0, "/b"),
        (0, "/c"),
    ] {
        clock.store(NOW - offset * DAY + offset, Ordering::SeqCst);
        let mut req = request(snapshot("GET", &server.url(path)));
        if path == "/yesterday" {
            req.origin = Some(origin.clone());
        }
        api.complete(req).await;
    }
    let page = |before: Option<String>| HistoryQuery {
        before,
        limit: 2,
        origin: None,
    };
    let first = api
        .api
        .history(page(None))
        .await
        .unwrap_or_else(|_| panic!("page"));
    assert_eq!(first.items.len(), 2);
    let second = api
        .api
        .history(page(first.next.clone()))
        .await
        .unwrap_or_else(|_| panic!("page"));
    let third = api
        .api
        .history(page(second.next.clone()))
        .await
        .unwrap_or_else(|_| panic!("page"));
    assert_eq!(third.items.len(), 1);
    assert_eq!(third.next, None);
    assert!(third.items[0].url.ends_with("/old"));
    assert!(second.items[1].url.ends_with("/yesterday"));
    let filtered = api
        .api
        .history(HistoryQuery {
            before: None,
            limit: 10,
            origin: Some(origin.clone()),
        })
        .await
        .unwrap_or_else(|_| panic!("filter"));
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].origin, Some(origin));
    let days_before = std::fs::read_dir(api.paths.data_dir.join("api-history"))
        .expect("dir")
        .count();
    assert_eq!(days_before, 3);
    clock.store(NOW, Ordering::SeqCst);
    api.api
        .prune_history()
        .await
        .unwrap_or_else(|_| panic!("prune"));
    assert_eq!(api.history_rows().await, 4);
    assert_eq!(
        std::fs::read_dir(api.paths.data_dir.join("api-history"))
            .expect("dir")
            .count(),
        2
    );
    let bad = api
        .api
        .history(HistoryQuery {
            before: None,
            limit: 0,
            origin: None,
        })
        .await;
    assert!(matches!(
        bad,
        Err(devbox_protocol::Fail::Domain(ApiError::InvalidInput { .. }))
    ));
    let missing = api.api.history_get(HistoryId { id: "0".repeat(32) }).await;
    assert!(matches!(
        missing,
        Err(devbox_protocol::Fail::Domain(ApiError::HistoryNotFound))
    ));
}

#[tokio::test]
async fn history_index_is_rebuilt_when_the_domain_version_changes() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    api.complete(request(snapshot("GET", &server.url("/one"))))
        .await;
    api.complete(request(snapshot("GET", &server.url("/two"))))
        .await;
    let index = devbox_core::IndexDb::open(&api.paths.index_db_path).expect("index");
    assert!(
        index
            .ensure_domain("api", 99, "CREATE TABLE api_history(id TEXT);")
            .expect("bump")
    );
    let reopened = ApiService::new(
        api.db.clone(),
        devbox_core::IndexDb::open(&api.paths.index_db_path).expect("index"),
        api.paths.clone(),
        api.projects.clone(),
        api.secrets.clone(),
    )
    .expect("reopen");
    reopened.wait_ready().await;
    let page = reopened
        .history(HistoryQuery {
            before: None,
            limit: 10,
            origin: None,
        })
        .await
        .unwrap_or_else(|_| panic!("page"));
    assert_eq!(page.items.len(), 2);
    let entry = reopened
        .history_get(HistoryId {
            id: page.items[0].id.clone(),
        })
        .await
        .unwrap_or_else(|_| panic!("entry"));
    assert!(entry.url.ends_with("/two"));
    assert_eq!(entry.outcome, HistoryOutcome::Ok);
}

#[tokio::test]
async fn history_body_is_bounded_and_returned_up_to_one_mebibyte() {
    let api = Harness::new().await;
    let big = vec![b'z'; 17 * 1024 * 1024];
    let server = Server::start(handler(move |_| {
        let big = big.clone();
        async move { Reply::ok(big).header("content-type", "text/plain") }
    }))
    .await;
    let events = api
        .complete(request(snapshot("GET", &server.url("/big"))))
        .await;
    assert_eq!(body(&events).len(), 17 * 1024 * 1024);
    let end = events.iter().find_map(|e| match e {
        SendEvent::End {
            history_truncated,
            size_bytes,
            ..
        } => Some((*history_truncated, *size_bytes)),
        _ => None,
    });
    assert_eq!(end, Some((true, 17 * 1024 * 1024)));
    let entry = api.only_history().await;
    assert!(entry.body_truncated);
    assert_eq!(
        data_encoding::BASE64
            .decode(entry.body_base64.as_bytes())
            .expect("b64")
            .len(),
        1024 * 1024
    );
    let body_file = api
        .history_files()
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "body"))
        .expect(".body");
    assert_eq!(
        std::fs::metadata(body_file).expect("meta").len(),
        16 * 1024 * 1024
    );
}
