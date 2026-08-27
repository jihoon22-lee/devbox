//! API Playground's `api-request/v1` handoff receiver.
//!
//! A handoff is claimed into process memory, validated into the existing
//! `RequestTemplate`, and exposed to the renderer only as a preview.  Apply is
//! the only path that acknowledges (and therefore deletes) the shared claim;
//! Cancel restores it for another explicit attempt.  The claim token never
//! crosses the IPC boundary.

use super::request::{
    AuthConfig, KeyValue, MultipartPart, RequestCookie, RequestHeader, RequestTemplate,
};
use devbox_applink::{handoff_root_in, HandoffClaim, HandoffError, HandoffStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

pub const API_REQUEST_HANDOFF_KIND: &str = "api-request/v1";
pub const API_PLAYGROUND_APP_ID: &str = "api-playground";
pub const WEBHOOK_LAB_APP_ID: &str = "webhook-lab";
pub const MAX_PENDING_HANDOFFS: usize = 8;
pub const HANDOFF_INVALID_ERROR: &str = "handoff 요청을 사용할 수 없습니다";
pub const HANDOFF_EXPIRED_ERROR: &str = "handoff 요청이 만료되었거나 더 이상 사용할 수 없습니다";
pub const HANDOFF_BUSY_ERROR: &str =
    "handoff 요청이 다른 작업에서 사용 중입니다. 잠시 후 다시 시도하세요";
pub const HANDOFF_STORAGE_ERROR: &str = "handoff 저장소를 사용할 수 없습니다";
pub const HANDOFF_LEASE_ERROR: &str = "handoff 미리보기가 만료되었습니다. 다시 전달하세요";

const MAX_METHOD_CHARS: usize = 16;
const MAX_METHOD_BYTES: usize = 16;
const MAX_URL_CHARS: usize = 4_096;
const MAX_URL_BYTES: usize = 16_384;
const MAX_HEADERS: usize = 100;
const MAX_HEADER_NAME_CHARS: usize = 256;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_VALUE_CHARS: usize = 16_384;
const MAX_HEADER_VALUE_BYTES: usize = 65_536;
const MAX_HEADER_TOTAL_CHARS: usize = 64_000;
const MAX_HEADER_TOTAL_BYTES: usize = 256_000;
const MAX_BODY_CHARS: usize = 256_000;
const MAX_BODY_BYTES: usize = 1_024_000;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 10_000;
const MAX_JSON_STRING_BYTES: usize = 1_024 * 1024;

#[derive(Default)]
pub struct ApiHandoffState {
    claims: Mutex<HashMap<String, HandoffClaim>>,
}

impl ApiHandoffState {
    fn claims(&self) -> MutexGuard<'_, HashMap<String, HandoffClaim>> {
        self.claims
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApiRequestHeader {
    name: String,
    value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApiRequestPayload {
    method: String,
    url: String,
    headers: Vec<ApiRequestHeader>,
    body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRequestHandoffPreview {
    pub handoff_id: String,
    pub kind: String,
    pub producer_id: String,
    pub consumer_id: String,
    pub expires_at_ms: u64,
    pub request: RequestTemplate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenewApiRequestResult {
    pub lease_until_ms: u64,
}

/// Claim and validate an opaque handoff ID.  The returned preview contains no
/// raw credential and no claim token, so the renderer cannot acknowledge a
/// different request by forging IPC arguments.
#[tauri::command]
pub fn claim_api_request(
    state: tauri::State<'_, ApiHandoffState>,
    handoff_id: String,
) -> Result<ApiRequestHandoffPreview, String> {
    if !is_handoff_id(&handoff_id) {
        return Err(HANDOFF_INVALID_ERROR.to_string());
    }
    {
        let claims = state.claims();
        if claims.len() >= MAX_PENDING_HANDOFFS {
            return Err(HANDOFF_BUSY_ERROR.to_string());
        }
    }

    let store = handoff_store();
    let claim_now_ms = now_ms();
    let claim = store
        .claim(
            &handoff_id,
            API_REQUEST_HANDOFF_KIND,
            API_PLAYGROUND_APP_ID,
            claim_now_ms,
        )
        .map_err(map_handoff_error)?;
    if !claim_matches_route(&claim) {
        let _ = store.restore(&claim, API_PLAYGROUND_APP_ID, claim_now_ms);
        return Err(HANDOFF_INVALID_ERROR.to_string());
    }
    let request = match request_from_payload(&claim.envelope.payload) {
        Ok(request) => request,
        Err(_) => {
            let _ = store.restore(&claim, API_PLAYGROUND_APP_ID, claim_now_ms);
            return Err(HANDOFF_INVALID_ERROR.to_string());
        }
    };

    let preview = ApiRequestHandoffPreview {
        handoff_id: claim.envelope.id.clone(),
        kind: claim.envelope.kind.clone(),
        producer_id: claim.envelope.source_app.clone(),
        consumer_id: API_PLAYGROUND_APP_ID.to_string(),
        expires_at_ms: claim.envelope.expires_at_ms,
        request,
    };
    let mut claims = state.claims();
    if claims.len() >= MAX_PENDING_HANDOFFS {
        drop(claims);
        let _ = store.restore(&claim, API_PLAYGROUND_APP_ID, claim_now_ms);
        return Err(HANDOFF_BUSY_ERROR.to_string());
    }
    claims.insert(claim.envelope.id.clone(), claim);
    Ok(preview)
}

fn claim_matches_route(claim: &HandoffClaim) -> bool {
    claim.envelope.source_app == WEBHOOK_LAB_APP_ID
        && claim.envelope.target_app.as_deref() == Some(API_PLAYGROUND_APP_ID)
}

/// Renew the short preview lease without extending the envelope TTL.
#[tauri::command]
pub fn renew_api_request(
    state: tauri::State<'_, ApiHandoffState>,
    handoff_id: String,
) -> Result<RenewApiRequestResult, String> {
    let claim = get_claim(&state, &handoff_id)?;
    match handoff_store().renew(
        &claim,
        API_PLAYGROUND_APP_ID,
        now_ms(),
        devbox_applink::DEFAULT_CLAIM_LEASE_MS,
    ) {
        Ok(renewed) => {
            let lease_until_ms = renewed.lease_until_ms;
            let mut claims = state.claims();
            if claims
                .get(&handoff_id)
                .is_some_and(|current| current.claim_token == claim.claim_token)
            {
                claims.insert(handoff_id, renewed);
                Ok(RenewApiRequestResult { lease_until_ms })
            } else {
                Err(HANDOFF_INVALID_ERROR.to_string())
            }
        }
        Err(error) => {
            if matches!(
                error,
                HandoffError::Expired
                    | HandoffError::LeaseExpired
                    | HandoffError::Missing
                    | HandoffError::Corrupt
                    | HandoffError::TokenMismatch
            ) {
                remove_claim(&state, &handoff_id, &claim);
            }
            Err(map_handoff_error(error))
        }
    }
}

/// Acknowledge a validated preview and return the editable request.  The
/// shared claim is deleted only after token/lease validation succeeds.
#[tauri::command]
pub fn ack_api_request(
    state: tauri::State<'_, ApiHandoffState>,
    handoff_id: String,
) -> Result<RequestTemplate, String> {
    let claim = get_claim(&state, &handoff_id)?;
    let request = match request_from_payload(&claim.envelope.payload) {
        Ok(request) => request,
        Err(_) => {
            restore_after_invalid(&state, &claim);
            return Err(HANDOFF_INVALID_ERROR.to_string());
        }
    };
    match handoff_store().ack(&claim, API_PLAYGROUND_APP_ID, now_ms()) {
        Ok(()) => {
            remove_claim(&state, &handoff_id, &claim);
            Ok(request)
        }
        Err(error) => {
            if matches!(
                error,
                HandoffError::Expired
                    | HandoffError::LeaseExpired
                    | HandoffError::Missing
                    | HandoffError::Corrupt
                    | HandoffError::TokenMismatch
            ) {
                remove_claim(&state, &handoff_id, &claim);
            }
            Err(map_handoff_error(error))
        }
    }
}

/// Restore a preview after the user cancels.  Restore is idempotent for this
/// claim and leaves the pending envelope available until its expiry.
#[tauri::command]
pub fn restore_api_request(
    state: tauri::State<'_, ApiHandoffState>,
    handoff_id: String,
) -> Result<(), String> {
    let claim = get_claim(&state, &handoff_id)?;
    match handoff_store().restore(&claim, API_PLAYGROUND_APP_ID, now_ms()) {
        Ok(()) => {
            remove_claim(&state, &handoff_id, &claim);
            Ok(())
        }
        Err(error) => {
            if matches!(
                error,
                HandoffError::Expired
                    | HandoffError::LeaseExpired
                    | HandoffError::Missing
                    | HandoffError::Corrupt
                    | HandoffError::TokenMismatch
            ) {
                remove_claim(&state, &handoff_id, &claim);
            }
            Err(map_handoff_error(error))
        }
    }
}

fn get_claim(state: &ApiHandoffState, id: &str) -> Result<HandoffClaim, String> {
    if !is_handoff_id(id) {
        return Err(HANDOFF_INVALID_ERROR.to_string());
    }
    state
        .claims()
        .get(id)
        .cloned()
        .ok_or_else(|| HANDOFF_INVALID_ERROR.to_string())
}

fn remove_claim(state: &ApiHandoffState, id: &str, claim: &HandoffClaim) {
    let mut claims = state.claims();
    if claims
        .get(id)
        .is_some_and(|current| current.claim_token == claim.claim_token)
    {
        claims.remove(id);
    }
}

fn restore_after_invalid(state: &ApiHandoffState, claim: &HandoffClaim) {
    match handoff_store().restore(claim, API_PLAYGROUND_APP_ID, now_ms()) {
        Ok(())
        | Err(
            HandoffError::Expired
            | HandoffError::LeaseExpired
            | HandoffError::Missing
            | HandoffError::Corrupt
            | HandoffError::TokenMismatch,
        ) => remove_claim(state, &claim.envelope.id, claim),
        Err(_) => {}
    }
}

fn handoff_store() -> HandoffStore {
    HandoffStore::new(handoff_root_in(&devbox_integration::common_root()))
}

fn map_handoff_error(error: HandoffError) -> String {
    match error {
        HandoffError::Expired => HANDOFF_EXPIRED_ERROR.to_string(),
        HandoffError::LeaseExpired => HANDOFF_LEASE_ERROR.to_string(),
        HandoffError::AlreadyClaimed => HANDOFF_BUSY_ERROR.to_string(),
        HandoffError::WrongTarget | HandoffError::WrongKind => HANDOFF_INVALID_ERROR.to_string(),
        HandoffError::Missing
        | HandoffError::InvalidRequest
        | HandoffError::InvalidPayload
        | HandoffError::TooLarge
        | HandoffError::Corrupt
        | HandoffError::TokenMismatch => HANDOFF_INVALID_ERROR.to_string(),
        HandoffError::UnsafeStorage | HandoffError::Storage | HandoffError::RandomUnavailable => {
            HANDOFF_STORAGE_ERROR.to_string()
        }
    }
}

fn request_from_payload(payload: &Value) -> Result<RequestTemplate, &'static str> {
    let payload: ApiRequestPayload =
        serde_json::from_value(payload.clone()).map_err(|_| HANDOFF_INVALID_ERROR)?;
    let method = validate_method(&payload.method)?;
    validate_url(&payload.url)?;
    if payload.headers.len() > MAX_HEADERS {
        return Err(HANDOFF_INVALID_ERROR);
    }
    let mut headers = Vec::with_capacity(payload.headers.len());
    let mut total_chars: usize = 0;
    let mut total_bytes: usize = 0;
    for header in payload.headers {
        if !within(&header.name, MAX_HEADER_NAME_CHARS, MAX_HEADER_NAME_BYTES)
            || !is_http_token(&header.name)
            || !within(
                &header.value,
                MAX_HEADER_VALUE_CHARS,
                MAX_HEADER_VALUE_BYTES,
            )
            || has_control(&header.name)
            || has_control(&header.value)
        {
            return Err(HANDOFF_INVALID_ERROR);
        }
        if is_sensitive_name(&header.name)
            && !header.value.is_empty()
            && !is_secret_reference(&header.value)
        {
            return Err(HANDOFF_INVALID_ERROR);
        }
        total_chars = total_chars
            .checked_add(header.name.chars().count())
            .and_then(|total| total.checked_add(header.value.chars().count()))
            .ok_or(HANDOFF_INVALID_ERROR)?;
        total_bytes = total_bytes
            .checked_add(header.name.len())
            .and_then(|total| total.checked_add(header.value.len()))
            .ok_or(HANDOFF_INVALID_ERROR)?;
        if total_chars > MAX_HEADER_TOTAL_CHARS || total_bytes > MAX_HEADER_TOTAL_BYTES {
            return Err(HANDOFF_INVALID_ERROR);
        }
        headers.push(RequestHeader {
            key: header.name,
            value: header.value,
            enabled: true,
        });
    }

    validate_body(&payload.body)?;
    let body_kind = if payload.body.is_empty() {
        "none"
    } else if serde_json::from_str::<Value>(&payload.body).is_ok() {
        "json"
    } else {
        "raw"
    };
    Ok(RequestTemplate {
        method,
        url: payload.url,
        headers,
        cookies: Vec::<RequestCookie>::new(),
        multipart: Vec::<MultipartPart>::new(),
        params: Vec::<KeyValue>::new(),
        body_kind: body_kind.to_string(),
        body: payload.body,
        auth: Some(AuthConfig {
            kind: "none".to_string(),
            ..AuthConfig::default()
        }),
        timeout_ms: 10_000,
        graphql: None,
    })
}

fn validate_method(value: &str) -> Result<String, &'static str> {
    if !within(value, MAX_METHOD_CHARS, MAX_METHOD_BYTES) || !is_http_token(value) {
        return Err(HANDOFF_INVALID_ERROR);
    }
    Ok(value.to_ascii_uppercase())
}

fn validate_url(value: &str) -> Result<(), &'static str> {
    if !within(value, MAX_URL_CHARS, MAX_URL_BYTES)
        || value.is_empty()
        || has_control(value)
        || value.contains('\\')
        || value.contains('#')
    {
        return Err(HANDOFF_INVALID_ERROR);
    }
    if value.starts_with('/') {
        if value.starts_with("//") {
            return Err(HANDOFF_INVALID_ERROR);
        }
        let pathname = value.split_once('?').map_or(value, |(path, _)| path);
        validate_url_path(pathname)?;
        validate_query(value.split_once('?').map(|(_, query)| query))?;
        return Ok(());
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| HANDOFF_INVALID_ERROR)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(HANDOFF_INVALID_ERROR);
    }
    // Validate the raw path before `Url::parse` can normalize encoded dot
    // segments such as `%2e%2e` away from the value the user supplied.
    if let Some(path_start) = value.find("://").and_then(|scheme_end| {
        value[scheme_end + 3..]
            .find(['/', '?', '#'])
            .map(|offset| scheme_end + 3 + offset)
    }) {
        let path_end = value[path_start..]
            .find(['?', '#'])
            .map_or(value.len(), |offset| path_start + offset);
        validate_url_path(&value[path_start..path_end])?;
    }
    validate_query(parsed.query())?;
    Ok(())
}

fn validate_url_path(path: &str) -> Result<(), &'static str> {
    let decoded = percent_decode(path).ok_or(HANDOFF_INVALID_ERROR)?;
    if decoded.starts_with("//")
        || decoded.contains('\\')
        || decoded.chars().any(char::is_control)
        || decoded.split('/').any(|part| part == "." || part == "..")
    {
        return Err(HANDOFF_INVALID_ERROR);
    }
    Ok(())
}

