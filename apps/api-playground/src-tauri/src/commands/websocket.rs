//! Native-first WebSocket transport for API Playground.
//!
//! The webview receives only an opaque session id and fixed, redacted update DTOs.  Resolved
//! headers/auth values and raw binary payloads remain in backend process memory.  Binary payloads
//! can cross the filesystem boundary only after the user explicitly invokes the save command and
//! chooses a destination in the native dialog.

use super::request::{
    self, build_cookie_header, is_sensitive_name, resolve_template, AuthConfig,
    EnvironmentVariable, Redactor, RequestHeader, RequestTemplate, ResolvedRequest,
};
use crate::core::websocket::{
    self, BufferedMessage, MessageBuffer, MessageDirection, MessageKind, MAX_BINARY_PREVIEW_BYTES,
    MAX_CLOSE_REASON_BYTES, MAX_CONTROL_PAYLOAD_BYTES, MAX_MESSAGE_BYTES, MAX_TEXT_PREVIEW_BYTES,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message, WebSocketConfig};

pub const WEBSOCKET_EVENT: &str = "api-playground/websocket";

const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_REQUEST_HEADERS: usize = 100;
const MAX_REQUEST_COOKIES: usize = 100;
const MAX_REQUEST_PARAMS: usize = 100;
const MAX_ENVIRONMENT_VARIABLES: usize = 100;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
const MAX_PARAMETER_BYTES: usize = 64 * 1024;
const MAX_AUTH_FIELD_BYTES: usize = 64 * 1024;
const MAX_COMMAND_QUEUE: usize = 32;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_HEX_PREVIEW_BYTES: usize = MAX_BINARY_PREVIEW_BYTES;

const ENDPOINT_ERROR: &str = "WebSocket endpoint URL이 올바르지 않습니다";
const HEADER_ERROR: &str = "WebSocket 요청 header가 올바르지 않습니다";
const HEADER_TOO_LARGE: &str = "WebSocket 요청 header가 너무 깁니다";
const REQUEST_ITEMS_TOO_LARGE: &str = "WebSocket 요청 항목 수가 제한을 초과했습니다";
const PARAMETER_ERROR: &str = "WebSocket 요청 parameter가 올바르지 않습니다";
const CREDENTIAL_QUERY_ERROR: &str = "WebSocket endpoint query에 credential을 넣을 수 없습니다";
const AUTH_ERROR: &str = "WebSocket 인증 설정이 올바르지 않습니다";
const TIMEOUT_ERROR: &str = "WebSocket 연결 timeout 범위가 올바르지 않습니다";
const SESSION_ERROR: &str = "WebSocket session 상태를 읽을 수 없습니다";
const NOT_OPEN_ERROR: &str = "WebSocket 연결이 열려 있지 않습니다";
const SEND_ERROR: &str = "WebSocket message를 보낼 수 없습니다";
const PING_ERROR: &str = "WebSocket ping을 보낼 수 없습니다";
const CLOSE_ERROR: &str = "WebSocket 연결을 닫을 수 없습니다";
const BINARY_ERROR: &str = "WebSocket binary payload가 올바르지 않습니다";
const SAVE_ERROR: &str = "WebSocket binary를 안전하게 저장할 수 없습니다";
const DISCONNECT_REASON: &str = "client disconnect";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketMessageInput {
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub data: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketCloseInput {
    pub code: Option<u16>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketUpdate {
    pub session_id: String,
    /// state | message
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<String>,
    pub sequence: u64,
    pub dropped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'static str>,
}

#[derive(Debug)]
enum WsCommand {
    Message(Message),
    Close(Option<CloseFrame>),
}

struct SessionData {
    next_message_id: u64,
    sequence: u64,
    buffer: MessageBuffer,
}

impl Default for SessionData {
    fn default() -> Self {
        Self {
            next_message_id: 1,
            sequence: 0,
            buffer: MessageBuffer::default(),
        }
    }
}

struct ActiveSession {
    id: String,
    sender: Option<mpsc::Sender<WsCommand>>,
    data: Arc<Mutex<SessionData>>,
    handle: Option<tauri::async_runtime::JoinHandle<()>>,
    finished: bool,
}

type ReservedSession = (String, mpsc::Receiver<WsCommand>, Arc<Mutex<SessionData>>);

/// The app permits one active socket.  Keeping one finished slot allows an explicit binary save
/// after the peer closes while a subsequent Connect safely discards the old bounded buffer.
pub struct WebSocketState {
    next_id: AtomicU64,
    active: Mutex<Option<ActiveSession>>,
}

impl Default for WebSocketState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active: Mutex::new(None),
        }
    }
}

impl WebSocketState {
    fn next_session_id(&self) -> Result<String, String> {
        loop {
            let current = self.next_id.load(Ordering::Relaxed);
            if current == u64::MAX {
                return Err("WebSocket session을 시작할 수 없습니다".to_string());
            }
            if self
                .next_id
                .compare_exchange(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(format!("ws-{current}"));
            }
        }
    }

    fn reserve(&self) -> Result<ReservedSession, String> {
        let mut active = self.active.lock().map_err(|_| SESSION_ERROR.to_string())?;
        if active.as_ref().is_some_and(|session| !session.finished) {
            return Err("이미 실행 중인 WebSocket 연결이 있습니다".to_string());
        }
        let id = self.next_session_id()?;
        let (sender, receiver) = mpsc::channel(MAX_COMMAND_QUEUE);
        let data = Arc::new(Mutex::new(SessionData::default()));
        *active = Some(ActiveSession {
            id: id.clone(),
            sender: Some(sender),
            data: Arc::clone(&data),
            handle: None,
            finished: false,
        });
        Ok((id, receiver, data))
    }

