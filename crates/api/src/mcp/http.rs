//! streamable HTTP 전송: 메시지마다 POST(`Accept: application/json, text/event-stream`), 답은 JSON 본문 또는
//! SSE로 온다(본문 읽기는 연결마다 작업으로, 세션이 끝나면 모두 멈춘다). `Mcp-Session-Id`를 이어 보내고
//! 끝날 때 DELETE로 세션을 닫는다. 401이면(사용자 Authorization 헤더가 없을 때만) OAuth 2.1을 한 번 한다.
use super::{McpEvent, oauth, session};
use crate::realtime::sse_parse::{ParseError, SseParser};
use crate::tls::{self, Resolver};
use crate::{ApiError, RequestSnapshot, validate};
use devbox_protocol::stream::StreamTx;
use reqwest::header::{HeaderName, HeaderValue};
use std::time::Duration;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

const MAX_SESSION_ID: usize = 1024;
/// 읽은 메시지 대기열 길이(검토 2 N2). 메시지 하나가 4MiB까지라 대기열 자체는 64MiB까지다. 세션 전체 상한은
/// 아니다: POST마다 응답 읽기 작업이 하나씩 있고(답을 기다리는 요청은 `MAX_PENDING` 64개까지), 작업마다
/// 끝나지 않은 메시지를 약 8MiB(JSON 본문 4MiB, SSE는 줄 4MiB + data 4MiB)까지 쥘 수 있다(검토 3 R2).
const READ_QUEUE: usize = 16;
const CLOSE_WAIT: Duration = Duration::from_secs(1);

pub(crate) struct Plan {
    url: url::Url,
    headers: Vec<(HeaderName, HeaderValue)>,
    client: reqwest::Client,
    /// OAuth 메타데이터·등록·토큰 요청용: 기본 검증·클라이언트 인증서 없음(MCP 연결의 TLS 설정을 쓰지 않음).
    oauth_client: reqwest::Client,
    /// 사용자가 Authorization을 직접 넣었으면 OAuth를 하지 않는다.
    user_auth: bool,
}

impl Plan {
    pub(crate) fn seed(&self, redaction: &mut crate::redact::Redaction) -> Result<(), ApiError> {
        redaction.request_headers(
            self.headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_bytes())),
        )
    }
}

pub(crate) fn prepare(
    filled: &RequestSnapshot,
    resolver: Option<&Resolver>,
) -> Result<Plan, ApiError> {
    let url = validate::url(&filled.url)?;
    let rows = crate::realtime::headers(
        &filled.headers,
        &[
            "mcp-session-id",
            "mcp-protocol-version",
            "accept",
            "content-type",
        ],
    )?;
    let mut headers = vec![];
    for (name, value) in rows {
        headers.push((
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ApiError::input("header", "invalid_token"))?,
            HeaderValue::from_str(&value)
                .map_err(|_| ApiError::input("header", "control_character"))?,
        ));
    }
    let user_auth = headers
        .iter()
        .any(|(n, _)| n == reqwest::header::AUTHORIZATION);
    let prepared = tls::prepare(&filled.tls)?;
    let client = tls::client(&prepared, true, resolver)?;
    let oauth_client = tls::client(
        &tls::prepare(&crate::TlsOptions::default())?,
        false,
        resolver,
    )?;
    Ok(Plan {
        url,
        headers,
        client,
        oauth_client,
        user_auth,
    })
}

pub(crate) struct Http {
    plan: Plan,
    session_id: Option<String>,
    protocol: Option<String>,
    token: Option<Zeroizing<String>>,
    tried_oauth: bool,
    tx: mpsc::Sender<Result<Vec<u8>, ApiError>>,
    rx: mpsc::Receiver<Result<Vec<u8>, ApiError>>,
    readers: tokio::task::JoinSet<()>,
}

/// 본문 읽기 실패·상한 초과는 세션 실패로 넘긴다.
async fn read_json(response: reqwest::Response, out: mpsc::Sender<Result<Vec<u8>, ApiError>>) {
    let mut response = response;
    let mut body = vec![];
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > super::MAX_MESSAGE_BYTES {
                    let _ = out
                        .send(Err(validate::too_large(super::MAX_MESSAGE_BYTES)))
                        .await;
                    return;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => {
                let _ = out.send(Err(ApiError::network("protocol"))).await;
                return;
            }
        }
    }
    match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(serde_json::Value::Array(items)) => {
            for item in items {
                let _ = out.send(Ok(item.to_string().into_bytes())).await;
            }
        }
        Ok(_) => {
            let _ = out.send(Ok(body)).await;
        }
        Err(_) => {
            let _ = out.send(Err(ApiError::mcp("protocol"))).await;
        }
    }
}

/// 크기 상한은 `api.too_large{limit}`, 나머지 형식 오류는 `api.mcp{protocol}`.
fn sse_error(error: ParseError) -> ApiError {
    match error {
        ParseError::LineTooLong | ParseError::FieldTooLong | ParseError::DataTooLong => {
            validate::too_large(super::MAX_MESSAGE_BYTES)
        }
        _ => ApiError::mcp("protocol"),
    }
}

