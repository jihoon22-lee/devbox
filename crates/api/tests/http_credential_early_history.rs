//! History must mask recognized input even when the outgoing request never starts.
mod support;
use devbox_api::*;
use std::time::Duration;
use support::api::*;
use support::{Reply, Server, handler};

const CREDENTIAL: &str = "owned-early-credential-8472";

fn input(url: &str, source: usize) -> RequestSnapshot {
    let mut input = snapshot("POST", &format!("{url}?trace={CREDENTIAL}"));
    input.body = Body::Raw {
        text: format!("ordinary-prefix|{CREDENTIAL}|ordinary-suffix"),
        encoding: None,
        content_type: Some("text/plain".into()),
    };
    match source {
        0 => input
            .headers
            .push(header(0, "Authorization", &format!("Bearer {CREDENTIAL}"))),
        1 => input.headers.push(header(0, "X-API-Key", CREDENTIAL)),
        2 => input
            .headers
            .push(header(0, "Cookie", &format!("session={CREDENTIAL}"))),
        3 => input.cookies.push(header(0, "session", CREDENTIAL)),
        _ => unreachable!("owned fixture source"),
    }
    input
}

async fn assert_history(api: &Harness, outcome: HistoryOutcome) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while api.history_rows().await == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("history recorded after early termination");
    let entry = api.only_history().await;
    assert_eq!(entry.outcome, outcome);
    assert!(entry.response.is_none(), "target produced no response");
    assert!(!entry.url.contains(CREDENTIAL));
    assert!(!entry.snapshot.url.contains(CREDENTIAL));
    let Body::Raw { text, .. } = entry.snapshot.body else {
        panic!("raw snapshot preserved")
    };
    assert_eq!(text, "ordinary-prefix|[REDACTED]|ordinary-suffix");
    assert!(entry.url.contains("[REDACTED]"));
    assert!(!contains(&api.history_bytes(), CREDENTIAL));
}

#[tokio::test]
async fn cancellation_before_dispatch_masks_credentials_in_history_snapshot_and_url() {
    for source in 0..4 {
        let api = Harness::new().await;
        let target = Server::ok().await;
        let sent = api
            .send(request(input(&target.url("/never"), source)))
            .await
            .expect("open request");
        // The current-thread task cannot begin between send returning and close.
        sent.close();
        assert_history(&api, HistoryOutcome::Cancelled).await;
        assert_eq!(target.accepted_connections(), 0);
    }
}

#[tokio::test]
async fn oauth_failure_before_dispatch_masks_credentials_in_history_snapshot_and_url() {
    for source in 0..4 {
        let api = Harness::new().await;
        let target = Server::ok().await;
        let tokens = Server::start(handler(|_| async {
            Reply::status(400, r#"{"error":"invalid_client"}"#)
                .header("content-type", "application/json")
        }))
        .await;
        let mut input = input(&target.url("/never"), source);
        input.auth = Auth::Oauth2ClientCredentials {
            token_url: tokens.url("/token"),
            client_id: "owned-client".into(),
            client_secret: "owned-client-password".into(),
            scope: None,
        };
        let events = api.complete(request(input)).await;
        assert_eq!(failure(&events).code, "api.oauth_failed");
        assert_history(&api, HistoryOutcome::Failed).await;
        assert_eq!(tokens.requests().len(), 1);
        assert_eq!(target.accepted_connections(), 0);
    }
}