    fn attach(&self, id: &str, handle: tauri::async_runtime::JoinHandle<()>) -> bool {
        let Ok(mut active) = self.active.lock() else {
            handle.abort();
            return false;
        };
        let Some(session) = active.as_mut() else {
            handle.abort();
            return false;
        };
        if session.id != id || session.handle.is_some() || session.finished {
            handle.abort();
            if session.id == id && session.finished {
                *active = None;
            }
            return false;
        }
        session.handle = Some(handle);
        true
    }

    fn finish(&self, id: &str) {
        if let Ok(mut active) = self.active.lock() {
            if let Some(session) = active.as_mut().filter(|session| session.id == id) {
                session.finished = true;
                session.sender = None;
                session.handle = None;
            }
        }
    }

    fn sender(&self, id: &str) -> Result<mpsc::Sender<WsCommand>, String> {
        let active = self.active.lock().map_err(|_| SESSION_ERROR.to_string())?;
        let session = active
            .as_ref()
            .filter(|session| session.id == id && !session.finished)
            .ok_or_else(|| NOT_OPEN_ERROR.to_string())?;
        session
            .sender
            .as_ref()
            .cloned()
            .ok_or_else(|| NOT_OPEN_ERROR.to_string())
    }

    fn binary_payload(&self, id: &str, message_id: u64) -> Result<Vec<u8>, String> {
        let active = self.active.lock().map_err(|_| SAVE_ERROR.to_string())?;
        let session = active
            .as_ref()
            .filter(|session| session.id == id)
            .ok_or_else(|| SAVE_ERROR.to_string())?;
        let data = session.data.lock().map_err(|_| SAVE_ERROR.to_string())?;
        let message = data
            .buffer
            .get(message_id)
            .filter(|message| message.kind == MessageKind::Binary)
            .ok_or_else(|| SAVE_ERROR.to_string())?;
        Ok(message.payload.clone())
    }
}

#[tauri::command]
pub fn start_websocket(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<WebSocketState>>,
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
) -> Result<String, String> {
    validate_template_request(&req, &environment)?;
    request::validate_cookie_rows(&req.headers, &req.cookies)?;
    let sealer = crate::platform::platform_sealer();
    let (mut resolved, environment_secrets) = resolve_template(&req, &environment, sealer.as_ref())
        .map_err(|_| request::safe_secret_error())?;
    let url = validate_resolved_request(&resolved)?;
    resolved.url = url;
    let redactor = Redactor::for_request(&resolved, environment_secrets);
    let (session_id, receiver, data) = state.reserve()?;
    let task_id = session_id.clone();
    let task_state = Arc::clone(state.inner());
    let task = tauri::async_runtime::spawn(async move {
        let result = run_session(
            app.clone(),
            task_id.clone(),
            resolved,
            redactor,
            receiver,
            data,
        )
        .await;
        if let Err(message) = result {
            emit_state(&app, &task_id, "error", Some(message));
        }
        task_state.finish(&task_id);
    });
    if !state.attach(&session_id, task) {
        state.finish(&session_id);
        return Err("WebSocket 연결을 시작할 수 없습니다".to_string());
    }
    Ok(session_id)
}

#[tauri::command]
pub async fn send_websocket_message(
    state: tauri::State<'_, Arc<WebSocketState>>,
    session_id: String,
    message: WebSocketMessageInput,
) -> Result<(), String> {
    validate_session_id(&session_id)?;
    let command = input_to_command(message)?;
    state
        .sender(&session_id)?
        .send(command)
        .await
        .map_err(|_| SEND_ERROR.to_string())
}

#[tauri::command]
pub async fn ping_websocket(
    state: tauri::State<'_, Arc<WebSocketState>>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    validate_session_id(&session_id)?;
    let payload = decode_base64_payload(&data, MessageKind::Ping)?;
    state
        .sender(&session_id)?
        .send(WsCommand::Message(Message::Ping(payload.into())))
        .await
        .map_err(|_| PING_ERROR.to_string())
}

#[tauri::command]
pub async fn close_websocket(
    state: tauri::State<'_, Arc<WebSocketState>>,
    session_id: String,
    close: WebSocketCloseInput,
) -> Result<(), String> {
    validate_session_id(&session_id)?;
    let frame = close_frame(close.code, &close.reason)?;
    state
        .sender(&session_id)?
        .send(WsCommand::Close(Some(frame)))
        .await
        .map_err(|_| CLOSE_ERROR.to_string())
}

/// Idempotent convenience command used by lifecycle cleanup.  It sends a normal close frame;
/// the native task emits closing and closed states and then releases the active slot.
#[tauri::command]
pub async fn disconnect_websocket(
    state: tauri::State<'_, Arc<WebSocketState>>,
    session_id: String,
) -> Result<(), String> {
    validate_session_id(&session_id)?;
    let Ok(sender) = state.sender(&session_id) else {
        return Ok(());
    };
    let frame = close_frame(Some(1000), DISCONNECT_REASON)?;
    sender
        .send(WsCommand::Close(Some(frame)))
        .await
        .map_err(|_| CLOSE_ERROR.to_string())
}

