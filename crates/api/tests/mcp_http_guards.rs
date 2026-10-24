//! MCP HTTP 방어(S5-5 검토 1): SSE 답의 MCP 상한(4MiB, 실시간 SSE의 64KiB 줄 상한과 별개), 인가 서버
//! `issuer`(RFC 8414 §3.3)·보호 자원 `resource`(RFC 9728 §3.3) 일치, OAuth 요청은 MCP 연결의 TLS 설정(검증 끔·
//! 클라이언트 인증서)을 쓰지 않음. 모두 소유 127.0.0.1:0 fixture.
mod support;
use devbox_api::*;
use serde_json::json;
use std::time::Duration;
use support::api::*;
use support::lines::{Lines, open};
use support::mcp::*;

const T: Duration = Duration::from_secs(20);

fn http(url: &str, tls: TlsOptions) -> McpConnect {
    McpConnect {
        transport: McpTransport::Http {
            url: url.into(),
            headers: vec![],
            tls,
        },
        env: EnvSnapshot::default(),
        source: McpSource::Draft,
    }
}

async fn connect(h: &Harness, p: McpConnect) -> Result<Lines, Rpc> {
    open(|ctx| h.api.mcp_connect(ctx, p, &h.collections)).await
}

fn insecure() -> TlsOptions {
    TlsOptions {
        verify: false,
        ..TlsOptions::default()
    }
}

#[tokio::test]
async fn mcp_http_sse_answers_use_the_mcp_size_limit() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        sse: true,
        ..Options::default()
    })
    .await;
    let mut lines = connect(&h, http(&server.url(), TlsOptions::default()))
        .await
        .expect("connect");
    lines.until("ready", T).await.0.expect("ready");
    // 100KiB: 실시간 SSE의 64KiB 줄 상한을 넘지만 MCP 메시지 상한(4MiB) 안이다.
    let bytes = 100 * 1024;
    lines
        .send(json!({"type": "call", "id": "a", "name": "big", "argumentsJson": json!({"bytes": bytes}).to_string()}))
        .await;
    let (result, before) = lines.until("result", T).await;
    let result = result.unwrap_or_else(|| panic!("100 KiB result, got {before:?}"));
    assert!(result["json"].as_str().expect("json").len() > bytes);
    // 4MiB를 조금 넘는 답: 세션이 깔끔히 실패한다(api.too_large, 결과 없음, 스트림 끝남).
    lines
        .send(json!({"type": "call", "id": "b", "name": "big", "argumentsJson": json!({"bytes": 4 * 1024 * 1024 + 1}).to_string()}))
        .await;
    let rest = lines.all(T).await;
    assert!(
        rest.iter().all(|e| e["type"] != "result"),
        "{:?}",
        rest.iter().map(|e| &e["type"]).collect::<Vec<_>>()
    );
    let failed = rest.iter().find(|e| e["type"] == "failed").expect("failed");
    assert_eq!(failed["error"]["code"], "api.too_large");
    assert_eq!(failed["error"]["detail"]["limit"], 4 * 1024 * 1024);
}

async fn oauth_failure(h: &Harness, server: &McpHttp, tls: TlsOptions) -> serde_json::Value {
    let mut lines = connect(h, http(&server.url(), tls)).await.expect("connect");
    let events = lines.all(T).await;
    assert!(
        events
            .iter()
            .all(|e| e["type"] != "authorize" && e["type"] != "ready"),
        "{events:?}"
    );
    assert!(
        server.seen.lock().expect("seen").registered.is_none(),
        "no registration"
    );
    events
        .into_iter()
        .find(|e| e["type"] == "failed")
        .expect("failed")["error"]
        .clone()
}

#[tokio::test]
async fn mcp_http_oauth_metadata_must_name_the_issuer_and_the_resource() {
    let h = Harness::new().await;
    let server = McpHttp::start(Options {
        oauth: true,
        bad_issuer: true,
        ..Options::default()
    })
    .await;
    let error = oauth_failure(&h, &server, TlsOptions::default()).await;
    assert_eq!(
        (error["code"].as_str(), error["detail"]["stage"].as_str()),
        (Some("api.oauth_failed"), Some("issuer_mismatch"))
    );
    let server = McpHttp::start(Options {
        oauth: true,
        bad_resource: true,
        ..Options::default()
    })
    .await;
    let error = oauth_failure(&h, &server, TlsOptions::default()).await;
    assert_eq!(
        (error["code"].as_str(), error["detail"]["stage"].as_str()),
        (Some("api.oauth_failed"), Some("resource_mismatch"))
    );
}

#[tokio::test]
async fn mcp_http_oauth_does_not_reuse_the_connection_tls_settings() {
    let h = Harness::new().await;
    // 검증을 끈 MCP 연결 자체는 된다(소유 CA를 신뢰하지 않아도).
    let plain = McpHttp::start(Options {
        tls: true,
        ..Options::default()
    })
    .await;
    let mut lines = connect(&h, http(&plain.url(), insecure()))
        .await
        .expect("connect");
    lines
        .until("ready", T)
        .await
        .0
        .expect("verify off applies to the MCP connection");
    lines.close();
    // 그러나 인가 서버 메타데이터·토큰 요청은 기본 검증으로 한다: 신뢰하지 않는 인증서면 인증을 시작하지 않는다.
    let server = McpHttp::start(Options {
        oauth: true,
        tls: true,
        ..Options::default()
    })
    .await;
    let error = oauth_failure(&h, &server, insecure()).await;
    assert_eq!(
        (error["code"].as_str(), error["detail"]["stage"].as_str()),
        (Some("api.oauth_failed"), Some("metadata"))
    );
}
