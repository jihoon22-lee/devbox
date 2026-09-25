//! 제품 host가 handoff store로 전달하는 요청 형식.
mod handoff;
mod log_source;
mod task_control;
mod toolbox_text;
mod webhook_log;

pub use handoff::{
    handoff_root_in, redact_handoff_text, validate_handoff_text, CreateHandoff, HandoffClaim,
    HandoffDescriptor, HandoffEnvelope, HandoffError, HandoffPublication, HandoffStatus,
    HandoffStatusRecord, HandoffStore, RecordHandoffStatus, RedactedHandoffText,
    DEFAULT_CLAIM_LEASE_MS, DEFAULT_HANDOFF_TTL_MS, MAX_HANDOFF_BYTES,
};
pub use log_source::{
    run_log_source_payload, validate_run_log_source_payload, LogSourceStream, RunLogSourceRef,
    LOG_SOURCE_HANDOFF_KIND, LOG_SOURCE_MAX_PAYLOAD_BYTES, LOG_SOURCE_TARGET_APP,
};
pub use task_control::{
    TaskControlAction, TaskControlRequest, TASK_CONTROL_HANDOFF_KIND, TASK_CONTROL_SCHEMA_VERSION,
    TASK_CONTROL_SOURCE_APP, TASK_CONTROL_TARGET_APP,
};
pub use toolbox_text::{
    ToolboxTextPayload, TOOLBOX_TEXT_HANDOFF_KIND, TOOLBOX_TEXT_MAX_BYTES, TOOLBOX_TEXT_MAX_CHARS,
    TOOLBOX_TEXT_TARGET_APP,
};
pub use webhook_log::{
    validate_webhook_log_payload, webhook_log_payload, WebhookLogPayload, WEBHOOK_LOG_HANDOFF_KIND,
    WEBHOOK_LOG_MAX_BODY_PREVIEW_BYTES, WEBHOOK_LOG_MAX_HEADER_NAMES,
    WEBHOOK_LOG_MAX_PAYLOAD_BYTES, WEBHOOK_LOG_SCHEMA_VERSION, WEBHOOK_LOG_SOURCE_APP,
    WEBHOOK_LOG_TARGET_APP,
};

use serde::{Deserialize, Serialize};

/// Version of the one-time handoff envelope.
pub const PROTOCOL_VERSION: u32 = 2;

/// Bounded, app-neutral query filter carried by the `query` applink target.
///
/// This is intentionally a small value object rather than an Everything+
/// database type.  Older receivers ignore the optional `--query-filter-v1`
/// flag and still receive the query text; receivers that understand v1 validate
/// and apply the filter before searching their own local index.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct QueryFilter {
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_after: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_before: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_root_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_status: Option<String>,
}