#[tauri::command]
pub async fn save_websocket_binary(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<WebSocketState>>,
    session_id: String,
    message_id: u64,
) -> Result<bool, String> {
    validate_session_id(&session_id)?;
    let payload = state.binary_payload(&session_id, message_id)?;
    let default_name = format!("websocket-message-{message_id}.bin");
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(default_name)
            .blocking_save_file()
    })
    .await
    .map_err(|_| SAVE_ERROR.to_string())?;
    let Some(path) = selected else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|_| SAVE_ERROR.to_string())?;
    validate_save_path(&path)?;
    tauri::async_runtime::spawn_blocking(move || devbox_filesystem::atomic_write(path, &payload))
        .await
        .map_err(|_| SAVE_ERROR.to_string())?
        .map_err(|_| SAVE_ERROR.to_string())?;
    Ok(true)
}

fn validate_template_request(
    req: &RequestTemplate,
    environment: &[EnvironmentVariable],
) -> Result<(), String> {
    if environment.len() > MAX_ENVIRONMENT_VARIABLES
        || environment.iter().any(|variable| {
            variable.key.is_empty()
                || variable.key.len() > 128
                || variable.value.len() > MAX_HEADER_VALUE_BYTES
        })
    {
        return Err("환경 변수 형식이 올바르지 않습니다".to_string());
    }
    if req.url.len() > MAX_URL_BYTES || req.url.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(ENDPOINT_ERROR.to_string());
    }
    if req.headers.len() > MAX_REQUEST_HEADERS
        || req.cookies.len() > MAX_REQUEST_COOKIES
        || req.params.len() > MAX_REQUEST_PARAMS
    {
        return Err(REQUEST_ITEMS_TOO_LARGE.to_string());
    }
    for header in &req.headers {
        if header.key.len() > MAX_HEADER_NAME_BYTES || header.value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(HEADER_TOO_LARGE.to_string());
        }
        if header.enabled && !header.key.trim().is_empty() {
            validate_header_value(header)?;
        }
    }
    for cookie in &req.cookies {
        if cookie.name.len() > MAX_HEADER_NAME_BYTES || cookie.value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err(HEADER_TOO_LARGE.to_string());
        }
    }
    for parameter in &req.params {
        if parameter.key.len() > MAX_PARAMETER_BYTES || parameter.value.len() > MAX_PARAMETER_BYTES
        {
            return Err(PARAMETER_ERROR.to_string());
        }
    }
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&req.timeout_ms) {
        return Err(TIMEOUT_ERROR.to_string());
    }
    if let Some(auth) = &req.auth {
        validate_auth(auth)?;
    }
    Ok(())
}

fn validate_resolved_request(req: &ResolvedRequest) -> Result<String, String> {
    if req.headers.len() > MAX_REQUEST_HEADERS
        || req.cookies.len() > MAX_REQUEST_COOKIES
        || req.params.len() > MAX_REQUEST_PARAMS
    {
        return Err(REQUEST_ITEMS_TOO_LARGE.to_string());
    }
    if req.timeout_ms < MIN_TIMEOUT_MS || req.timeout_ms > MAX_TIMEOUT_MS {
        return Err(TIMEOUT_ERROR.to_string());
    }
    for header in &req.headers {
        if header.key.len() > MAX_HEADER_NAME_BYTES || header.value.len() > MAX_HEADER_VALUE_BYTES {
            return Err(HEADER_TOO_LARGE.to_string());
        }
        if header.enabled && !header.key.trim().is_empty() {
            validate_header_value(header)?;
        }
    }
    let url = append_params(&req.url, &req.params)?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| ENDPOINT_ERROR.to_string())?;
    if !matches!(parsed.scheme(), "ws" | "wss")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || parsed.as_str().len() > MAX_URL_BYTES
    {
        return Err(ENDPOINT_ERROR.to_string());
    }
    for (key, _) in parsed.query_pairs() {
        if is_sensitive_name(&key) {
            return Err(CREDENTIAL_QUERY_ERROR.to_string());
        }
    }
    request::validate_cookie_rows(&req.headers, &req.cookies)?;
    Ok(parsed.to_string())
}

fn append_params(base: &str, params: &[request::KeyValue]) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base).map_err(|_| ENDPOINT_ERROR.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        for parameter in params.iter().filter(|parameter| !parameter.key.is_empty()) {
            if is_sensitive_name(&parameter.key) {
                return Err(CREDENTIAL_QUERY_ERROR.to_string());
            }
            query.append_pair(&parameter.key, &parameter.value);
        }
    }
    if url.as_str().len() > MAX_URL_BYTES {
        return Err("WebSocket 요청 URL이 너무 깁니다".to_string());
    }
    Ok(url.to_string())
}

fn validate_header_value(header: &RequestHeader) -> Result<(), String> {
    HeaderName::from_bytes(header.key.trim().as_bytes()).map_err(|_| HEADER_ERROR.to_string())?;
    HeaderValue::from_str(&header.value).map_err(|_| HEADER_ERROR.to_string())?;
    Ok(())
}

fn validate_auth(auth: &AuthConfig) -> Result<(), String> {
    if !matches!(auth.kind.as_str(), "none" | "basic" | "bearer" | "apikey") {
        return Err(AUTH_ERROR.to_string());
    }
    if [
        auth.kind.as_str(),
        auth.username.as_str(),
        auth.password.as_str(),
        auth.token.as_str(),
        auth.api_key.as_str(),
        auth.api_value.as_str(),
    ]
    .iter()
    .any(|value| value.len() > MAX_AUTH_FIELD_BYTES)
    {
        return Err(AUTH_ERROR.to_string());
    }
    match auth.kind.as_str() {
        "bearer" => {
            HeaderValue::from_str(&format!("Bearer {}", auth.token))
                .map_err(|_| AUTH_ERROR.to_string())?;
        }
        "apikey" if !auth.api_key.is_empty() => {
            HeaderName::from_bytes(auth.api_key.trim().as_bytes())
                .map_err(|_| AUTH_ERROR.to_string())?;
            HeaderValue::from_str(&auth.api_value).map_err(|_| AUTH_ERROR.to_string())?;
        }
        _ => {}
    }
    Ok(())
}

