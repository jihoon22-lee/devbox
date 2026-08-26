use crate::platform::platform_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::Instant;
use zeroize::Zeroizing;

const REDACTED: &str = "[REDACTED]";
const MAX_REDIRECTS: usize = 10;
const MAX_REQUEST_HEADERS: usize = 100;
const MAX_REQUEST_COOKIES: usize = 100;
const MAX_MULTIPART_PARTS: usize = 50;
const MAX_MULTIPART_TEXT_BYTES: usize = 1_000_000;
const MAX_MULTIPART_FILE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_MULTIPART_TOTAL_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_RESPONSE_HEADERS: usize = 100;
const MAX_RESPONSE_HEADER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

fn default_header_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHeader {
    pub key: String,
    pub value: String,
    #[serde(default = "default_header_enabled")]
    pub enabled: bool,
}

impl Default for RequestHeader {
    fn default() -> Self {
        Self {
            key: String::new(),
            value: String::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestCookie {
    pub name: String,
    pub value: String,
    #[serde(default = "default_header_enabled")]
    pub enabled: bool,
}

impl Default for RequestCookie {
    fn default() -> Self {
        Self {
            name: String::new(),
            value: String::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultipartPart {
    pub kind: String,
    pub name: String,
    pub value: String,
    pub file_path: String,
    pub file_name: String,
    pub content_type: String,
    #[serde(default = "default_header_enabled")]
    pub enabled: bool,
}

impl Default for MultipartPart {
    fn default() -> Self {
        Self {
            kind: "text".to_string(),
            name: String::new(),
            value: String::new(),
            file_path: String::new(),
            file_name: String::new(),
            content_type: String::new(),
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub kind: String,
    pub username: String,
    pub password: String,
    pub token: String,
    pub api_key: String,
    pub api_value: String,
}

/// Frontend가 편집·저장하는 원본. 변수 참조는 해석되지 않은 상태다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestTemplate {
    pub method: String,
    pub url: String,
    pub headers: Vec<RequestHeader>,
    #[serde(default)]
    pub cookies: Vec<RequestCookie>,
    #[serde(default)]
    pub multipart: Vec<MultipartPart>,
    pub params: Vec<KeyValue>,
    /// none | json | form | multipart | raw
    pub body_kind: String,
    pub body: String,
    pub auth: Option<AuthConfig>,
    pub timeout_ms: u64,
}

/// 전송 직전 backend 메모리에만 존재하며 직렬화하지 않는다.
#[derive(Debug, Clone)]
struct ResolvedRequest {
    method: String,
    url: String,
    headers: Vec<RequestHeader>,
    cookies: Vec<RequestCookie>,
    multipart: Vec<MultipartPart>,
    params: Vec<KeyValue>,
    body_kind: String,
    body: String,
    auth: Option<AuthConfig>,
    timeout_ms: u64,
}

/// History v2의 wire 형식을 Rust 테스트에서도 고정한다.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedHistoryRequest {
    #[serde(flatten)]
    template: RequestTemplate,
    requires_secret_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub key: String,
    /// secret=true이면 DPAPI로 봉인된 base64 envelope다.
    pub value: String,
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedirectHop {
    pub status: u16,
    pub location: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseCookie {
    pub name: String,
    pub value: String,
    pub attributes: Vec<KeyValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<KeyValue>,
    pub duration_ms: u64,
    pub size_bytes: usize,
    pub body: String,
    pub is_json: bool,
    pub final_url: String,
    pub redirects: Vec<RedirectHop>,
    pub cookies: Vec<ResponseCookie>,
    pub response_id: Option<String>,
    pub raw_headers_available: bool,
    pub headers_truncated: bool,
}

type RawResponseHeader = (Zeroizing<String>, Zeroizing<String>);

struct ResponseHeaderEntry {
    id: String,
    // Serialize/Debug를 구현하지 않는다. 명시적인 일회성 복사 외 경계로 내보내지 않는다.
    raw_headers: Vec<RawResponseHeader>,
}

struct ResponseHeaderVaultInner {
    next_id: u64,
    current_request_id: Option<String>,
    entry: Option<ResponseHeaderEntry>,
}

/// 가장 최근 요청 1건의 bounded raw response headers만 process memory에 보관한다.
pub struct ResponseHeaderVault {
    inner: Mutex<ResponseHeaderVaultInner>,
}

impl Default for ResponseHeaderVault {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ResponseHeaderVaultInner {
                next_id: 1,
                current_request_id: None,
                entry: None,
            }),
        }
    }
}

impl ResponseHeaderVault {
    fn begin_request(&self) -> Result<String, String> {
        let mut inner = self.inner.lock().map_err(|_| response_copy_error())?;
        // 새 요청이 시작된 시점부터 이전 응답 원문은 어떤 오류 경로에서도 다시 읽히지 않는다.
        inner.current_request_id = None;
        inner.entry = None;
        let id = format!("response-{}", inner.next_id);
        inner.next_id = inner
            .next_id
            .checked_add(1)
            .ok_or_else(response_copy_error)?;
        inner.current_request_id = Some(id.clone());
        Ok(id)
    }

    fn store_if_current(
        &self,
        id: &str,
        raw_headers: Vec<RawResponseHeader>,
    ) -> Result<bool, String> {
        let mut inner = self.inner.lock().map_err(|_| response_copy_error())?;
        if inner.current_request_id.as_deref() != Some(id) {
            return Ok(false);
        }
        inner.entry = Some(ResponseHeaderEntry {
            id: id.to_string(),
            raw_headers,
        });
        Ok(true)
    }

    fn copy(&self, id: &str, cookies_only: bool) -> Result<String, String> {
        let inner = self.inner.lock().map_err(|_| response_copy_error())?;
        let entry = inner
            .entry
            .as_ref()
            .filter(|entry| entry.id == id)
            .ok_or_else(response_copy_error)?;
        let lines = entry
            .raw_headers
            .iter()
            .filter(|(name, _)| !cookies_only || name.as_str().eq_ignore_ascii_case("set-cookie"))
            .map(|(name, value)| format!("{}: {}", name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        if cookies_only && lines.is_empty() {
            return Err(response_copy_error());
        }
        Ok(lines.join("\n"))
    }
}

struct ExecutedResponse {
    response: ApiResponse,
    raw_headers: Vec<RawResponseHeader>,
}

/// HTTP 요청을 backend-only resolve 뒤 수행한다. resolved 값은 응답에 포함하지 않는다.
#[tauri::command]
pub async fn send_request(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
    response_headers: tauri::State<'_, ResponseHeaderVault>,
) -> Result<ApiResponse, String> {
    send_request_with_vault(req, environment, response_headers.inner()).await
}

async fn send_request_with_vault(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
    response_headers: &ResponseHeaderVault,
) -> Result<ApiResponse, String> {
    let response_id = response_headers.begin_request()?;
    validate_cookie_rows(&req.headers, &req.cookies)?;
    validate_multipart_rows(&req)?;
    let sealer = platform_sealer();
    let (mut resolved, environment_secrets) =
        resolve_template(&req, &environment, sealer.as_ref()).map_err(|_| safe_secret_error())?;
    validate_cookie_configuration(&resolved)?;
    validate_multipart_configuration(&resolved)?;
    prepare_multipart_files(&mut resolved)?;
    let redactor = Redactor::for_request(&resolved, environment_secrets);
    let mut executed = execute_request(resolved, &redactor).await?;
    if !executed.response.headers_truncated
        && response_headers.store_if_current(&response_id, executed.raw_headers)?
    {
        executed.response.response_id = Some(response_id);
        executed.response.raw_headers_available = true;
    }
    Ok(executed.response)
}

/// 확인된 현재 응답의 모든 원문 header를 한 번 복사할 때만 호출한다.
#[tauri::command]
pub fn copy_raw_response_headers(
    response_headers: tauri::State<'_, ResponseHeaderVault>,
    response_id: String,
) -> Result<String, String> {
    response_headers.copy(&response_id, false)
}

/// 확인된 현재 응답의 Set-Cookie 원문만 한 번 복사할 때 호출한다.
#[tauri::command]
pub fn copy_raw_response_cookies(
    response_headers: tauri::State<'_, ResponseHeaderVault>,
    response_id: String,
) -> Result<String, String> {
    response_headers.copy(&response_id, true)
}

/// 사용자가 확인한 일회성 원문 복사에만 사용한다. 호출자는 결과를 저장해서는 안 된다.
#[tauri::command]
pub fn build_revealed_curl(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
) -> Result<String, String> {
    validate_cookie_rows(&req.headers, &req.cookies)?;
    validate_multipart_rows(&req)?;
    let sealer = platform_sealer();
    let (mut resolved, _environment_secrets) =
        resolve_template(&req, &environment, sealer.as_ref()).map_err(|_| safe_secret_error())?;
    validate_cookie_configuration(&resolved)?;
    validate_multipart_configuration(&resolved)?;
    prepare_multipart_files(&mut resolved)?;
    Ok(build_curl(&resolved))
}

/// persistence 직전 현재 environment secret과 알려진 token 패턴을 제거한다.
#[tauri::command]
pub fn sanitize_persisted_json(
    serialized: String,
    environment: Vec<EnvironmentVariable>,
) -> Result<String, String> {
    let sealer = platform_sealer();
    sanitize_persisted_json_with_sealer(&serialized, &environment, sealer.as_ref())
        .map_err(|_| "민감정보 안전 저장 검증에 실패했습니다".to_string())
}

async fn execute_request(
    req: ResolvedRequest,
    redactor: &Redactor,
) -> Result<ExecutedResponse, String> {
    validate_cookie_configuration(&req)?;
    validate_multipart_configuration(&req)?;
    if req.body_kind == "json" && !req.body.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&req.body)
            .map_err(|_| "JSON 본문 형식이 올바르지 않습니다".to_string())?;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(req.timeout_ms.max(1000)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "HTTP 클라이언트를 준비하지 못했습니다".to_string())?;
    let mut method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| "HTTP 메서드가 올바르지 않습니다".to_string())?;
    let initial_url = append_query(&req.url, &req.params);
    let mut current_url = reqwest::Url::parse(&initial_url)
        .map_err(|_| "요청 URL이 올바르지 않습니다".to_string())?;
    let mut allow_sensitive = true;
    let mut include_body = true;
    let mut redirects = Vec::new();
    let started = Instant::now();
    let cookie_header = build_cookie_header(&req.cookies);

    for redirect_count in 0..=MAX_REDIRECTS {
        let mut builder = client.request(method.clone(), current_url.clone());
        for header in &req.headers {
            if !header.enabled
                || header.key.is_empty()
                || !should_send_header(&header.key, allow_sensitive)
                || !include_body && is_body_header(&header.key)
                || !allow_sensitive && redactor.redact_text(&header.value) != header.value
                || req.body_kind == "multipart" && is_multipart_derived_header(&header.key)
            {
                continue;
            }
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(header.key.as_bytes()),
                reqwest::header::HeaderValue::from_str(&header.value),
            ) {
                builder = builder.header(name, value);
            }
        }
        if allow_sensitive {
            if let Some(value) = &cookie_header {
                builder = builder.header(reqwest::header::COOKIE, value.as_str());
            }
        }
        if allow_sensitive {
            if let Some(auth) = &req.auth {
                match auth.kind.as_str() {
                    "basic" => builder = builder.basic_auth(&auth.username, Some(&auth.password)),
                    "bearer" => builder = builder.bearer_auth(&auth.token),
                    "apikey" if !auth.api_key.is_empty() => {
                        let value = reqwest::header::HeaderValue::from_str(&auth.api_value)
                            .map_err(|_| "API key 헤더 값이 올바르지 않습니다".to_string())?;
                        builder = builder.header(auth.api_key.as_str(), value);
                    }
                    _ => {}
                }
            }
        }
        if include_body {
            builder = apply_body(builder, &req).await?;
        }

        let response = builder.send().await.map_err(safe_request_error)?;
        let status = response.status();
        if status.is_redirection() {
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if redirect_count == MAX_REDIRECTS {
                    return Err("리다이렉트 횟수 제한을 초과했습니다".to_string());
                }
                let location = location
                    .to_str()
                    .map_err(|_| "리다이렉트 위치가 올바르지 않습니다".to_string())?;
                let next_url = current_url
                    .join(location)
                    .map_err(|_| "리다이렉트 위치가 올바르지 않습니다".to_string())?;
                let redacted_location = redactor.redact_url(next_url.as_str());
                let cross_origin = is_cross_origin(&current_url, &next_url);
                if cross_origin && redacted_location != next_url.as_str() {
                    return Err(safe_cross_origin_redirect_error());
                }
                redirects.push(RedirectHop {
                    status: status.as_u16(),
                    location: redacted_location,
                });
                if cross_origin {
                    allow_sensitive = false;
                    include_body = false;
                }
                if redirect_switches_to_get(status.as_u16(), &method) {
                    method = reqwest::Method::GET;
                    include_body = false;
                }
                current_url = next_url;
                continue;
            }
        }

        let captured_headers = capture_response_headers(response.headers(), redactor);
        let body = response
            .text()
            .await
            .map_err(|_| "응답 본문을 안전하게 읽지 못했습니다".to_string())?;
        let body = redactor.redact_body(&body);
        let is_json = captured_headers.masked.iter().any(|header| {
            header.key.eq_ignore_ascii_case("content-type") && header.value.contains("json")
        });
        return Ok(ExecutedResponse {
            response: ApiResponse {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or("").to_string(),
                headers: captured_headers.masked,
                duration_ms: started.elapsed().as_millis() as u64,
                size_bytes: body.len(),
                body,
                is_json,
                final_url: redactor.redact_url(current_url.as_str()),
                redirects,
                cookies: captured_headers.cookies,
                response_id: None,
                raw_headers_available: false,
                headers_truncated: captured_headers.truncated,
            },
            raw_headers: captured_headers.raw,
        });
    }

    Err("리다이렉트 처리에 실패했습니다".to_string())
}

async fn apply_body(
    builder: reqwest::RequestBuilder,
    req: &ResolvedRequest,
) -> Result<reqwest::RequestBuilder, String> {
    Ok(match req.body_kind.as_str() {
        "json" => builder
            .header("Content-Type", "application/json")
            .body(req.body.clone()),
        "form" => builder
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(encode_form(&req.body)),
        "multipart" => builder.multipart(build_multipart_form(req).await?),
        _ if !req.body.is_empty() => builder.body(req.body.clone()),
        _ => builder,
    })
}

async fn build_multipart_form(req: &ResolvedRequest) -> Result<reqwest::multipart::Form, String> {
    let mut form = reqwest::multipart::Form::new();
    for item in active_multipart_parts(&req.multipart) {
        let mut part = if item.kind == "file" {
            reqwest::multipart::Part::file(&item.file_path)
                .await
                .map_err(|_| "선택한 multipart 파일을 읽을 수 없습니다".to_string())?
        } else {
            reqwest::multipart::Part::text(item.value.clone())
        };
        if !item.content_type.is_empty() {
            part = part
                .mime_str(&item.content_type)
                .map_err(|_| "Content-Type은 type/subtype 형식이어야 합니다.".to_string())?;
        }
        form = form.part(item.name.clone(), part);
    }
    Ok(form)
}

fn resolve_template(
    req: &RequestTemplate,
    environment: &[EnvironmentVariable],
    sealer: &dyn devbox_secrets::Sealer,
) -> Result<(ResolvedRequest, Vec<Zeroizing<String>>), ()> {
    let referenced = referenced_variable_names(req);
    let mut values = HashMap::<String, Zeroizing<String>>::new();
    let mut environment_secrets = Vec::new();
    for variable in environment {
        if !referenced.contains(&variable.key) {
            continue;
        }
        let value = if variable.secret {
            let plaintext = unseal_environment_value(variable, sealer)?;
            environment_secrets.push(Zeroizing::new(plaintext.to_string()));
            plaintext
        } else {
            Zeroizing::new(variable.value.clone())
        };
        values.insert(variable.key.clone(), value);
    }

    let replace = |value: &str| replace_references(value, &values);
    Ok((
        ResolvedRequest {
            method: req.method.clone(),
            url: replace(&req.url),
            headers: req
                .headers
                .iter()
                .take(MAX_REQUEST_HEADERS)
                .map(|item| RequestHeader {
                    key: replace(&item.key),
                    value: replace(&item.value),
                    enabled: item.enabled,
                })
                .collect(),
            cookies: req
                .cookies
                .iter()
                .take(MAX_REQUEST_COOKIES)
                .map(|cookie| RequestCookie {
                    name: cookie.name.clone(),
                    value: replace(&cookie.value),
                    enabled: cookie.enabled,
                })
                .collect(),
            multipart: req
                .multipart
                .iter()
                .take(MAX_MULTIPART_PARTS)
                .map(|part| MultipartPart {
                    kind: part.kind.clone(),
                    name: part.name.clone(),
                    value: if part.kind == "text" {
                        replace(&part.value)
                    } else {
                        String::new()
                    },
                    file_path: if part.kind == "file" {
                        part.file_path.clone()
                    } else {
                        String::new()
                    },
                    file_name: part.file_name.clone(),
                    content_type: part.content_type.clone(),
                    enabled: part.enabled,
                })
                .collect(),
            params: req
                .params
                .iter()
                .map(|item| KeyValue {
                    key: replace(&item.key),
                    value: replace(&item.value),
                })
                .collect(),
            body_kind: req.body_kind.clone(),
            body: if req.body_kind == "multipart" {
                String::new()
            } else {
                replace(&req.body)
            },
            auth: req.auth.as_ref().map(|auth| AuthConfig {
                kind: auth.kind.clone(),
                username: replace(&auth.username),
                password: replace(&auth.password),
                token: replace(&auth.token),
                api_key: replace(&auth.api_key),
                api_value: replace(&auth.api_value),
            }),
            timeout_ms: req.timeout_ms,
        },
        environment_secrets,
    ))
}

fn unseal_environment_value(
    variable: &EnvironmentVariable,
    sealer: &dyn devbox_secrets::Sealer,
) -> Result<Zeroizing<String>, ()> {
    let blob = B64.decode(&variable.value).map_err(|_| ())?;
    devbox_secrets::unseal_v1(sealer, &blob).map_err(|_| ())
}

fn referenced_variable_names(req: &RequestTemplate) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut collect = |value: &str| {
        visit_references(value, |name| {
            names.insert(name.to_string());
        })
    };
    collect(&req.url);
    if req.body_kind != "multipart" {
        collect(&req.body);
    }
    for header in req
        .headers
        .iter()
        .take(MAX_REQUEST_HEADERS)
        .filter(|header| header.enabled)
    {
        collect(&header.key);
        collect(&header.value);
    }
    for cookie in req
        .cookies
        .iter()
        .take(MAX_REQUEST_COOKIES)
        .filter(|cookie| cookie.enabled)
    {
        collect(&cookie.value);
    }
    if req.body_kind == "multipart" {
        for part in active_multipart_parts(&req.multipart).filter(|part| part.kind == "text") {
            collect(&part.value);
        }
    }
    for param in &req.params {
        collect(&param.key);
        collect(&param.value);
    }
    if let Some(auth) = &req.auth {
        collect(&auth.username);
        collect(&auth.password);
        collect(&auth.token);
        collect(&auth.api_key);
        collect(&auth.api_value);
    }
    names
}

fn visit_references(value: &str, mut visitor: impl FnMut(&str)) {
    let mut rest = value;
    while !rest.is_empty() {
        let moustache = rest.find("{{").map(|index| (index, "}}", 2));
        let dollar = rest.find("${").map(|index| (index, "}", 2));
        let next = match (moustache, dollar) {
            (Some(left), Some(right)) if left.0 <= right.0 => Some(left),
            (Some(_), Some(right)) => Some(right),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        let Some((start, closing, opening_len)) = next else {
            break;
        };
        let after_open = &rest[start + opening_len..];
        let Some(end) = after_open.find(closing) else {
            break;
        };
        let name = after_open[..end].trim();
        if !name.is_empty() && name.chars().all(is_reference_char) {
            visitor(name);
        }
        rest = &after_open[end + closing.len()..];
    }
}

fn replace_references(value: &str, values: &HashMap<String, Zeroizing<String>>) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        let moustache = rest.find("{{").map(|index| (index, "}}", 2));
        let dollar = rest.find("${").map(|index| (index, "}", 2));
        let next = match (moustache, dollar) {
            (Some(left), Some(right)) if left.0 <= right.0 => Some(left),
            (Some(_), Some(right)) => Some(right),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        };
        let Some((start, closing, opening_len)) = next else {
            output.push_str(rest);
            break;
        };
        output.push_str(&rest[..start]);
        let after_open = &rest[start + opening_len..];
        let Some(end) = after_open.find(closing) else {
            output.push_str(&rest[start..]);
            break;
        };
        let name = after_open[..end].trim();
        if let Some(replacement) = values.get(name) {
            output.push_str(replacement.as_str());
        } else {
            output.push_str(&rest[start..start + opening_len + end + closing.len()]);
        }
        rest = &after_open[end + closing.len()..];
    }
    output
}

fn is_reference_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
}

