//! Native SSE transport for API Playground.
//!
//! This module is intentionally a thin command boundary around the existing request resolver and
//! redactor.  It owns one bounded stream task, never persists stream events, and emits only a
//! small masked DTO.  URLs, headers, request bodies, paths, raw chunks, and transport errors do
//! not cross this boundary.

use super::request::{
    self, append_query, apply_body, build_cookie_header, is_body_header, is_cross_origin,
    is_multipart_derived_header, redirect_switches_to_get, safe_secret_error, should_send_header,
    AuthConfig, EnvironmentVariable, MultipartPart, Redactor, RequestHeader, RequestTemplate,
    ResolvedRequest,
};
use crate::core::sse::{
    EventBuffer, ParseError, SseEvent, SseParser, MAX_DECODED_BYTES, MAX_EVENT_DATA_BYTES,
    MAX_EVENT_ID_BYTES, MAX_EVENT_NAME_BYTES,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const SSE_EVENT: &str = "api-playground/sse";

const MAX_REDIRECTS: usize = 10;
const MAX_REQUEST_HEADERS: usize = 100;
const MAX_REQUEST_COOKIES: usize = 100;
const MAX_REQUEST_PARAMS: usize = 100;
const MAX_ENVIRONMENT_VARIABLES: usize = 100;
const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_BYTES: usize = 64 * 1024;
const MAX_PARAMETER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_AUTH_FIELD_BYTES: usize = 64 * 1024;

const MIN_CONNECT_TIMEOUT_MS: u64 = 100;
const MAX_CONNECT_TIMEOUT_MS: u64 = 30_000;
const MIN_IDLE_TIMEOUT_MS: u64 = 100;
const MAX_IDLE_TIMEOUT_MS: u64 = 300_000;
const MIN_TOTAL_TIMEOUT_MS: u64 = 1_000;
const MAX_TOTAL_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_RETRY_MS: u64 = 1_000;
const MIN_RETRY_DELAY_MS: u64 = 250;
const MAX_RECONNECT_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SseOptions {
    pub connect_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub total_timeout_ms: u64,
    #[serde(default)]
    pub reconnect: bool,
}

impl Default for SseOptions {
    fn default() -> Self {
        Self {
            connect_timeout_ms: 10_000,
            idle_timeout_ms: 30_000,
            total_timeout_ms: 300_000,
            reconnect: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SseUpdate {
    pub session_id: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_ms: Option<u64>,
    pub sequence: u64,
    pub dropped: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}

struct ActiveStream {
    id: String,
    handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

/// At most one native stream is active.  Keeping the task handle here gives stop an immediate,
/// race-safe abort path and prevents a second request from consuming another unbounded socket.
pub struct SseState {
    next_id: AtomicU64,
    active: Mutex<Option<ActiveStream>>,
}

impl Default for SseState {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active: Mutex::new(None),
        }
    }
}

impl SseState {
    fn new_id(&self) -> Result<String, String> {
        loop {
            let current = self.next_id.load(Ordering::Relaxed);
            if current == u64::MAX {
                return Err("SSE stream을 시작할 수 없습니다".to_string());
            }
            if self
                .next_id
                .compare_exchange(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(format!("sse-{current}"));
            }
        }
    }

    fn reserve(&self) -> Result<String, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "SSE stream 상태를 읽을 수 없습니다".to_string())?;
        if active.is_some() {
            return Err("이미 실행 중인 SSE stream이 있습니다".to_string());
        }
        let id = self.new_id()?;
        *active = Some(ActiveStream {
            id: id.clone(),
            handle: None,
        });
        Ok(id)
    }

    fn attach(&self, id: &str, handle: tauri::async_runtime::JoinHandle<()>) -> bool {
        let Ok(mut active) = self.active.lock() else {
            handle.abort();
            return false;
        };
        let Some(current) = active.as_mut() else {
            handle.abort();
            return false;
        };
        if current.id != id || current.handle.is_some() {
            handle.abort();
            return false;
        }
        current.handle = Some(handle);
        true
    }

    fn finish(&self, id: &str) {
        if let Ok(mut active) = self.active.lock() {
            // A very short-lived task can finish before `attach` records its JoinHandle. Keep the
            // reservation in that narrow window so the command does not report a false start
            // failure; `attach` will then publish the already-finished handle and the caller can
            // perform its normal terminal cleanup.
            if active
                .as_ref()
                .is_some_and(|current| current.id == id && current.handle.is_some())
            {
                *active = None;
            }
        }
    }

    fn cancel(&self, id: &str) -> Result<(), String> {
        let handle = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "SSE stream 상태를 읽을 수 없습니다".to_string())?;
            if active.as_ref().is_none_or(|current| current.id != id) {
                return Ok(());
            }
            active.take().and_then(|current| current.handle)
        };
        if let Some(handle) = handle {
            handle.abort();
        }
        Ok(())
    }
}