fn build_client_request(
    req: &ResolvedRequest,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, &'static str> {
    let mut request = req
        .url
        .as_str()
        .into_client_request()
        .map_err(|_| ENDPOINT_ERROR)?;
    for header in req.headers.iter().filter(|header| {
        header.enabled && !header.key.trim().is_empty() && !is_websocket_derived_header(&header.key)
    }) {
        let name =
            HeaderName::from_bytes(header.key.trim().as_bytes()).map_err(|_| HEADER_ERROR)?;
        let value = HeaderValue::from_str(&header.value).map_err(|_| HEADER_ERROR)?;
        request.headers_mut().append(name, value);
    }
    if let Some(cookie) = build_cookie_header(&req.cookies) {
        request.headers_mut().append(
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(&cookie).map_err(|_| HEADER_ERROR)?,
        );
    }
    if let Some(auth) = &req.auth {
        match auth.kind.as_str() {
            "basic" if !auth.username.is_empty() => {
                let value = format!(
                    "Basic {}",
                    B64.encode(format!("{}:{}", auth.username, auth.password))
                );
                request.headers_mut().append(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&value).map_err(|_| AUTH_ERROR)?,
                );
            }
            "bearer" if !auth.token.is_empty() => {
                let value = format!("Bearer {}", auth.token);
                request.headers_mut().append(
                    HeaderName::from_static("authorization"),
                    HeaderValue::from_str(&value).map_err(|_| AUTH_ERROR)?,
                );
            }
            "apikey" if !auth.api_key.is_empty() => {
                let name = HeaderName::from_bytes(auth.api_key.trim().as_bytes())
                    .map_err(|_| AUTH_ERROR)?;
                let value = HeaderValue::from_str(&auth.api_value).map_err(|_| AUTH_ERROR)?;
                request.headers_mut().append(name, value);
            }
            _ => {}
        }
    }
    Ok(request)
}

fn is_websocket_derived_header(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase().replace('_', "-");
    matches!(
        normalized.as_str(),
        "host"
            | "connection"
            | "upgrade"
            | "sec-websocket-key"
            | "sec-websocket-version"
            | "sec-websocket-extensions"
            | "sec-websocket-protocol"
            | "content-length"
            | "transfer-encoding"
    ) || normalized.starts_with("sec-websocket-")
}

fn input_to_command(input: WebSocketMessageInput) -> Result<WsCommand, String> {
    match input.kind.trim().to_ascii_lowercase().as_str() {
        "text" => {
            if input.text.len() > MAX_MESSAGE_BYTES {
                return Err(websocket::MESSAGE_TOO_LARGE.to_string());
            }
            Ok(WsCommand::Message(Message::text(input.text)))
        }
        "binary" => {
            let payload = decode_base64_payload(&input.data, MessageKind::Binary)?;
            Ok(WsCommand::Message(Message::binary(payload)))
        }
        "ping" => {
            let payload = decode_base64_payload(&input.data, MessageKind::Ping)?;
            Ok(WsCommand::Message(Message::Ping(payload.into())))
        }
        "pong" => {
            let payload = decode_base64_payload(&input.data, MessageKind::Pong)?;
            Ok(WsCommand::Message(Message::Pong(payload.into())))
        }
        _ => Err(BINARY_ERROR.to_string()),
    }
}

fn decode_base64_payload(value: &str, kind: MessageKind) -> Result<Vec<u8>, String> {
    let max_bytes = match kind {
        MessageKind::Ping | MessageKind::Pong => MAX_CONTROL_PAYLOAD_BYTES,
        MessageKind::Text | MessageKind::Binary | MessageKind::Close => MAX_MESSAGE_BYTES,
    };
    let max_encoded = max_bytes.div_ceil(3).saturating_mul(4).saturating_add(4);
    if value.len() > max_encoded {
        return Err(websocket::MESSAGE_TOO_LARGE.to_string());
    }
    let payload = B64.decode(value).map_err(|_| BINARY_ERROR.to_string())?;
    websocket::validate_payload(kind, &payload).map_err(str::to_string)?;
    Ok(payload)
}

fn close_frame(code: Option<u16>, reason: &str) -> Result<CloseFrame, String> {
    let code = websocket::validate_close_code(code).map_err(str::to_string)?;
    websocket::validate_close_reason(reason).map_err(str::to_string)?;
    Ok(CloseFrame {
        code: code.into(),
        reason: reason.to_string().into(),
    })
}

fn validate_session_id(value: &str) -> Result<(), String> {
    if value.len() > 32
        || !value.strip_prefix("ws-").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err("WebSocket session 식별자가 올바르지 않습니다".to_string());
    }
    Ok(())
}

fn validate_save_path(path: &Path) -> Result<(), String> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(SAVE_ERROR.to_string());
    };
    if file_name.is_empty()
        || file_name.len() > 255
        || file_name.bytes().any(|byte| byte.is_ascii_control())
        || path.parent().is_none_or(|parent| !parent.is_dir())
    {
        return Err(SAVE_ERROR.to_string());
    }
    Ok(())
}

