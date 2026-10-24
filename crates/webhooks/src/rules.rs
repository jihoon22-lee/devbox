//! response rule (순수). method+path 매치 → 고정 status/header/body 응답.

pub use crate::matcher::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Rule validation is deliberately stricter than the serde wire types. These
/// values are also mirrored by the editor's TypeScript validator.
pub const MAX_RULES: usize = 200;
pub const MAX_RULE_ID_CHARS: usize = 128;
pub const MAX_RULE_ID_BYTES: usize = 128;
pub const MAX_METHOD_CHARS: usize = 16;
pub const MAX_METHOD_BYTES: usize = 16;
pub const MAX_PATH_CHARS: usize = 4_096;
pub const MAX_PATH_BYTES: usize = 16_384;
pub const MAX_RULE_HEADERS: usize = 100;
pub const MAX_HEADER_NAME_CHARS: usize = 256;
pub const MAX_HEADER_NAME_BYTES: usize = 256;
pub const MAX_HEADER_VALUE_CHARS: usize = 16_384;
pub const MAX_HEADER_VALUE_BYTES: usize = 65_536;
pub const MAX_HEADER_TOTAL_CHARS: usize = 64_000;
pub const MAX_HEADER_TOTAL_BYTES: usize = 256_000;
pub const MAX_BODY_CHARS: usize = 256_000;
pub const MAX_BODY_BYTES: usize = 1_024_000;
pub const MAX_RULE_COLLECTION_CHARS: usize = 2_000_000;
pub const MAX_RULE_COLLECTION_BYTES: usize = 8_000_000;
pub const MIN_RESPONSE_STATUS: u16 = 200;
pub const MAX_RESPONSE_STATUS: u16 = 599;
pub const MAX_RESPONSE_DELAY_MS: u64 = 60_000;
pub const MIN_RULE_PRIORITY: i32 = -1_000;
pub const MAX_RULE_PRIORITY: i32 = 1_000;
/// A sequence is intentionally short and data-only.  It is a local testing
/// aid, not an arbitrary scripting or workflow engine.
pub const MAX_RESPONSE_SEQUENCE: usize = 16;
pub const INVALID_RULE_ERROR: &str = "규칙 입력이 유효하지 않습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleValidationError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StringMetrics {
    chars: usize,
    bytes: usize,
}

impl StringMetrics {
    fn add_str(&mut self, value: &str) -> Option<()> {
        self.chars = self.chars.checked_add(value.chars().count())?;
        self.bytes = self.bytes.checked_add(value.len())?;
        Some(())
    }

    fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            chars: self.chars.checked_add(other.chars)?,
            bytes: self.bytes.checked_add(other.bytes)?,
        })
    }

    fn checked_sub(self, other: Self) -> Option<Self> {
        Some(Self {
            chars: self.chars.checked_sub(other.chars)?,
            bytes: self.bytes.checked_sub(other.bytes)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct ResponseSequenceStep {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct ResponseRule {
    pub id: String,
    // Higher values win before path/method specificity. Missing values in
    // v0.5.x documents deserialize as zero for backward compatibility.
    #[serde(default)]
    pub priority: i32,
    // None이면 모든 method
    pub method: Option<String>,
    pub path: String,
    // 매치된 요청에 반환할 HTTP response status
    pub status: u16,
    // 매치된 요청에 반환할 HTTP response headers
    pub headers: Vec<(String, String)>,
    // 매치된 요청에 반환할 HTTP response body
    pub body: String,
    // HTTP response를 보내기 전 대기 시간 (ms)
    pub delay_ms: u64,
    // Additional responses after the base response.  Request number zero
    // uses the base fields above, then steps are consumed in order.  Once
    // the final step is reached it remains active until an explicit reset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<ResponseSequenceStep>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub enum RuleConflictKind {
    CandidateShadowsExisting,
    ExistingShadowsCandidate,
    PartialOverlap,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub enum RulePrecedenceReason {
    Priority,
    ExactPath,
    MethodSpecific,
    LongerWildcardPrefix,
    IdTieBreak,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct RuleConflict {
    pub existing_rule_id: String,
    pub winner_rule_id: String,
    pub loser_rule_id: String,
    pub kind: RuleConflictKind,
    pub reason: RulePrecedenceReason,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct RuleConflictPreview {
    pub candidate_id: String,
    pub conflicts: Vec<RuleConflict>,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleUpsertPlan {
    pub candidate: ResponseRule,
    pub preview: RuleConflictPreview,
}

impl ResponseRule {
    /// Return the response for a zero-based matched-request position.  The
    /// final step is held after the sequence is exhausted so a webhook sender
    /// cannot accidentally wrap around and create an unbounded scenario.
    pub fn response_at(&self, sequence_index: usize) -> ResponseSequenceStep {
        if sequence_index == 0 || self.sequence.is_empty() {
            return ResponseSequenceStep {
                status: self.status,
                headers: self.headers.clone(),
                body: self.body.clone(),
                delay_ms: self.delay_ms,
            };
        }
        let step_index = (sequence_index - 1).min(self.sequence.len() - 1);
        self.sequence[step_index].clone()
    }
}

pub use crate::sequence::ResponseSequenceState;

fn within(value: &str, max_chars: usize, max_bytes: usize) -> bool {
    value.chars().count() <= max_chars && value.len() <= max_bytes
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn is_method(value: &str) -> bool {
    is_token(value)
}

fn is_token(value: &str) -> bool {
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

fn is_header_name(value: &str) -> bool {
    is_token(value)
}

fn is_transport_header(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "connection"
            | "content-length"
            | "expect"
            | "host"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn validate_headers(headers: &[(String, String)]) -> Result<(), RuleValidationError> {
    if headers.len() > MAX_RULE_HEADERS {
        return Err(RuleValidationError);
    }

    let mut total = StringMetrics::default();
    for (name, value) in headers {
        if !within(name, MAX_HEADER_NAME_CHARS, MAX_HEADER_NAME_BYTES)
            || !is_header_name(name)
            || is_transport_header(name)
            || !within(value, MAX_HEADER_VALUE_CHARS, MAX_HEADER_VALUE_BYTES)
            // The native response writer emits visible ASCII header values.
            // Reject anything it cannot encode at the rule boundary so a
            // saved rule never differs from the wire.
            || !value.is_ascii()
            || has_control(value)
        {
            return Err(RuleValidationError);
        }
        total.add_str(name).ok_or(RuleValidationError)?;
        total.add_str(value).ok_or(RuleValidationError)?;
    }

    if total.chars > MAX_HEADER_TOTAL_CHARS || total.bytes > MAX_HEADER_TOTAL_BYTES {
        return Err(RuleValidationError);
    }
    Ok(())
}

fn validate_response_payload(
    status: u16,
    headers: &[(String, String)],
    body: &str,
    delay_ms: u64,
) -> Result<(), RuleValidationError> {
    if !(MIN_RESPONSE_STATUS..=MAX_RESPONSE_STATUS).contains(&status)
        || !within(body, MAX_BODY_CHARS, MAX_BODY_BYTES)
        || delay_ms > MAX_RESPONSE_DELAY_MS
    {
        return Err(RuleValidationError);
    }
    validate_headers(headers)
}

fn rule_metrics(rule: &ResponseRule) -> Option<StringMetrics> {
    let mut metrics = StringMetrics::default();
    // A new rule receives a UUID before it reaches storage. Reserve the UUID
    // footprint here too so frontend and backend collection checks agree.
    metrics.add_str(if rule.id.is_empty() {
        "00000000-0000-0000-0000-000000000000"
    } else {
        &rule.id
    })?;
    if let Some(method) = &rule.method {
        metrics.add_str(method)?;
    }
    metrics.add_str(&rule.path)?;
    for (name, value) in &rule.headers {
        metrics.add_str(name)?;
        metrics.add_str(value)?;
    }
    metrics.add_str(&rule.body)?;
    for step in &rule.sequence {
        for (name, value) in &step.headers {
            metrics.add_str(name)?;
            metrics.add_str(value)?;
        }
        metrics.add_str(&step.body)?;
    }
    Some(metrics)
}

/// Validate one response rule at the storage boundary.
pub fn validate_rule(rule: &ResponseRule) -> Result<(), RuleValidationError> {
    if (!rule.id.is_empty()
        && (!within(&rule.id, MAX_RULE_ID_CHARS, MAX_RULE_ID_BYTES) || has_control(&rule.id)))
        || !within(&rule.path, MAX_PATH_CHARS, MAX_PATH_BYTES)
        || !rule.path.starts_with('/')
        || !rule.path.is_ascii()
        || has_control(&rule.path)
        || !within(&rule.body, MAX_BODY_CHARS, MAX_BODY_BYTES)
        || !(MIN_RULE_PRIORITY..=MAX_RULE_PRIORITY).contains(&rule.priority)
    {
        return Err(RuleValidationError);
    }

    if let Some(method) = &rule.method
        && (!within(method, MAX_METHOD_CHARS, MAX_METHOD_BYTES) || !is_method(method))
    {
        return Err(RuleValidationError);
    }
    if rule.sequence.len() > MAX_RESPONSE_SEQUENCE {
        return Err(RuleValidationError);
    }
    validate_response_payload(rule.status, &rule.headers, &rule.body, rule.delay_ms)?;
    for step in &rule.sequence {
        validate_response_payload(step.status, &step.headers, &step.body, step.delay_ms)?;
    }

    let metrics = rule_metrics(rule).ok_or(RuleValidationError)?;
    if metrics.chars > MAX_RULE_COLLECTION_CHARS || metrics.bytes > MAX_RULE_COLLECTION_BYTES {
        return Err(RuleValidationError);
    }
    Ok(())
}

fn collection_metrics<'a, I>(rules: I) -> Result<StringMetrics, RuleValidationError>
where
    I: IntoIterator<Item = &'a ResponseRule>,
{
    let mut count: usize = 0;
    let mut metrics = StringMetrics::default();
    for rule in rules {
        count = count.checked_add(1).ok_or(RuleValidationError)?;
        if count > MAX_RULES {
            return Err(RuleValidationError);
        }
        validate_rule(rule)?;
        metrics = metrics
            .checked_add(rule_metrics(rule).ok_or(RuleValidationError)?)
            .ok_or(RuleValidationError)?;
        if metrics.chars > MAX_RULE_COLLECTION_CHARS || metrics.bytes > MAX_RULE_COLLECTION_BYTES {
            return Err(RuleValidationError);
        }
    }
    Ok(metrics)
}

/// Validate the complete in-memory rule collection without exposing input in
/// the error type.
pub fn validate_rule_collection<'a, I>(rules: I) -> Result<(), RuleValidationError>
where
    I: IntoIterator<Item = &'a ResponseRule>,
{
    collection_metrics(rules).map(|_| ())
}

/// rule이 요청과 매치하는지. method는 대소문자 무시하고, None은 모든 method다.
/// path는 전체 문자열이 같거나, rule path의 마지막 문자가 `*`일 때 그 앞부분으로
/// 시작해야 한다. 여러 rule의 우선순위는 [`select_matching_rule`]이 소유한다.
pub fn upsert(
    rules: &mut HashMap<String, ResponseRule>,
    mut rule: ResponseRule,
) -> Result<String, RuleValidationError> {
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    let id = rule.id.clone();
    validate_rule(&rule)?;

    let mut collection = collection_metrics(rules.values())?;
    if let Some(existing) = rules.get(&id) {
        collection = collection
            .checked_sub(rule_metrics(existing).ok_or(RuleValidationError)?)
            .ok_or(RuleValidationError)?;
    } else if rules.len() >= MAX_RULES {
        return Err(RuleValidationError);
    }
    collection = collection
        .checked_add(rule_metrics(&rule).ok_or(RuleValidationError)?)
        .ok_or(RuleValidationError)?;
    if collection.chars > MAX_RULE_COLLECTION_CHARS || collection.bytes > MAX_RULE_COLLECTION_BYTES
    {
        return Err(RuleValidationError);
    }
    rules.insert(id.clone(), rule);
    Ok(id)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