#[tauri::command]
pub fn start_sse_stream(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SseState>>,
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
    options: SseOptions,
) -> Result<String, String> {
    validate_options(&options)?;
    validate_environment(&environment)?;
    request::validate_cookie_rows(&req.headers, &req.cookies)?;
    request::validate_multipart_rows(&req)?;

    let method = req.method.trim().to_ascii_uppercase();
    if method != "GET" && method != "POST" {
        return Err("SSE stream은 GET 또는 POST만 지원합니다".to_string());
    }
    if req.url.len() > MAX_URL_BYTES {
        return Err("SSE 요청 URL이 너무 깁니다".to_string());
    }
    validate_template_request(&req)?;

    let sealer = crate::platform::platform_sealer();
    let (mut resolved, environment_secrets) = request::resolve_template(
        &RequestTemplate { method, ..req },
        &environment,
        sealer.as_ref(),
    )
    .map_err(|_| safe_secret_error())?;
    validate_resolved_request(&resolved)?;
    request::validate_cookie_configuration(&resolved)?;
    request::validate_multipart_configuration(&resolved)?;
    request::prepare_multipart_files(&mut resolved)?;
    let redactor = Redactor::for_request(&resolved, environment_secrets);
    let session_id = state.reserve()?;
    let task_state = Arc::clone(state.inner());
    let task_id = session_id.clone();
    let task = tauri::async_runtime::spawn(async move {
        let deadline = Instant::now() + Duration::from_millis(options.total_timeout_ms);
        let result = run_stream(
            app.clone(),
            task_id.clone(),
            resolved,
            redactor,
            options,
            deadline,
        )
        .await;
        if let Err(failure) = result {
            emit_update(
                &app,
                &task_id,
                "error",
                None,
                None,
                None,
                None,
                0,
                0,
                Some(failure.message),
                None,
            );
        }
        task_state.finish(&task_id);
    });
    if !state.attach(&session_id, task) {
        state.finish(&session_id);
        return Err("SSE stream을 시작할 수 없습니다".to_string());
    }
    Ok(session_id)
}

#[tauri::command]
pub fn stop_sse_stream(
    state: tauri::State<'_, Arc<SseState>>,
    session_id: String,
) -> Result<(), String> {
    if !is_valid_session_id(&session_id) {
        return Err("SSE stream 식별자가 올바르지 않습니다".to_string());
    }
    state.cancel(&session_id)
}