async fn run_session(
    app: tauri::AppHandle,
    session_id: String,
    req: ResolvedRequest,
    redactor: Redactor,
    mut receiver: mpsc::Receiver<WsCommand>,
    data: Arc<Mutex<SessionData>>,
) -> Result<(), &'static str> {
    emit_state(&app, &session_id, "connecting", None);
    let request = build_client_request(&req)?;
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE_BYTES);
    config.max_frame_size = Some(MAX_MESSAGE_BYTES);
    let timeout = Duration::from_millis(req.timeout_ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS));
    let connected = tokio::time::timeout(
        timeout,
        connect_async_with_config(request, Some(config), true),
    )
    .await
    .map_err(|_| "WebSocket 연결 시간이 초과되었습니다")?
    .map_err(|_| "WebSocket 연결에 실패했습니다")?;
    let (mut socket, _response) = connected;
    emit_state(&app, &session_id, "open", None);

    loop {
        tokio::select! {
            command = receiver.recv() => {
                let Some(command) = command else {
                    let _ = socket.close(None).await;
                    emit_state(&app, &session_id, "closed", None);
                    return Ok(());
                };
                match command {
                    WsCommand::Message(message) => {
                        let view = command_message_view(&message)?;
                        socket.send(message).await.map_err(|_| SEND_ERROR)?;
                        emit_message(&app, &session_id, &data, &redactor, view, MessageDirection::Sent)?;
                    }
                    WsCommand::Close(frame) => {
                        emit_state(&app, &session_id, "closing", None);
                        if let Some(frame) = frame.as_ref() {
                            let view = MessageView::close(
                                MessageDirection::Sent,
                                Some(frame.code.into()),
                                frame.reason.to_string(),
                            );
                            socket
                                .send(Message::Close(Some(frame.clone())))
                                .await
                                .map_err(|_| CLOSE_ERROR)?;
                            emit_message(&app, &session_id, &data, &redactor, view, MessageDirection::Sent)?;
                        } else {
                            socket.send(Message::Close(None)).await.map_err(|_| CLOSE_ERROR)?;
                        }
                        emit_state(&app, &session_id, "closed", None);
                        return Ok(());
                    }
                }
            }
            incoming = socket.next() => {
                let Some(incoming) = incoming else {
                    emit_state(&app, &session_id, "closed", None);
                    return Ok(());
                };
                let incoming = incoming.map_err(|_| "WebSocket 연결이 끊어졌습니다")?;
                match incoming {
                    Message::Text(text) => {
                        let bytes = text.as_bytes();
                        websocket::validate_payload(MessageKind::Text, bytes).map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
                        emit_message(
                            &app,
                            &session_id,
                            &data,
                            &redactor,
                            MessageView::text(MessageDirection::Received, text.to_string()),
                            MessageDirection::Received,
                        )?;
                    }
                    Message::Binary(bytes) => {
                        let bytes = bytes.to_vec();
                        websocket::validate_payload(MessageKind::Binary, &bytes).map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
                        emit_message(
                            &app,
                            &session_id,
                            &data,
                            &redactor,
                            MessageView::binary(MessageDirection::Received, bytes),
                            MessageDirection::Received,
                        )?;
                    }
                    Message::Ping(payload) => {
                        let payload = payload.to_vec();
                        websocket::validate_payload(MessageKind::Ping, &payload).map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
                        emit_message(
                            &app,
                            &session_id,
                            &data,
                            &redactor,
                            MessageView::payload(MessageKind::Ping, MessageDirection::Received, payload.clone()),
                            MessageDirection::Received,
                        )?;
                        // Tungstenite queues the RFC-mandated pong while reading a ping;
                        // flushing it here avoids a duplicate manually-written pong.
                        socket.flush().await.map_err(|_| PING_ERROR)?;
                        emit_message(
                            &app,
                            &session_id,
                            &data,
                            &redactor,
                            MessageView::payload(MessageKind::Pong, MessageDirection::Sent, payload),
                            MessageDirection::Sent,
                        )?;
                    }
                    Message::Pong(payload) => {
                        let payload = payload.to_vec();
                        websocket::validate_payload(MessageKind::Pong, &payload).map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
                        emit_message(
                            &app,
                            &session_id,
                            &data,
                            &redactor,
                            MessageView::payload(MessageKind::Pong, MessageDirection::Received, payload),
                            MessageDirection::Received,
                        )?;
                    }
                    Message::Close(frame) => {
                        emit_state(&app, &session_id, "closing", None);
                        let view = frame.as_ref().map_or_else(
                            || MessageView::close(MessageDirection::Received, None, String::new()),
                            |frame| MessageView::close(
                                MessageDirection::Received,
                                Some(frame.code.into()),
                                frame.reason.to_string(),
                            ),
                        );
                        emit_message(&app, &session_id, &data, &redactor, view, MessageDirection::Received)?;
                        let _ = socket.close(frame).await;
                        emit_state(&app, &session_id, "closed", None);
                        return Ok(());
                    }
                    Message::Frame(_) => {}
                }
            }
        }
    }
}

#[derive(Debug)]
enum MessageView {
    Text {
        direction: MessageDirection,
        text: String,
    },
    Binary {
        direction: MessageDirection,
        payload: Vec<u8>,
    },
    Payload {
        kind: MessageKind,
        direction: MessageDirection,
        payload: Vec<u8>,
    },
    Close {
        direction: MessageDirection,
        code: Option<u16>,
        reason: String,
    },
}

