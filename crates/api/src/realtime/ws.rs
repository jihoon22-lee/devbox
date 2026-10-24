//! WebSocket 세션: 연결 전 검사 → TCP·TLS·핸드셰이크(시한 30초) → 소켓 읽기·화면 입력·화면 쓰기.
//! 소켓 읽기는 화면 credit을 기다리지 않는다(큐 `Buffer`가 넘치면 버리고 센다).
use super::buffer::Buffer;
use super::{CONNECT_TIMEOUT, Direction, MessageKind, RealtimeEvent, WsInput};
use crate::redact::Redactor;
use crate::tls::{self, PreparedTls};
use crate::validate;
use crate::{ApiError, Auth, Body, RequestSnapshot, WsOpen};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::error::{Error as WsError, ProtocolError};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes, http};

/// 메시지 하나의 상한(보내기·받기 모두, tungstenite 기본 64MiB 대신).
pub(crate) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// 화면 입력 한 줄 상한(4MiB 이진의 Base64와 JSON 틀).
pub(crate) const MAX_INPUT_LINE: usize = 6 * 1024 * 1024;
const MAX_CLOSE_REASON_BYTES: usize = 123;
const MAX_SUBPROTOCOLS: usize = 16;
/// 닫기 요청 뒤 서버의 닫기 응답을 기다리는 시간.
const CLOSE_WAIT: Duration = Duration::from_secs(5);
/// 화면이 스트림을 닫았을 때 정중한 닫기 프레임을 보내 보는 시간.
const GOODBYE: Duration = Duration::from_secs(1);

pub(crate) struct Prepared {
    url: url::Url,
    headers: Vec<(String, String)>,
    subprotocols: Vec<String>,
    tls: PreparedTls,
}

/// 치환·비밀 해석에 넣을 모양(URL·헤더·TLS 경로만 활성 필드).
pub(crate) fn snapshot(p: &WsOpen) -> RequestSnapshot {
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

pub(crate) fn prepare(
    filled: &RequestSnapshot,
    subprotocols: &[String],
) -> Result<Prepared, ApiError> {
    let url = validate::url_with(&filled.url, &["ws", "wss"])?;
    let headers = super::headers(&filled.headers, &["sec-websocket-*"])?;
    if subprotocols.len() > MAX_SUBPROTOCOLS {
        return Err(ApiError::input("subprotocols", "too_many"));
    }
    if subprotocols
        .iter()
        .any(|p| !validate::is_token(p) || p.len() > 128)
    {
        return Err(ApiError::input("subprotocols", "invalid_token"));
    }
    let tls = tls::prepare(&filled.tls)?;
    Ok(Prepared {
        url,
        headers,
        subprotocols: subprotocols.to_vec(),
        tls,
    })
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// tungstenite 오류를 고정 종류로만 옮긴다(오류 문장에 URL·값이 있을 수 있어 쓰지 않는다).
fn mapped(error: &WsError) -> ApiError {
    match error {
        WsError::Io(e) if crate::http::is_tls(e) => ApiError::network("tls"),
        WsError::Io(e) if e.kind() == std::io::ErrorKind::TimedOut => ApiError::network("timeout"),
        WsError::Io(_) => ApiError::network("connect"),
        WsError::Tls(_) => ApiError::network("tls"),
        WsError::Capacity(_) => validate::too_large(MAX_MESSAGE_BYTES),
        WsError::Http(response) => ApiError::StreamRejected {
            status: response.status().as_u16(),
            reason: "status".into(),
        },
        WsError::Protocol(ProtocolError::SecWebSocketSubProtocolError(_)) => {
            ApiError::StreamRejected {
                status: 101,
                reason: "subprotocol".into(),
            }
        }
        WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake)
        | WsError::ConnectionClosed
        | WsError::AlreadyClosed => ApiError::network("closed"),
        WsError::Url(_) => ApiError::url("parse"),
        _ => ApiError::network("protocol"),
    }
}

async fn connect(p: &Prepared) -> Result<(Socket, Option<String>), ApiError> {
    let host = p.url.host_str().ok_or_else(|| ApiError::url("host"))?;
    let port = p
        .url
        .port_or_known_default()
        .ok_or_else(|| ApiError::url("port"))?;
    let tcp = tokio::net::TcpStream::connect(format!("{host}:{port}"))
        .await
        .map_err(|_| ApiError::network("connect"))?;
    let _ = tcp.set_nodelay(true);
    let mut request = p
        .url
        .as_str()
        .into_client_request()
        .map_err(|_| ApiError::url("parse"))?;
    for (name, value) in &p.headers {
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ApiError::input("header", "invalid_token"))?;
        let value = http::HeaderValue::from_str(value)
            .map_err(|_| ApiError::input("header", "control_character"))?;
        request.headers_mut().append(name, value);
    }
    if !p.subprotocols.is_empty() {
        let joined = http::HeaderValue::from_str(&p.subprotocols.join(", "))
            .map_err(|_| ApiError::input("subprotocols", "invalid_token"))?;
        request
            .headers_mut()
            .insert(http::header::SEC_WEBSOCKET_PROTOCOL, joined);
    }
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES));
    let connector = if p.url.scheme() == "wss" {
        tokio_tungstenite::Connector::Rustls(tls::rustls_config(&p.tls)?)
    } else {
        tokio_tungstenite::Connector::Plain
    };
    let (socket, response) = tokio_tungstenite::client_async_tls_with_config(
        request,
        tcp,
        Some(config),
        Some(connector),
    )
    .await
    .map_err(|e| mapped(&e))?;
    let protocol = response
        .headers()
        .get(http::header::SEC_WEBSOCKET_PROTOCOL)
        .map(|v| v.to_str().map(str::to_owned))
        .transpose()
        .map_err(|_| ApiError::StreamRejected {
            status: 101,
            reason: "subprotocol".into(),
        })?;
    if protocol
        .as_ref()
        .is_some_and(|chosen| !p.subprotocols.contains(chosen))
    {
        return Err(ApiError::StreamRejected {
            status: 101,
            reason: "subprotocol".into(),
        });
    }
    Ok((socket, protocol))
}