fn validate_query(query: Option<&str>) -> Result<(), &'static str> {
    let Some(query) = query else {
        return Ok(());
    };
    for component in query.split('&') {
        if component.is_empty() {
            continue;
        }
        let (raw_key, raw_value) = component.split_once('=').unwrap_or((component, ""));
        let key = percent_decode(raw_key).ok_or(HANDOFF_INVALID_ERROR)?;
        let value = percent_decode(raw_value).ok_or(HANDOFF_INVALID_ERROR)?;
        if key.is_empty()
            || key.chars().any(char::is_control)
            || value.chars().any(char::is_control)
            || (is_sensitive_name(&key) && !value.is_empty() && !is_secret_reference(&value))
        {
            return Err(HANDOFF_INVALID_ERROR);
        }
    }
    Ok(())
}

fn validate_body(value: &str) -> Result<(), &'static str> {
    if !within(value, MAX_BODY_CHARS, MAX_BODY_BYTES) || value.contains('\0') {
        return Err(HANDOFF_INVALID_ERROR);
    }
    if value.is_empty() {
        return Ok(());
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        let mut nodes = 0;
        validate_json_value(&parsed, None, 0, &mut nodes)
    } else if has_raw_credential(value) {
        Err(HANDOFF_INVALID_ERROR)
    } else {
        Ok(())
    }
}