impl MessageView {
    fn text(direction: MessageDirection, text: String) -> Self {
        Self::Text { direction, text }
    }

    fn binary(direction: MessageDirection, payload: Vec<u8>) -> Self {
        Self::Binary { direction, payload }
    }

    fn payload(kind: MessageKind, direction: MessageDirection, payload: Vec<u8>) -> Self {
        Self::Payload {
            kind,
            direction,
            payload,
        }
    }

    fn close(direction: MessageDirection, code: Option<u16>, reason: String) -> Self {
        Self::Close {
            direction,
            code,
            reason,
        }
    }
}

fn command_message_view(message: &Message) -> Result<MessageView, &'static str> {
    match message {
        Message::Text(text) => {
            websocket::validate_payload(MessageKind::Text, text.as_bytes())
                .map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
            Ok(MessageView::text(MessageDirection::Sent, text.to_string()))
        }
        Message::Binary(bytes) => {
            let payload = bytes.to_vec();
            websocket::validate_payload(MessageKind::Binary, &payload)
                .map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
            Ok(MessageView::binary(MessageDirection::Sent, payload))
        }
        Message::Ping(bytes) => {
            let payload = bytes.to_vec();
            websocket::validate_payload(MessageKind::Ping, &payload)
                .map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
            Ok(MessageView::payload(
                MessageKind::Ping,
                MessageDirection::Sent,
                payload,
            ))
        }
        Message::Pong(bytes) => {
            let payload = bytes.to_vec();
            websocket::validate_payload(MessageKind::Pong, &payload)
                .map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
            Ok(MessageView::payload(
                MessageKind::Pong,
                MessageDirection::Sent,
                payload,
            ))
        }
        Message::Close(_) | Message::Frame(_) => Err(CLOSE_ERROR),
    }
}

fn emit_message(
    app: &tauri::AppHandle,
    session_id: &str,
    data: &Arc<Mutex<SessionData>>,
    redactor: &Redactor,
    view: MessageView,
    _direction: MessageDirection,
) -> Result<(), &'static str> {
    let (record, safe) = {
        let mut data = data.lock().map_err(|_| SESSION_ERROR)?;
        let (kind, direction, payload, close_code, close_reason) = match view {
            MessageView::Text { direction, text } => (
                MessageKind::Text,
                direction,
                text.into_bytes(),
                None,
                String::new(),
            ),
            MessageView::Binary { direction, payload } => {
                (MessageKind::Binary, direction, payload, None, String::new())
            }
            MessageView::Payload {
                kind,
                direction,
                payload,
            } => (kind, direction, payload, None, String::new()),
            MessageView::Close {
                direction,
                code,
                reason,
            } => (
                MessageKind::Close,
                direction,
                reason.as_bytes().to_vec(),
                code,
                reason,
            ),
        };
        websocket::validate_payload(kind, &payload).map_err(|_| websocket::MESSAGE_TOO_LARGE)?;
        let id = data.next_message_id;
        data.next_message_id = data.next_message_id.checked_add(1).ok_or(SESSION_ERROR)?;
        let record = BufferedMessage {
            id,
            kind,
            direction,
            payload,
            close_code,
            close_reason,
        };
        data.buffer.push(record.clone());
        data.sequence = data.sequence.checked_add(1).ok_or(SESSION_ERROR)?;
        let mut safe = safe_message(&record, redactor);
        safe.sequence = data.sequence;
        safe.dropped = data.buffer.evicted();
        (record, safe)
    };
    let _ = record;
    emit_update(
        WebSocketUpdate {
            session_id: session_id.to_string(),
            kind: "message",
            state: None,
            direction: Some(safe.direction),
            message_id: Some(safe.message_id),
            message_type: Some(safe.kind),
            text: safe.text,
            text_truncated: safe.text_truncated,
            binary_hex: safe.binary_hex,
            binary_text: safe.binary_text,
            binary_size: safe.binary_size,
            binary_truncated: safe.binary_truncated,
            close_code: safe.close_code,
            close_reason: safe.close_reason,
            sequence: safe.sequence,
            dropped: safe.dropped,
            message: None,
        },
        app,
    );
    Ok(())
}

struct SafeMessage {
    message_id: u64,
    kind: &'static str,
    direction: &'static str,
    text: Option<String>,
    text_truncated: Option<bool>,
    binary_hex: Option<String>,
    binary_text: Option<String>,
    binary_size: Option<usize>,
    binary_truncated: Option<bool>,
    close_code: Option<u16>,
    close_reason: Option<String>,
    sequence: u64,
    dropped: usize,
}