/// Conservative detector for values that must never be persisted in a query
/// or handoff. It intentionally prefers a false positive over copying a
/// credential into an integration snapshot or argv. This is syntax-oriented,
/// not a claim that arbitrary secrets can be identified.
pub fn contains_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin private key-----")
        || lower.contains("-----begin rsa private key-----")
        || lower.contains("-----begin openssh private key-----")
        || lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || lower.split_whitespace().any(|token| {
            token == "bearer"
                || token == "basic"
                || token.starts_with("bearer ")
                || token.starts_with("basic ")
        })
    {
        return true;
    }

    for key in [
        "authorization",
        "password",
        "passwd",
        "secret",
        "api_key",
        "api-key",
        "access_token",
        "access-token",
        "refresh_token",
        "refresh-token",
        "client_secret",
        "client-secret",
        "cookie",
        "set-cookie",
        "x-api-key",
        "token",
    ] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(key) {
            let start = offset + found;
            let suffix = lower[start + key.len()..].trim_start();
            let Some(rest) = suffix
                .strip_prefix(':')
                .or_else(|| suffix.strip_prefix('='))
            else {
                offset = start + key.len();
                continue;
            };
            if !rest.trim_start().is_empty() {
                return true;
            }
            offset = start + key.len();
        }
    }

    // URL userinfo (`scheme://user:password@host`) is a credential even when
    // it is not written as a named query parameter.
    for marker in ["://", "//"] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(marker) {
            let authority_start = offset + found + marker.len();
            let authority = lower[authority_start..]
                .split(|character: char| character.is_whitespace() || "/?#".contains(character))
                .next()
                .unwrap_or_default();
            // Treat any URL userinfo as sensitive. A username-only authority
            // and percent-encoded `user%3Apass@` are still unsafe to copy into
            // a snapshot, and rejecting them avoids trying to decode URL
            // escapes in this intentionally conservative detector.
            if authority.contains('@') {
                return true;
            }
            offset = authority_start;
        }
    }

    let token_is_jwt = |token: &str| {
        let mut parts = token.split('.');
        let Some(header) = parts.next() else {
            return false;
        };
        let Some(payload) = parts.next() else {
            return false;
        };
        let Some(signature) = parts.next() else {
            return false;
        };
        parts.next().is_none()
            && header.len() >= 8
            && payload.len() >= 8
            && signature.len() >= 8
            && [header, payload, signature].iter().all(|part| {
                part.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
    };
    if lower
        .split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '"' | '\''
                        | '='
                        | ':'
                        | ','
                        | ';'
                        | '&'
                        | '?'
                        | '/'
                        | '\\'
                        | '('
                        | '['
                        | '{'
                        | ')'
                )
        })
        .any(|token| {
            token_is_jwt(token)
                || [
                    "sk-",
                    "ghp_",
                    "github_pat_",
                    "xoxb-",
                    "xoxp-",
                    "glpat-",
                    "npm_",
                    "gho_",
                    "akia",
                    "eyj",
                ]
                .iter()
                .any(|prefix| token.starts_with(prefix) && token.len() > prefix.len())
        })
    {
        return true;
    }
    false
}

impl QueryFilter {
    pub const MAX_EXTENSIONS: usize = 64;
    pub const MAX_EXTENSION_BYTES: usize = 16;

    pub fn normalized(&self) -> Result<Self, ParseError> {
        if self.extensions.len() > Self::MAX_EXTENSIONS {
            return Err(ParseError("query filter extension bound exceeded".into()));
        }
        let mut extensions = Vec::with_capacity(self.extensions.len());
        for extension in &self.extensions {
            let normalized = extension
                .trim()
                .trim_start_matches('.')
                .to_ascii_lowercase();
            if normalized.is_empty()
                || normalized.len() > Self::MAX_EXTENSION_BYTES
                || !normalized.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-' | b'+')
                })
            {
                return Err(ParseError("query filter extension is invalid".into()));
            }
            extensions.push(normalized);
        }
        extensions.sort_unstable();
        extensions.dedup();

        if self
            .modified_after
            .zip(self.modified_before)
            .is_some_and(|(after, before)| after > before)
            || self.modified_after.is_some_and(|timestamp| timestamp < 0)
            || self.modified_before.is_some_and(|timestamp| timestamp < 0)
            || self.min_size.is_some_and(|size| size < 0)
            || self.max_size.is_some_and(|size| size < 0)
            || self
                .min_size
                .zip(self.max_size)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            || self.source_root_id.is_some_and(|root_id| root_id <= 0)
        {
            return Err(ParseError("query filter range is invalid".into()));
        }

        let content_status = self
            .content_status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        if content_status.as_deref().is_some_and(|status| {
            !matches!(
                status,
                "indexed"
                    | "truncated"
                    | "partial"
                    | "failed"
                    | "not_indexed"
                    | "too_large"
                    | "unsupported_encoding"
                    | "read_error"
                    | "timeout"
                    | "changed_during_read"
                    | "skipped_sensitive"
                    | "no_text"
                    | "unsupported_encrypted"
                    | "extract_error"
            )
        }) {
            return Err(ParseError("query filter status is invalid".into()));
        }

        Ok(Self {
            extensions,
            modified_after: self.modified_after,
            modified_before: self.modified_before,
            min_size: self.min_size,
            max_size: self.max_size,
            source_root_id: self.source_root_id,
            content_status,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty()
            && self.modified_after.is_none()
            && self.modified_before.is_none()
            && self.min_size.is_none()
            && self.max_size.is_none()
            && self.source_root_id.is_none()
            && self.content_status.is_none()
    }
}