struct Redactor {
    secrets: Vec<Zeroizing<String>>,
}

impl Redactor {
    fn for_request(req: &ResolvedRequest, mut environment_secrets: Vec<Zeroizing<String>>) -> Self {
        collect_request_secrets(req, &mut environment_secrets);
        environment_secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        environment_secrets.dedup_by(|left, right| left.as_str() == right.as_str());
        Self {
            secrets: environment_secrets,
        }
    }

    fn redact_text(&self, value: &str) -> String {
        redact_text(value, &self.secrets)
    }

    fn redact_url(&self, value: &str) -> String {
        redact_url(value, &self.secrets)
    }

    fn redact_body(&self, value: &str) -> String {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(value) {
            sanitize_json_value(&mut json, "", &self.secrets);
            serde_json::to_string(&json).unwrap_or_else(|_| REDACTED.to_string())
        } else {
            self.redact_text(value)
        }
    }
}

fn collect_request_secrets(req: &ResolvedRequest, secrets: &mut Vec<Zeroizing<String>>) {
    let mut push = |value: &str| {
        if !value.is_empty() {
            secrets.push(Zeroizing::new(value.to_string()));
        }
    };
    if let Some(auth) = &req.auth {
        push(&auth.username);
        push(&auth.password);
        push(&auth.token);
        push(&auth.api_value);
    }
    for header in req.headers.iter().filter(|header| header.enabled) {
        if is_sensitive_name(&header.key) {
            push(&header.value);
        }
    }
    for cookie in req.cookies.iter().filter(|cookie| cookie.enabled) {
        push(&cookie.value);
    }
    if req.body_kind == "multipart" {
        for part in active_multipart_parts(&req.multipart) {
            if part.kind == "text" && is_sensitive_name(&part.name) {
                push(&part.value);
            }
        }
    }
    for param in &req.params {
        if is_sensitive_name(&param.key) {
            push(&param.value);
        }
    }
    if let Ok(url) = reqwest::Url::parse(&req.url) {
        for (key, value) in url.query_pairs() {
            if is_sensitive_name(&key) {
                push(&value);
            }
        }
        push(url.password().unwrap_or(""));
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&req.body) {
        collect_json_secrets(&json, "", secrets);
    }
}