async fn read_sse(response: reqwest::Response, out: mpsc::Sender<Result<Vec<u8>, ApiError>>) {
    let mut response = response;
    // MCP 메시지는 한 SSE 이벤트에 4MiB까지 온다(실시간 SSE 도구의 64KiB 줄 상한과 별개, 검토 1 B2).
    let mut parser = SseParser::with_limit(super::MAX_MESSAGE_BYTES);
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => {
                let _ = out.send(Err(ApiError::network("protocol"))).await;
                return;
            }
        };
        let events = match parser.feed(&chunk) {
            Ok(events) => events,
            Err(error) => {
                let _ = out.send(Err(sse_error(error))).await;
                return;
            }
        };
        for event in events {
            if !event.data.is_empty() && out.send(Ok(event.data.into_bytes())).await.is_err() {
                return;
            }
        }
    }
    if let Ok(events) = parser.finish() {
        for event in events.into_iter().filter(|e| !e.data.is_empty()) {
            let _ = out.send(Ok(event.data.into_bytes())).await;
        }
    }
}

impl Http {
    pub(crate) fn new(plan: Plan) -> Self {
        let (tx, rx) = mpsc::channel(READ_QUEUE);
        Self {
            plan,
            session_id: None,
            protocol: None,
            token: None,
            tried_oauth: false,
            tx,
            rx,
            readers: tokio::task::JoinSet::new(),
        }
    }

    pub(crate) fn set_protocol(&mut self, version: &str) {
        if version.len() <= 64 && validate::field_value(version).is_ok() {
            self.protocol = Some(version.to_owned());
        }
    }

    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        let mut request = self.plan.client.request(method, self.plan.url.clone());
        for (name, value) in &self.plan.headers {
            request = request.header(name, value);
        }
        if let Some(token) = &self.token {
            request = request.bearer_auth(token.as_str());
        }
        if let Some(id) = &self.session_id {
            request = request.header("mcp-session-id", id);
        }
        if let Some(version) = &self.protocol {
            request = request.header("mcp-protocol-version", version);
        }
        request
    }

    async fn post(&self, message: &[u8]) -> Result<reqwest::Response, ApiError> {
        let send = self
            .request(reqwest::Method::POST)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .body(message.to_vec())
            .send();
        tokio::time::timeout(super::REQUEST_TIMEOUT, send)
            .await
            .map_err(|_| ApiError::network("timeout"))?
            .map_err(|e| crate::http::kind(&e))
    }

    pub(crate) async fn send(
        &mut self,
        message: &[u8],
        tx: &StreamTx,
        redaction: &mut crate::redact::Redaction,
    ) -> Result<(), ApiError> {
        let mut response = self.post(message).await?;
        redaction.response_headers(
            response
                .headers()
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_bytes())),
        )?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            && !self.plan.user_auth
            && !self.tried_oauth
        {
            self.tried_oauth = true;
            let challenge = response
                .headers()
                .get(reqwest::header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let token = oauth::authorize(
                &self.plan.oauth_client,
                &self.plan.url,
                challenge.as_deref(),
                |url| async move {
                    session::write(tx, &McpEvent::Authorize { url })
                        .await
                        .map_err(|_| ApiError::network("cancelled"))
                },
            )
            .await?;
            redaction.add(token.as_str())?;
            self.token = Some(token);
            response = self.post(message).await?;
            redaction.response_headers(
                response
                    .headers()
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_bytes())),
            )?;
        }
        let status = response.status();
        if status == reqwest::StatusCode::ACCEPTED {
            return Ok(());
        }
        if !status.is_success() {
            return Err(ApiError::StreamRejected {
                status: status.as_u16(),
                reason: if matches!(status.as_u16(), 401 | 403) {
                    "unauthorized"
                } else {
                    "status"
                }
                .into(),
            });
        }
        if self.session_id.is_none()
            && let Some(id) = response.headers().get("mcp-session-id")
        {
            let id = id.to_str().map_err(|_| ApiError::mcp("session_id"))?;
            if id.is_empty()
                || id.len() > MAX_SESSION_ID
                || !id.bytes().all(|b| (0x21..=0x7e).contains(&b))
            {
                return Err(ApiError::mcp("session_id"));
            }
            self.session_id = Some(id.to_owned());
        }
        let kind = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let out = self.tx.clone();
        if kind.starts_with("text/event-stream") {
            self.readers.spawn(read_sse(response, out));
        } else if kind.starts_with("application/json") {
            self.readers.spawn(read_json(response, out));
        }
        Ok(())
    }

    pub(crate) async fn recv(&mut self) -> Result<Option<Vec<u8>>, ApiError> {
        match self.rx.recv().await {
            Some(Ok(message)) => Ok(Some(message)),
            Some(Err(error)) => Err(error),
            None => Ok(None),
        }
    }

    /// 읽기 작업을 멈추고 세션이 있으면 DELETE로 닫는다(실패해도 상관없음, 1초 상한).
    pub(crate) async fn close(mut self) {
        self.readers.abort_all();
        if self.session_id.is_some() {
            let _ = tokio::time::timeout(CLOSE_WAIT, self.request(reqwest::Method::DELETE).send())
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_read_queue_is_bounded_by_bytes() {
        assert!(super::READ_QUEUE * super::super::MAX_MESSAGE_BYTES <= 64 * 1024 * 1024);
    }
}
