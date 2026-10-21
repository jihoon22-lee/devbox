//! 인증(Basic·Bearer·API key·OAuth 2)과 쿠키 jar.
mod support;
use devbox_api::*;
use sha2::Digest;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use support::api::*;
use support::{Reply, Seen, Server, handler};

#[tokio::test]
async fn basic_bearer_and_api_key_are_sent_from_the_active_auth() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    for auth in [
        Auth::Basic {
            username: "user".into(),
            password: "pass".into(),
        },
        Auth::Bearer {
            token: "bearer-token".into(),
        },
        Auth::ApiKey {
            name: "X-Api-Key".into(),
            value: "key-value".into(),
            placement: KeyPlacement::Header,
        },
        Auth::ApiKey {
            name: "api_key".into(),
            value: "a b+c".into(),
            placement: KeyPlacement::Query,
        },
    ] {
        api.complete(request(RequestSnapshot {
            auth,
            ..snapshot("GET", &server.url("/auth"))
        }))
        .await;
    }
    let seen = server.requests();
    assert_eq!(seen[0].header("authorization"), ["Basic dXNlcjpwYXNz"]);
    assert_eq!(seen[1].header("authorization"), ["Bearer bearer-token"]);
    assert_eq!(seen[2].header("x-api-key"), ["key-value"]);
    assert_eq!(
        seen[3].query_pairs(),
        [("api_key".to_string(), "a b+c".to_string())]
    );
    let err = api
        .send(request(RequestSnapshot {
            auth: Auth::Bearer {
                token: "a\nb".into(),
            },
            ..snapshot("GET", &server.url("/"))
        }))
        .await
        .err()
        .expect("control character");
    assert_eq!(
        (err.code(), err.detail()["field"].as_str()),
        ("api.invalid_input", Some("auth"))
    );
}

fn token_server(token: &'static str, delay: Duration) -> impl std::future::Future<Output = Server> {
    Server::start(handler(move |_| async move {
        Reply::ok(format!(
            r#"{{"access_token":"{token}","token_type":"bearer","expires_in":3600}}"#
        ))
        .header("content-type", "application/json")
        .delayed(delay)
    }))
}

fn client_credentials(token_url: String, secret: &str) -> Auth {
    Auth::Oauth2ClientCredentials {
        token_url,
        client_id: "devbox".into(),
        client_secret: secret.into(),
        scope: Some("read".into()),
    }
}

#[tokio::test]
async fn oauth2_actual_send_fetches_once_and_masks_the_token_response() {
    let api = Harness::new().await;
    let tokens = token_server("actual-oauth-token", Duration::ZERO).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let server = Server::start(handler(move |_| {
        let n = counter.fetch_add(1, Ordering::SeqCst);
        async move { Reply::status(if n == 1 { 401 } else { 200 }, "actual-oauth-token") }
    }))
    .await;
    let send = || {
        request(RequestSnapshot {
            auth: client_credentials(tokens.url("/token"), "client-secret"),
            ..snapshot("GET", &server.url("/me"))
        })
    };
    let first = api.complete(send()).await;
    assert_eq!(body(&first), b"[REDACTED]");
    let token_request = tokens.only().body_text();
    assert!(token_request.contains("grant_type=client_credentials"));
    assert!(token_request.contains("client_id=devbox") && token_request.contains("scope=read"));
    let second = api.complete(send()).await;
    assert_eq!(head(&second).0, 401);
    assert_eq!(tokens.requests().len(), 1, "cached token reused");
    api.complete(send()).await;
    assert_eq!(tokens.requests().len(), 2, "401 expires the cached token");
    for seen in server.requests() {
        assert_eq!(seen.header("authorization"), ["Bearer actual-oauth-token"]);
    }
    let history = api.history_bytes();
    assert!(!contains(&history, "actual-oauth-token") && !contains(&history, "client-secret"));
}

