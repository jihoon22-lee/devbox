//! SSE 세션: 연결(응답 머리까지 시한 30초) → 상한 있는 파서 → 큐. 끊기면 `retry`(없으면 3초, 0.1–60초)
//! 뒤 `Last-Event-ID`를 보내며 다시 연결한다. 이벤트 없이 끝난 연결이 5번 이어지거나, 이벤트가 있어도
//! 1분 안에 30번 다시 연결하면 멈춘다(연결마다 이벤트 하나를 주고 끊는 서버가 끝없이 돌리지 못하게).
//! 화면이 스트림을 닫으면 응답 future를 버리고(연결 종료) 다시 연결하지 않는다.
use super::buffer::Buffer;
use super::sse_parse::SseParser;
use super::{CONNECT_TIMEOUT, RealtimeEvent};
use crate::redact::Redactor;
use crate::tls::{self, Resolver};
use crate::validate;
use crate::{ApiError, Auth, Body, RequestSnapshot, SseOpen};
use reqwest::header::{ACCEPT, CACHE_CONTROL, HeaderName, HeaderValue};
use std::time::Duration;

pub(crate) const MAX_RECONNECTS: u32 = 5;
/// 이벤트를 받았는지와 상관없는 전체 상한: `RECONNECT_WINDOW` 안에 이만큼.
const MAX_RECONNECTS_PER_WINDOW: usize = 30;
const RECONNECT_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_RETRY: Duration = Duration::from_secs(3);
const MIN_RETRY: Duration = Duration::from_millis(100);
const MAX_LAST_EVENT_ID: usize = 256;
const LAST_EVENT_ID: &str = "last-event-id";

pub(crate) struct Prepared {
    url: url::Url,
    headers: Vec<(HeaderName, HeaderValue)>,
    client: reqwest::Client,
    last_event_id: Option<String>,
}

pub(crate) fn snapshot(p: &SseOpen) -> RequestSnapshot {
    RequestSnapshot {
        method: "GET".into(),
        url: p.url.clone(),
        query: vec![],
        headers: p.headers.clone(),
        cookies: vec![],
        body: Body::None,
        auth: Auth::None,
        tls: p.tls.clone(),
        timeout_ms: CONNECT_TIMEOUT.as_millis() as i64,
        follow_redirects: false,
    }
}

/// 서버가 준 id도 다시 보낼 때 헤더 값이므로 같은 규칙(제어 문자 없음, 256바이트)으로 본다.
fn header_id(id: &str) -> Option<HeaderValue> {
    if id.len() > MAX_LAST_EVENT_ID || id.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return None;
    }
    HeaderValue::from_str(id).ok()
}

pub(crate) fn prepare(
    filled: &RequestSnapshot,
    last_event_id: Option<&str>,
    resolver: Option<&Resolver>,
) -> Result<Prepared, ApiError> {
    let url = validate::url(&filled.url)?;
    let rows = super::headers(&filled.headers, &[LAST_EVENT_ID])?;
    let mut headers = Vec::with_capacity(rows.len());
    for (name, value) in rows {
        headers.push((
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::input("header", "invalid_token"))?,
            HeaderValue::from_str(&value)
                .map_err(|_| ApiError::input("header", "control_character"))?,
        ));
    }
    if let Some(id) = last_event_id
        && header_id(id).is_none()
    {
        return Err(ApiError::input("lastEventId", "invalid"));
    }
    let tls = tls::prepare(&filled.tls)?;
    let client = tls::client(&tls, true, resolver)?;
    Ok(Prepared {
        url,
        headers,
        client,
        last_event_id: last_event_id.map(str::to_owned),
    })
}

pub(crate) struct Session {
    pub(crate) prepared: Prepared,
    pub(crate) tx: devbox_protocol::stream::StreamTx,
    pub(crate) redactor: Redactor,
    pub(crate) clock: crate::Clock,
    /// 세션이 끝나 Session이 버려질 때 자리를 돌려준다.
    pub(crate) _permit: tokio::sync::OwnedSemaphorePermit,
}

enum Ended {
    /// 화면이 닫음.
    Cancelled,
    /// 다시 연결하지 않고 이 이벤트로 끝낸다.
    Stop(RealtimeEvent),
    /// 연결이 끊김(받은 이벤트가 있었는지).
    Dropped { delivered: bool },
}

struct State {
    last_event_id: Option<String>,
    retry_ms: Option<u64>,
}

