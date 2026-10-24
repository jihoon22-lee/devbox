//! 네트워크 오류 로그에 URL·비밀이 남지 않는다(v0.9.0 http-client-engine 이식).
//! tracing의 스레드 단위 `set_default`는 같은 바이너리의 다른 시험과 callsite interest 캐시를 다투므로,
//! 이 바이너리는 전역 subscriber를 한 번만 설치하고 로그를 읽는 시험만 둔다.
mod support;
use devbox_api::*;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use support::api::*;

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

/// 바이너리 전역 subscriber(한 번만 설치). 이후 모든 스레드·task의 로그가 이 버퍼로 온다.
/// 필터는 daemon 기본값(`info`, core::log)에 이 crate만 trace로 올린 것이다.
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
async fn live_network_error_is_generic_and_contains_no_request_secret() {
    let capture = capture();
    let options = ApiOptions::default().with_resolver(|host| host != "devbox-invalid.test");
    let api = Harness::with_options(options).await;
    api.secret("NET", "network-secret").await;
    let port = support::closed_port();
    for (url, kind) in [
        (
            format!("http://127.0.0.1:{port}/?token={{{{secret:NET}}}}"),
            "connect",
        ),
        (
            "http://devbox-invalid.test/?token={{secret:NET}}".to_string(),
            "dns",
        ),
    ] {
        let events = api.complete(request(snapshot("GET", &url))).await;
        assert_eq!(failure_kind(&events), format!("api.network:{kind}"));
        let text = serde_json::to_string(&events).expect("events");
        assert!(
            !text.contains("network-secret")
                && !text.contains(&port.to_string())
                && !text.contains("devbox-invalid")
        );
    }
    let logs = String::from_utf8(capture.0.lock().expect("capture").clone()).expect("utf8");
    assert!(logs.contains("api.send"), "{logs}");
    for leak in ["network-secret", "devbox-invalid", &port.to_string()] {
        let lines: Vec<_> = logs.lines().filter(|l| l.contains(leak)).collect();
        assert!(lines.is_empty(), "log leaked {leak}: {lines:?}");
    }
}
