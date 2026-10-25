//! 리디렉션: origin 경계·본문·시한 하나(v0.9.0 http-client-engine 이식).
mod support;
use devbox_api::*;
use std::time::{Duration, Instant};
use support::api::*;
use support::{Reply, Seen, Server, handler};

#[tokio::test]
async fn graphql_redirects_only_keep_payload_on_the_original_origin() {
    for status in [301, 302, 303, 307, 308] {
        for cross_origin in [false, true] {
            let api = Harness::new().await;
            let target = Server::start(handler(|_| async { Reply::ok("{}") })).await;
            let target_addr = target.addr;
            let source = Server::start(handler(move |seen: Seen| async move {
                if seen.path() == "/graphql" {
                    let port = if cross_origin {
                        target_addr.port()
                    } else {
                        seen_port(&seen)
                    };
                    Reply::redirect(status, &format!("http://127.0.0.1:{port}/collect"))
                } else {
                    Reply::ok("{}").header("content-type", "application/json")
                }
            }))
            .await;
            let events = api
                .complete(request(RequestSnapshot {
                    query: vec![pair("label", "original-param")],
                    body: Body::Graphql {
                        query: "query Viewer($token: String!, $name: String!) { viewer(token: $token, name: $name) { id } }".into(),
                        variables: r#"{"token":"synthetic-private-token","name":"ordinary-variable"}"#.into(),
                        operation_name: Some("Viewer".into()),
                        method: GraphqlMethod::Post,
                    },
                    ..snapshot("POST", &source.url("/graphql"))
                }))
                .await;
            assert_eq!(head(&events).0, 200, "status={status}");
            let original = source.requests()[0].raw();
            assert!(original.contains("synthetic-private-token"));
            let receiver = if cross_origin { &target } else { &source };
            let redirected = receiver
                .requests()
                .into_iter()
                .find(|r| r.path() == "/collect")
                .expect("redirected request")
                .raw();
            for payload in ["synthetic-private-token", "ordinary-variable"] {
                assert_eq!(
                    redirected.contains(payload),
                    !cross_origin,
                    "status={status}, cross={cross_origin}, field={payload}"
                );
            }
            if cross_origin {
                assert!(!redirected.contains("original-param"));
                assert!(!redirected.contains("query="));
            }
            assert_eq!(
                redirected.starts_with("GET "),
                status <= 303,
                "status={status}"
            );
            let stream = serde_json::to_string(&events).expect("events");
            assert!(!stream.contains("synthetic-private-token"));
        }
    }
}

fn seen_port(seen: &Seen) -> u16 {
    seen.header("host")[0]
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .expect("host port")
}

#[tokio::test]
async fn redirect_headers_and_final_body_share_one_deadline() {
    for slow_body in [false, true] {
        let api = Harness::new().await;
        let server = Server::start(handler(move |seen: Seen| async move {
            if seen.path() == "/start" {
                Reply::redirect(302, "/final").delayed(Duration::from_millis(600))
            } else if slow_body {
                Reply::ok("ok").slow_body(Duration::from_millis(600))
            } else {
                Reply::ok("ok").delayed(Duration::from_millis(600))
            }
        }))
        .await;
        let started = Instant::now();
        let events = api
            .complete(request(RequestSnapshot {
                timeout_ms: 1000,
                ..snapshot("GET", &server.url("/start"))
            }))
            .await;
        let elapsed = started.elapsed();
        assert_eq!(
            failure_kind(&events),
            "api.network:timeout",
            "slow_body={slow_body}"
        );
        assert!(elapsed < Duration::from_millis(1700), "{elapsed:?}");
        assert_eq!(server.requests().len(), 2);
    }
}