fn is_event_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// 연결 하나: 응답 머리를 확인하고 본문을 끝까지 읽는다.
async fn connection(s: &Session, state: &mut State, buffer: &Buffer) -> Ended {
    let closed = s.tx.closed();
    let p = &s.prepared;
    let mut request = p
        .client
        .get(p.url.clone())
        .header(ACCEPT, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache");
    for (name, value) in &p.headers {
        request = request.header(name.clone(), value.clone());
    }
    if let Some(id) = state.last_event_id.as_deref().and_then(header_id) {
        request = request.header(LAST_EVENT_ID, id);
    }
    let response = tokio::select! {
        biased;
        _ = closed.cancelled() => return Ended::Cancelled,
        r = tokio::time::timeout(CONNECT_TIMEOUT, request.send()) => r,
    };
    let mut response = match response {
        Err(_) | Ok(Err(_)) => return Ended::Dropped { delivered: false },
        Ok(Ok(response)) => response,
    };
    let status = response.status().as_u16();
    if status == 204 {
        return Ended::Stop(RealtimeEvent::Closed {
            code: None,
            reason: None,
        });
    }
    if status != 200 || !is_event_stream(&response) {
        let reason = if status != 200 {
            "status"
        } else {
            "content_type"
        };
        return Ended::Stop(super::failed(&ApiError::StreamRejected {
            status,
            reason: reason.into(),
        }));
    }
    buffer.control(RealtimeEvent::Open { protocol: None });
    let mut parser = SseParser::resumed(state.last_event_id.as_deref(), state.retry_ms);
    let mut delivered = false;
    loop {
        let chunk = tokio::select! {
            biased;
            _ = closed.cancelled() => return Ended::Cancelled,
            chunk = response.chunk() => chunk,
        };
        let parsed = match &chunk {
            Ok(Some(bytes)) => parser.feed(bytes),
            Ok(None) => parser.finish(),
            Err(_) => Ok(vec![]),
        };
        let events = match parsed {
            Ok(events) => events,
            Err(error) => return Ended::Stop(super::failed(&ApiError::network(error.kind()))),
        };
        for event in events {
            delivered = true;
            let size = event.data.len();
            buffer.message(
                RealtimeEvent::SseEvent {
                    id: event.id.as_deref().map(|id| s.redactor.text(id)),
                    event: Some(s.redactor.text(&event.event)),
                    data: s.redactor.text(&event.data),
                    retry_ms: event.retry_ms.map(|ms| ms as i64),
                    at_ms: (s.clock)(),
                },
                size,
            );
        }
        // 화면에는 가린 값, 다시 연결할 때는 서버가 준 값 그대로.
        state.last_event_id = parser.last_event_id().map(str::to_owned);
        state.retry_ms = parser.retry_ms();
        if !matches!(chunk, Ok(Some(_))) {
            return Ended::Dropped { delivered };
        }
    }
}

async fn session(s: &Session, buffer: &Buffer) -> &'static str {
    let closed = s.tx.closed();
    let mut state = State {
        last_event_id: s.prepared.last_event_id.clone(),
        retry_ms: None,
    };
    let mut attempt = 0u32;
    let mut recent = std::collections::VecDeque::with_capacity(MAX_RECONNECTS_PER_WINDOW);
    loop {
        match connection(s, &mut state, buffer).await {
            Ended::Cancelled => return "cancelled",
            Ended::Stop(event) => {
                let failed = matches!(event, RealtimeEvent::Failed { .. });
                buffer.control(event);
                return if failed { "failed" } else { "closed" };
            }
            Ended::Dropped { delivered } => {
                if delivered {
                    attempt = 0;
                }
                attempt += 1;
                let now = tokio::time::Instant::now();
                while recent.front().is_some_and(|at: &tokio::time::Instant| {
                    now.duration_since(*at) >= RECONNECT_WINDOW
                }) {
                    recent.pop_front();
                }
                if attempt > MAX_RECONNECTS || recent.len() >= MAX_RECONNECTS_PER_WINDOW {
                    buffer.control(super::failed(&ApiError::network("reconnect_limit")));
                    return "failed";
                }
                recent.push_back(now);
                buffer.control(RealtimeEvent::Reconnecting {
                    attempt,
                    last_event_id: state.last_event_id.as_deref().map(|id| s.redactor.text(id)),
                });
                let wait = state
                    .retry_ms
                    .map(Duration::from_millis)
                    .unwrap_or(DEFAULT_RETRY)
                    .clamp(
                        MIN_RETRY,
                        Duration::from_millis(super::sse_parse::MAX_RETRY_MS),
                    );
                tokio::select! {
                    biased;
                    _ = closed.cancelled() => return "cancelled",
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
    }
}

/// 세션과 화면 쓰기를 함께 돌린다. 반환 뒤에는 연결·쓰기가 끝나 있고 세션 자리(permit)도 돌아간다.
pub(crate) async fn run(s: Session) {
    let started = std::time::Instant::now();
    let buffer = Buffer::default();
    let closed = s.tx.closed();
    let work = async {
        let outcome = session(&s, &buffer).await;
        buffer.end();
        outcome
    };
    let (outcome, written) = tokio::join!(work, super::drain(&buffer, &s.tx));
    if written.is_ok() && !closed.is_cancelled() {
        s.tx.finish().await;
    }
    tracing::debug!(
        method = "api.sse_open",
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn last_event_id_must_be_a_safe_header_value() {
        assert!(super::header_id("7").is_some());
        assert!(super::header_id("a\nb").is_none());
        assert!(super::header_id("a\rb").is_none());
        assert!(super::header_id("a\u{1}b").is_none());
        assert!(super::header_id(&"i".repeat(257)).is_none());
    }
}