/// "어디를 열지"를 나타내는 타깃.
///
/// 새 variant를 추가할 때는 §1.3 표를 다시 확인한다 — 구버전 수신 앱은 새
/// variant의 태그를 모르므로 (모르는 플래그로 취급돼) 무시하고 `Ok(None)`으로
/// degrade해야 한다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub enum OpenTarget {
    Path {
        path: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    Profile {
        id: String,
    },
    Workspace {
        path: String,
    },
    Query {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<QueryFilter>,
    },
    /// Run Manager의 저장된 job/service 하나를 연다. id만 전달하며 실제 job
    /// 명령·환경변수는 수신 앱이 자기 저장소에서 재검증한다.
    Task {
        id: String,
    },
    /// 대상 앱이 설치되어 있지 않을 때 Manager의 해당 앱 설치 화면을 연다.
    Install {
        #[serde(rename = "appId")]
        app_id: String,
    },
    Handoff {
        #[serde(rename = "handoffKind")]
        kind: String,
        id: String,
    },
}

impl From<HandoffDescriptor> for OpenTarget {
    fn from(descriptor: HandoffDescriptor) -> Self {
        Self::Handoff {
            kind: descriptor.kind,
            id: descriptor.id,
        }
    }
}

/// 파싱된 인바운드 요청. Tauri 이벤트(`devbox://open`) payload로 그대로 직렬화된다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct OpenRequest {
    pub target: OpenTarget,
    /// 보낸 앱의 카탈로그 id — 로깅·"되돌아가기" 버튼용. 모르면 `None`.
    pub from: Option<String>,
}

/// Typed request validation failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "applink: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Validate the typed request without constructing command-line arguments.
pub fn validate_request(request: &OpenRequest) -> Result<(), ParseError> {
    if let OpenTarget::Query {
        filter: Some(filter),
        ..
    } = &request.target
    {
        filter.normalized()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }
    #[test]
    fn json_round_trip_open_request() {
        let req = OpenRequest {
            target: OpenTarget::Path {
                path: s("/tmp/x"),
                line: Some(10),
                column: None,
            },
            from: Some(s("repo-manager")),
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: OpenRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn json_uses_camel_case_kind_tag() {
        let req = OpenRequest {
            target: OpenTarget::Workspace { path: s("/ws") },
            from: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["target"]["kind"], "workspace");
        assert_eq!(json["target"]["path"], "/ws");
        assert!(json["from"].is_null());
    }

    #[test]
    fn json_round_trip_all_target_variants() {
        let reqs = vec![
            OpenRequest {
                target: OpenTarget::Path {
                    path: s("/a"),
                    line: None,
                    column: None,
                },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Profile { id: s("p") },
                from: Some(s("wsl-desktop")),
            },
            OpenRequest {
                target: OpenTarget::Workspace { path: s("/w") },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Query {
                    text: s("t"),
                    filter: None,
                },
                from: None,
            },
            OpenRequest {
                target: OpenTarget::Task { id: s("job-1") },
                from: Some(s("devbox-launcher")),
            },
            OpenRequest {
                target: OpenTarget::Install {
                    app_id: s("run-manager"),
                },
                from: Some(s("devbox-launcher")),
            },
            OpenRequest {
                target: OpenTarget::Handoff {
                    kind: s("knowledge-draft/v1"),
                    id: s("0123456789abcdef0123456789abcdef"),
                },
                from: Some(s("life-log")),
            },
        ];
        for req in reqs {
            let json = serde_json::to_string(&req).unwrap();
            let back: OpenRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(back, req);
        }
    }

    #[test]
    fn handoff_json_keeps_variant_tag_and_payload_kind_distinct() {
        let request = OpenRequest {
            target: OpenTarget::Handoff {
                kind: s("knowledge-draft/v1"),
                id: s("0123456789abcdef0123456789abcdef"),
            },
            from: Some(s("life-log")),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["target"]["kind"], "handoff");
        assert_eq!(json["target"]["handoffKind"], "knowledge-draft/v1");
        assert_eq!(json["target"]["id"], "0123456789abcdef0123456789abcdef");
    }
}