fn validate_json_value(
    value: &Value,
    field: Option<&str>,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), &'static str> {
    if depth > MAX_JSON_DEPTH || *nodes >= MAX_JSON_NODES {
        return Err(HANDOFF_INVALID_ERROR);
    }
    *nodes += 1;
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key.is_empty() || key.len() > 256 || has_control(key) {
                    return Err(HANDOFF_INVALID_ERROR);
                }
                if is_sensitive_name(key) && !is_safe_sensitive_value(child) {
                    return Err(HANDOFF_INVALID_ERROR);
                }
                validate_json_value(child, Some(key), depth + 1, nodes)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                validate_json_value(child, field, depth + 1, nodes)?;
            }
        }
        Value::String(text) => {
            if text.len() > MAX_JSON_STRING_BYTES
                || has_raw_credential(text)
                || (field.is_some_and(is_sensitive_name)
                    && !text.is_empty()
                    && !is_secret_reference(text))
            {
                return Err(HANDOFF_INVALID_ERROR);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

fn is_safe_sensitive_value(value: &Value) -> bool {
    value.is_null()
        || value
            .as_str()
            .is_some_and(|text| text.is_empty() || is_secret_reference(text))
}

fn has_raw_credential(value: &str) -> bool {
    let lower = value.trim_start().to_ascii_lowercase();
    if lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || value.trim_start().starts_with("sk-")
        || value.contains("-----BEGIN ") && value.contains("PRIVATE KEY-----")
    {
        return true;
    }
    if has_unsafe_sensitive_assignment(value) {
        return true;
    }
    let mut jwt_parts = value.trim().split('.');
    jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_some_and(|part| part.len() >= 8)
        && jwt_parts.next().is_none()
}

fn has_unsafe_sensitive_assignment(value: &str) -> bool {
    value
        .split(['&', ',', ';', '?', '\n', '\r', '\t'])
        .any(|segment| {
            segment.char_indices().any(|(operator, character)| {
                if !matches!(character, '=' | ':') {
                    return false;
                }
                let before = &segment[..operator];
                let key_end = before.trim_end().len();
                let key_start = before[..key_end]
                    .char_indices()
                    .rev()
                    .find(|(_, character)| {
                        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
                    })
                    .map_or(0, |(index, character)| index + character.len_utf8());
                let key = &before[key_start..key_end];
                let raw_value = segment[operator + character.len_utf8()..].trim_start();
                is_sensitive_name(key)
                    && !raw_value.is_empty()
                    && !is_safe_assignment_reference(raw_value)
            })
        })
}

fn is_safe_assignment_reference(raw_value: &str) -> bool {
    let value = raw_value.trim();
    let Some(quote) = value
        .chars()
        .next()
        .filter(|character| *character == '"' || *character == '\'')
    else {
        return is_secret_reference(value.trim_end_matches(','));
    };
    let quote_width = quote.len_utf8();
    let rest = &value[quote_width..];
    let Some(end) = rest.find(quote) else {
        return false;
    };
    let candidate = &rest[..end];
    let trailing = rest[end + quote_width..].trim();
    trailing
        .chars()
        .all(|character| matches!(character, '}' | ']' | ','))
        && is_secret_reference(candidate)
}

fn is_secret_reference(value: &str) -> bool {
    let Some(name) = value
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 128
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        })
}