fn collect_json_secrets(
    value: &serde_json::Value,
    key: &str,
    secrets: &mut Vec<Zeroizing<String>>,
) {
    if is_sensitive_name(key) {
        if let Some(value) = value.as_str() {
            if !value.is_empty() {
                secrets.push(Zeroizing::new(value.to_string()));
            }
        }
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            for (child_key, child) in object {
                collect_json_secrets(child, child_key, secrets);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_json_secrets(child, "", secrets);
            }
        }
        _ => {}
    }
}

fn sanitize_persisted_json_with_sealer(
    serialized: &str,
    environment: &[EnvironmentVariable],
    sealer: &dyn devbox_secrets::Sealer,
) -> Result<String, ()> {
    let mut secrets = Vec::new();
    for variable in environment.iter().filter(|variable| variable.secret) {
        secrets.push(unseal_environment_value(variable, sealer)?);
    }
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    let mut value = serde_json::from_str::<serde_json::Value>(serialized).map_err(|_| ())?;
    sanitize_json_value(&mut value, "", &secrets);
    let sanitized = serde_json::to_string(&value).map_err(|_| ())?;
    if secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .any(|secret| sanitized.contains(secret.as_str()))
    {
        return Err(());
    }
    Ok(sanitized)
}

fn sanitize_json_value(value: &mut serde_json::Value, key: &str, secrets: &[Zeroizing<String>]) {
    // `requiresSecretReview` is persistence schema metadata, not a credential.
    // Preserve only its exact boolean wire shape; redact every other value under
    // the same name immediately so schema parsing fails closed without leakage.
    if key == "requiresSecretReview" {
        if value.is_boolean() {
            return;
        }
        *value = serde_json::Value::String(REDACTED.to_string());
        return;
    }
    if let serde_json::Value::Object(object) = value {
        let is_multipart_request =
            object.get("body_kind").and_then(serde_json::Value::as_str) == Some("multipart");
        if is_multipart_request {
            if let Some(body) = object.get_mut("body") {
                *body = serde_json::Value::String(String::new());
            }
        }
    }
    if key == "multipart" {
        sanitize_persisted_multipart(value, secrets);
        return;
    }
    if key == "cookies" {
        let serde_json::Value::Array(cookies) = value else {
            *value = serde_json::Value::String(REDACTED.to_string());
            return;
        };
        for cookie in cookies {
            let serde_json::Value::Object(object) = cookie else {
                *cookie = serde_json::Value::String(REDACTED.to_string());
                continue;
            };
            for (child_key, child) in object {
                if child_key == "value" {
                    match child.as_str() {
                        Some("") => {}
                        Some(text) if is_exact_reference(text) => {}
                        Some(_) => *child = serde_json::Value::String(REDACTED.to_string()),
                        None => *child = serde_json::Value::String(REDACTED.to_string()),
                    }
                } else {
                    sanitize_json_value(child, child_key, secrets);
                }
            }
        }
        return;
    }
    if is_sensitive_name(key) {
        if value.as_str().is_some_and(contains_reference) {
            return;
        }
        *value = serde_json::Value::String(REDACTED.to_string());
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            for (child_key, child) in object {
                sanitize_json_value(child, child_key, secrets);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                sanitize_json_value(child, "", secrets);
            }
        }
        serde_json::Value::String(text) => {
            if let Ok(mut nested) = serde_json::from_str::<serde_json::Value>(text) {
                if nested.is_object() || nested.is_array() {
                    sanitize_json_value(&mut nested, "", secrets);
                    *text = serde_json::to_string(&nested).unwrap_or_else(|_| REDACTED.to_string());
                    return;
                }
            }
            *text = redact_text(text, secrets);
        }
        _ => {}
    }
}

fn sanitize_persisted_multipart(value: &mut serde_json::Value, secrets: &[Zeroizing<String>]) {
    let serde_json::Value::Array(parts) = value else {
        *value = serde_json::Value::String(REDACTED.to_string());
        return;
    };
    for part in parts {
        let serde_json::Value::Object(object) = part else {
            *part = serde_json::Value::String(REDACTED.to_string());
            continue;
        };
        let kind = object
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let name = object
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut safe = serde_json::Map::new();
        for field in [
            "kind",
            "name",
            "value",
            "file_name",
            "content_type",
            "enabled",
        ] {
            let Some(mut child) = object.get(field).cloned() else {
                continue;
            };
            match field {
                "value" if kind == "file" => {
                    child = serde_json::Value::String(String::new());
                }
                "value" if is_sensitive_name(&name) => match child.as_str() {
                    Some("") => {}
                    Some(text) if is_exact_reference(text) => {}
                    _ => child = serde_json::Value::String(REDACTED.to_string()),
                },
                "file_name" => {
                    child = serde_json::Value::String(
                        child
                            .as_str()
                            .map(safe_file_name)
                            .map(|name| redact_text(&name, secrets))
                            .unwrap_or_else(|| REDACTED.to_string()),
                    );
                }
                _ => sanitize_json_value(&mut child, field, secrets),
            }
            safe.insert(field.to_string(), child);
        }
        safe.insert(
            "file_path".to_string(),
            serde_json::Value::String(String::new()),
        );
        *part = serde_json::Value::Object(safe);
    }
}

fn contains_reference(value: &str) -> bool {
    let mut found = false;
    visit_references(value, |_| found = true);
    found
}

fn is_exact_reference(value: &str) -> bool {
    let candidate = if value.starts_with("{{") && value.ends_with("}}") {
        &value[2..value.len().saturating_sub(2)]
    } else if value.starts_with("${") && value.ends_with('}') {
        &value[2..value.len().saturating_sub(1)]
    } else {
        return false;
    };
    let name = candidate.trim();
    !name.is_empty() && name.chars().all(is_reference_char)
}

fn redact_text(value: &str, secrets: &[Zeroizing<String>]) -> String {
    let mut redacted = value.to_string();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        redacted = redacted.replace(secret.as_str(), REDACTED);
    }
    redact_sensitive_assignments(&redact_known_token_patterns(&redacted))
}

fn redact_sensitive_assignments(value: &str) -> String {
    let mut output = value.to_string();
    for segment in value
        .split(|character: char| character.is_whitespace() || matches!(character, '&' | ',' | ';'))
    {
        let assignment = segment.split_once('=').or_else(|| segment.split_once(':'));
        let Some((raw_key, raw_value)) = assignment else {
            continue;
        };
        let key = raw_key.trim_matches(|character: char| {
            matches!(character, '"' | '\'' | '{' | '}' | '[' | ']')
        });
        if is_sensitive_name(key) && !contains_reference(raw_value) {
            let separator = if segment.contains('=') { '=' } else { ':' };
            output = output.replace(segment, &format!("{raw_key}{separator}{REDACTED}"));
        }
    }
    output
}

fn redact_known_token_patterns(value: &str) -> String {
    if value.contains("-----BEGIN PRIVATE KEY-----")
        || value.contains("-----BEGIN RSA PRIVATE KEY-----")
        || value.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        return REDACTED.to_string();
    }
    let mut output = value.to_string();
    let candidates = value
        .split(|character: char| {
            character.is_whitespace() || matches!(character, '"' | '\'' | '=' | ':' | ',' | '&')
        })
        .filter(|candidate| !candidate.is_empty());
    for candidate in candidates {
        if looks_like_secret(candidate) {
            output = output.replace(candidate, REDACTED);
        }
    }
    output
}

