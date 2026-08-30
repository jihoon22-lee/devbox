use crate::{contains_sensitive_value, redact_handoff_text, validate_handoff_text};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const WEBHOOK_LOG_HANDOFF_KIND: &str = "webhook-log/v1";
pub const WEBHOOK_LOG_SOURCE_APP: &str = "webhook-lab";
pub const WEBHOOK_LOG_TARGET_APP: &str = "log-lens";
pub const WEBHOOK_LOG_SCHEMA_VERSION: u32 = 1;
pub const WEBHOOK_LOG_MAX_PAYLOAD_BYTES: usize = 16 * 1024;
pub const WEBHOOK_LOG_MAX_BODY_PREVIEW_BYTES: usize = 4 * 1024;
pub const WEBHOOK_LOG_MAX_HEADER_NAMES: usize = 64;

const MAX_METHOD_BYTES: usize = 16;
const MAX_TARGET_BYTES: usize = 4 * 1024;
const MAX_HEADER_NAME_BYTES: usize = 256;
const MAX_HEADER_NAME_TOTAL_BYTES: usize = 4 * 1024;
const MAX_INPUT_BODY_BYTES: usize = 1024 * 1024;
const MAX_INPUT_HEADERS: usize = 100;
const MIN_TIMESTAMP_MS: i64 = -8_640_000_000_000_000;
const MAX_TIMESTAMP_MS: i64 = 8_640_000_000_000_000;
const REDACTED: &str = "[REDACTED]";

/// A deliberately small, display-only projection of one Webhook Lab capture.
/// Header values, raw request bytes, paths, commands, environments, and
/// archives have no field in this schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WebhookLogPayload {
    pub schema_version: u32,
    pub method: String,
    pub target: String,
    pub received_at_ms: i64,
    pub header_names: Vec<String>,
    pub body_preview: String,
    pub redacted: bool,
    pub truncated: bool,
}

/// Build the only Webhook capture representation that may cross into Log
/// Lens. The caller may provide an already-masked capture, but this boundary
/// still scans the complete bounded body before taking a preview so a secret
/// beyond the preview limit cannot be cut into a seemingly safe prefix.
pub fn webhook_log_payload(
    method: &str,
    target: &str,
    received_at_ms: i64,
    headers: &[(String, String)],
    body: &str,
) -> Result<WebhookLogPayload, String> {
    if !valid_method(method)
        || target.len() > MAX_TARGET_BYTES
        || headers.len() > MAX_INPUT_HEADERS
        || body.len() > MAX_INPUT_BODY_BYTES
        || !(MIN_TIMESTAMP_MS..=MAX_TIMESTAMP_MS).contains(&received_at_ms)
    {
        return Err(invalid());
    }

    let (target, target_redacted) = redact_target(target)?;
    let mut header_names = Vec::with_capacity(headers.len().min(WEBHOOK_LOG_MAX_HEADER_NAMES));
    let mut header_bytes = 0_usize;
    let mut seen = HashSet::with_capacity(headers.len().min(WEBHOOK_LOG_MAX_HEADER_NAMES));
    let mut truncated = false;
    let mut redacted = target_redacted;
    for (name, value) in headers {
        if !valid_header_name(name) {
            return Err(invalid());
        }
        if sensitive_header_name(name)
            || value.contains(REDACTED)
            || value.contains("•••••")
            || contains_sensitive_value(value)
        {
            redacted = true;
        }
        let normalized = name.to_ascii_lowercase();
        if !seen.insert(normalized) {
            continue;
        }
        if header_names.len() >= WEBHOOK_LOG_MAX_HEADER_NAMES
            || header_bytes.saturating_add(name.len()) > MAX_HEADER_NAME_TOTAL_BYTES
        {
            truncated = true;
            continue;
        }
        header_bytes += name.len();
        header_names.push(name.clone());
    }

    let body_redaction = if body.is_empty() {
        None
    } else {
        Some(redact_handoff_text(body).map_err(|_| invalid())?)
    };
    let redacted_body = body_redaction
        .as_ref()
        .map(|result| result.text.as_str())
        .unwrap_or_default();
    redacted |= body_redaction
        .as_ref()
        .is_some_and(|result| result.redacted)
        || body.contains(REDACTED)
        || body.contains("•••••");
    let body_preview = truncate_utf8(redacted_body, WEBHOOK_LOG_MAX_BODY_PREVIEW_BYTES);
    truncated |= body_preview.len() < redacted_body.len();

    let payload = WebhookLogPayload {
        schema_version: WEBHOOK_LOG_SCHEMA_VERSION,
        method: method.to_ascii_uppercase(),
        target,
        received_at_ms,
        header_names,
        body_preview,
        redacted,
        truncated,
    };
    validate_webhook_log_payload(&payload)?;
    Ok(payload)
}