fn is_valid_session_id(value: &str) -> bool {
    value.len() <= 32
        && value.strip_prefix("sse-").is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn validate_options(options: &SseOptions) -> Result<(), String> {
    if !(MIN_CONNECT_TIMEOUT_MS..=MAX_CONNECT_TIMEOUT_MS).contains(&options.connect_timeout_ms) {
        return Err("SSE 연결 timeout 범위가 올바르지 않습니다".to_string());
    }
    if !(MIN_IDLE_TIMEOUT_MS..=MAX_IDLE_TIMEOUT_MS).contains(&options.idle_timeout_ms) {
        return Err("SSE idle timeout 범위가 올바르지 않습니다".to_string());
    }
    if !(MIN_TOTAL_TIMEOUT_MS..=MAX_TOTAL_TIMEOUT_MS).contains(&options.total_timeout_ms) {
        return Err("SSE 전체 timeout 범위가 올바르지 않습니다".to_string());
    }
    Ok(())
}

fn validate_environment(environment: &[EnvironmentVariable]) -> Result<(), String> {
    if environment.len() > MAX_ENVIRONMENT_VARIABLES {
        return Err("환경 변수는 최대 100개까지 사용할 수 있습니다".to_string());
    }
    if environment.iter().any(|variable| {
        variable.key.is_empty()
            || variable.key.len() > 128
            || variable.value.len() > MAX_HEADER_VALUE_BYTES
    }) {
        return Err("환경 변수 형식이 올바르지 않습니다".to_string());
    }
    Ok(())
}

fn validate_resolved_request(req: &ResolvedRequest) -> Result<(), String> {
    if !matches!(
        req.body_kind.as_str(),
        "none" | "json" | "form" | "multipart" | "raw"
    ) {
        return Err("SSE 요청 본문 형식이 올바르지 않습니다".to_string());
    }
    if req.headers.len() > MAX_REQUEST_HEADERS
        || req.cookies.len() > MAX_REQUEST_COOKIES
        || req.params.len() > MAX_REQUEST_PARAMS
    {
        return Err("SSE 요청 항목 수가 제한을 초과했습니다".to_string());
    }
    if req.body.len() > MAX_BODY_BYTES {
        return Err("SSE 요청 본문이 너무 큽니다".to_string());
    }
    if req.method.eq_ignore_ascii_case("GET")
        && (!req.body.trim().is_empty() || has_multipart_content(&req.multipart))
    {
        return Err("GET SSE 요청에는 본문을 사용할 수 없습니다".to_string());
    }
    if req.body_kind == "none" && !req.body.trim().is_empty() {
        return Err("SSE 요청 본문 형식이 올바르지 않습니다".to_string());
    }
    for header in &req.headers {
        if header.key.len() > MAX_HEADER_NAME_BYTES || header.value.len() > MAX_HEADER_VALUE_BYTES {
            return Err("SSE 요청 header가 너무 깁니다".to_string());
        }
    }
    for cookie in &req.cookies {
        if cookie.name.len() > MAX_HEADER_NAME_BYTES || cookie.value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err("SSE 요청 Cookie가 너무 깁니다".to_string());
        }
    }
    for parameter in &req.params {
        if parameter.key.len() > MAX_PARAMETER_BYTES || parameter.value.len() > MAX_PARAMETER_BYTES
        {
            return Err("SSE 요청 parameter가 너무 깁니다".to_string());
        }
    }
    if let Some(auth) = &req.auth {
        if !matches!(auth.kind.as_str(), "none" | "basic" | "bearer" | "apikey") {
            return Err("SSE 인증 설정이 올바르지 않습니다".to_string());
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
            return Err("SSE 인증 설정이 너무 깁니다".to_string());
        }
    }
    validate_sse_header_values(&req.headers, req.auth.as_ref())?;
    if req.body_kind == "multipart"
        && req
            .multipart
            .iter()
            .any(|part| part.file_path.len() > MAX_URL_BYTES)
    {
        return Err("SSE multipart 파일 경로가 너무 깁니다".to_string());
    }
    let initial = append_query(&req.url, &req.params);
    if initial.len() > MAX_URL_BYTES {
        return Err("SSE 요청 URL이 너무 깁니다".to_string());
    }
    let url = reqwest::Url::parse(&initial)
        .map_err(|_| "SSE 요청 URL이 올바르지 않습니다".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("SSE 요청 URL이 올바르지 않습니다".to_string());
    }
    Ok(())
}