fn looks_like_secret(value: &str) -> bool {
    let prefixed = [
        "sk-",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxa-",
        "xoxp-",
        "xoxr-",
        "xoxs-",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix) && value.len() >= prefix.len() + 12);
    let aws = value.starts_with("AKIA")
        && value.len() == 20
        && value
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit());
    let jwt = value.split('.').count() == 3
        && value.split('.').all(|part| {
            part.len() >= 10
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        });
    prefixed || aws || jwt
}

fn redact_url(value: &str, secrets: &[Zeroizing<String>]) -> String {
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return redact_text(value, secrets);
    };
    let pairs = url
        .query_pairs()
        .map(|(key, value)| {
            let value = if is_sensitive_name(&key) {
                REDACTED.to_string()
            } else {
                redact_text(&value, secrets)
            };
            (key.into_owned(), value)
        })
        .collect::<Vec<_>>();
    if url.query().is_some() {
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    if !url.username().is_empty() {
        let _ = url.set_username("REDACTED");
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("REDACTED"));
    }
    redact_text(url.as_str(), secrets)
}

fn redact_header_value(name: &str, value: &str, redactor: &Redactor) -> String {
    if is_sensitive_name(name) {
        REDACTED.to_string()
    } else if name.eq_ignore_ascii_case("location") {
        redactor.redact_url(value)
    } else {
        redactor.redact_text(value)
    }
}

struct CapturedResponseHeaders {
    masked: Vec<KeyValue>,
    raw: Vec<RawResponseHeader>,
    cookies: Vec<ResponseCookie>,
    truncated: bool,
}

fn capture_response_headers(
    headers: &reqwest::header::HeaderMap,
    redactor: &Redactor,
) -> CapturedResponseHeaders {
    let mut captured = CapturedResponseHeaders {
        masked: Vec::new(),
        raw: Vec::new(),
        cookies: Vec::new(),
        truncated: false,
    };
    let mut remaining = MAX_RESPONSE_HEADER_BYTES;

    for (index, (name, value)) in headers.iter().enumerate() {
        if index >= MAX_RESPONSE_HEADERS {
            captured.truncated = true;
            break;
        }
        let raw_name = name.as_str();
        let display_name = redactor.redact_text(raw_name);
        let Ok(value) = value.to_str() else {
            captured.truncated = true;
            let unavailable = if is_sensitive_name(raw_name) {
                REDACTED
            } else {
                "[NON-TEXT HEADER]"
            };
            if raw_name.len() + unavailable.len() + 2 <= remaining {
                remaining -= raw_name.len() + unavailable.len() + 2;
                captured.masked.push(KeyValue {
                    key: display_name,
                    value: unavailable.to_string(),
                });
            }
            continue;
        };

        if raw_name.eq_ignore_ascii_case("set-cookie") {
            captured.cookies.push(response_cookie(value, redactor));
        }

        let line_bytes = raw_name.len() + value.len() + 2;
        if line_bytes > remaining {
            captured.truncated = true;
            let unavailable = if is_sensitive_name(raw_name) {
                REDACTED
            } else {
                "[TRUNCATED]"
            };
            if raw_name.len() + unavailable.len() + 2 <= remaining {
                captured.masked.push(KeyValue {
                    key: display_name,
                    value: unavailable.to_string(),
                });
            }
            break;
        }

        remaining -= line_bytes;
        captured.masked.push(KeyValue {
            key: display_name,
            value: redact_header_value(raw_name, value, redactor),
        });
        captured.raw.push((
            Zeroizing::new(raw_name.to_string()),
            Zeroizing::new(value.to_string()),
        ));
    }
    captured
}

fn response_cookie(value: &str, redactor: &Redactor) -> ResponseCookie {
    let mut segments = value.split(';');
    let name = segments
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(name, _)| name.trim())
        .unwrap_or("");
    let name = if is_http_token(name) && name.len() <= 120 {
        redactor.redact_text(name)
    } else {
        "(unparsed)".to_string()
    };
    let attributes = segments
        .take(20)
        .filter_map(|segment| {
            let segment = segment.trim();
            if segment.is_empty() {
                return None;
            }
            let (attribute, value) = segment
                .split_once('=')
                .map(|(attribute, value)| (attribute.trim(), Some(value.trim())))
                .unwrap_or((segment, None));
            if !is_http_token(attribute) || attribute.len() > 64 {
                return None;
            }
            let value = value.map_or_else(String::new, |value| {
                if matches!(
                    attribute.to_ascii_lowercase().as_str(),
                    "domain" | "path" | "expires" | "max-age" | "samesite" | "priority"
                ) {
                    bounded_cookie_attribute(&redactor.redact_text(value))
                } else {
                    REDACTED.to_string()
                }
            });
            Some(KeyValue {
                key: redactor.redact_text(attribute),
                value,
            })
        })
        .collect();
    ResponseCookie {
        name,
        value: REDACTED.to_string(),
        attributes,
    }
}

fn bounded_cookie_attribute(value: &str) -> String {
    let mut chars = value.chars();
    let bounded = chars.by_ref().take(256).collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace('_', "-");
    let compact = normalized.replace('-', "");
    compact.contains("authorization")
        || compact.contains("cookie")
        || compact.contains("apikey")
        || compact.contains("apivalue")
        || compact.contains("token")
        || compact.contains("secret")
        || compact.contains("password")
        || compact.contains("passwd")
        || compact.contains("privatekey")
        || compact.contains("username")
}

fn should_send_header(name: &str, allow_sensitive: bool) -> bool {
    allow_sensitive || !is_sensitive_name(name)
}

fn is_body_header(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace('_', "-");
    normalized.starts_with("content-")
        || matches!(
            normalized.as_str(),
            "transfer-encoding" | "trailer" | "expect" | "digest" | "repr-digest"
        )
}

fn is_multipart_derived_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().replace('_', "-").as_str(),
        "content-type" | "content-length" | "transfer-encoding"
    )
}

fn active_multipart_parts(parts: &[MultipartPart]) -> impl Iterator<Item = &MultipartPart> {
    parts
        .iter()
        .filter(|part| part.enabled && multipart_part_has_content(part))
}

fn multipart_part_has_content(part: &MultipartPart) -> bool {
    !part.name.is_empty()
        || !part.value.is_empty()
        || !part.file_path.is_empty()
        || !part.file_name.is_empty()
        || !part.content_type.is_empty()
}

fn validate_multipart_rows(req: &RequestTemplate) -> Result<(), String> {
    validate_multipart_parts(&req.body_kind, &req.multipart)
}

fn validate_multipart_configuration(req: &ResolvedRequest) -> Result<(), String> {
    validate_multipart_parts(&req.body_kind, &req.multipart)
}

fn validate_multipart_parts(body_kind: &str, parts: &[MultipartPart]) -> Result<(), String> {
    if body_kind != "multipart" {
        return Ok(());
    }
    if parts.len() > MAX_MULTIPART_PARTS {
        return Err("multipart는 최대 50개 part까지 사용할 수 있습니다.".to_string());
    }
    let mut text_bytes = 0usize;
    for part in active_multipart_parts(parts) {
        if !matches!(part.kind.as_str(), "text" | "file") {
            return Err("multipart part 종류가 올바르지 않습니다".to_string());
        }
        if part.name.is_empty() {
            return Err("part 이름이 필요합니다.".to_string());
        }
        if part.name.len() > 120 || !is_http_token(&part.name) {
            return Err("part 이름은 120자 이하의 HTTP token이어야 합니다.".to_string());
        }
        if !part.content_type.is_empty() && !is_valid_content_type(&part.content_type) {
            return Err("Content-Type은 type/subtype 형식이어야 합니다.".to_string());
        }
        if part.kind == "file" {
            if part.file_path.is_empty() {
                return Err("전송할 파일을 선택하세요.".to_string());
            }
        } else {
            text_bytes = text_bytes.saturating_add(part.value.len());
        }
    }
    if text_bytes > MAX_MULTIPART_TEXT_BYTES {
        return Err(
            "활성 text part 전체는 UTF-8 기준 1,000,000바이트 이하여야 합니다.".to_string(),
        );
    }
    Ok(())
}

fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_valid_content_type(value: &str) -> bool {
    if value.len() > 127 || value.bytes().any(|byte| byte.is_ascii_control()) {
        return false;
    }
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && !subtype.is_empty()
        && !subtype.contains('/')
        && kind.bytes().all(is_content_type_char)
        && subtype.bytes().all(is_content_type_char)
}

fn is_content_type_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

fn prepare_multipart_files(req: &mut ResolvedRequest) -> Result<(), String> {
    if req.body_kind != "multipart" {
        return Ok(());
    }
    let mut total = 0u64;
    for part in req
        .multipart
        .iter_mut()
        .filter(|part| part.enabled && part.kind == "file" && multipart_part_has_content(part))
    {
        let canonical = std::fs::canonicalize(&part.file_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "선택한 multipart 파일을 찾을 수 없습니다".to_string()
            } else {
                "선택한 multipart 파일을 읽을 수 없습니다".to_string()
            }
        })?;
        let metadata = std::fs::metadata(&canonical)
            .map_err(|_| "선택한 multipart 파일을 읽을 수 없습니다".to_string())?;
        if !metadata.is_file() {
            return Err("선택한 multipart 파일을 읽을 수 없습니다".to_string());
        }
        if metadata.len() > MAX_MULTIPART_FILE_BYTES {
            return Err("multipart 파일은 각각 25 MiB 이하여야 합니다".to_string());
        }
        total = total
            .checked_add(metadata.len())
            .ok_or_else(|| "multipart 파일 전체는 50 MiB 이하여야 합니다".to_string())?;
        if total > MAX_MULTIPART_TOTAL_FILE_BYTES {
            return Err("multipart 파일 전체는 50 MiB 이하여야 합니다".to_string());
        }
        part.file_path = canonical.to_string_lossy().into_owned();
        part.file_name = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .map(safe_file_name)
            .unwrap_or_else(|| "file".to_string());
    }
    Ok(())
}