fn safe_message(message: &BufferedMessage, redactor: &Redactor) -> SafeMessage {
    let mut safe = SafeMessage {
        message_id: message.id,
        kind: message.kind.as_str(),
        direction: message.direction.as_str(),
        text: None,
        text_truncated: None,
        binary_hex: None,
        binary_text: None,
        binary_size: None,
        binary_truncated: None,
        close_code: message.close_code,
        close_reason: None,
        sequence: 0,
        dropped: 0,
    };
    match message.kind {
        MessageKind::Text => {
            let text = String::from_utf8_lossy(&message.payload);
            let text = redact_websocket_text(&text, redactor);
            let (text, truncated) = websocket::utf8_truncate(&text, MAX_TEXT_PREVIEW_BYTES);
            safe.text = Some(text);
            safe.text_truncated = Some(truncated);
        }
        MessageKind::Binary | MessageKind::Ping | MessageKind::Pong => {
            safe.binary_size = Some(message.payload.len());
            let mut contains_secret = redactor.contains_binary_secret(&message.payload);
            if let Ok(original) = std::str::from_utf8(&message.payload) {
                let text = redact_websocket_text(original, redactor);
                contains_secret |= text != original;
                let (text, truncated) = websocket::utf8_truncate(&text, MAX_TEXT_PREVIEW_BYTES);
                safe.binary_text = Some(text);
                safe.binary_truncated =
                    Some(truncated || message.payload.len() > MAX_HEX_PREVIEW_BYTES);
            } else {
                safe.binary_truncated = Some(message.payload.len() > MAX_HEX_PREVIEW_BYTES);
            }
            safe.binary_hex = Some(if contains_secret {
                "[REDACTED]".to_string()
            } else {
                binary_hex(&message.payload)
            });
        }
        MessageKind::Close => {
            let reason = redactor.redact_text(&message.close_reason);
            let (reason, _) = websocket::utf8_truncate(&reason, MAX_CLOSE_REASON_BYTES);
            safe.close_reason = Some(reason);
        }
    }
    safe
}

fn redact_websocket_text(value: &str, redactor: &Redactor) -> String {
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
        if parsed.is_object() || parsed.is_array() {
            return redactor.redact_body(value);
        }
    }
    redactor.redact_text(value)
}