pub fn validate_webhook_log_payload(payload: &WebhookLogPayload) -> Result<(), String> {
    let encoded = serde_json::to_vec(payload).map_err(|_| invalid())?;
    if encoded.len() > WEBHOOK_LOG_MAX_PAYLOAD_BYTES
        || payload.schema_version != WEBHOOK_LOG_SCHEMA_VERSION
        || !valid_method(&payload.method)
        || payload.method != payload.method.to_ascii_uppercase()
        || !valid_target(&payload.target)
        || !(MIN_TIMESTAMP_MS..=MAX_TIMESTAMP_MS).contains(&payload.received_at_ms)
        || payload.header_names.len() > WEBHOOK_LOG_MAX_HEADER_NAMES
        || payload.body_preview.len() > WEBHOOK_LOG_MAX_BODY_PREVIEW_BYTES
        || has_disallowed_control(&payload.body_preview)
        || validate_handoff_text(&payload.body_preview).is_err()
        || contains_sensitive_value(&payload.body_preview)
    {
        return Err(invalid());
    }

    let mut total = 0_usize;
    let mut seen = HashSet::with_capacity(payload.header_names.len());
    let mut requires_redacted = payload.target.contains(REDACTED)
        || payload.body_preview.contains(REDACTED)
        || payload.body_preview.contains("•••••");
    for name in &payload.header_names {
        total = total.checked_add(name.len()).ok_or_else(invalid)?;
        let normalized = name.to_ascii_lowercase();
        if !valid_header_name(name)
            || total > MAX_HEADER_NAME_TOTAL_BYTES
            || !seen.insert(normalized)
        {
            return Err(invalid());
        }
        requires_redacted |= sensitive_header_name(name);
    }
    if requires_redacted && !payload.redacted {
        return Err(invalid());
    }
    Ok(())
}

fn redact_target(value: &str) -> Result<(String, bool), String> {
    if !valid_target_shape(value) {
        return Err(invalid());
    }
    let redaction = redact_handoff_text(value).map_err(|_| invalid())?;
    if !redaction.redacted && valid_target(&redaction.text) {
        return Ok((redaction.text, false));
    }

    let pathname = value
        .split_once('?')
        .map_or(value, |(pathname, _)| pathname);
    let target = if valid_pathname(pathname) && !contains_sensitive_value(pathname) {
        format!("{pathname}?{REDACTED}")
    } else {
        format!("/{REDACTED}")
    };
    if !valid_target(&target) {
        return Err(invalid());
    }
    Ok((target, true))
}

fn valid_target(value: &str) -> bool {
    valid_target_shape(value)
        && validate_handoff_text(value).is_ok()
        && !contains_sensitive_value(value)
}

fn valid_target_shape(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_TARGET_BYTES
        || value.trim() != value
        || has_disallowed_control(value)
        || value.contains('\\')
    {
        return false;
    }
    let (pathname, query) = value
        .split_once('?')
        .map_or((value, None), |(pathname, query)| (pathname, Some(query)));
    valid_pathname(pathname)
        && query.is_none_or(|query| !query.contains('#') && !has_disallowed_control(query))
}

