//! MCP streamable HTTP + OAuth 2.1(S5-5): 소유 hyper 인가 서버·MCP 서버(127.0.0.1:0). 시험이 브라우저 역할로
//! `Authorize{url}`을 따라가 콜백에 code를 넘긴다. PKCE S256·state·resource·loopback redirect·토큰 메모리 보관.
mod support;
use devbox_api::*;
use serde_json::Value;
use std::time::Duration;
use support::api::*;
use support::lines::{Lines, open};
use support::mcp::*;

const T: Duration = Duration::from_secs(10);

fn http(url: &str) -> McpConnect {
    McpConnect {
        transport: McpTransport::Http {
            url: url.into(),
            headers: vec![],
            tls: TlsOptions::default(),
        },
        env: EnvSnapshot::default(),
        source: McpSource::Draft,
    }
}

async fn connect(h: &Harness, p: McpConnect) -> Result<Lines, Rpc> {
    open(|ctx| h.api.mcp_connect(ctx, p, &h.collections)).await
}

fn files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_owned()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn query(url: &str) -> Vec<(String, String)> {
    url::Url::parse(url)
        .expect("url")
        .query_pairs()
        .into_owned()
        .collect()
}
fn get<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

async fn list_tools(lines: &mut Lines) -> Value {
    lines
        .send(serde_json::json!({"type": "list", "id": "t", "what": "tools", "cursor": null}))
        .await;
    let (result, before) = lines.until("result", T).await;
    result.unwrap_or_else(|| panic!("tools result, got {before:?}"))
}

#[tokio::test]
async fn mcp_http_oauth_registers_and_uses_pkce() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        oauth: true,
        ..Options::default()
    })
    .await;
    let before = files(h.dir.path());
    let mut lines = connect(&h, http(&server.url())).await.expect("connect");
    let (authorize, _) = lines.until("authorize", T).await;
    let url = authorize.expect("authorize event")["url"]
        .as_str()
        .expect("url")
        .to_owned();
    let q = query(&url);
    assert!(url.starts_with(&format!("http://127.0.0.1:{}/authorize?", server.port)));
    assert_eq!(get(&q, "response_type"), "code");
    assert_eq!(
        get(&q, "client_id"),
        "client-1",
        "dynamically registered client"
    );
    assert_eq!(get(&q, "code_challenge_method"), "S256");
    assert_eq!(get(&q, "code_challenge").len(), 43);
    assert_eq!(get(&q, "resource"), server.url());
    assert_eq!(get(&q, "scope"), "mcp:tools");
    let redirect = get(&q, "redirect_uri").to_owned();
    assert!(
        redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"),
        "{redirect}"
    );
    let registered = server
        .seen
        .lock()
        .expect("seen")
        .registered
        .clone()
        .expect("registration");
    assert!(
        registered.contains(&redirect) && registered.contains("\"none\""),
        "{registered}"
    );
    // 브라우저 역할: authorize → 302 Location(콜백). 먼저 state가 틀린 콜백은 거절되고 흐름은 계속 기다린다.
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("client");
    let location = browser
        .get(&url)
        .send()
        .await
        .expect("authorize")
        .headers()
        .get("location")
        .expect("location")
        .to_str()
        .expect("text")
        .to_owned();
    let wrong = format!("{redirect}?code=code-1&state=forged");
    assert_eq!(
        browser.get(&wrong).send().await.expect("callback").status(),
        400
    );
    assert_eq!(
        browser
            .get(&location)
            .send()
            .await
            .expect("callback")
            .status(),
        200
    );
    let (ready, before_ready) = lines.until("ready", T).await;
    let ready = ready.unwrap_or_else(|| panic!("ready, got {before_ready:?}"));
    assert_eq!(ready["serverName"], "http-fixture");
    let tools = list_tools(&mut lines).await;
    assert!(tools["json"].as_str().expect("json").contains("remote"));
    // 서버가 받은 Bearer 토큰을 되돌려 보내도 이벤트에서는 가려진다(토큰은 이 연결의 가림 목록에 든다).
    lines
        .send(
            serde_json::json!({"type": "call", "id": "w", "name": "whoami", "argumentsJson": "{}"}),
        )
        .await;
    let (echo, earlier) = lines.until("result", T).await;
    let echo = echo.unwrap_or_else(|| panic!("call result, got {earlier:?}"));
    let echo = echo["json"].as_str().expect("json");
    assert!(
        echo.contains(REDACTED) && !echo.contains(&server.token),
        "{echo}"
    );
    let form = server.seen.lock().expect("seen").token_form.clone();
    assert_eq!(
        get(&form, "resource"),
        server.url(),
        "token request names the resource"
    );
    assert_eq!(get(&form, "redirect_uri"), redirect);
    // 이후 요청은 Bearer·세션 id·프로토콜 버전을 보낸다.
    let requests = server.seen.lock().expect("seen").requests.clone();
    let list = requests
        .iter()
        .rev()
        .find(|r| r.0 == "POST" && r.5.contains("tools/list"))
        .expect("tools/list request");
    assert_eq!(
        list.2.as_deref(),
        Some(format!("Bearer {}", server.token).as_str())
    );
    assert_eq!(list.3.as_deref(), Some("sess-1"));
    assert_eq!(list.4.as_deref(), Some("2025-06-18"));
    lines.close();
    let rest = lines.all(T).await;
    let text = serde_json::to_string(&rest).expect("events");
    assert!(!text.contains(&server.token));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        server
            .seen
            .lock()
            .expect("seen")
            .requests
            .iter()
            .any(|r| r.0 == "DELETE"),
        "session closed with DELETE"
    );
    assert_eq!(
        files(h.dir.path()),
        before,
        "no token or client file was written"
    );
}