fn binary_hex(payload: &[u8]) -> String {
    let shown = payload.len().min(MAX_HEX_PREVIEW_BYTES);
    let mut output = String::with_capacity(shown.saturating_mul(2) + 1);
    for byte in &payload[..shown] {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    if payload.len() > shown {
        output.push('…');
    }
    output
}

fn emit_state(
    app: &tauri::AppHandle,
    session_id: &str,
    state: &'static str,
    message: Option<&'static str>,
) {
    emit_update(
        WebSocketUpdate {
            session_id: session_id.to_string(),
            kind: "state",
            state: Some(state),
            direction: None,
            message_id: None,
            message_type: None,
            text: None,
            text_truncated: None,
            binary_hex: None,
            binary_text: None,
            binary_size: None,
            binary_truncated: None,
            close_code: None,
            close_reason: None,
            sequence: 0,
            dropped: 0,
            message,
        },
        app,
    );
}

fn emit_update(update: WebSocketUpdate, app: &tauri::AppHandle) {
    let _ = app.emit(WEBSOCKET_EVENT, update);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn endpoint_rejects_unsafe_schemes_userinfo_fragments_and_credentials() {
        let mut req = test_request("ws://user:pass@example.test/socket");
        assert!(validate_resolved_request(&resolved(&req)).is_err());
        req.url = "http://example.test/socket".into();
        assert!(validate_resolved_request(&resolved(&req)).is_err());
        req.url = "ws://example.test/socket#fragment".into();
        assert!(validate_resolved_request(&resolved(&req)).is_err());
        req.url = "ws://example.test/socket?token=secret".into();
        assert_eq!(
            validate_resolved_request(&resolved(&req)).unwrap_err(),
            CREDENTIAL_QUERY_ERROR
        );
    }

    #[test]
    fn headers_and_auth_are_validated_and_transport_headers_are_ignored() {
        let mut req = resolved(&test_request("ws://example.test/socket"));
        req.headers = vec![
            RequestHeader {
                key: "Host".into(),
                value: "bad.example".into(),
                enabled: true,
            },
            RequestHeader {
                key: "X-Trace".into(),
                value: "ok".into(),
                enabled: true,
            },
        ];
        let client = build_client_request(&req).unwrap();
        assert_ne!(
            client
                .headers()
                .get("host")
                .and_then(|value| value.to_str().ok()),
            Some("bad.example")
        );
        assert_eq!(client.headers().get("x-trace").unwrap(), "ok");
        let bad = RequestHeader {
            key: "X-Bad\nHeader".into(),
            value: "x".into(),
            enabled: true,
        };
        assert!(validate_header_value(&bad).is_err());
        let bad_auth = AuthConfig {
            kind: "apikey".into(),
            api_key: "bad key".into(),
            ..AuthConfig::default()
        };
        assert!(validate_auth(&bad_auth).is_err());
    }

    #[test]
    fn close_and_binary_inputs_are_bounded() {
        assert!(close_frame(Some(1000), "normal").is_ok());
        assert!(close_frame(Some(1006), "bad").is_err());
        assert!(close_frame(Some(3000), &"x".repeat(MAX_CLOSE_REASON_BYTES)).is_ok());
        assert!(close_frame(Some(3000), &"x".repeat(MAX_CLOSE_REASON_BYTES + 1)).is_err());
        let input = WebSocketMessageInput {
            kind: "binary".into(),
            text: String::new(),
            data: B64.encode(vec![0_u8; MAX_MESSAGE_BYTES + 1]),
        };
        assert!(input_to_command(input).is_err());
        let ping = WebSocketMessageInput {
            kind: "ping".into(),
            text: String::new(),
            data: B64.encode([0_u8; MAX_CONTROL_PAYLOAD_BYTES]),
        };
        assert!(input_to_command(ping).is_ok());
    }

    #[test]
    fn opaque_session_ids_do_not_accept_paths_or_urls() {
        assert!(validate_session_id("ws-1").is_ok());
        assert!(validate_session_id("ws-1/path").is_err());
        assert!(validate_session_id("ws-https://secret.example").is_err());
    }

    #[test]
    fn binary_preview_masks_known_request_secret() {
        let req = resolved(&RequestTemplate {
            auth: Some(AuthConfig {
                kind: "bearer".into(),
                token: "loopback-secret".into(),
                ..AuthConfig::default()
            }),
            ..test_request("ws://example.test/socket")
        });
        let redactor = Redactor::for_request(&req, Vec::new());
        let message = BufferedMessage {
            id: 1,
            kind: MessageKind::Binary,
            direction: MessageDirection::Received,
            payload: b"loopback-secret".to_vec(),
            close_code: None,
            close_reason: String::new(),
        };
        let safe = safe_message(&message, &redactor);
        assert_eq!(safe.binary_hex.as_deref(), Some("[REDACTED]"));
        assert_eq!(safe.binary_text.as_deref(), Some("[REDACTED]"));
    }

    #[test]
    fn binary_preview_masks_known_token_patterns_even_without_request_match() {
        let req = resolved(&test_request("ws://example.test/socket"));
        let redactor = Redactor::for_request(&req, Vec::new());
        let message = BufferedMessage {
            id: 1,
            kind: MessageKind::Binary,
            direction: MessageDirection::Received,
            payload: b"ghp_1234567890abcdef".to_vec(),
            close_code: None,
            close_reason: String::new(),
        };
        let safe = safe_message(&message, &redactor);
        assert_eq!(safe.binary_hex.as_deref(), Some("[REDACTED]"));
    }

    #[test]
    fn text_preview_masks_sensitive_json_fields_without_request_secret_match() {
        let req = resolved(&test_request("ws://example.test/socket"));
        let redactor = Redactor::for_request(&req, Vec::new());
        let message = BufferedMessage {
            id: 1,
            kind: MessageKind::Text,
            direction: MessageDirection::Received,
            payload: br#"{"token":"server-only-secret","value":"safe"}"#.to_vec(),
            close_code: None,
            close_reason: String::new(),
        };
        let safe = safe_message(&message, &redactor);
        let text = safe.text.unwrap();
        assert!(!text.contains("server-only-secret"));
        assert!(text.contains("[REDACTED]"));

        let binary = BufferedMessage {
            kind: MessageKind::Binary,
            ..message
        };
        let binary_safe = safe_message(&binary, &redactor);
        assert_eq!(binary_safe.binary_hex.as_deref(), Some("[REDACTED]"));
    }

    #[test]
    fn native_loopback_handshake_and_text_binary_frames_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let headers = read_until_headers(&mut stream);
            assert!(headers
                .to_ascii_lowercase()
                .contains("\r\nx-trace: loopback"));
            let key = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Sec-WebSocket-Key:")
                        .or_else(|| line.strip_prefix("sec-websocket-key:"))
                })
                .map(str::trim)
                .unwrap();
            let accept = handshake_accept(key);
            write!(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").unwrap();
            stream.flush().unwrap();
            let text = read_frame(&mut stream);
            write_server_frame(&mut stream, 0x1, b"echo-text");
            let binary = read_frame(&mut stream);
            write_server_frame(&mut stream, 0x2, &binary);
            (text, binary)
        });
        let request = resolved(&RequestTemplate {
            url: format!("ws://127.0.0.1:{port}/socket"),
            headers: vec![RequestHeader {
                key: "X-Trace".into(),
                value: "loopback".into(),
                enabled: true,
            }],
            timeout_ms: 5_000,
            ..test_request("")
        });
        let client_request = build_client_request(&request).unwrap();
        let (mut socket, _) =
            tauri::async_runtime::block_on(connect_async_with_config(client_request, None, true))
                .unwrap();
        tauri::async_runtime::block_on(async {
            socket.send(Message::text("client-text")).await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::text("echo-text")
            );
            socket
                .send(Message::binary(b"client-binary".to_vec()))
                .await
                .unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap(),
                Message::binary(b"client-binary".to_vec())
            );
        });
        let (text, binary) = server.join().unwrap();
        assert_eq!(text, b"client-text");
        assert_eq!(binary, b"client-binary");
    }

    fn test_request(url: &str) -> RequestTemplate {
        RequestTemplate {
            method: "GET".into(),
            url: url.into(),
            headers: Vec::new(),
            cookies: Vec::new(),
            multipart: Vec::new(),
            params: Vec::new(),
            body_kind: "none".into(),
            body: String::new(),
            auth: None,
            timeout_ms: 10_000,
            graphql: None,
        }
    }

    fn resolved(req: &RequestTemplate) -> ResolvedRequest {
        resolve_template(req, &[], crate::platform::platform_sealer().as_ref())
            .unwrap()
            .0
    }

    fn read_until_headers(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 512];
        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn handshake_accept(key: &str) -> String {
        tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes())
    }

    fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
        let mut head = [0_u8; 2];
        stream.read_exact(&mut head).unwrap();
        let masked = head[1] & 0x80 != 0;
        let mut length = (head[1] & 0x7f) as usize;
        if length == 126 {
            let mut bytes = [0_u8; 2];
            stream.read_exact(&mut bytes).unwrap();
            length = u16::from_be_bytes(bytes) as usize;
        }
        if length == 127 {
            panic!("fixture payload too large");
        }
        let mut mask = [0_u8; 4];
        if masked {
            stream.read_exact(&mut mask).unwrap();
        }
        let mut payload = vec![0_u8; length];
        stream.read_exact(&mut payload).unwrap();
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
        }
        payload
    }

    fn write_server_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
        assert!(payload.len() < 126);
        stream
            .write_all(&[0x80 | opcode, payload.len() as u8])
            .unwrap();
        stream.write_all(payload).unwrap();
        stream.flush().unwrap();
    }
}
