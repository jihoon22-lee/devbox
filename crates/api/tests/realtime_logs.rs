//! WebSocket·SSE 세션 로그에 URL·헤더 값·비밀·메시지가 남지 않는다(01 §4.5).
//! `http_logs.rs`와 같은 이유로 전역 subscriber를 한 번만 설치하는 단독 바이너리다.
mod support;
use devbox_api::*;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use support::api::*;
use support::realtime::*;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter("info,devbox_api=trace")
            .with_writer(move || writer.clone())
            .without_time()
            .with_ansi(false)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
        capture
    })
}

#[tokio::test]
async fn realtime_logs_contain_no_url_header_or_message_values() {
    let capture = capture();
    let h = Harness::new().await;
    h.secret("RT", "realtime-secret").await;
    let t = Duration::from_secs(10);
    let ws_server = WsServer::start(echo()).await;
    let mut io = open_ws(
        &h.api,
        WsOpen {
            url: ws_server.url("/ws-path-marker?q=ws-query-marker"),
            headers: vec![header(0, "Authorization", "Bearer {{secret:RT}}")],
            subprotocols: vec![],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            origin: None,
        },
    )
    .await
    .expect("ws open");
    assert!(matches!(
        io.next_event(t).await,
        Some(RealtimeEvent::Open { .. })
    ));
    io.send(WsInput::Text {
        data: "ws-message-marker".into(),
    })
    .await;
    io.send(WsInput::Close {
        code: Some(1000),
        reason: None,
    })
    .await;
    io.events(t).await;
    // 연결 실패 경로(닫힌 포트)도 URL 없이 남는다.
    let port = support::closed_port();
    let mut io = open_ws(
        &h.api,
        WsOpen {
            url: format!("ws://127.0.0.1:{port}/closed-path-marker"),
            headers: vec![],
            subprotocols: vec![],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            origin: None,
        },
    )
    .await
    .expect("ws open");
    io.events(t).await;
    let sse_server = SseServer::start(vec![vec![SseStep::Write(
        "data: sse-data-marker\n\n".into(),
    )]])
    .await;
    let mut io = open_sse(
        &h.api,
        SseOpen {
            url: sse_server.url("/sse-path-marker"),
            headers: vec![header(0, "X-Key", "{{secret:RT}}")],
            tls: TlsOptions::default(),
            env: EnvSnapshot::default(),
            last_event_id: None,
            origin: None,
        },
    )
    .await
    .expect("sse open");
    assert!(matches!(
        io.next_event(t).await,
        Some(RealtimeEvent::Open { .. })
    ));
    io.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let logs = String::from_utf8(capture.0.lock().expect("capture").clone()).expect("utf8");
    assert!(logs.contains("api.ws_open"), "{logs}");
    for leak in [
        "realtime-secret",
        "ws-path-marker",
        "ws-query-marker",
        "ws-message-marker",
        "closed-path-marker",
        "sse-path-marker",
        "sse-data-marker",
        &port.to_string(),
        &ws_server.addr.port().to_string(),
    ] {
        let lines: Vec<_> = logs.lines().filter(|l| l.contains(leak)).collect();
        assert!(lines.is_empty(), "log leaked {leak}: {lines:?}");
    }
}