#[tokio::test]
async fn mcp_http_refuses_servers_without_s256_before_authorizing() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        oauth: true,
        s256: false,
        ..Options::default()
    })
    .await;
    let mut lines = connect(&h, http(&server.url())).await.expect("connect");
    let events = lines.all(T).await;
    assert!(
        events.iter().all(|e| e["type"] != "authorize"),
        "{events:?}"
    );
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(failed["error"]["code"], "api.oauth_failed");
    assert_eq!(failed["error"]["detail"]["stage"], "pkce_unsupported");
    assert!(
        server.seen.lock().expect("seen").registered.is_none(),
        "no registration"
    );
}

#[tokio::test]
async fn mcp_http_rejects_oversized_tokens() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        oauth: true,
        token_bytes: 20 * 1024,
        ..Options::default()
    })
    .await;
    let mut lines = connect(&h, http(&server.url())).await.expect("connect");
    let (authorize, _) = lines.until("authorize", T).await;
    let url = authorize.expect("authorize")["url"]
        .as_str()
        .expect("url")
        .to_owned();
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("client");
    let location = browser.get(&url).send().await.expect("authorize").headers()["location"]
        .to_str()
        .expect("text")
        .to_owned();
    browser.get(&location).send().await.expect("callback");
    let events = lines.all(T).await;
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(failed["error"]["detail"]["stage"], "token_too_large");
    assert!(events.iter().all(|e| e["type"] != "ready"));
}

#[tokio::test]
async fn mcp_http_reads_sse_answers_and_masks_header_secrets() {
    let h = Harness::new().await;
    h.secret("K", "fixture-mcp-header-secret").await;
    let server = McpHttp::start(Options {
        sse: true,
        ..Options::default()
    })
    .await;
    let mut p = http(&server.url());
    if let McpTransport::Http { headers, .. } = &mut p.transport {
        headers.push(header(0, "X-Key", "{{secret:K}}"));
    }
    let mut lines = connect(&h, p).await.expect("connect");
    lines.until("ready", T).await.0.expect("ready");
    lines
        .send(serde_json::json!({"type": "call", "id": "c", "name": "x", "argumentsJson": "{\"v\":\"fixture-mcp-header-secret\"}"}))
        .await;
    let (result, _) = lines.until("result", T).await;
    let json = result.expect("result")["json"]
        .as_str()
        .expect("json")
        .to_owned();
    assert!(
        json.contains(REDACTED) && !json.contains("fixture-mcp-header-secret"),
        "{json}"
    );
}

#[tokio::test]
async fn mcp_http_invalid_headers_are_rejected_before_connecting() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options::default()).await;
    for (row, expect) in [
        (header(0, "X Bad", "1"), "invalid_token"),
        (header(1, "Mcp-Session-Id", "forged"), "reserved"),
        (header(2, "X-A", "a\nb"), "control_character"),
    ] {
        let mut p = http(&server.url());
        if let McpTransport::Http { headers, .. } = &mut p.transport {
            headers.push(row);
        }
        let err = connect(&h, p).await.err().expect("rejected");
        assert_eq!(err.code(), "api.invalid_header");
        assert_eq!(err.detail()["reason"], expect);
    }
    assert!(server.seen.lock().expect("seen").requests.is_empty());
}