fn safe_file_name(value: &str) -> String {
    value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|character| !character.is_control())
        .take(255)
        .collect()
}

fn validate_cookie_configuration(req: &ResolvedRequest) -> Result<(), String> {
    validate_cookie_rows(&req.headers, &req.cookies)
}

fn validate_cookie_rows(
    headers: &[RequestHeader],
    cookies: &[RequestCookie],
) -> Result<(), String> {
    if cookies.len() > MAX_REQUEST_COOKIES {
        return Err("Cookie는 최대 100행까지 사용할 수 있습니다".to_string());
    }
    let active = cookies
        .iter()
        .filter(|cookie| cookie.enabled && (!cookie.name.is_empty() || !cookie.value.is_empty()))
        .collect::<Vec<_>>();
    if !active.is_empty()
        && headers
            .iter()
            .any(|header| header.enabled && header.key.trim().eq_ignore_ascii_case("cookie"))
    {
        return Err("Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다".to_string());
    }
    for cookie in active {
        if !is_valid_cookie_name(&cookie.name) {
            return Err("Cookie 이름이 올바르지 않습니다".to_string());
        }
        if !is_valid_cookie_value(&cookie.value) {
            return Err("Cookie 값에 허용되지 않는 문자가 있습니다".to_string());
        }
    }
    Ok(())
}

fn is_valid_cookie_name(name: &str) -> bool {
    is_http_token(name)
}

fn is_valid_cookie_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e))
}

fn build_cookie_header(cookies: &[RequestCookie]) -> Option<String> {
    let value = cookies
        .iter()
        .filter(|cookie| {
            cookie.enabled
                && (!cookie.name.is_empty() || !cookie.value.is_empty())
                && is_valid_cookie_name(&cookie.name)
                && is_valid_cookie_value(&cookie.value)
        })
        .map(|cookie| format!("{}={}", cookie.name, cookie.value))
        .collect::<Vec<_>>()
        .join("; ");
    (!value.is_empty()).then_some(value)
}

fn is_cross_origin(from: &reqwest::Url, to: &reqwest::Url) -> bool {
    from.scheme() != to.scheme()
        || from.host_str() != to.host_str()
        || from.port_or_known_default() != to.port_or_known_default()
}

fn redirect_switches_to_get(status: u16, method: &reqwest::Method) -> bool {
    status == 303 && *method != reqwest::Method::HEAD
        || matches!(status, 301 | 302) && *method == reqwest::Method::POST
}

fn safe_secret_error() -> String {
    "요청에 필요한 secret을 안전하게 해제할 수 없습니다".to_string()
}

fn safe_cross_origin_redirect_error() -> String {
    "교차 출처 리다이렉트에 민감정보가 포함되어 요청을 차단했습니다".to_string()
}

fn safe_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "요청 시간이 초과되었습니다".to_string()
    } else {
        "요청 전송에 실패했습니다".to_string()
    }
}

fn response_copy_error() -> String {
    "현재 응답의 원문 header를 안전하게 복사할 수 없습니다".to_string()
}

fn append_query(url: &str, params: &[KeyValue]) -> String {
    let pairs = params
        .iter()
        .filter(|pair| !pair.key.is_empty())
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return url.to_string();
    }
    let query = pairs
        .iter()
        .map(|pair| format!("{}={}", pair.key, pair.value))
        .collect::<Vec<_>>()
        .join("&");
    if url.contains('?') {
        format!("{url}&{query}")
    } else {
        format!("{url}?{query}")
    }
}