/// RFC 6455: 1004–1006·1015는 예약, 응용 코드는 3000–4999(v0.9.0 `validate_close_code`).
pub(crate) fn close_code(code: Option<u16>) -> Result<u16, ApiError> {
    let code = code.unwrap_or(1000);
    if matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999) {
        Ok(code)
    } else {
        Err(ApiError::input("input", "close_code"))
    }
}

pub(crate) fn close_reason(reason: &str) -> Result<(), ApiError> {
    if reason.len() > MAX_CLOSE_REASON_BYTES || reason.contains('\0') {
        return Err(ApiError::input("input", "close_reason"));
    }
    Ok(())
}

pub(crate) fn payload(bytes: usize) -> Result<(), ApiError> {
    if bytes > MAX_MESSAGE_BYTES {
        return Err(validate::too_large(MAX_MESSAGE_BYTES));
    }
    Ok(())
}

/// 화면 입력 한 줄 → 소켓 메시지.
fn message(line: &[u8]) -> Result<Message, ApiError> {
    let input: WsInput =
        serde_json::from_slice(line).map_err(|_| ApiError::input("input", "parse"))?;
    Ok(match input {
        WsInput::Text { data } => {
            payload(data.len())?;
            Message::text(data)
        }
        WsInput::Binary { data_base64 } => {
            let data = data_encoding::BASE64
                .decode(data_base64.as_bytes())
                .map_err(|_| ApiError::input("input", "base64"))?;
            payload(data.len())?;
            Message::binary(data)
        }
        WsInput::Close { code, reason } => {
            let code = close_code(code)?;
            let reason = reason.unwrap_or_default();
            close_reason(&reason)?;
            Message::Close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: Utf8Bytes::from(reason),
            }))
        }
    })
}

/// 조각으로 오는 입력을 줄로 모은다. 한 줄이 상한을 넘으면 오류(메모리 증가 없음).
#[derive(Default)]
pub(crate) struct Lines {
    pending: Vec<u8>,
}

impl Lines {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, ApiError> {
        let mut out = vec![];
        let mut rest = chunk;
        while let Some(at) = rest.iter().position(|b| *b == b'\n') {
            self.pending.extend_from_slice(&rest[..at]);
            if self.pending.len() > MAX_INPUT_LINE {
                return Err(ApiError::input("input", "too_long"));
            }
            let line = std::mem::take(&mut self.pending);
            if !line.iter().all(u8::is_ascii_whitespace) {
                out.push(line);
            }
            rest = &rest[at + 1..];
        }
        self.pending.extend_from_slice(rest);
        if self.pending.len() > MAX_INPUT_LINE {
            return Err(ApiError::input("input", "too_long"));
        }
        Ok(out)
    }
}

pub(crate) struct Session {
    pub(crate) prepared: Prepared,
    pub(crate) tx: devbox_protocol::stream::StreamTx,
    pub(crate) input: mpsc::Receiver<Bytes>,
    pub(crate) redactor: Redactor,
    pub(crate) clock: crate::Clock,
    /// 세션이 끝나 Session이 버려질 때 자리를 돌려준다.
    pub(crate) _permit: tokio::sync::OwnedSemaphorePermit,
}