fn validate_template_request(req: &RequestTemplate) -> Result<(), String> {
    if req.headers.len() > MAX_REQUEST_HEADERS
        || req.cookies.len() > MAX_REQUEST_COOKIES
        || req.params.len() > MAX_REQUEST_PARAMS
    {
        return Err("SSE 요청 항목 수가 제한을 초과했습니다".to_string());
    }
    if req.body.len() > MAX_BODY_BYTES {
        return Err("SSE 요청 본문이 너무 큽니다".to_string());
    }
    if req.method.eq_ignore_ascii_case("GET")
        && (!req.body.trim().is_empty() || has_multipart_content(&req.multipart))
    {
        return Err("GET SSE 요청에는 본문을 사용할 수 없습니다".to_string());
    }
    if req.body_kind == "none" && !req.body.trim().is_empty() {
        return Err("SSE 요청 본문 형식이 올바르지 않습니다".to_string());
    }
    for header in &req.headers {
        if header.key.len() > MAX_HEADER_NAME_BYTES || header.value.len() > MAX_HEADER_VALUE_BYTES {
            return Err("SSE 요청 header가 너무 깁니다".to_string());
        }
    }
    for cookie in &req.cookies {
        if cookie.name.len() > MAX_HEADER_NAME_BYTES || cookie.value.len() > MAX_HEADER_VALUE_BYTES
        {
            return Err("SSE 요청 Cookie가 너무 깁니다".to_string());
        }
    }
    for parameter in &req.params {
        if parameter.key.len() > MAX_PARAMETER_BYTES || parameter.value.len() > MAX_PARAMETER_BYTES
        {
            return Err("SSE 요청 parameter가 너무 깁니다".to_string());
        }
    }
    if let Some(auth) = &req.auth {
        if !matches!(auth.kind.as_str(), "none" | "basic" | "bearer" | "apikey") {
            return Err("SSE 인증 설정이 올바르지 않습니다".to_string());
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
            return Err("SSE 인증 설정이 너무 깁니다".to_string());
        }
    }
    validate_sse_header_values(&req.headers, req.auth.as_ref())?;
    if req.body_kind == "multipart"
        && req
            .multipart
            .iter()
            .any(|part| part.file_path.len() > MAX_URL_BYTES)
    {
        return Err("SSE multipart 파일 경로가 너무 깁니다".to_string());
    }
    Ok(())
}

fn has_multipart_content(parts: &[MultipartPart]) -> bool {
    parts.iter().any(|part| {
        part.enabled
            && (!part.name.is_empty()
                || !part.value.is_empty()
                || !part.file_path.is_empty()
                || !part.file_name.is_empty()
                || !part.content_type.is_empty())
    })
}

