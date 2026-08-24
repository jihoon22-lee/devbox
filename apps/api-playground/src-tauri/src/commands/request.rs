use crate::platform::platform_sealer;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;
use zeroize::Zeroizing;

const REDACTED: &str = "[REDACTED]";
const MAX_REDIRECTS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
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
    pub headers: Vec<KeyValue>,
    pub params: Vec<KeyValue>,
    /// none | json | form | raw
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
    headers: Vec<KeyValue>,
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
}

/// HTTP 요청을 backend-only resolve 뒤 수행한다. resolved 값은 응답에 포함하지 않는다.
#[tauri::command]
pub async fn send_request(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
) -> Result<ApiResponse, String> {
    let sealer = platform_sealer();
    let (resolved, environment_secrets) =
        resolve_template(&req, &environment, sealer.as_ref()).map_err(|_| safe_secret_error())?;
    let redactor = Redactor::for_request(&resolved, environment_secrets);
    execute_request(resolved, &redactor).await
}

/// 사용자가 확인한 일회성 원문 복사에만 사용한다. 호출자는 결과를 저장해서는 안 된다.
#[tauri::command]
pub fn build_revealed_curl(
    req: RequestTemplate,
    environment: Vec<EnvironmentVariable>,
) -> Result<String, String> {
    let sealer = platform_sealer();
    let (resolved, _environment_secrets) =
        resolve_template(&req, &environment, sealer.as_ref()).map_err(|_| safe_secret_error())?;
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

async fn execute_request(req: ResolvedRequest, redactor: &Redactor) -> Result<ApiResponse, String> {
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

    for redirect_count in 0..=MAX_REDIRECTS {
        let mut builder = client.request(method.clone(), current_url.clone());
        for header in &req.headers {
            if header.key.is_empty() || !should_send_header(&header.key, allow_sensitive) {
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
            builder = apply_body(builder, &req);
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
                redirects.push(RedirectHop {
                    status: status.as_u16(),
                    location: redactor.redact_url(next_url.as_str()),
                });
                if is_cross_origin(&current_url, &next_url) {
                    allow_sensitive = false;
                }
                if redirect_switches_to_get(status.as_u16(), &method) {
                    method = reqwest::Method::GET;
                    include_body = false;
                }
                current_url = next_url;
                continue;
            }
        }

        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| KeyValue {
                key: name.to_string(),
                value: redact_header_value(name.as_str(), value.to_str().unwrap_or(""), redactor),
            })
            .collect::<Vec<_>>();
        let body = response
            .text()
            .await
            .map_err(|_| "응답 본문을 안전하게 읽지 못했습니다".to_string())?;
        let body = redactor.redact_body(&body);
        let is_json = headers.iter().any(|header| {
            header.key.eq_ignore_ascii_case("content-type") && header.value.contains("json")
        });
        return Ok(ApiResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").to_string(),
            headers,
            duration_ms: started.elapsed().as_millis() as u64,
            size_bytes: body.len(),
            body,
            is_json,
            final_url: redactor.redact_url(current_url.as_str()),
            redirects,
        });
    }

    Err("리다이렉트 처리에 실패했습니다".to_string())
}

fn apply_body(builder: reqwest::RequestBuilder, req: &ResolvedRequest) -> reqwest::RequestBuilder {
    match req.body_kind.as_str() {
        "json" => builder
            .header("Content-Type", "application/json")
            .body(req.body.clone()),
        "form" => builder
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(encode_form(&req.body)),
        _ if !req.body.is_empty() => builder.body(req.body.clone()),
        _ => builder,
    }
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
                .map(|item| KeyValue {
                    key: replace(&item.key),
                    value: replace(&item.value),
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
            body: replace(&req.body),
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
    collect(&req.body);
    for pair in req.headers.iter().chain(req.params.iter()) {
        collect(&pair.key);
        collect(&pair.value);
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
    for header in &req.headers {
        if is_sensitive_name(&header.key) {
            push(&header.value);
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

fn contains_reference(value: &str) -> bool {
    let mut found = false;
    visit_references(value, |_| found = true);
    found
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

fn safe_request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "요청 시간이 초과되었습니다".to_string()
    } else {
        "요청 전송에 실패했습니다".to_string()
    }
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
    for header in req.headers.iter().filter(|header| !header.key.is_empty()) {
        lines.push(format!(
            "  --header {}",
            shell_quote(&format!("{}: {}", header.key, header.value))
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
    if req.body_kind != "none" && !req.body.is_empty() {
        lines.push(format!("  --data {}", shell_quote(&req.body)));
    }
    lines.join(" \\\n")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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
            headers: vec![KeyValue {
                key: "Authorization".into(),
                value: "Bearer ${TOKEN}".into(),
            }],
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
    }

    #[test]
    fn persisted_history_wire_shape_is_distinct() {
        let persisted = PersistedHistoryRequest {
            template: template(),
            requires_secret_review: true,
        };
        let json = serde_json::to_value(persisted).unwrap();
        assert_eq!(json["requiresSecretReview"], true);
        assert!(json.get("url").is_some());
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
    fn live_cross_origin_redirect_strips_auth_and_redacts_every_return_path() {
        let redirect_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let destination_server = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_port = redirect_server.local_addr().unwrap().port();
        let destination_port = destination_server.local_addr().unwrap().port();
        let (observed_tx, observed_rx) = mpsc::channel();

        let destination = std::thread::spawn(move || {
            let (mut stream, _) = destination_server.accept().unwrap();
            let request = read_http_head(&mut stream);
            observed_tx
                .send(request.to_ascii_lowercase().contains("\r\nauthorization:"))
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
            let _ = read_http_head(&mut stream);
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

        let response = tauri::async_runtime::block_on(send_request(request, vec![])).unwrap();
        assert!(!observed_rx.recv().unwrap());
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
        assert!(!response.final_url.contains("cross-origin-secret"));
        assert!(!response.redirects[0]
            .location
            .contains("cross-origin-secret"));
        redirect.join().unwrap();
        destination.join().unwrap();
    }

    #[test]
    fn live_network_error_is_generic_and_contains_no_request_secret() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut request = template();
        request.url = format!("http://127.0.0.1:{port}/?token=network-secret");
        request.auth = None;
        let error = tauri::async_runtime::block_on(send_request(request, vec![])).unwrap_err();
        assert_eq!(error, "요청 전송에 실패했습니다");
        assert!(!error.contains("network-secret"));
        assert!(!error.contains(&port.to_string()));
    }

    fn read_http_head(stream: &mut std::net::TcpStream) -> String {
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
}