fn encode_form(body: &str) -> String {
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            match line.split_once('=') {
                Some((key, value)) => Some(format!("{}={}", key.trim(), value.trim())),
                None => Some(line.to_string()),
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn build_curl(req: &ResolvedRequest) -> String {
    let url = append_query(&req.url, &req.params);
    let mut lines = vec![format!(
        "curl --request {} {}",
        req.method,
        shell_quote(&url)
    )];
    for header in req.headers.iter().filter(|header| {
        header.enabled
            && !header.key.is_empty()
            && !(req.body_kind == "multipart" && is_multipart_derived_header(&header.key))
    }) {
        lines.push(format!(
            "  --header {}",
            shell_quote(&format!("{}: {}", header.key, header.value))
        ));
    }
    if let Some(cookie_header) = build_cookie_header(&req.cookies) {
        lines.push(format!(
            "  --header {}",
            shell_quote(&format!("Cookie: {cookie_header}"))
        ));
    }
    if let Some(auth) = &req.auth {
        match auth.kind.as_str() {
            "basic" if !auth.username.is_empty() => lines.push(format!(
                "  --header {}",
                shell_quote(&format!(
                    "Authorization: Basic {}",
                    B64.encode(format!("{}:{}", auth.username, auth.password))
                ))
            )),
            "bearer" if !auth.token.is_empty() => lines.push(format!(
                "  --header {}",
                shell_quote(&format!("Authorization: Bearer {}", auth.token))
            )),
            "apikey" if !auth.api_key.is_empty() => lines.push(format!(
                "  --header {}",
                shell_quote(&format!("{}: {}", auth.api_key, auth.api_value))
            )),
            _ => {}
        }
    }
    if req.body_kind == "multipart" {
        for part in active_multipart_parts(&req.multipart) {
            let suffix = if part.content_type.is_empty() {
                String::new()
            } else {
                format!(";type={}", part.content_type)
            };
            let value = if part.kind == "file" {
                format!("@{}", curl_form_quote(&part.file_path))
            } else {
                curl_form_quote(&part.value)
            };
            lines.push(format!(
                "  --form {}",
                shell_quote(&format!("{}={value}{suffix}", part.name))
            ));
        }
    } else if req.body_kind != "none" && !req.body.is_empty() {
        lines.push(format!("  --data {}", shell_quote(&req.body)));
    }
    lines.join(" \\\n")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn curl_form_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_secrets::{SealError, Sealer};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    struct MockSealer;

    impl Sealer for MockSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            let mut bytes = plaintext.bytes().rev().collect::<Vec<_>>();
            bytes.push(0);
            Ok(bytes)
        }

        fn unseal(&self, blob: &[u8]) -> Result<Zeroizing<String>, SealError> {
            let trimmed = blob.strip_suffix(&[0]).unwrap_or(blob);
            Ok(Zeroizing::new(
                trimmed.iter().rev().map(|byte| *byte as char).collect(),
            ))
        }
    }

    fn template() -> RequestTemplate {
        RequestTemplate {
            method: "POST".into(),
            url: "https://example.com/api?token={{ TOKEN }}".into(),
            headers: vec![RequestHeader {
                key: "Authorization".into(),
                value: "Bearer ${TOKEN}".into(),
                enabled: true,
            }],
            cookies: vec![],
            multipart: vec![],
            params: vec![],
            body_kind: "json".into(),
            body: r#"{"password":"${TOKEN}"}"#.into(),
            auth: Some(AuthConfig {
                kind: "bearer".into(),
                token: "{{TOKEN}}".into(),
                ..Default::default()
            }),
            timeout_ms: 5_000,
        }
    }

    fn sealed_variable(key: &str, plaintext: &str) -> EnvironmentVariable {
        let blob = devbox_secrets::seal_v1(&MockSealer, plaintext).unwrap();
        EnvironmentVariable {
            key: key.into(),
            value: B64.encode(blob),
            secret: true,
        }
    }

    fn send_test(request: RequestTemplate) -> Result<ApiResponse, String> {
        let response_headers = ResponseHeaderVault::default();
        tauri::async_runtime::block_on(send_request_with_vault(request, vec![], &response_headers))
    }

    #[test]
    fn resolves_both_reference_styles_including_auth_only_in_backend() {
        let (resolved, secrets) = resolve_template(
            &template(),
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        assert!(resolved.url.contains("top-secret"));
        assert_eq!(resolved.auth.unwrap().token, "top-secret");
        assert_eq!(secrets[0].as_str(), "top-secret");
    }

    #[test]
    fn legacy_header_defaults_enabled_and_disabled_reference_is_not_unsealed() {
        let legacy: RequestHeader =
            serde_json::from_str(r#"{"key":"X-Legacy","value":"one"}"#).unwrap();
        assert!(legacy.enabled);

        let mut request = template();
        request.url = "https://example.com/api".into();
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = None;
        request.headers = vec![
            RequestHeader {
                key: "X-Trace".into(),
                value: "one".into(),
                enabled: true,
            },
            RequestHeader {
                key: "X-Trace".into(),
                value: "${TOKEN}".into(),
                enabled: true,
            },
            RequestHeader {
                key: "X-Skip".into(),
                value: "${BROKEN}".into(),
                enabled: false,
            },
        ];

        let (resolved, secrets) = resolve_template(
            &request,
            &[
                sealed_variable("TOKEN", "top-secret"),
                EnvironmentVariable {
                    key: "BROKEN".into(),
                    value: "not-base64".into(),
                    secret: true,
                },
            ],
            &MockSealer,
        )
        .unwrap();

        assert_eq!(resolved.headers.len(), 3);
        assert_eq!(resolved.headers[0].value, "one");
        assert_eq!(resolved.headers[1].value, "top-secret");
        assert_eq!(resolved.headers[2].value, "${BROKEN}");
        assert!(!resolved.headers[2].enabled);
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].as_str(), "top-secret");

        let curl = build_curl(&resolved);
        assert_eq!(curl.matches("X-Trace:").count(), 2);
        assert!(!curl.contains("X-Skip"));
        assert!(!curl.contains("BROKEN"));
    }

    #[test]
    fn cookie_defaults_and_backend_only_secret_resolution_are_safe() {
        let legacy_cookie: RequestCookie =
            serde_json::from_str(r#"{"name":"session","value":"one"}"#).unwrap();
        assert!(legacy_cookie.enabled);

        let legacy_template = serde_json::json!({
            "method": "GET",
            "url": "https://example.test",
            "headers": [],
            "params": [],
            "body_kind": "none",
            "body": "",
            "auth": null,
            "timeout_ms": 1000
        });
        let parsed: RequestTemplate = serde_json::from_value(legacy_template).unwrap();
        assert!(parsed.cookies.is_empty());
        assert!(parsed.multipart.is_empty());

        let mut request = template();
        request.url = "https://example.test".into();
        request.headers.clear();
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = None;
        request.cookies = vec![
            RequestCookie {
                name: "session".into(),
                value: "${COOKIE_TOKEN}".into(),
                enabled: true,
            },
            RequestCookie {
                name: "disabled".into(),
                value: "${BROKEN}".into(),
                enabled: false,
            },
        ];

        let (resolved, secrets) = resolve_template(
            &request,
            &[
                sealed_variable("COOKIE_TOKEN", "cookie-secret"),
                EnvironmentVariable {
                    key: "BROKEN".into(),
                    value: "not-base64".into(),
                    secret: true,
                },
            ],
            &MockSealer,
        )
        .unwrap();

        assert_eq!(resolved.cookies[0].value, "cookie-secret");
        assert_eq!(resolved.cookies[1].value, "${BROKEN}");
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].as_str(), "cookie-secret");
        assert_eq!(
            build_cookie_header(&resolved.cookies).as_deref(),
            Some("session=cookie-secret")
        );
        let curl = build_curl(&resolved);
        assert!(curl.contains("Cookie: session=cookie-secret"));
        assert!(!curl.contains("BROKEN"));

        let mut direct = resolved.clone();
        direct.cookies = vec![RequestCookie {
            name: "sid".into(),
            value: "cookie-only-secret".into(),
            enabled: true,
        }];
        let redactor = Redactor::for_request(&direct, vec![]);
        assert_eq!(
            redactor.redact_body(r#"{"echo":"cookie-only-secret"}"#),
            r#"{"echo":"[REDACTED]"}"#
        );
    }

    #[test]
    fn invalid_or_ambiguous_cookie_configuration_fails_closed() {
        let mut resolved = resolve_template(&template(), &[], &MockSealer).unwrap().0;
        resolved.headers.clear();
        resolved.cookies = vec![RequestCookie {
            name: "bad name".into(),
            value: "one".into(),
            enabled: true,
        }];
        assert_eq!(
            validate_cookie_configuration(&resolved).unwrap_err(),
            "Cookie 이름이 올바르지 않습니다"
        );

        resolved.cookies[0].name = "session".into();
        resolved.cookies[0].value = "bad;value".into();
        assert_eq!(
            validate_cookie_configuration(&resolved).unwrap_err(),
            "Cookie 값에 허용되지 않는 문자가 있습니다"
        );

        resolved.cookies[0].value = "one".into();
        resolved.headers.push(RequestHeader {
            key: "Cookie".into(),
            value: "legacy=two".into(),
            enabled: true,
        });
        assert_eq!(
            validate_cookie_configuration(&resolved).unwrap_err(),
            "Cookie header와 구조화 Cookie를 동시에 전송할 수 없습니다"
        );

        resolved.headers[0].enabled = false;
        assert!(validate_cookie_configuration(&resolved).is_ok());
        resolved.cookies = vec![RequestCookie::default(); MAX_REQUEST_COOKIES + 1];
        assert_eq!(
            validate_cookie_configuration(&resolved).unwrap_err(),
            "Cookie는 최대 100행까지 사용할 수 있습니다"
        );
    }

    #[test]
    fn corrupt_secret_fails_closed_without_ciphertext_fallback() {
        let result = resolve_template(
            &template(),
            &[EnvironmentVariable {
                key: "TOKEN".into(),
                value: "not-base64".into(),
                secret: true,
            }],
            &MockSealer,
        );
        assert!(result.is_err());
    }

    #[test]
    fn redacts_response_body_error_and_url_values() {
        let (resolved, secrets) = resolve_template(
            &template(),
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        let redactor = Redactor::for_request(&resolved, secrets);
        assert_eq!(
            redactor.redact_body(r#"{"echo":"top-secret","access_token":"other"}"#),
            r#"{"access_token":"[REDACTED]","echo":"[REDACTED]"}"#
        );
        assert!(!redactor
            .redact_text("failed top-secret")
            .contains("top-secret"));
        assert!(!redactor
            .redact_url("https://other.test/cb?token=top-secret")
            .contains("top-secret"));
        assert_eq!(
            redactor.redact_body("apiKey=server-issued"),
            "apiKey=[REDACTED]"
        );
        assert_eq!(
            redactor.redact_body(r#"{"apiKey":"server-issued"}"#),
            r#"{"apiKey":"[REDACTED]"}"#
        );
    }

    #[test]
    fn response_headers_and_cookie_rows_are_masked_while_raw_values_stay_in_vault() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.append(
            reqwest::header::SET_COOKIE,
            reqwest::header::HeaderValue::from_static("session=server-secret; HttpOnly"),
        );
        headers.append(
            reqwest::header::SET_COOKIE,
            reqwest::header::HeaderValue::from_static("theme=dark; Path=/; Custom=hidden"),
        );
        headers.insert(
            "x-trace",
            reqwest::header::HeaderValue::from_static("trace-ok"),
        );
        let captured = capture_response_headers(
            &headers,
            &Redactor {
                secrets: vec![Zeroizing::new("server-secret".to_string())],
            },
        );

        assert!(!captured.truncated);
        assert_eq!(
            captured
                .masked
                .iter()
                .filter(|header| header.key.eq_ignore_ascii_case("set-cookie"))
                .map(|header| header.value.as_str())
                .collect::<Vec<_>>(),
            vec![REDACTED, REDACTED]
        );
        assert_eq!(
            captured
                .cookies
                .iter()
                .map(|cookie| (cookie.name.as_str(), cookie.value.as_str()))
                .collect::<Vec<_>>(),
            vec![("session", REDACTED), ("theme", REDACTED)]
        );
        assert_eq!(captured.cookies[0].attributes[0].key, "HttpOnly");
        assert!(captured.cookies[0].attributes[0].value.is_empty());
        assert_eq!(captured.cookies[1].attributes[0].key, "Path");
        assert_eq!(captured.cookies[1].attributes[0].value, "/");
        assert_eq!(captured.cookies[1].attributes[1].key, "Custom");
        assert_eq!(captured.cookies[1].attributes[1].value, REDACTED);

        let vault = ResponseHeaderVault::default();
        let response_id = vault.begin_request().unwrap();
        assert!(vault.store_if_current(&response_id, captured.raw).unwrap());
        let raw_headers = vault.copy(&response_id, false).unwrap();
        let raw_cookies = vault.copy(&response_id, true).unwrap();
        assert!(raw_headers.contains("x-trace: trace-ok"));
        assert!(raw_cookies.contains("set-cookie: session=server-secret; HttpOnly"));
        assert!(raw_cookies.contains("set-cookie: theme=dark; Path=/; Custom=hidden"));
    }

    #[test]
    fn response_header_vault_rejects_stale_ids_and_capture_disables_raw_on_overflow() {
        let vault = ResponseHeaderVault::default();
        let stale = vault.begin_request().unwrap();
        let current = vault.begin_request().unwrap();
        assert!(!vault
            .store_if_current(
                &stale,
                vec![(
                    Zeroizing::new("x-old".into()),
                    Zeroizing::new("secret".into()),
                )],
            )
            .unwrap());
        assert!(vault.copy(&stale, false).is_err());
        assert!(vault
            .store_if_current(
                &current,
                vec![(
                    Zeroizing::new("x-new".into()),
                    Zeroizing::new("safe".into()),
                )],
            )
            .unwrap());
        assert_eq!(vault.copy(&current, false).unwrap(), "x-new: safe");

        let mut oversized = reqwest::header::HeaderMap::new();
        oversized.insert(
            "x-large",
            reqwest::header::HeaderValue::from_str(&"a".repeat(MAX_RESPONSE_HEADER_BYTES)).unwrap(),
        );
        let captured = capture_response_headers(
            &oversized,
            &Redactor {
                secrets: Vec::new(),
            },
        );
        assert!(captured.truncated);
        assert_eq!(captured.masked[0].value, "[TRUNCATED]");

        let retained = vault.begin_request().unwrap();
        assert!(vault
            .store_if_current(
                &retained,
                vec![(
                    Zeroizing::new("x-retained".into()),
                    Zeroizing::new("secret".into()),
                )],
            )
            .unwrap());
        vault.inner.lock().unwrap().next_id = u64::MAX;
        assert!(vault.begin_request().is_err());
        assert!(vault.copy(&retained, false).is_err());
    }

    #[test]
    fn persisted_json_removes_environment_secret_and_sensitive_literals() {
        let input = r#"{"body":"{\"password\":\"top-secret\",\"safe\":\"ok\"}","note":"top-secret","token":"direct"}"#;
        let output = sanitize_persisted_json_with_sealer(
            input,
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        assert!(!output.contains("top-secret"));
        assert!(!output.contains("direct"));
        assert!(output.contains(REDACTED));
    }

    #[test]
    fn persisted_json_preserves_only_boolean_secret_review_metadata() {
        let input = r#"{
            "requiresSecretReview": true,
            "request": {"requiresSecretReview": false},
            "invalidString": {"requiresSecretReview": "direct-secret"},
            "invalidReference": {"requiresSecretReview": "{{SECRET_REVIEW}}"},
            "invalidNumber": {"requiresSecretReview": 7},
            "secret": true
        }"#;
        let output = sanitize_persisted_json_with_sealer(input, &[], &MockSealer).unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(json["requiresSecretReview"], true);
        assert_eq!(json["request"]["requiresSecretReview"], false);
        assert_eq!(json["invalidString"]["requiresSecretReview"], REDACTED);
        assert_eq!(json["invalidReference"]["requiresSecretReview"], REDACTED);
        assert_eq!(json["invalidNumber"]["requiresSecretReview"], REDACTED);
        assert_eq!(json["secret"], REDACTED);
        assert!(!output.contains("direct-secret"));
    }

    #[test]
    fn persisted_history_display_name_survives_and_redacts_environment_secret() {
        let input = r#"{
            "version": 2,
            "history": [{
                "id": "history-1",
                "name": "deploy top-secret request",
                "saved_at": 1,
                "request": {
                    "method": "GET",
                    "url": "https://example.test/path",
                    "headers": [],
                    "params": [],
                    "body_kind": "none",
                    "body": "",
                    "auth": null,
                    "timeout_ms": 30000,
                    "requiresSecretReview": true
                }
            }]
        }"#;
        let output = sanitize_persisted_json_with_sealer(
            input,
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let entry = &json["history"][0];

        assert_eq!(entry["name"], format!("deploy {REDACTED} request"));
        assert_eq!(entry["request"]["requiresSecretReview"], true);
        assert!(!output.contains("top-secret"));
    }

    #[test]
    fn persisted_collection_wire_shape_survives_backend_sanitization() {
        let input = r#"{
            "version": 2,
            "collections": [{
                "id": "collection-1",
                "name": "protected request",
                "folder": "security",
                "saved_at": 1,
                "requiresSecretReview": true,
                "request": {
                    "method": "POST",
                    "url": "https://example.test/path",
                    "headers": [{"key": "Authorization", "value": "[REDACTED]"}],
                    "cookies": [
                        {"name": "session", "value": "direct-cookie", "enabled": true},
                        {"name": "token", "value": "${COOKIE_TOKEN}", "enabled": true},
                        {"name": "mixed", "value": "prefix-${COOKIE_TOKEN}", "enabled": true}
                    ],
                    "params": [],
                    "body_kind": "json",
                    "body": "{\"password\":\"top-secret\",\"safe\":\"ok\"}",
                    "auth": {
                        "kind": "bearer",
                        "username": "",
                        "password": "",
                        "token": "direct-token",
                        "api_key": "",
                        "api_value": ""
                    },
                    "timeout_ms": 30000,
                    "requiresSecretReview": true
                }
            }]
        }"#;
        let output = sanitize_persisted_json_with_sealer(
            input,
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let entry = &json["collections"][0];

        assert_eq!(json["version"], 2);
        assert_eq!(entry["requiresSecretReview"], true);
        assert!(entry["requiresSecretReview"].is_boolean());
        assert_eq!(entry["request"]["requiresSecretReview"], true);
        assert!(entry["request"]["requiresSecretReview"].is_boolean());
        assert_eq!(entry["request"]["headers"][0]["value"], REDACTED);
        assert_eq!(entry["request"]["cookies"][0]["value"], REDACTED);
        assert_eq!(entry["request"]["cookies"][1]["value"], "${COOKIE_TOKEN}");
        assert_eq!(entry["request"]["cookies"][2]["value"], REDACTED);
        assert_eq!(
            entry["request"]["body"],
            r#"{"password":"[REDACTED]","safe":"ok"}"#
        );
        assert!(!output.contains("direct-token"));
        assert!(!output.contains("direct-cookie"));
        assert!(!output.contains("top-secret"));
    }

    #[test]
    fn cross_origin_redirect_strips_sensitive_headers() {
        let same = reqwest::Url::parse("https://a.test/next").unwrap();
        let other = reqwest::Url::parse("https://b.test/next").unwrap();
        let origin = reqwest::Url::parse("https://a.test/start").unwrap();
        assert!(!is_cross_origin(&origin, &same));
        assert!(is_cross_origin(&origin, &other));
        assert!(should_send_header("Authorization", true));
        assert!(!should_send_header("Authorization", false));
        assert!(!should_send_header("Cookie", false));
        assert!(should_send_header("Accept", false));
    }

    #[test]
    fn body_headers_are_identified_for_body_suppression() {
        assert!(is_body_header("Content-Length"));
        assert!(is_body_header("content_type"));
        assert!(is_body_header("Transfer-Encoding"));
        assert!(is_body_header("Expect"));
        assert!(!is_body_header("Accept"));
        assert!(!is_body_header("X-Request-Id"));
    }

    #[test]
    fn redirect_method_rules_preserve_307_and_switch_post_302() {
        assert!(redirect_switches_to_get(302, &reqwest::Method::POST));
        assert!(redirect_switches_to_get(303, &reqwest::Method::PUT));
        assert!(!redirect_switches_to_get(307, &reqwest::Method::POST));
        assert!(!redirect_switches_to_get(302, &reqwest::Method::PUT));
    }

    #[test]
    fn revealed_curl_is_only_built_from_resolved_backend_request() {
        let (resolved, _) = resolve_template(
            &template(),
            &[sealed_variable("TOKEN", "top-secret")],
            &MockSealer,
        )
        .unwrap();
        let curl = build_curl(&resolved);
        assert!(curl.contains("top-secret"));
        assert!(curl.contains("Authorization: Bearer"));
        assert_eq!(
            curl_form_quote("C:\\tmp\\a;\"b\".txt"),
            "\"C:\\\\tmp\\\\a;\\\"b\\\".txt\""
        );
    }

    #[test]
    fn persisted_history_wire_shape_survives_backend_sanitization() {
        let persisted = PersistedHistoryRequest {
            template: template(),
            requires_secret_review: true,
        };
        let serialized = serde_json::json!({
            "version": 2,
            "history": [{
                "id": "history-1",
                "saved_at": 1,
                "request": persisted,
                "status": 200
            }]
        })
        .to_string();
        let output = sanitize_persisted_json_with_sealer(&serialized, &[], &MockSealer).unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let request = &json["history"][0]["request"];

        assert_eq!(json["version"], 2);
        assert_eq!(request["requiresSecretReview"], true);
        assert!(request["requiresSecretReview"].is_boolean());
        assert!(request.get("url").is_some());
    }

    #[test]
    fn append_query_and_form_keep_existing_behavior() {
        assert_eq!(
            append_query(
                "https://x.test?a=1",
                &[KeyValue {
                    key: "b".into(),
                    value: "2".into(),
                }]
            ),
            "https://x.test?a=1&b=2"
        );
        assert_eq!(
            encode_form("# comment\nname=John Doe\nage=30\n"),
            "name=John Doe&age=30"
        );
    }

    #[test]
    fn multipart_persistence_removes_paths_backup_body_and_sensitive_text() {
        let input = r#"{
            "version": 2,
            "history": [{
                "id": "history-1",
                "saved_at": 1,
                "request": {
                    "method": "POST",
                    "url": "https://example.test/upload",
                    "headers": [],
                    "cookies": [],
                    "multipart": [
                        {"kind":"file","name":"upload","value":"raw-bytes","file_path":"C:\\\\Users\\\\private\\\\report.txt","file_name":"C:\\\\Users\\\\private\\\\report.txt","content_type":"text/plain","enabled":true,"raw_backup":"raw-bytes"},
                        {"kind":"text","name":"token","value":"direct-secret","file_path":"","file_name":"","content_type":"","enabled":true},
                        {"kind":"text","name":"token","value":"${TOKEN}","file_path":"","file_name":"","content_type":"","enabled":true}
                    ],
                    "params": [],
                    "body_kind": "multipart",
                    "body": "raw-file-backup",
                    "auth": null,
                    "timeout_ms": 30000,
                    "requiresSecretReview": true
                }
            }]
        }"#;
        let output = sanitize_persisted_json_with_sealer(input, &[], &MockSealer).unwrap();
        let json: serde_json::Value = serde_json::from_str(&output).unwrap();
        let request = &json["history"][0]["request"];

        assert_eq!(request["body"], "");
        assert_eq!(request["multipart"][0]["file_path"], "");
        assert_eq!(request["multipart"][0]["file_name"], "report.txt");
        assert!(request["multipart"][0].get("raw_backup").is_none());
        assert_eq!(request["multipart"][0]["value"], "");
        assert_eq!(request["multipart"][1]["value"], REDACTED);
        assert_eq!(request["multipart"][2]["value"], "${TOKEN}");
        assert!(!output.contains("Users"));
        assert!(!output.contains("raw-file-backup"));
        assert!(!output.contains("raw-bytes"));
        assert!(!output.contains("direct-secret"));
    }

    #[test]
    fn missing_multipart_file_error_never_contains_path() {
        let missing = format!(
            "{}/devbox-missing-multipart-{}",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let mut request = template();
        request.url = "https://example.test/upload".into();
        request.headers.clear();
        request.cookies.clear();
        request.auth = None;
        request.body_kind = "multipart".into();
        request.body = "${BROKEN}".into();
        request.multipart = vec![MultipartPart {
            kind: "file".into(),
            name: "upload".into(),
            file_path: missing.clone(),
            file_name: "missing.bin".into(),
            ..Default::default()
        }];

        let error = send_test(request).unwrap_err();
        assert_eq!(error, "선택한 multipart 파일을 찾을 수 없습니다");
        assert!(!error.contains(&missing));
        assert!(!error.contains("devbox-missing"));
    }

    #[test]
    fn multipart_secret_resolution_ignores_disabled_parts() {
        let mut request = template();
        request.url = "https://example.test/upload".into();
        request.headers.clear();
        request.cookies.clear();
        request.auth = None;
        request.body_kind = "multipart".into();
        request.body.clear();
        request.multipart = vec![
            MultipartPart {
                name: "token".into(),
                value: "${TOKEN}".into(),
                ..Default::default()
            },
            MultipartPart {
                name: "skip".into(),
                value: "${BROKEN}".into(),
                enabled: false,
                ..Default::default()
            },
        ];
        let (resolved, secrets) = resolve_template(
            &request,
            &[
                sealed_variable("TOKEN", "multipart-secret"),
                EnvironmentVariable {
                    key: "BROKEN".into(),
                    value: "not-base64".into(),
                    secret: true,
                },
            ],
            &MockSealer,
        )
        .unwrap();

        assert_eq!(resolved.multipart[0].value, "multipart-secret");
        assert_eq!(resolved.multipart[1].value, "${BROKEN}");
        assert!(resolved.body.is_empty());
        assert_eq!(secrets.len(), 1);
    }

    #[test]
    fn live_request_streams_text_and_file_multipart_with_derived_boundary() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let file_path = std::env::temp_dir().join(format!(
            "devbox-multipart-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&file_path, b"file-content-unique").unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            )
            .unwrap();
            request
        });

        let mut request = template();
        request.url = format!("http://127.0.0.1:{port}/multipart");
        request.headers = vec![RequestHeader {
            key: "Content-Type".into(),
            value: "text/plain".into(),
            enabled: true,
        }];
        request.cookies.clear();
        request.auth = None;
        request.body_kind = "multipart".into();
        request.body.clear();
        request.multipart = vec![
            MultipartPart {
                name: "note".into(),
                value: "hello-multipart".into(),
                content_type: "text/plain".into(),
                ..Default::default()
            },
            MultipartPart {
                kind: "file".into(),
                name: "upload".into(),
                file_path: file_path.to_string_lossy().into_owned(),
                file_name: "ignored-name.bin".into(),
                content_type: "text/plain".into(),
                ..Default::default()
            },
        ];

        let result = send_test(request);
        let _ = std::fs::remove_file(&file_path);
        assert_eq!(result.unwrap().status, 200);
        let observed = server.join().unwrap();
        let lowered = observed.to_ascii_lowercase();
        assert!(lowered.contains("content-type: multipart/form-data; boundary="));
        assert!(!lowered.contains("content-type: text/plain\r\ncontent-length"));
        assert!(observed.contains("name=\"note\""));
        assert!(observed.contains("hello-multipart"));
        assert!(observed.contains("name=\"upload\""));
        assert!(observed.contains("file-content-unique"));
        assert!(observed.contains("Content-Type: text/plain"));
        assert!(!observed.contains("ignored-name.bin"));
    }

    #[test]
    fn live_request_sends_enabled_duplicate_headers_in_order_and_skips_disabled() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            )
            .unwrap();
            request
        });

        let mut request = template();
        request.method = "GET".into();
        request.url = format!("http://127.0.0.1:{port}/headers");
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = None;
        request.headers = vec![
            RequestHeader {
                key: "X-Trace".into(),
                value: "one".into(),
                enabled: true,
            },
            RequestHeader {
                key: "X-Trace".into(),
                value: "two".into(),
                enabled: true,
            },
            RequestHeader {
                key: "X-Skip".into(),
                value: "not-sent".into(),
                enabled: false,
            },
        ];

        let response = send_test(request).unwrap();
        assert_eq!(response.status, 200);
        let observed = server.join().unwrap();
        let trace_lines = observed
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("x-trace:"))
            .collect::<Vec<_>>();
        assert_eq!(trace_lines, vec!["x-trace: one", "x-trace: two"]);
        assert!(!observed.to_ascii_lowercase().contains("x-skip:"));
        assert!(!observed.contains("not-sent"));
    }

    #[test]
    fn live_request_builds_one_ordered_cookie_header_and_skips_disabled_rows() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
            )
            .unwrap();
            request
        });

        let mut request = template();
        request.method = "GET".into();
        request.url = format!("http://127.0.0.1:{port}/cookies");
        request.headers.clear();
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = None;
        request.cookies = vec![
            RequestCookie {
                name: "session".into(),
                value: "one".into(),
                enabled: true,
            },
            RequestCookie {
                name: "token".into(),
                value: "two".into(),
                enabled: true,
            },
            RequestCookie {
                name: "skip".into(),
                value: "not-sent".into(),
                enabled: false,
            },
        ];

        let response = send_test(request).unwrap();
        assert_eq!(response.status, 200);
        let observed = server.join().unwrap();
        let cookie_lines = observed
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("cookie:"))
            .collect::<Vec<_>>();
        assert_eq!(cookie_lines, vec!["cookie: session=one; token=two"]);
        assert!(!observed.contains("not-sent"));
    }

    #[test]
    fn live_cross_origin_redirect_strips_auth_and_redacts_every_return_path() {
        let redirect_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let destination_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_port = redirect_server.local_addr().unwrap().port();
        let destination_port = destination_server.local_addr().unwrap().port();
        let (observed_tx, observed_rx) = mpsc::channel();

        let destination = std::thread::spawn(move || {
            let (mut stream, _) = destination_server.accept().unwrap();
            let request = read_http_request(&mut stream);
            let lowered = request.to_ascii_lowercase();
            observed_tx
                .send((
                    lowered.contains("\r\nauthorization:"),
                    lowered.contains("\r\ncookie:"),
                ))
                .unwrap();
            let body = r#"{"echo":"cross-origin-secret","access_token":"server-token"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nSet-Cookie: sid=cross-origin-secret\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let redirect = std::thread::spawn(move || {
            let (mut stream, _) = redirect_server.accept().unwrap();
            let _ = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{destination_port}/finish?request_id=redirect-ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let mut request = template();
        request.method = "GET".into();
        request.url = format!("http://127.0.0.1:{redirect_port}/start");
        request.headers.clear();
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = Some(AuthConfig {
            kind: "bearer".into(),
            token: "cross-origin-secret".into(),
            ..Default::default()
        });
        request.cookies = vec![RequestCookie {
            name: "session".into(),
            value: "cross-origin-secret".into(),
            enabled: true,
        }];

        let response_headers = ResponseHeaderVault::default();
        let response = tauri::async_runtime::block_on(send_request_with_vault(
            request,
            vec![],
            &response_headers,
        ))
        .unwrap();
        let (has_auth, has_cookie) = observed_rx.recv().unwrap();
        assert!(!has_auth);
        assert!(!has_cookie);
        assert!(!response.body.contains("cross-origin-secret"));
        assert!(!response.body.contains("server-token"));
        assert_eq!(
            response
                .headers
                .iter()
                .find(|header| header.key.eq_ignore_ascii_case("set-cookie"))
                .unwrap()
                .value,
            REDACTED
        );
        assert_eq!(response.cookies.len(), 1);
        assert_eq!(response.cookies[0].name, "sid");
        assert_eq!(response.cookies[0].value, REDACTED);
        assert!(response.raw_headers_available);
        let response_id = response.response_id.as_deref().unwrap();
        let raw_cookies = response_headers.copy(response_id, true).unwrap();
        assert!(raw_cookies.contains("sid=cross-origin-secret"));
        assert!(!response.final_url.contains("cross-origin-secret"));
        assert!(!response.redirects[0]
            .location
            .contains("cross-origin-secret"));
        redirect.join().unwrap();
        destination.join().unwrap();
    }

    #[test]
    fn live_cross_origin_307_and_308_suppress_body_and_derived_secret_headers() {
        for status in [307, 308] {
            let redirect_server = TcpListener::bind("127.0.0.1:0").unwrap();
            let destination_server = TcpListener::bind("127.0.0.1:0").unwrap();
            let redirect_port = redirect_server.local_addr().unwrap().port();
            let destination_port = destination_server.local_addr().unwrap().port();

            let destination = std::thread::spawn(move || {
                let (mut stream, _) = destination_server.accept().unwrap();
                let request = read_http_request(&mut stream);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                )
                .unwrap();
                request
            });

            let redirect = std::thread::spawn(move || {
                let (mut stream, _) = redirect_server.accept().unwrap();
                let _ = read_http_request(&mut stream);
                write!(
                    stream,
                    "HTTP/1.1 {status} Redirect\r\nLocation: http://127.0.0.1:{destination_port}/finish\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });

            let body = "payload=cross-origin-secret";
            let mut request = template();
            request.url = format!("http://127.0.0.1:{redirect_port}/start");
            request.body_kind = "raw".into();
            request.body = body.into();
            request.headers = vec![
                RequestHeader {
                    key: "Cookie".into(),
                    value: "sid=cross-origin-secret".into(),
                    enabled: true,
                },
                RequestHeader {
                    key: "X-Api-Key".into(),
                    value: "cross-origin-secret".into(),
                    enabled: true,
                },
                RequestHeader {
                    key: "X-Debug".into(),
                    value: "cross-origin-secret".into(),
                    enabled: true,
                },
                RequestHeader {
                    key: "Content-Type".into(),
                    value: "text/plain".into(),
                    enabled: true,
                },
                RequestHeader {
                    key: "Content-Encoding".into(),
                    value: "identity".into(),
                    enabled: true,
                },
            ];
            request.auth = Some(AuthConfig {
                kind: "bearer".into(),
                token: "cross-origin-secret".into(),
                ..Default::default()
            });

            let response = send_test(request).unwrap();
            assert_eq!(response.status, 200);
            let observed = destination.join().unwrap();
            let observed_lower = observed.to_ascii_lowercase();
            assert!(observed.starts_with("POST /finish HTTP/1.1"));
            assert!(!observed.contains("cross-origin-secret"));
            assert!(!observed_lower.contains("\r\nauthorization:"));
            assert!(!observed_lower.contains("\r\ncookie:"));
            assert!(!observed_lower.contains("\r\nx-api-key:"));
            assert!(!observed_lower.contains("\r\nx-debug:"));
            assert!(!observed_lower.contains("\r\ncontent-type:"));
            assert!(!observed_lower.contains("\r\ncontent-encoding:"));
            redirect.join().unwrap();
        }
    }

    #[test]
    fn live_cross_origin_redirect_with_sensitive_destination_is_blocked() {
        let redirect_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let destination_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_port = redirect_server.local_addr().unwrap().port();
        let destination_port = destination_server.local_addr().unwrap().port();

        let redirect = std::thread::spawn(move || {
            let (mut stream, _) = redirect_server.accept().unwrap();
            let _ = read_http_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{destination_port}/finish?token=cross-origin-secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });

        let mut request = template();
        request.method = "GET".into();
        request.url = format!("http://127.0.0.1:{redirect_port}/start");
        request.headers.clear();
        request.body_kind = "none".into();
        request.body.clear();
        request.auth = Some(AuthConfig {
            kind: "bearer".into(),
            token: "cross-origin-secret".into(),
            ..Default::default()
        });

        let error = send_test(request).unwrap_err();
        assert_eq!(error, safe_cross_origin_redirect_error());
        assert!(!error.contains("cross-origin-secret"));
        assert!(!error.contains(&destination_port.to_string()));
        redirect.join().unwrap();

        destination_server.set_nonblocking(true).unwrap();
        let accept_error = destination_server.accept().unwrap_err();
        assert_eq!(accept_error.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn live_network_error_is_generic_and_contains_no_request_secret() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut request = template();
        request.url = format!("http://127.0.0.1:{port}/?token=network-secret");
        request.auth = None;
        let error = send_test(request).unwrap_err();
        assert_eq!(error, "요청 전송에 실패했습니다");
        assert!(!error.contains("network-secret"));
        assert!(!error.contains(&port.to_string()));
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
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
        let header_end = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4)
            .unwrap_or(bytes.len());
        let head = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        while bytes.len() < header_end + content_length {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}
