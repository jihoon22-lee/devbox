//! Owned localhost credential-echo regressions; fixtures never use personal credentials.
mod support;
use devbox_api::*;
use support::api::*;
use support::{Reply, Seen, Server, handler};

const CREDENTIAL: &str = "owned-literal-credential-v1";
async fn authorization_echo(structured: bool) {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        let value = seen.header("authorization")[0]
            .strip_prefix("Bearer ")
            .expect("owned bearer")
            .to_owned();
        Reply::ok(format!("readable-prefix|{value}|readable-suffix"))
            .header("x-echo-credential", &value)
            .header("content-type", "text/plain")
    }))
    .await;
    let mut input = snapshot("GET", &server.url("/echo"));
    if structured {
        input.auth = Auth::Bearer {
            token: CREDENTIAL.into(),
        };
    } else {
        input
            .headers
            .push(header(0, "Authorization", &format!("Bearer {CREDENTIAL}")));
    }
    let events = api.complete(request(input)).await;
    let bytes = body(&events);
    assert!(
        !contains(&bytes, CREDENTIAL),
        "credential must not reach streamed body"
    );
    assert!(
        contains(&bytes, "readable-prefix"),
        "noncredential content stays readable"
    );
    assert!(
        !contains(
            &serde_json::to_vec(&head(&events).1).expect("headers"),
            CREDENTIAL
        ),
        "credential echo header must be masked"
    );
    assert!(
        !contains(&api.history_bytes(), CREDENTIAL),
        "metadata and decoded history body must both be masked"
    );
}
#[tokio::test]
async fn structured_bearer_echo_control_is_masked() {
    authorization_echo(true).await;
}
#[tokio::test]
async fn direct_authorization_echo_is_masked_like_structured_auth() {
    authorization_echo(false).await;
}

fn assert_masked(events: &[SendEvent], credentials: &[&str]) {
    let decoded = body(events);
    let metadata = serde_json::to_vec(events).expect("event json");
    for value in credentials {
        assert!(
            !contains(&decoded, value),
            "credential must not reach decoded body"
        );
        assert!(
            !contains(&metadata, value),
            "credential must not reach event metadata"
        );
    }
}

#[tokio::test]
async fn literal_headers_cookie_rows_and_ordinary_content_have_precise_scope() {
    let api = Harness::new().await;
    let password = "fixture-basic-password-124";
    let basic = format!(
        "Basic {}",
        data_encoding::BASE64.encode(format!("fixture-user:{password}").as_bytes())
    );
    let server = Server::start(handler(move |seen: Seen| async move {
        let mut echoes = seen.raw();
        if seen
            .header("proxy-authorization")
            .iter()
            .any(|v| v.starts_with("Basic "))
        {
            echoes.push_str("|fixture-basic-password-124|");
        }
        echoes.push_str("|public-response-token|readable-marker|");
        Reply::ok(echoes)
    }))
    .await;
    let cases = [
        ("pRoXy-AuThOrIzAtIoN", basic.as_str(), password),
        (
            "Authorization",
            "Custom fixture-unknown-scheme-245",
            "fixture-unknown-scheme-245",
        ),
        ("X-API-Key", "fixture-api-key-356", "fixture-api-key-356"),
        ("API-Key", "fixture-api-key-467", "fixture-api-key-467"),
        (
            "X-Auth-Token",
            "fixture-auth-token-578",
            "fixture-auth-token-578",
        ),
        (
            "Cookie",
            "sid=fixture-cookie-689; q=\"fixture-cookie-780\"",
            "fixture-cookie-689",
        ),
    ];
    for (name, value, secret) in cases {
        let mut input = snapshot("POST", &server.url("/echo"));
        input.headers.push(header(0, name, value));
        input
            .headers
            .push(header(1, "x-public", "public-header-value"));
        input.body = Body::Json {
            text: r#"{"access_token":"public-response-token","plain":"ordinary-body"}"#.into(),
        };
        let events = api.complete(request(input)).await;
        assert_masked(&events, &[secret]);
        if name == "Cookie" {
            assert_masked(&events, &["fixture-cookie-780"]);
        }
        assert!(
            contains(&body(&events), "public-response-token"),
            "arbitrary response JSON values are not inferred"
        );
        assert!(contains(&body(&events), "public-header-value"));
        assert!(contains(&body(&events), "ordinary-body"));
    }
    let mut input = snapshot("GET", &server.url("/rows"));
    input
        .cookies
        .push(header(0, "row_sid", "fixture-cookie-row-891"));
    let events = api.complete(request(input)).await;
    assert_masked(&events, &["fixture-cookie-row-891"]);
    let history = api.history_bytes();
    for value in [
        password,
        "fixture-unknown-scheme-245",
        "fixture-api-key-356",
        "fixture-api-key-467",
        "fixture-auth-token-578",
        "fixture-cookie-689",
        "fixture-cookie-780",
        "fixture-cookie-row-891",
    ] {
        assert!(
            !contains(&history, value),
            "recognized credential must not reach history"
        );
    }
}