fn shown(
    redactor: &Redactor,
    clock: &crate::Clock,
    direction: Direction,
    message: &Message,
) -> Option<(RealtimeEvent, usize)> {
    let (kind, data, data_base64, size) = match message {
        Message::Text(text) => (
            MessageKind::Text,
            Some(redactor.text(text)),
            None,
            text.len(),
        ),
        Message::Binary(bytes) => (
            MessageKind::Binary,
            None,
            Some(data_encoding::BASE64.encode(&redactor.bytes(bytes))),
            bytes.len(),
        ),
        _ => return None,
    };
    Some((
        RealtimeEvent::Message {
            direction,
            kind,
            data,
            data_base64,
            at_ms: clock(),
        },
        size,
    ))
}

enum End {
    /// 화면이 스트림을 닫음(FIN 없이 끝).
    Cancelled,
    Event(RealtimeEvent),
}

/// 열린 소켓을 끝날 때까지 돌린다. 반환 뒤에는 소켓·입력·쓰기 태스크가 모두 끝나 있다.
async fn pump(socket: Socket, s: &mut Session, buffer: &Buffer) -> End {
    let closed = s.tx.closed();
    let (mut sink, mut stream) = socket.split();
    let mut lines = Lines::default();
    let mut input_open = true;
    let mut close_by: Option<tokio::time::Instant> = None;
    let mut server_close: Option<RealtimeEvent> = None;
    let goodbye = |mut sink: futures::stream::SplitSink<Socket, Message>| async move {
        let _ = tokio::time::timeout(GOODBYE, sink.send(Message::Close(None))).await;
    };
    loop {
        let deadline = close_by.unwrap_or_else(|| tokio::time::Instant::now() + CLOSE_WAIT);
        tokio::select! {
            biased;
            _ = closed.cancelled() => {
                goodbye(sink).await;
                return End::Cancelled;
            }
            next = stream.next() => match next {
                Some(Ok(Message::Close(frame))) => {
                    server_close = Some(RealtimeEvent::Closed {
                        code: frame.as_ref().map(|f| u16::from(f.code)),
                        reason: frame.as_ref().map(|f| s.redactor.text(&f.reason)),
                    });
                }
                Some(Ok(message)) => {
                    if let Some((event, size)) = shown(&s.redactor, &s.clock, Direction::Received, &message) {
                        buffer.message(event, size);
                    }
                }
                Some(Err(error)) => {
                    let ended = matches!(
                        error,
                        WsError::ConnectionClosed
                            | WsError::AlreadyClosed
                            | WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake)
                    );
                    return End::Event(match server_close {
                        Some(event) => event,
                        // 우리가 닫기를 보낸 뒤 서버가 답 없이 끊음: 닫힘으로 본다.
                        None if ended && close_by.is_some() => RealtimeEvent::Closed { code: None, reason: None },
                        None => super::failed(&mapped(&error)),
                    });
                }
                None => {
                    return End::Event(server_close.unwrap_or(RealtimeEvent::Closed { code: None, reason: None }));
                }
            },
            chunk = s.input.recv(), if input_open => {
                let Some(chunk) = chunk else {
                    input_open = false;
                    continue;
                };
                let parsed = lines.push(&chunk).and_then(|lines| {
                    lines.iter().map(|line| message(line)).collect::<Result<Vec<_>, _>>()
                });
                let messages = match parsed {
                    Ok(messages) => messages,
                    Err(error) => {
                        goodbye(sink).await;
                        return End::Event(super::failed(&error));
                    }
                };
                for message in messages {
                    if close_by.is_some() || server_close.is_some() {
                        // 닫는 중에 온 입력은 보내지 않는다.
                        continue;
                    }
                    let closing = matches!(message, Message::Close(_));
                    let echo = shown(&s.redactor, &s.clock, Direction::Sent, &message);
                    let sent = tokio::select! {
                        biased;
                        _ = closed.cancelled() => return End::Cancelled,
                        sent = sink.send(message) => sent,
                    };
                    if let Err(error) = sent {
                        return End::Event(super::failed(&mapped(&error)));
                    }
                    if let Some((event, size)) = echo {
                        buffer.message(event, size);
                    }
                    if closing {
                        close_by = Some(tokio::time::Instant::now() + CLOSE_WAIT);
                    }
                }
            }
            _ = tokio::time::sleep_until(deadline), if close_by.is_some() => {
                // 서버가 닫기에 답하지 않았다.
                return End::Event(RealtimeEvent::Closed { code: None, reason: None });
            }
        }
    }
}

