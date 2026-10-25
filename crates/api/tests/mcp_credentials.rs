//! MCP actual HTTP headers share credential policy; ordinary MCP JSON is not inferred secret.
mod support;
use devbox_api::*;
use serde_json::json;
use std::time::Duration;
use support::api::*;
use support::lines::open;
use support::mcp::*;
const T: Duration = Duration::from_secs(5);

fn request(url: &str, env: EnvSnapshot, auth: bool) -> McpConnect {
    McpConnect {
        transport: McpTransport::Http {
            url: url.into(),
            headers: if auth {
                vec![header(0, "Authorization", "Bearer mcp-direct-value")]
            } else {
                vec![]
            },
            tls: TlsOptions::default(),
        },
        env,
        source: McpSource::Draft,
    }
}
fn full_budget(count: usize) -> EnvSnapshot {
    EnvSnapshot {
        variables: (0..count)
            .map(|i| EnvVar {
                name: format!("V{i}"),
                value: format!("fixture-seed-{i:04}"),
                sensitive: true,
                origin: VarOrigin::Environment,
            })
            .collect(),
    }
}

#[tokio::test]
async fn mcp_json_headers_seed_initial_and_later_responses_before_emission() {
    echo(false).await;
}
#[tokio::test]
async fn mcp_sse_headers_seed_initial_and_later_responses_before_emission() {
    echo(true).await;
}
async fn echo(sse: bool) {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        credential_echo: true,
        sse,
        ..Options::default()
    })
    .await;
    let p = request(&server.url(), EnvSnapshot::default(), true);
    let mut io = open(|ctx| h.api.mcp_connect(ctx, p, &h.collections))
        .await
        .expect("connect");
    let (ready, _) = io.until("ready", T).await;
    let ready = ready.expect("ready");
    assert_eq!(
        ready["serverName"],
        "[REDACTED] [REDACTED] ordinary-literal"
    );
    io.send(json!({"type":"list","id":"1","what":"tools","cursor":null}))
        .await;
    let (result, _) = io.until("result", T).await;
    let result = result.expect("result");
    let body: serde_json::Value =
        serde_json::from_str(result["json"].as_str().expect("json")).expect("body");
    assert_eq!(body["echo"], "[REDACTED] [REDACTED] ordinary-literal");
    io.close();
}

#[tokio::test]
async fn mcp_initial_redaction_capacity_failure_prevents_connecting() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options::default()).await;
    let p = request(&server.url(), full_budget(1025), false);
    let error = open(|ctx| h.api.mcp_connect(ctx, p, &h.collections))
        .await
        .err()
        .expect("capacity");
    assert_eq!(error.code(), "api.redaction_failed");
    assert!(server.seen.lock().expect("seen").requests.is_empty());
}

#[tokio::test]
async fn mcp_response_cookie_capacity_failure_emits_failed_without_ready_or_body() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        credential_echo: true,
        ..Options::default()
    })
    .await;
    let p = request(&server.url(), full_budget(1024), false);
    let mut io = open(|ctx| h.api.mcp_connect(ctx, p, &h.collections))
        .await
        .expect("connect");
    let events = io.all(T).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["type"], "failed");
    assert_eq!(events[0]["error"]["code"], "api.redaction_failed");
    assert!(
        !serde_json::to_string(&events)
            .expect("events")
            .contains("mcp-initial-issued")
    );
}

#[tokio::test]
async fn mcp_dynamic_oauth_token_capacity_failure_prevents_authenticated_retry_and_output() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        oauth: true,
        ..Options::default()
    })
    .await;
    let p = request(&server.url(), full_budget(1024), false);
    let mut io = open(|ctx| h.api.mcp_connect(ctx, p, &h.collections))
        .await
        .expect("connect");
    let (authorize, _) = io.until("authorize", T).await;
    let authorize = authorize.expect("owned authorization event");
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("browser");
    let response = browser
        .get(authorize["url"].as_str().expect("url"))
        .send()
        .await
        .expect("owned auth");
    let callback = response
        .headers()
        .get("location")
        .expect("redirect")
        .to_str()
        .expect("url");
    assert_eq!(
        browser
            .get(callback)
            .send()
            .await
            .expect("callback")
            .status(),
        200
    );
    let events = io.all(T).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["error"]["code"], "api.redaction_failed");
    assert!(
        !server
            .seen
            .lock()
            .expect("seen")
            .requests
            .iter()
            .any(|(_, path, auth, _, _, _)| path == "/mcp" && auth.is_some()),
        "token must be admitted before authenticated retry"
    );
}