#[tokio::test]
async fn oauth2_token_wait_is_cancelled_by_the_request_owner() {
    let api = Harness::new().await;
    let tokens = token_server("late", Duration::from_secs(30)).await;
    let server = Server::ok().await;
    let sent = api
        .send(request(RequestSnapshot {
            auth: client_credentials(tokens.url("/token"), "secret"),
            ..snapshot("GET", &server.url("/me"))
        }))
        .await
        .expect("open");
    assert!(
        tokens
            .wait(|s| s.requests().len() == 1, Duration::from_secs(5))
            .await
    );
    sent.close();
    assert!(
        tokens
            .wait(|s| s.closed_connections() == 1, Duration::from_secs(1))
            .await
    );
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn oauth2_resolves_active_configuration_and_masks_client_and_access_tokens() {
    let api = Harness::new().await;
    let tokens = token_server("opaque-access-token", Duration::ZERO).await;
    let server = Server::start(handler(|_| async {
        Reply::ok("private-client-secret opaque-access-token")
    }))
    .await;
    let events = api
        .complete(with_env(
            RequestSnapshot {
                auth: client_credentials(tokens.url("/token"), "{{CLIENT_SECRET}}"),
                ..snapshot("GET", &server.url("/me"))
            },
            &[("CLIENT_SECRET", "private-client-secret", false)],
        ))
        .await;
    assert!(
        tokens
            .only()
            .body_text()
            .contains("client_secret=private-client-secret")
    );
    assert_eq!(body(&events), b"[REDACTED] [REDACTED]");
    let failing = Server::start(handler(|_| async {
        Reply::status(400, "{\"error\":\"invalid_client\"}")
    }))
    .await;
    let events = api
        .complete(request(RequestSnapshot {
            auth: client_credentials(failing.url("/token"), "other"),
            ..snapshot("GET", &server.url("/me"))
        }))
        .await;
    let error = failure(&events);
    assert_eq!(error.code, "api.oauth_failed");
    assert!(error.detail.expect("detail").get().contains("token_status"));
}

#[tokio::test]
async fn oauth2_token_response_is_bounded() {
    let api = Harness::new().await;
    let padding = "a".repeat(80 * 1024);
    let tokens = Server::start(handler(move |_| {
        let body = format!(r#"{{"access_token":"bounded-token","pad":"{padding}"}}"#);
        async move { Reply::ok(body).header("content-type", "application/json") }
    }))
    .await;
    let server = Server::ok().await;
    let events = api
        .complete(request(RequestSnapshot {
            auth: client_credentials(tokens.url("/token"), "secret"),
            ..snapshot("GET", &server.url("/me"))
        }))
        .await;
    let error = failure(&events);
    assert_eq!(error.code, "api.oauth_failed");
    assert!(
        error
            .detail
            .expect("detail")
            .get()
            .contains("token_too_large")
    );
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn oauth2_authorization_url_rejects_secret_references() {
    let api = Harness::new().await;
    api.secret("AUTHZ", "authz-secret-value").await;
    let server = Server::ok().await;
    let tokens = Server::ok().await;
    let auth =
        |authorization_url: String, client_id: &str, scope: Option<&str>| Auth::Oauth2AuthCode {
            authorization_url,
            token_url: tokens.url("/token"),
            client_id: client_id.into(),
            client_secret: None,
            scope: scope.map(Into::into),
        };
    let authorize = server.url("/authorize");
    for send in [
        request(RequestSnapshot {
            auth: auth(
                server.url("/authorize?hint={{secret:AUTHZ}}"),
                "devbox-app",
                None,
            ),
            ..snapshot("GET", &server.url("/me"))
        }),
        with_env(
            RequestSnapshot {
                auth: auth(server.url("/authorize?hint={{HINT}}"), "devbox-app", None),
                ..snapshot("GET", &server.url("/me"))
            },
            &[("HINT", "sensitive-hint", true)],
        ),
        // client_id·scope도 브라우저로 가는 인증 URL에 실린다.
        request(RequestSnapshot {
            auth: auth(authorize.clone(), "{{secret:AUTHZ}}", None),
            ..snapshot("GET", &server.url("/me"))
        }),
        with_env(
            RequestSnapshot {
                auth: auth(authorize.clone(), "devbox-app", Some("read {{HINT}}")),
                ..snapshot("GET", &server.url("/me"))
            },
            &[("HINT", "sensitive-hint", true)],
        ),
    ] {
        let err = api
            .send(send)
            .await
            .err()
            .expect("rejected before the browser");
        assert_eq!(
            (err.code(), err.detail()["reason"].as_str()),
            ("api.invalid_input", Some("secret_in_authorization_url"))
        );
        assert!(!err.detail_text().contains("authz-secret-value"));
    }
    assert_eq!(server.accepted_connections(), 0);
    assert_eq!(tokens.accepted_connections(), 0);
}

async fn get(port: u16, target: &str) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .ok()?;
    let head = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(head.as_bytes()).await.ok()?;
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut out)).await;
    String::from_utf8_lossy(&out)
        .split(' ')
        .nth(1)?
        .parse()
        .ok()
}