#[tokio::test]
async fn live_cross_origin_redirect_strips_auth_and_redacts_every_return_path() {
    let api = Harness::new().await;
    let destination = Server::start(handler(|_| async {
        Reply::ok(r#"{"echo":"cross-origin-secret","access_token":"server-token"}"#)
            .header("content-type", "application/json")
            .header("set-cookie", "sid=cross-origin-secret")
    }))
    .await;
    let port = destination.addr.port();
    let redirect = Server::start(handler(move |_| async move {
        Reply::redirect(
            302,
            &format!("http://127.0.0.1:{port}/finish?request_id=redirect-ok"),
        )
    }))
    .await;
    let events = api
        .complete(request(RequestSnapshot {
            auth: Auth::Bearer {
                token: "cross-origin-secret".into(),
            },
            cookies: vec![header(0, "session", "cross-origin-secret")],
            ..snapshot("GET", &redirect.url("/start"))
        }))
        .await;
    assert_eq!(
        redirect.only().header("authorization"),
        ["Bearer cross-origin-secret"]
    );
    let seen = destination.only();
    assert!(seen.header("authorization").is_empty());
    assert!(seen.header("cookie").is_empty());
    let (status, headers, redirects) = head(&events);
    assert_eq!(status, 200);
    assert!(
        !String::from_utf8(body(&events))
            .expect("utf8")
            .contains("cross-origin-secret")
    );
    let set_cookie = headers
        .iter()
        .find(|h| h.name == "set-cookie")
        .expect("set-cookie");
    assert_eq!(
        set_cookie.value, "[REDACTED]",
        "Set-Cookie display is masked by header name"
    );
    assert_eq!(redirects.len(), 1);
    assert!(redirects[0].location.contains("request_id=redirect-ok"));
    assert!(
        !serde_json::to_string(&events)
            .expect("events")
            .contains("cross-origin-secret")
    );
    assert!(!contains(&api.history_bytes(), "cross-origin-secret"));
}

#[tokio::test]
async fn live_cross_origin_307_and_308_suppress_body_and_derived_secret_headers() {
    for status in [307, 308] {
        let api = Harness::new().await;
        let destination = Server::ok().await;
        let port = destination.addr.port();
        let redirect = Server::start(handler(move |_| async move {
            Reply::redirect(status, &format!("http://127.0.0.1:{port}/finish"))
        }))
        .await;
        let events = api
            .complete(request(RequestSnapshot {
                headers: vec![
                    header(0, "Cookie", "sid=cross-origin-secret"),
                    header(1, "X-Api-Key", "cross-origin-secret"),
                    header(2, "X-Debug", "cross-origin-secret"),
                    header(3, "Content-Type", "text/plain"),
                    header(4, "Content-Encoding", "identity"),
                ],
                body: Body::Raw {
                    encoding: None,
                    text: "payload=cross-origin-secret".into(),
                    content_type: None,
                },
                auth: Auth::Bearer {
                    token: "cross-origin-secret".into(),
                },
                ..snapshot("POST", &redirect.url("/start"))
            }))
            .await;
        assert_eq!(head(&events).0, 200);
        assert!(redirect.only().body_text().contains("cross-origin-secret"));
        let observed = destination.only().raw();
        let lower = observed.to_ascii_lowercase();
        assert!(observed.starts_with("POST /finish HTTP/1.1"), "{observed}");
        assert!(!observed.contains("cross-origin-secret"));
        for name in [
            "authorization",
            "cookie",
            "x-api-key",
            "x-debug",
            "content-type",
            "content-encoding",
        ] {
            assert!(
                !lower.contains(&format!("\r\n{name}:")),
                "status={status} leaked {name}"
            );
        }
    }
}

#[tokio::test]
async fn live_cross_origin_redirect_with_sensitive_destination_is_blocked() {
    let api = Harness::new().await;
    let destination = Server::ok().await;
    let port = destination.addr.port();
    let redirect = Server::start(handler(move |_| async move {
        Reply::redirect(
            302,
            &format!("http://127.0.0.1:{port}/finish?token=cross-origin-secret"),
        )
    }))
    .await;
    let events = api
        .complete(request(RequestSnapshot {
            auth: Auth::Bearer {
                token: "cross-origin-secret".into(),
            },
            ..snapshot("GET", &redirect.url("/start"))
        }))
        .await;
    assert_eq!(failure_kind(&events), "api.network:redirect");
    let text = serde_json::to_string(&events).expect("events");
    assert!(!text.contains("cross-origin-secret") && !text.contains(&port.to_string()));
    assert_eq!(destination.accepted_connections(), 0);
}

#[tokio::test]
async fn redirect_limit_and_disabled_following_are_explicit() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        let n: u32 = seen.path().trim_start_matches("/r").parse().unwrap_or(0);
        Reply::redirect(302, &format!("/r{}", n + 1))
    }))
    .await;
    let events = api
        .complete(request(snapshot("GET", &server.url("/r0"))))
        .await;
    assert_eq!(failure_kind(&events), "api.network:redirect");
    assert_eq!(server.requests().len(), 11);
    let events = api
        .complete(request(RequestSnapshot {
            follow_redirects: false,
            ..snapshot("GET", &server.url("/r0"))
        }))
        .await;
    let (status, headers, redirects) = head(&events);
    assert_eq!(status, 302);
    assert!(redirects.is_empty());
    assert!(
        headers
            .iter()
            .any(|h| h.name == "location" && h.value == "/r1")
    );
}

#[tokio::test]
async fn same_origin_redirect_keeps_credentials() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        if seen.path() == "/start" {
            Reply::redirect(307, "/next")
        } else {
            Reply::ok("ok")
        }
    }))
    .await;
    api.complete(request(RequestSnapshot {
        auth: Auth::Bearer {
            token: "same-origin".into(),
        },
        body: Body::Json { text: "{}".into() },
        ..snapshot("PUT", &server.url("/start"))
    }))
    .await;
    let next = server
        .requests()
        .into_iter()
        .find(|r| r.path() == "/next")
        .expect("next");
    assert_eq!(next.method, "PUT");
    assert_eq!(next.header("authorization"), ["Bearer same-origin"]);
    assert_eq!(next.body_text(), "{}");
}