fn valid_pathname(value: &str) -> bool {
    if !value.starts_with('/')
        || value.starts_with("//")
        || value.contains('?')
        || value.contains('#')
    {
        return false;
    }
    let Some(decoded) = percent_decode(value) else {
        return false;
    };
    !decoded.starts_with("//")
        && !decoded.contains('\\')
        && !has_disallowed_control(&decoded)
        && !decoded
            .split('/')
            .any(|component| matches!(component, "." | ".."))
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(bytes.get(index + 1).copied()?)?;
            let low = hex(bytes.get(index + 2).copied()?)?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn valid_method(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_METHOD_BYTES && value.bytes().all(http_token_byte)
}

fn valid_header_name(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_HEADER_NAME_BYTES && value.bytes().all(http_token_byte)
}

fn http_token_byte(byte: u8) -> bool {
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
}

fn sensitive_header_name(value: &str) -> bool {
    let compact: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    [
        "authorization",
        "proxyauthorization",
        "cookie",
        "setcookie",
        "apikey",
        "accesstoken",
        "refreshtoken",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "privatekey",
        "auth",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

fn has_disallowed_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

fn invalid() -> String {
    "webhook log payload is invalid".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_contains_names_and_redacted_preview_but_never_header_values() {
        let payload = webhook_log_payload(
            "POST",
            "/hook?event=push&access_token=[REDACTED]",
            42,
            &[
                ("Authorization".into(), "[REDACTED]".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            r#"{"event":"push","password":"[REDACTED]"}"#,
        )
        .unwrap();
        assert_eq!(payload.target, "/hook?[REDACTED]");
        assert_eq!(payload.header_names, ["Authorization", "Content-Type"]);
        assert_eq!(payload.body_preview, REDACTED);
        assert!(payload.redacted);
        let encoded = serde_json::to_string(&payload).unwrap();
        assert!(!encoded.contains("application/json"));
        assert!(!encoded.contains("headerValues"));
        assert!(validate_webhook_log_payload(&payload).is_ok());
    }

    #[test]
    fn body_is_redacted_before_the_bounded_preview_is_taken() {
        let body = format!("{}\npassword=raw-secret", "x".repeat(8 * 1024));
        let payload = webhook_log_payload("POST", "/hook", 1, &[], &body).unwrap();
        assert!(payload.truncated);
        assert!(payload.redacted);
        assert!(payload.body_preview.len() <= WEBHOOK_LOG_MAX_BODY_PREVIEW_BYTES);
        assert!(!payload.body_preview.contains("raw-secret"));
    }

    #[test]
    fn strict_validator_rejects_unknown_raw_and_path_fields() {
        let payload = webhook_log_payload("GET", "/hook", 1, &[], "ok").unwrap();
        let mut value = serde_json::to_value(payload).unwrap();
        value["path"] = serde_json::json!("/private/log");
        assert!(serde_json::from_value::<WebhookLogPayload>(value).is_err());

        let raw = WebhookLogPayload {
            schema_version: WEBHOOK_LOG_SCHEMA_VERSION,
            method: "POST".into(),
            target: "/hook".into(),
            received_at_ms: 1,
            header_names: vec![],
            body_preview: "Authorization: Bearer raw-secret".into(),
            redacted: false,
            truncated: false,
        };
        assert!(validate_webhook_log_payload(&raw).is_err());
    }

    #[test]
    fn unsafe_targets_and_bounds_fail_closed() {
        for target in [
            "https://example.test/hook",
            "//host/hook",
            "/a/../b",
            "/a/%2e%2e/b",
        ] {
            assert!(webhook_log_payload("GET", target, 1, &[], "").is_err());
        }
        assert!(webhook_log_payload(
            "GET",
            "/hook",
            1,
            &[],
            &"x".repeat(MAX_INPUT_BODY_BYTES + 1)
        )
        .is_err());
    }
}