fn is_sensitive_name(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        compact.as_str(),
        "authorization"
            | "proxyauthorization"
            | "cookie"
            | "setcookie"
            | "password"
            | "passwd"
            | "secret"
            | "secrets"
            | "secretkey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "credential"
            | "credentials"
            | "apikey"
            | "xapikey"
            | "xauth"
            | "accesskey"
            | "clientsecret"
            | "privatekey"
            | "auth"
    ) || [
        "authorization",
        "cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "credential",
        "apikey",
        "privatekey",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

fn is_handoff_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn within(value: &str, max_chars: usize, max_bytes: usize) -> bool {
    value.chars().count() <= max_chars && value.len() <= max_bytes
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
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

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_digit)?;
            let low = bytes.get(index + 2).copied().and_then(hex_digit)?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::{CreateHandoff, HandoffStore, DEFAULT_HANDOFF_TTL_MS};
    use tempfile::tempdir;

    fn payload() -> Value {
        serde_json::json!({
            "method": "POST",
            "url": "/hooks/push?access_token=${WEBHOOK_SECRET}",
            "headers": [
                { "name": "Authorization", "value": "${WEBHOOK_SECRET}" },
                { "name": "Content-Type", "value": "application/json" }
            ],
            "body": "{\"token\":\"${WEBHOOK_SECRET}\",\"ok\":true}"
        })
    }

    #[test]
    fn parses_bounded_payload_without_resolving_secret_or_url() {
        let request = request_from_payload(&payload()).unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.url, "/hooks/push?access_token=${WEBHOOK_SECRET}");
        assert_eq!(request.headers[0].value, "${WEBHOOK_SECRET}");
        assert_eq!(request.body_kind, "json");
        assert!(serde_json::to_string(&request)
            .unwrap()
            .contains("${WEBHOOK_SECRET}"));
    }

    #[test]
    fn raw_sensitive_values_and_unsafe_targets_are_rejected() {
        let mut raw = payload();
        raw["headers"][0]["value"] = Value::String("Bearer raw-secret".into());
        assert!(matches!(
            request_from_payload(&raw),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut unsafe_target = payload();
        unsafe_target["url"] = Value::String("/hooks/../secret".into());
        assert!(matches!(
            request_from_payload(&unsafe_target),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut raw_origin_query = payload();
        raw_origin_query["url"] = Value::String("/hooks/push?access_token=raw-secret".into());
        assert!(matches!(
            request_from_payload(&raw_origin_query),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut encoded_absolute_path = payload();
        encoded_absolute_path["url"] =
            Value::String("https://example.test/hooks/%2e%2e/private".into());
        assert!(matches!(
            request_from_payload(&encoded_absolute_path),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut raw_named_token = payload();
        raw_named_token["headers"][0]["name"] = Value::String("X-Client-Token".into());
        raw_named_token["headers"][0]["value"] = Value::String("opaque-raw-token".into());
        assert!(matches!(
            request_from_payload(&raw_named_token),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut raw_x_auth = payload();
        raw_x_auth["headers"][0]["name"] = Value::String("X-Auth".into());
        raw_x_auth["headers"][0]["value"] = Value::String("opaque-raw-auth".into());
        assert!(matches!(
            request_from_payload(&raw_x_auth),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut raw_body_assignment = payload();
        raw_body_assignment["body"] = Value::String("token=opaque-raw-token".into());
        assert!(matches!(
            request_from_payload(&raw_body_assignment),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut spaced_raw_body_assignment = payload();
        spaced_raw_body_assignment["body"] =
            Value::String("mode=test token = opaque-raw-token".into());
        assert!(matches!(
            request_from_payload(&spaced_raw_body_assignment),
            Err(HANDOFF_INVALID_ERROR)
        ));

        let mut spaced_reference = payload();
        spaced_reference["body"] = Value::String("mode=test token = ${WEBHOOK_SECRET}".into());
        assert!(request_from_payload(&spaced_reference).is_ok());
    }

    #[test]
    fn handoff_id_is_bounded_before_store_access() {
        assert!(is_handoff_id("0123456789abcdef0123456789abcdef"));
        assert!(!is_handoff_id("0123456789abcdef0123456789abcde"));
        assert!(!is_handoff_id("0123456789ABCDEF0123456789abcdef"));
        assert!(!is_handoff_id(&"x".repeat(1024)));
    }

    #[test]
    fn handoff_store_claim_preview_ack_deletes_once() {
        let directory = tempdir().unwrap();
        let store = HandoffStore::new(directory.path().join("handoff/v1"));
        let descriptor = store
            .create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: "webhook-lab".into(),
                    target_app: Some(API_PLAYGROUND_APP_ID.into()),
                    payload: payload(),
                },
                1_000,
            )
            .unwrap();
        let claim = store
            .claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                API_PLAYGROUND_APP_ID,
                1_001,
            )
            .unwrap();
        let request = request_from_payload(&claim.envelope.payload).unwrap();
        assert_eq!(request.method, "POST");
        store.ack(&claim, API_PLAYGROUND_APP_ID, 1_002).unwrap();
        assert_eq!(
            store.claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                API_PLAYGROUND_APP_ID,
                1_003
            ),
            Err(HandoffError::Missing)
        );
        const { assert!(DEFAULT_HANDOFF_TTL_MS > 0) };
    }

    #[test]
    fn receiver_accepts_only_the_webhook_lab_route() {
        let directory = tempdir().unwrap();
        let store = HandoffStore::new(directory.path().join("handoff/v1"));
        let descriptor = store
            .create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: WEBHOOK_LAB_APP_ID.into(),
                    target_app: Some(API_PLAYGROUND_APP_ID.into()),
                    payload: payload(),
                },
                1_000,
            )
            .unwrap();
        let mut claim = store
            .claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                API_PLAYGROUND_APP_ID,
                1_001,
            )
            .unwrap();
        assert!(claim_matches_route(&claim));
        claim.envelope.source_app = "developer-toolbox".into();
        assert!(!claim_matches_route(&claim));
        claim.envelope.source_app = WEBHOOK_LAB_APP_ID.into();
        claim.envelope.target_app = Some("knowledge-base".into());
        assert!(!claim_matches_route(&claim));
    }

    #[test]
    fn api_preview_renewal_is_capped_by_the_envelope_ttl() {
        let directory = tempdir().unwrap();
        let store = HandoffStore::new(directory.path().join("handoff/v1"));
        let descriptor = store
            .create_with_ttl(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: WEBHOOK_LAB_APP_ID.into(),
                    target_app: Some(API_PLAYGROUND_APP_ID.into()),
                    payload: payload(),
                },
                1_000,
                90_000,
            )
            .unwrap();
        let claim = store
            .claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                API_PLAYGROUND_APP_ID,
                2_000,
            )
            .unwrap();
        let renewed = store
            .renew(
                &claim,
                API_PLAYGROUND_APP_ID,
                50_000,
                devbox_applink::DEFAULT_CLAIM_LEASE_MS,
            )
            .unwrap();
        assert_eq!(renewed.lease_until_ms, 91_000);
        store.ack(&renewed, API_PLAYGROUND_APP_ID, 90_999).unwrap();
    }
}