fn validate_sse_header_values(
    headers: &[RequestHeader],
    auth: Option<&AuthConfig>,
) -> Result<(), String> {
    for header in headers.iter().filter(|header| header.enabled) {
        if header.key.is_empty() {
            continue;
        }
        reqwest::header::HeaderName::from_bytes(header.key.as_bytes())
            .map_err(|_| "SSE 요청 header가 올바르지 않습니다")?;
        reqwest::header::HeaderValue::from_str(&header.value)
            .map_err(|_| "SSE 요청 header가 올바르지 않습니다")?;
    }
    if let Some(auth) = auth {
        match auth.kind.as_str() {
            "bearer" => {
                reqwest::header::HeaderValue::from_str(&format!("Bearer {}", auth.token))
                    .map_err(|_| "SSE 인증 설정이 올바르지 않습니다")?;
            }
            "apikey" if !auth.api_key.is_empty() => {
                reqwest::header::HeaderName::from_bytes(auth.api_key.as_bytes())
                    .map_err(|_| "SSE 인증 설정이 올바르지 않습니다")?;
                reqwest::header::HeaderValue::from_str(&auth.api_value)
                    .map_err(|_| "SSE 인증 설정이 올바르지 않습니다")?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct StreamFailure {
    message: &'static str,
    retryable: bool,
}

impl StreamFailure {
    const fn request() -> Self {
        Self {
            message: "SSE 요청을 보낼 수 없습니다",
            retryable: false,
        }
    }

    const fn timeout() -> Self {
        Self {
            message: "SSE stream 시간이 초과되었습니다",
            retryable: true,
        }
    }

    const fn transport() -> Self {
        Self {
            message: "SSE stream 연결에 실패했습니다",
            retryable: true,
        }
    }

    const fn response() -> Self {
        Self {
            message: "SSE 응답 형식이 아닙니다",
            retryable: false,
        }
    }

    const fn redirect() -> Self {
        Self {
            message: "SSE 리다이렉트 정책으로 요청을 차단했습니다",
            retryable: false,
        }
    }

    const fn parse() -> Self {
        Self {
            message: "SSE stream 데이터가 올바르지 않습니다",
            retryable: false,
        }
    }
}

async fn run_stream(
    app: tauri::AppHandle,
    session_id: String,
    req: ResolvedRequest,
    redactor: Redactor,
    options: SseOptions,
    deadline: Instant,
) -> Result<(), StreamFailure> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(options.connect_timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| StreamFailure::request())?;
    let mut attempts = 0u32;
    let mut retry_ms = DEFAULT_RETRY_MS;
    let mut redirect_count = 0usize;
    let mut total_decoded_bytes = 0usize;
    let mut sequence = 0u64;
    let mut history = EventBuffer::default();

    loop {
        if Instant::now() >= deadline {
            return Err(StreamFailure::timeout());
        }
        let response =
            open_connection(&client, &req, &redactor, deadline, &mut redirect_count).await?;
        emit_update(
            &app,
            &session_id,
            "connected",
            None,
            None,
            None,
            None,
            sequence,
            history.evicted(),
            None,
            Some(attempts),
        );

        let consume = consume_response(
            response,
            &app,
            &session_id,
            &redactor,
            &options,
            deadline,
            &mut total_decoded_bytes,
            &mut sequence,
            &mut retry_ms,
            &mut history,
        )
        .await;
        match consume {
            Ok(()) => {
                if !options.reconnect || attempts >= MAX_RECONNECT_ATTEMPTS {
                    emit_update(
                        &app,
                        &session_id,
                        "closed",
                        None,
                        None,
                        None,
                        None,
                        sequence,
                        history.evicted(),
                        None,
                        None,
                    );
                    return Ok(());
                }
            }
            Err(failure)
                if options.reconnect && failure.retryable && attempts < MAX_RECONNECT_ATTEMPTS =>
            {
                // Retry is opt-in, capped, and never forwards Last-Event-ID.  A small floor avoids
                // a server-provided `retry: 0` turning into a hot reconnect loop.
                if Instant::now() >= deadline {
                    return Err(StreamFailure::timeout());
                }
            }
            Err(failure) => return Err(failure),
        }

        attempts = attempts.saturating_add(1);
        let delay_ms = retry_ms.clamp(MIN_RETRY_DELAY_MS, crate::core::sse::MAX_RETRY_MS);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(StreamFailure::timeout());
        }
        tokio::time::sleep(Duration::from_millis(delay_ms).min(remaining)).await;
    }
}

async fn open_connection(
    client: &reqwest::Client,
    req: &ResolvedRequest,
    redactor: &Redactor,
    deadline: Instant,
    redirect_count: &mut usize,
) -> Result<reqwest::Response, StreamFailure> {
    let initial_url = append_query(&req.url, &req.params);
    let mut current_url =
        reqwest::Url::parse(&initial_url).map_err(|_| StreamFailure::request())?;
    let mut method =
        reqwest::Method::from_bytes(req.method.as_bytes()).map_err(|_| StreamFailure::request())?;
    let mut allow_sensitive = true;
    let mut include_body = true;

    loop {
        let mut builder = client.request(method.clone(), current_url.clone());
        for header in &req.headers {
            let key = header.key.trim();
            if !header.enabled
                || key.is_empty()
                || key.eq_ignore_ascii_case("accept")
                || key.eq_ignore_ascii_case("last-event-id")
                || !should_send_header(key, allow_sensitive)
                || (!include_body && is_body_header(key))
                || (!allow_sensitive && redactor.redact_text(&header.value) != header.value)
                || (req.body_kind == "multipart" && is_multipart_derived_header(key))
            {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(&header.value),
            ) {
                builder = builder.header(name, value);
            }
        }
        // SSE always requests the media type explicitly.  A user-supplied Accept is ignored so
        // two contradictory values cannot make a server return an unbounded ordinary response.
        builder = builder.header(reqwest::header::ACCEPT, "text/event-stream");
        if allow_sensitive {
            if let Some(cookie) = build_cookie_header(&req.cookies) {
                builder = builder.header(reqwest::header::COOKIE, cookie);
            }
            if let Some(auth) = &req.auth {
                match auth.kind.as_str() {
                    "basic" => {
                        builder = builder.basic_auth(&auth.username, Some(&auth.password));
                    }
                    "bearer" => {
                        builder = builder.bearer_auth(&auth.token);
                    }
                    "apikey" if !auth.api_key.is_empty() => {
                        let value = reqwest::header::HeaderValue::from_str(&auth.api_value)
                            .map_err(|_| StreamFailure::request())?;
                        builder = builder.header(auth.api_key.as_str(), value);
                    }
                    _ => {}
                }
            }
        }
        if include_body {
            builder = apply_body(builder, req)
                .await
                .map_err(|_| StreamFailure::request())?;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(StreamFailure::timeout());
        }
        let response = tokio::time::timeout(remaining, builder.send())
            .await
            .map_err(|_| StreamFailure::timeout())?
            .map_err(|error| {
                if error.is_timeout() {
                    StreamFailure::timeout()
                } else {
                    StreamFailure::transport()
                }
            })?;
        let status = response.status();
        if status.is_redirection() {
            if *redirect_count >= MAX_REDIRECTS {
                return Err(StreamFailure::redirect());
            }
            let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                return Err(StreamFailure::redirect());
            };
            let location = location.to_str().map_err(|_| StreamFailure::redirect())?;
            if location.len() > MAX_URL_BYTES {
                return Err(StreamFailure::redirect());
            }
            let next_url = current_url
                .join(location)
                .map_err(|_| StreamFailure::redirect())?;
            if next_url.as_str().len() > MAX_URL_BYTES {
                return Err(StreamFailure::redirect());
            }
            if !matches!(next_url.scheme(), "http" | "https")
                || next_url.host_str().is_none()
                || !next_url.username().is_empty()
                || next_url.password().is_some()
                || next_url.fragment().is_some()
            {
                return Err(StreamFailure::redirect());
            }
            if is_cross_origin(&current_url, &next_url) {
                if redactor.redact_url(next_url.as_str()) != next_url.as_str() {
                    return Err(StreamFailure::redirect());
                }
                allow_sensitive = false;
                include_body = false;
            }
            if redirect_switches_to_get(status.as_u16(), &method) {
                method = reqwest::Method::GET;
                include_body = false;
            }
            current_url = next_url;
            *redirect_count = (*redirect_count).saturating_add(1);
            continue;
        }
        if !status.is_success() {
            return Err(StreamFailure::response());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .unwrap_or("");
        if !content_type.eq_ignore_ascii_case("text/event-stream") {
            return Err(StreamFailure::response());
        }
        return Ok(response);
    }
}

#[allow(clippy::too_many_arguments)]
async fn consume_response(
    mut response: reqwest::Response,
    app: &tauri::AppHandle,
    session_id: &str,
    redactor: &Redactor,
    options: &SseOptions,
    deadline: Instant,
    total_decoded_bytes: &mut usize,
    sequence: &mut u64,
    retry_ms: &mut u64,
    history: &mut EventBuffer,
) -> Result<(), StreamFailure> {
    let mut parser = SseParser::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(StreamFailure::timeout());
        }
        let wait = remaining.min(Duration::from_millis(options.idle_timeout_ms));
        let chunk = tokio::time::timeout(wait, response.chunk())
            .await
            .map_err(|_| StreamFailure::timeout())?
            .map_err(|error| {
                if error.is_timeout() {
                    StreamFailure::timeout()
                } else {
                    StreamFailure::transport()
                }
            })?;
        let Some(chunk) = chunk else {
            let events = parser.finish().map_err(map_parse_error)?;
            for event in events {
                emit_event(app, session_id, redactor, event, sequence, history)?;
            }
            if let Some(value) = parser.retry_ms() {
                *retry_ms = value;
            }
            return Ok(());
        };
        *total_decoded_bytes = total_decoded_bytes
            .checked_add(chunk.len())
            .ok_or_else(StreamFailure::parse)?;
        if *total_decoded_bytes > MAX_DECODED_BYTES {
            return Err(StreamFailure::parse());
        }
        let events = parser.feed(&chunk).map_err(map_parse_error)?;
        for event in events {
            emit_event(app, session_id, redactor, event, sequence, history)?;
        }
        if let Some(value) = parser.retry_ms() {
            *retry_ms = value;
        }
    }
}

