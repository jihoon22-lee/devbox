//! 요청 단위 TLS(소유 rcgen CA·127.0.0.1 HTTPS fixture).
mod support;
use devbox_api::*;
use support::api::*;
use support::tls::Pki;
use support::{Reply, Server, handler};

fn tls(ca: Option<String>, cert: Option<String>, key: Option<String>, verify: bool) -> TlsOptions {
    TlsOptions {
        verify,
        ca_path: ca,
        client_cert_path: cert,
        client_key_path: key,
    }
}

async fn https(pki: &Pki, require_client: bool) -> Server {
    Server::spawn(
        handler(|_| async { Reply::ok("secure") }),
        Some(pki.server(require_client)),
    )
    .await
}

#[tokio::test]
async fn http_clients_accept_valid_pem_material_and_reject_broken_material() {
    let api = Harness::new().await;
    let pki = Pki::new();
    let server = https(&pki, false).await;
    let ca = pki.write("ca.pem", &pki.ca_pem);
    let cert = pki.write("client.pem", &pki.client_cert_pem);
    let key = pki.write("client.key", &pki.client_key_pem);
    let events = api
        .complete(request(RequestSnapshot {
            tls: tls(
                Some(ca.clone()),
                Some(cert.clone()),
                Some(key.clone()),
                true,
            ),
            ..snapshot("GET", &server.url("/"))
        }))
        .await;
    assert_eq!(body(&events), b"secure");
    assert!(server.only().client_cert, "client identity is presented");
    let broken = pki.write(
        "broken.pem",
        "-----BEGIN CERTIFICATE-----\nnope\n-----END CERTIFICATE-----\n",
    );
    let garbage = pki.write(
        "garbage.key",
        "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n",
    );
    for (options, reason) in [
        (tls(Some(broken.clone()), None, None, true), "invalid_ca"),
        (
            tls(None, Some(cert.clone()), None, true),
            "incomplete_identity",
        ),
        (
            tls(None, Some(cert.clone()), Some(garbage), true),
            "invalid_key",
        ),
        (
            tls(None, Some(broken), Some(key.clone()), true),
            "invalid_certificate",
        ),
    ] {
        let err = api
            .send(request(RequestSnapshot {
                tls: options,
                ..snapshot("GET", &server.url("/"))
            }))
            .await
            .err()
            .expect("broken material");
        assert_eq!(
            (err.code(), err.detail()["reason"].as_str()),
            ("api.tls_invalid", Some(reason))
        );
        assert!(
            !err.detail_text()
                .contains(pki.dir.path().to_str().expect("utf8 path"))
        );
    }
    assert_eq!(server.accepted_connections(), 1);
}

#[tokio::test]
async fn tls_missing_credential_precedes_oauth_token_resolution() {
    let api = Harness::new().await;
    let tokens = Server::ok().await;
    let missing = format!(
        "{}/devbox-missing-ca-{}.pem",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let err = api
        .send(request(RequestSnapshot {
            tls: tls(Some(missing.clone()), None, None, false),
            auth: Auth::Oauth2ClientCredentials {
                token_url: tokens.url("/token"),
                client_id: "client".into(),
                client_secret: "secret".into(),
                scope: None,
            },
            ..snapshot("GET", "https://not-sent.test/")
        }))
        .await
        .err()
        .expect("missing credential");
    assert_eq!(
        (err.code(), err.detail()["reason"].as_str()),
        ("api.tls_invalid", Some("missing_file"))
    );
    assert!(!err.detail_text().contains(&missing));
    assert_eq!(tokens.accepted_connections(), 0);
}

#[tokio::test]
async fn tls_missing_credential_fails_before_connecting_even_when_verification_is_off() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let err = api
        .send(request(RequestSnapshot {
            tls: tls(
                None,
                Some("/nonexistent/devbox/client.pem".into()),
                Some("/nonexistent/devbox/client.key".into()),
                false,
            ),
            ..snapshot("GET", &server.url("/"))
        }))
        .await
        .err()
        .expect("missing credential");
    assert_eq!(err.code(), "api.tls_invalid");
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn tls_verification_defaults_on_and_is_recorded_when_disabled() {
    assert!(TlsOptions::default().verify);
    let pki = Pki::new();
    let server = https(&pki, false).await;
    let api = Harness::new().await;
    let events = api
        .complete(request(snapshot("GET", &server.url("/untrusted"))))
        .await;
    assert_eq!(failure_kind(&events), "api.network:tls");
    let events = api
        .complete(request(RequestSnapshot {
            tls: tls(None, None, None, false),
            ..snapshot("GET", &server.url("/insecure"))
        }))
        .await;
    assert_eq!(body(&events), b"secure");
    let ca = pki.write("ca.pem", &pki.ca_pem);
    api.complete(request(RequestSnapshot {
        tls: tls(Some(ca), None, None, true),
        ..snapshot("GET", &server.url("/trusted"))
    }))
    .await;
    let page = api
        .api
        .history(HistoryQuery {
            before: None,
            limit: 10,
            origin: None,
        })
        .await
        .unwrap_or_else(|_| panic!("page"));
    let mut verify = vec![];
    for item in page.items {
        let entry = api
            .api
            .history_get(HistoryId { id: item.id })
            .await
            .unwrap_or_else(|_| panic!("entry"));
        verify.push((
            entry.url.rsplit('/').next().unwrap_or("").to_owned(),
            entry.tls_verify,
        ));
    }
    verify.sort();
    assert_eq!(
        verify,
        [
            ("insecure".into(), false),
            ("trusted".into(), true),
            ("untrusted".into(), true)
        ]
    );
}

#[tokio::test]
async fn mutual_tls_requires_the_configured_identity() {
    let api = Harness::new().await;
    let pki = Pki::new();
    let server = https(&pki, true).await;
    let ca = pki.write("ca.pem", &pki.ca_pem);
    let events = api
        .complete(request(RequestSnapshot {
            tls: tls(Some(ca.clone()), None, None, true),
            ..snapshot("GET", &server.url("/"))
        }))
        .await;
    assert!(failure_kind(&events).starts_with("api.network:"));
    let cert = pki.write("client.pem", &pki.client_cert_pem);
    let key = pki.write("client.key", &pki.client_key_pem);
    let events = api
        .complete(request(RequestSnapshot {
            tls: tls(Some(ca), Some(cert), Some(key), true),
            ..snapshot("GET", &server.url("/"))
        }))
        .await;
    assert_eq!(body(&events), b"secure");
}

#[tokio::test]
async fn tls_client_identity_does_not_cross_redirect_origins() {
    let api = Harness::new().await;
    let pki = Pki::new();
    let target = Server::ok().await;
    let destination = target.url("/other");
    let source = Server::start(handler(move |_| {
        let destination = destination.clone();
        async move { Reply::redirect(302, &destination) }
    }))
    .await;
    let cert = pki.write("client.pem", &pki.client_cert_pem);
    let key = pki.write("client.key", &pki.client_key_pem);
    let events = api
        .complete(request(RequestSnapshot {
            tls: tls(None, Some(cert), Some(key), true),
            ..snapshot("GET", &source.url("/start"))
        }))
        .await;
    assert_eq!(failure_kind(&events), "api.network:redirect");
    assert_eq!(source.requests().len(), 1);
    assert_eq!(target.accepted_connections(), 0);
}