#[tokio::test]
async fn response_cookies_mask_binary_body_headers_redirects_and_history() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        if seen.path() == "/start" {
            Reply::redirect(302,"/final?ordinary=fixture-future-cookie-102")
                .header("set-cookie","first=fixture-first-cookie-213; Path=/")
        } else {
            let mut bytes = b"\xff\x00readable-marker|fixture-first-cookie-213|fixture-future-cookie-102|fixture-quoted-cookie-324|".to_vec();
            bytes.extend_from_slice(&[0xfe,0]);
            Reply::ok(bytes)
                .header("set-cookie","next=fixture-future-cookie-102; Path=/; HttpOnly")
                .header("set-cookie","quoted=\"fixture-quoted-cookie-324\"; SameSite=Lax")
                .header("set-cookie","empty=; Path=/")
                .header("set-cookie","emptyquoted=\"\"; Path=/")
                .header("x-echo","fixture-future-cookie-102")
                .header("content-type","application/octet-stream")
        }
    })).await;
    let events = api
        .complete(request(snapshot("GET", &server.url("/start"))))
        .await;
    let values = [
        "fixture-first-cookie-213",
        "fixture-future-cookie-102",
        "fixture-quoted-cookie-324",
    ];
    assert_masked(&events, &values);
    assert!(contains(&body(&events), "readable-marker"));
    assert!(body(&events).starts_with(&[0xff, 0]));
    assert_eq!(head(&events).2.len(), 1);
    for value in values {
        assert!(
            !contains(&api.history_bytes(), value),
            "new response cookie must not reach history"
        );
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SendEvent::End { binary: true, .. }))
    );
}

#[tokio::test]
async fn only_actually_selected_jar_cookie_values_are_inferred() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        if seen.path() == "/set" {
            Reply::ok("created")
                .header("set-cookie", "selected=fixture-selected-435; Path=/use")
                .header("set-cookie", "override=fixture-overridden-546; Path=/use")
                .header(
                    "set-cookie",
                    "unselected=fixture-unselected-657; Path=/elsewhere",
                )
        } else {
            Reply::ok(format!(
                "{}|fixture-overridden-546|fixture-unselected-657|readable-marker",
                seen.raw()
            ))
        }
    }))
    .await;
    let mut set = request(snapshot("GET", &server.url("/set")));
    set.cookie_jar = Some("credential-fixture-jar".into());
    api.complete(set).await;
    let mut use_jar = request(snapshot("GET", &server.url("/use")));
    use_jar.cookie_jar = Some("credential-fixture-jar".into());
    use_jar
        .snapshot
        .cookies
        .push(header(0, "override", "fixture-row-override-768"));
    let events = api.complete(use_jar).await;
    assert_masked(
        &events,
        &["fixture-selected-435", "fixture-row-override-768"],
    );
    let decoded = body(&events);
    assert!(
        contains(&decoded, "fixture-overridden-546"),
        "overridden jar value is not inferred"
    );
    assert!(
        contains(&decoded, "fixture-unselected-657"),
        "unselected jar path is not inferred"
    );
    assert!(!contains(&api.history_bytes(), "fixture-selected-435"));
    assert!(!contains(&api.history_bytes(), "fixture-row-override-768"));
    let outgoing = &server.requests()[1];
    assert!(
        outgoing.header("cookie")[0].contains("fixture-selected-435"),
        "selected cookie was sent"
    );
    assert!(!outgoing.header("cookie")[0].contains("fixture-overridden-546"));
}

#[tokio::test]
async fn response_seed_capacity_failure_emits_no_head_body_or_history() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        if seen.path() == "/start" {
            Reply::redirect(302, "/overflow")
                .header("set-cookie", "prior=fixture-prior-879; Path=/")
        } else {
            Reply::ok("must-not-publish-body").header(
                "set-cookie",
                &format!("overflow={}", "x".repeat(256 * 1024 + 1)),
            )
        }
    }))
    .await;
    let events = api
        .complete(request(snapshot("GET", &server.url("/start"))))
        .await;
    assert_eq!(failure(&events).code, "api.redaction_failed");
    assert!(!events.iter().any(|event| matches!(
        event,
        SendEvent::Head { .. } | SendEvent::Body { .. } | SendEvent::End { .. }
    )));
    assert!(api.history_files().is_empty());
    assert_eq!(api.history_rows().await, 0);
    assert_eq!(
        server.requests().len(),
        2,
        "failure follows actual redirect response"
    );
}