fn map_parse_error(_error: ParseError) -> StreamFailure {
    StreamFailure::parse()
}

fn emit_event(
    app: &tauri::AppHandle,
    session_id: &str,
    redactor: &Redactor,
    event: SseEvent,
    sequence: &mut u64,
    history: &mut EventBuffer,
) -> Result<(), StreamFailure> {
    let safe_event = redactor.redact_text(&event.event);
    let safe_data = redactor.redact_body(&event.data);
    let safe_id = event.id.map(|id| redactor.redact_text(&id));
    if safe_event.len() > MAX_EVENT_NAME_BYTES
        || safe_data.len() > MAX_EVENT_DATA_BYTES
        || safe_id
            .as_ref()
            .is_some_and(|id| id.len() > MAX_EVENT_ID_BYTES)
    {
        return Err(StreamFailure::parse());
    }
    let safe = SseEvent {
        event: safe_event,
        data: safe_data,
        id: safe_id,
        retry_ms: event.retry_ms,
    };
    history.push(safe.clone());
    *sequence = sequence.checked_add(1).ok_or_else(StreamFailure::parse)?;
    emit_update(
        app,
        session_id,
        "event",
        Some(safe.event),
        Some(safe.data),
        safe.id,
        safe.retry_ms,
        *sequence,
        history.evicted(),
        None,
        None,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_update(
    app: &tauri::AppHandle,
    session_id: &str,
    kind: &'static str,
    event: Option<String>,
    data: Option<String>,
    id: Option<String>,
    retry_ms: Option<u64>,
    sequence: u64,
    dropped: usize,
    message: Option<&'static str>,
    attempt: Option<u32>,
) {
    let update = SseUpdate {
        session_id: session_id.to_string(),
        kind,
        event,
        data,
        id,
        retry_ms,
        sequence,
        dropped,
        message,
        attempt,
    };
    let _ = app.emit(SSE_EVENT, update);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn options_are_bounded_and_reconnect_is_opt_in() {
        assert!(!SseOptions::default().reconnect);
        assert!(validate_options(&SseOptions::default()).is_ok());
        assert!(validate_options(&SseOptions {
            total_timeout_ms: MAX_TOTAL_TIMEOUT_MS + 1,
            ..SseOptions::default()
        })
        .is_err());
    }

    #[test]
    fn session_ids_are_opaque_and_bounded() {
        assert!(is_valid_session_id("sse-1"));
        assert!(!is_valid_session_id("sse-"));
        assert!(!is_valid_session_id("sse-1/path"));
        assert!(!is_valid_session_id("https://secret.example"));
    }

    #[test]
    fn early_task_finish_keeps_the_reservation_until_attach_or_cancel() {
        let state = SseState::default();
        let session = state.reserve().unwrap();
        state.finish(&session);
        assert!(state.active.lock().unwrap().is_some());
        assert!(state.reserve().is_err());
        state.cancel(&session).unwrap();
        assert!(state.active.lock().unwrap().is_none());
    }

    #[test]
    fn resolved_url_rejects_userinfo_and_unsafe_schemes() {
        let req = ResolvedRequest {
            method: "GET".into(),
            url: "https://user:pass@example.test/stream".into(),
            headers: vec![],
            cookies: vec![],
            multipart: vec![],
            params: vec![],
            body_kind: "none".into(),
            body: String::new(),
            auth: None,
            timeout_ms: 1_000,
            graphql: None,
        };
        assert!(validate_resolved_request(&req).is_err());
        let mut req = req;
        req.url = "file:///tmp/stream".into();
        assert!(validate_resolved_request(&req).is_err());
    }

    #[test]
    fn request_header_and_auth_syntax_fail_closed() {
        let headers = vec![RequestHeader {
            key: "X-Bad\nHeader".into(),
            value: "value".into(),
            enabled: true,
        }];
        assert!(validate_sse_header_values(&headers, None).is_err());
        let auth = AuthConfig {
            kind: "apikey".into(),
            api_key: "bad key".into(),
            ..AuthConfig::default()
        };
        assert!(validate_sse_header_values(&[], Some(&auth)).is_err());
    }

    #[test]
    fn get_rejects_multipart_content_that_is_not_stored_in_the_body_field() {
        let request = ResolvedRequest {
            method: "GET".into(),
            url: "https://example.test/stream".into(),
            headers: vec![],
            cookies: vec![],
            multipart: vec![MultipartPart {
                kind: "text".into(),
                name: "field".into(),
                value: "value".into(),
                file_path: String::new(),
                file_name: String::new(),
                content_type: String::new(),
                enabled: true,
            }],
            params: vec![],
            body_kind: "multipart".into(),
            body: String::new(),
            auth: None,
            timeout_ms: 1_000,
            graphql: None,
        };
        assert_eq!(
            validate_resolved_request(&request),
            Err("GET SSE 요청에는 본문을 사용할 수 없습니다".to_string())
        );
    }

    #[test]
    fn parser_fixture_covers_chunk_boundary_and_standard_fields() {
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        for chunk in [
            b"data: caf\xc3".as_slice(),
            b"\xa9\r\n\r\n",
            b"event: ping\r\n",
            b"data: ok\r\n\r\n",
        ] {
            events.extend(parser.feed(chunk).unwrap());
        }
        events.extend(parser.finish().unwrap());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "café");
        assert_eq!(events[1].event, "ping");
    }

    #[test]
    fn event_buffer_has_the_documented_limits() {
        let mut buffer = EventBuffer::default();
        for index in 0..=crate::core::sse::MAX_RETAINED_EVENTS {
            buffer.push(SseEvent {
                event: "message".into(),
                data: index.to_string(),
                id: None,
                retry_ms: None,
            });
        }
        assert_eq!(buffer.events().len(), crate::core::sse::MAX_RETAINED_EVENTS);
        assert_eq!(buffer.evicted(), 1);
    }

    #[test]
    fn native_loopback_streams_chunked_utf8_and_sse_metadata() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let observed = read_http_headers(&mut stream);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream; charset=utf-8\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            write_chunk(&mut stream, b"event: update\r\ndata: caf");
            write_chunk(&mut stream, b"\xc3\xa9\r\n");
            write_chunk(&mut stream, b"data: next\r\nid: 7\r\nretry: 1200\r\n\r\n");
            write_chunk(&mut stream, &[]);
            observed
        });

        let request = ResolvedRequest {
            method: "GET".into(),
            url: format!("http://127.0.0.1:{port}/stream"),
            headers: vec![RequestHeader {
                key: "Last-Event-ID".into(),
                value: "must-not-be-forwarded".into(),
                enabled: true,
            }],
            cookies: vec![],
            multipart: vec![],
            params: vec![],
            body_kind: "none".into(),
            body: String::new(),
            auth: None,
            timeout_ms: 1_000,
            graphql: None,
        };
        let redactor = Redactor::for_request(&request, vec![]);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let mut redirect_count = 0;
        let response = tauri::async_runtime::block_on(open_connection(
            &client,
            &request,
            &redactor,
            Instant::now() + Duration::from_secs(5),
            &mut redirect_count,
        ))
        .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let events = tauri::async_runtime::block_on(async move {
            let mut response = response;
            let mut parser = SseParser::new();
            let mut events = Vec::new();
            while let Some(chunk) = response.chunk().await.unwrap() {
                events.extend(parser.feed(&chunk).unwrap());
            }
            events.extend(parser.finish().unwrap());
            (events, parser.retry_ms())
        });
        let observed = server.join().unwrap();
        let lowered = observed.to_ascii_lowercase();
        assert!(lowered.contains("\r\naccept: text/event-stream"));
        assert!(!lowered.contains("last-event-id"));
        assert_eq!(events.0.len(), 1);
        assert_eq!(events.0[0].event, "update");
        assert_eq!(events.0[0].data, "café\nnext");
        assert_eq!(events.0[0].id.as_deref(), Some("7"));
        assert_eq!(events.0[0].retry_ms, Some(1_200));
        assert_eq!(events.1, Some(1_200));
    }

    #[test]
    fn event_redaction_masks_request_secrets_before_emit() {
        let request = ResolvedRequest {
            method: "GET".into(),
            url: "http://127.0.0.1/stream".into(),
            headers: vec![],
            cookies: vec![],
            multipart: vec![],
            params: vec![],
            body_kind: "none".into(),
            body: String::new(),
            auth: Some(AuthConfig {
                kind: "bearer".into(),
                token: "loopback-secret".into(),
                ..AuthConfig::default()
            }),
            timeout_ms: 1_000,
            graphql: None,
        };
        let redactor = Redactor::for_request(&request, vec![]);
        let safe_text = redactor.redact_text("echo=loopback-secret");
        let safe_body = redactor
            .redact_body(r#"{"echo":"loopback-secret","access_token":"server-issued-token"}"#);
        assert!(!safe_text.contains("loopback-secret"));
        assert!(!safe_body.contains("loopback-secret"));
        assert!(!safe_body.contains("server-issued-token"));
        assert!(safe_body.contains("[REDACTED]"));
    }

    fn read_http_headers(stream: &mut TcpStream) -> String {
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

    fn write_chunk(stream: &mut TcpStream, bytes: &[u8]) {
        write!(stream, "{:X}\r\n", bytes.len()).unwrap();
        stream.write_all(bytes).unwrap();
        stream.write_all(b"\r\n").unwrap();
        stream.flush().unwrap();
    }
}