/// 연결부터 닫힘까지. 연결 실패도 스트림 이벤트(`Failed`)로 알린다(스트림은 이미 열렸다).
async fn session(s: &mut Session, buffer: &Buffer) -> &'static str {
    let closed = s.tx.closed();
    let connected = tokio::select! {
        biased;
        _ = closed.cancelled() => return "cancelled",
        r = tokio::time::timeout(CONNECT_TIMEOUT, connect(&s.prepared)) => r,
    };
    let (socket, protocol) = match connected {
        Err(_) => {
            buffer.control(super::failed(&ApiError::network("timeout")));
            return "failed";
        }
        Ok(Err(error)) => {
            buffer.control(super::failed(&error));
            return "failed";
        }
        Ok(Ok(open)) => open,
    };
    buffer.control(RealtimeEvent::Open { protocol });
    match pump(socket, s, buffer).await {
        End::Cancelled => "cancelled",
        End::Event(event) => {
            let failed = matches!(event, RealtimeEvent::Failed { .. });
            buffer.control(event);
            if failed { "failed" } else { "closed" }
        }
    }
}

/// 세션과 화면 쓰기를 함께 돌린다. 반환 뒤에는 소켓·입력·쓰기가 모두 끝나 있고 세션 자리(permit)도 돌아간다.
pub(crate) async fn run(mut s: Session) {
    let started = std::time::Instant::now();
    let buffer = Buffer::default();
    let tx = s.tx.clone();
    let closed = tx.closed();
    let work = async {
        let outcome = session(&mut s, &buffer).await;
        buffer.end();
        outcome
    };
    let (outcome, written) = tokio::join!(work, super::drain(&buffer, &tx));
    if written.is_ok() && !closed.is_cancelled() {
        tx.finish().await;
    }
    tracing::debug!(
        method = "api.ws_open",
        elapsed_ms = started.elapsed().as_millis() as u64,
        outcome
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_and_reasons_are_fail_closed() {
        assert_eq!(close_code(None).ok(), Some(1000));
        assert!(close_code(Some(1006)).is_err());
        assert!(close_code(Some(1005)).is_err());
        assert!(close_code(Some(2999)).is_err());
        assert!(close_code(Some(3000)).is_ok());
        assert!(close_reason(&"x".repeat(MAX_CLOSE_REASON_BYTES)).is_ok());
        assert!(close_reason(&"x".repeat(MAX_CLOSE_REASON_BYTES + 1)).is_err());
        assert!(close_reason("bad\0reason").is_err());
    }

    #[test]
    fn message_bounds_apply_to_text_and_binary() {
        assert!(payload(MAX_MESSAGE_BYTES).is_ok());
        assert!(payload(MAX_MESSAGE_BYTES + 1).is_err());
        let big = format!(
            r#"{{"type":"text","data":"{}"}}"#,
            "x".repeat(MAX_MESSAGE_BYTES + 1)
        );
        assert!(message(big.as_bytes()).is_err());
    }

    #[test]
    fn input_lines_span_chunks_and_stop_at_the_limit() {
        let mut lines = Lines::default();
        assert!(lines.push(b"{\"type\":\"te").expect("partial").is_empty());
        let out = lines
            .push(b"xt\",\"data\":\"a\"}\n\n{\"x\":1}\n")
            .expect("lines");
        assert_eq!(
            out,
            vec![
                br#"{"type":"text","data":"a"}"#.to_vec(),
                br#"{"x":1}"#.to_vec()
            ]
        );
        let mut lines = Lines::default();
        let chunk = vec![b'x'; 64 * 1024];
        let mut failed = false;
        for _ in 0..(MAX_INPUT_LINE / chunk.len() + 2) {
            if lines.push(&chunk).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "an endless line is refused");
    }

    #[test]
    fn handshake_headers_are_reserved() {
        let row = |name: &str| crate::HeaderRow {
            row: 3,
            name: name.into(),
            value: "x".into(),
        };
        for name in [
            "Host",
            "Connection",
            "Upgrade",
            "Sec-WebSocket-Key",
            "sec-websocket-extensions",
        ] {
            let err = super::super::headers(&[row(name)], &["sec-websocket-*"]).expect_err(name);
            assert!(
                matches!(err, ApiError::InvalidHeader { index: 3, ref reason, .. } if reason == "reserved")
            );
        }
        assert!(super::super::headers(&[row("Origin")], &["sec-websocket-*"]).is_ok());
    }
}