#[tokio::test]
async fn auth_code_pkce_uses_a_one_time_loopback_callback() {
    let api = Harness::new().await;
    let tokens = token_server("pkce-token", Duration::ZERO).await;
    let server = Server::ok().await;
    let mut sent = api
        .send(request(RequestSnapshot {
            auth: Auth::Oauth2AuthCode {
                authorization_url: server.url("/authorize?prompt=login"),
                token_url: tokens.url("/token"),
                client_id: "devbox-app".into(),
                client_secret: None,
                scope: Some("read".into()),
            },
            ..snapshot("GET", &server.url("/me"))
        }))
        .await
        .expect("open");
    assert!(matches!(
        sent.next(Duration::from_secs(5)).await,
        Some(SendEvent::Started { .. })
    ));
    let Some(SendEvent::Authorize { url }) = sent.next(Duration::from_secs(5)).await else {
        panic!("authorize event")
    };
    let url = url::Url::parse(&url).expect("authorize url");
    let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(
        (q["response_type"].as_str(), q["client_id"].as_str()),
        ("code", "devbox-app")
    );
    assert_eq!(
        (
            q["code_challenge_method"].as_str(),
            q["scope"].as_str(),
            q["prompt"].as_str()
        ),
        ("S256", "read", "login")
    );
    let redirect = url::Url::parse(&q["redirect_uri"]).expect("redirect uri");
    assert_eq!(
        (redirect.host_str(), redirect.path()),
        (Some("127.0.0.1"), "/callback")
    );
    let port = redirect.port().expect("loopback port");
    let state = q["state"].clone();
    assert!(state.len() >= 32);
    assert_eq!(get(port, "/callback?code=x&state=wrong").await, Some(400));
    assert_eq!(
        get(port, &format!("/callback?code=fixture-code&state={state}")).await,
        Some(200)
    );
    let events = sent.events().await;
    assert_eq!(head(&events).0, 200);
    assert_eq!(server.only().header("authorization"), ["Bearer pkce-token"]);
    let exchange: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(&tokens.only().body)
            .into_owned()
            .collect();
    assert_eq!(exchange["grant_type"], "authorization_code");
    assert_eq!(exchange["code"], "fixture-code");
    assert_eq!(exchange["redirect_uri"], q["redirect_uri"]);
    let digest = sha2::Sha256::digest(exchange["code_verifier"].as_bytes());
    assert_eq!(
        data_encoding::BASE64URL_NOPAD.encode(&digest),
        q["code_challenge"]
    );
    assert_eq!(
        get(port, &format!("/callback?code=again&state={state}")).await,
        None,
        "callback closed after use"
    );
}

#[tokio::test]
async fn cookie_jar_persists_per_collection_and_rows_override() {
    let api = Harness::new().await;
    let server = Server::start(handler(|seen: Seen| async move {
        if seen.path() == "/set" {
            Reply::ok("set")
                .header("set-cookie", "sid=jar-value; Path=/")
                .header("set-cookie", "theme=dark; Path=/")
        } else {
            Reply::ok("ok")
        }
    }))
    .await;
    let jar = |key: &str, cookies: Vec<HeaderRow>, path: &str| SendRequest {
        cookie_jar: Some(key.into()),
        ..request(RequestSnapshot {
            cookies,
            ..snapshot("GET", &server.url(path))
        })
    };
    api.complete(jar("personal:/a", vec![], "/set")).await;
    api.complete(jar(
        "personal:/a",
        vec![header(0, "theme", "light")],
        "/use",
    ))
    .await;
    api.complete(jar("personal:/b", vec![], "/other")).await;
    let seen = server.requests();
    assert_eq!(seen[1].header("cookie"), ["sid=jar-value; theme=light"]);
    assert!(
        seen[2].header("cookie").is_empty(),
        "other jars are separate"
    );
    let listed = api
        .api
        .cookies(CookieJarRef {
            jar: "personal:/a".into(),
        })
        .await
        .unwrap_or_else(|_| panic!("cookies"));
    let names: Vec<_> = listed
        .cookies
        .iter()
        .map(|c| (c.name.as_str(), c.value.as_str()))
        .collect();
    assert!(names.contains(&("sid", "jar-value")) && names.contains(&("theme", "dark")));
    let digest = data_encoding::HEXLOWER.encode(&sha2::Sha256::digest(b"personal:/a"));
    let file = api
        .paths
        .data_dir
        .join("api-cookies")
        .join(format!("{digest}.json"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&file)
            .expect("jar file")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(file.parent().expect("jar dir"))
            .expect("jar dir")
            .permissions()
            .mode()
            & 0o777,
        0o700,
        "api-cookies directory is private"
    );
    api.api
        .cookies_clear(CookieJarRef {
            jar: "personal:/a".into(),
        })
        .await
        .unwrap_or_else(|_| panic!("clear"));
    assert!(!file.exists());
    api.complete(jar("personal:/a", vec![], "/after")).await;
    assert!(server.requests()[3].header("cookie").is_empty());
}
