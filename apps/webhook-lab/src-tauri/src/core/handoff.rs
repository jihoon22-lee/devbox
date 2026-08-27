//! Webhook Lab's `api-request/v1` producer contract.
//!
//! The producer accepts only the already-masked fixture projection.  It never
//! reaches into the in-memory raw-header vault and it does not put request
//! bytes on argv.  Redaction markers that represent credentials are converted
//! to an explicit environment reference so the shared handoff validator can
//! reject accidental raw values at publication time.

use super::fixtures::{validate_fixture, CapturedFixture, REDACTED, REDACTED_PATH};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API_REQUEST_HANDOFF_KIND: &str = "api-request/v1";
pub const PRODUCER_APP_ID: &str = "webhook-lab";
pub const CONSUMER_APP_ID: &str = "api-playground";
pub const WEBHOOK_SECRET_REFERENCE: &str = "${WEBHOOK_SECRET}";
pub const HANDOFF_INPUT_ERROR: &str = "handoff 요청에 사용할 fixture가 유효하지 않습니다";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiRequestHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiRequestPayload {
    pub method: String,
    pub url: String,
    pub headers: Vec<ApiRequestHeader>,
    pub body: String,
}

/// Convert a validated masked fixture to the versioned handoff payload.
///
/// The URL remains an origin-form target (for example `/hooks/push`) because
/// Webhook Lab intentionally does not invent a host or port.  API Playground
/// shows it in the preview and lets the user edit it before sending.
pub fn build_api_request_payload(
    fixture: &CapturedFixture,
) -> Result<ApiRequestPayload, &'static str> {
    validate_fixture(fixture).map_err(|_| HANDOFF_INPUT_ERROR)?;
    let payload = ApiRequestPayload {
        method: fixture.method.to_ascii_uppercase(),
        url: rewrite_sensitive_query(&fixture.url),
        headers: fixture
            .headers
            .iter()
            .map(|(name, value)| ApiRequestHeader {
                name: name.clone(),
                value: rewrite_header_value(name, value),
            })
            .collect(),
        body: rewrite_body(&fixture.body)?,
    };
    // Keep this check close to the producer so future fixture fields cannot
    // silently bypass the serde shape that is published to the shared store.
    serde_json::to_value(&payload).map_err(|_| HANDOFF_INPUT_ERROR)?;
    Ok(payload)
}

fn rewrite_header_value(name: &str, value: &str) -> String {
    if is_sensitive_name(name)
        || value == "•••••"
        || value.contains(REDACTED)
        || starts_with_auth_scheme(value)
    {
        WEBHOOK_SECRET_REFERENCE.to_string()
    } else {
        value.to_string()
    }
}

fn rewrite_sensitive_query(target: &str) -> String {
    if target == REDACTED_PATH {
        return target.to_string();
    }
    let Some((pathname, query)) = target.split_once('?') else {
        return target.to_string();
    };
    let query = query
        .split('&')
        .map(|component| {
            let Some((raw_key, raw_value)) = component.split_once('=') else {
                return component.to_string();
            };
            let key = percent_decode(raw_key).unwrap_or_else(|| raw_key.to_string());
            if is_sensitive_name(&key) && raw_value == REDACTED {
                format!("{raw_key}={WEBHOOK_SECRET_REFERENCE}")
            } else {
                component.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{pathname}?{query}")
}

fn rewrite_body(body: &str) -> Result<String, &'static str> {
    if body.is_empty() {
        return Ok(String::new());
    }
    match serde_json::from_str::<Value>(body) {
        Ok(mut value) => {
            rewrite_json_strings(&mut value);
            serde_json::to_string(&value).map_err(|_| HANDOFF_INPUT_ERROR)
        }
        Err(_) => Ok(rewrite_body_text(body)),
    }
}

fn rewrite_json_strings(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = rewrite_body_text(text);
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_strings(item);
            }
        }
        Value::Object(object) => {
            for item in object.values_mut() {
                rewrite_json_strings(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn rewrite_body_text(value: &str) -> String {
    if starts_with_auth_scheme(value) {
        return WEBHOOK_SECRET_REFERENCE.to_string();
    }
    value.replace(REDACTED, WEBHOOK_SECRET_REFERENCE)
}

fn starts_with_auth_scheme(value: &str) -> bool {
    let lower = value.trim_start().to_ascii_lowercase();
    lower.starts_with("bearer ") || lower.starts_with("basic ")
}

fn is_sensitive_name(name: &str) -> bool {
    let compact: String = name
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
            | "accesskey"
            | "clientsecret"
            | "privatekey"
            | "auth"
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_applink::{CreateHandoff, HandoffError, HandoffStore};
    use tempfile::tempdir;

    fn fixture() -> CapturedFixture {
        CapturedFixture {
            id: "fixture-1".into(),
            method: "POST".into(),
            url: "/hooks/push?event=push&access_token=[REDACTED]".into(),
            headers: vec![
                ("Authorization".into(), "[REDACTED]".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: r#"{"event":"push","token":"[REDACTED]"}"#.into(),
            received_at_ms: 1,
        }
    }

    #[test]
    fn payload_preserves_safe_request_shape_and_uses_references_for_redactions() {
        let payload = build_api_request_payload(&fixture()).unwrap();
        assert_eq!(payload.method, "POST");
        assert_eq!(
            payload.url,
            "/hooks/push?event=push&access_token=${WEBHOOK_SECRET}"
        );
        assert_eq!(payload.headers[0].value, WEBHOOK_SECRET_REFERENCE);
        assert_eq!(payload.headers[1].value, "application/json");
        assert!(payload.body.contains(WEBHOOK_SECRET_REFERENCE));
        let serialized = serde_json::to_string(&payload).unwrap();
        assert!(!serialized.contains("[REDACTED]"));
    }

    #[test]
    fn invalid_fixture_never_becomes_a_handoff_payload() {
        let mut invalid = fixture();
        invalid.url = "https://example.test/private".into();
        assert_eq!(
            build_api_request_payload(&invalid),
            Err(HANDOFF_INPUT_ERROR)
        );
    }

    #[test]
    fn producer_payload_round_trips_through_shared_store_once() {
        let directory = tempdir().unwrap();
        let payload = build_api_request_payload(&fixture()).unwrap();
        let serialized = serde_json::to_value(payload).unwrap();
        let store = HandoffStore::new(directory.path().join("handoff/v1"));
        let descriptor = store
            .create(
                CreateHandoff {
                    kind: API_REQUEST_HANDOFF_KIND.into(),
                    source_app: PRODUCER_APP_ID.into(),
                    target_app: Some(CONSUMER_APP_ID.into()),
                    payload: serialized,
                },
                1_000,
            )
            .unwrap();
        let claim = store
            .claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                CONSUMER_APP_ID,
                1_001,
            )
            .unwrap();
        assert_eq!(claim.envelope.source_app, PRODUCER_APP_ID);
        assert_eq!(claim.envelope.target_app.as_deref(), Some(CONSUMER_APP_ID));
        assert_eq!(
            claim.envelope.payload["headers"][0]["value"],
            WEBHOOK_SECRET_REFERENCE
        );
        store.ack(&claim, CONSUMER_APP_ID, 1_002).unwrap();
        assert_eq!(
            store.claim(
                &descriptor.id,
                API_REQUEST_HANDOFF_KIND,
                CONSUMER_APP_ID,
                1_003,
            ),
            Err(HandoffError::Missing)
        );
    }
}
