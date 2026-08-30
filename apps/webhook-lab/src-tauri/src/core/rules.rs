//! response rule (순수). method+path 매치 → 고정 status/header/body 응답.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
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
pub const MIN_RESPONSE_STATUS: u16 = 100;
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
pub struct ResponseSequenceStep {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseRule {
    pub id: String,
    /// Higher values win before path/method specificity. Missing values in
    /// v0.5.x documents deserialize as zero for backward compatibility.
    #[serde(default)]
    pub priority: i32,
    /// None이면 모든 method
    pub method: Option<String>,
    pub path: String,
    /// 매치된 요청에 반환할 HTTP response status
    pub status: u16,
    /// 매치된 요청에 반환할 HTTP response headers
    pub headers: Vec<(String, String)>,
    /// 매치된 요청에 반환할 HTTP response body
    pub body: String,
    /// HTTP response를 보내기 전 대기 시간 (ms)
    pub delay_ms: u64,
    /// Additional responses after the base response.  Request number zero
    /// uses the base fields above, then steps are consumed in order.  Once
    /// the final step is reached it remains active until an explicit reset.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<ResponseSequenceStep>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleConflictKind {
    CandidateShadowsExisting,
    ExistingShadowsCandidate,
    PartialOverlap,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RulePrecedenceReason {
    Priority,
    ExactPath,
    MethodSpecific,
    LongerWildcardPrefix,
    IdTieBreak,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuleConflict {
    pub existing_rule_id: String,
    pub winner_rule_id: String,
    pub loser_rule_id: String,
    pub kind: RuleConflictKind,
    pub reason: RulePrecedenceReason,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
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

/// Process-local cursor state for response sequences.  Keeping this separate
/// from `ResponseRule` makes it explicit that current position is ephemeral:
/// it is never serialized, persisted, or shared with another process.
#[derive(Debug, Default)]
pub struct ResponseSequenceState {
    cursors: HashMap<String, usize>,
}

impl ResponseSequenceState {
    pub fn next_response(&mut self, rule: &ResponseRule) -> ResponseSequenceStep {
        if rule.sequence.is_empty() {
            return rule.response_at(0);
        }
        let cursor = self.cursors.entry(rule.id.clone()).or_insert(0);
        let index = *cursor;
        *cursor = cursor.saturating_add(1);
        rule.response_at(index)
    }

    pub fn reset(&mut self, rule_id: &str) {
        self.cursors.remove(rule_id);
    }
}

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

    if let Some(method) = &rule.method {
        if !within(method, MAX_METHOD_CHARS, MAX_METHOD_BYTES) || !is_method(method) {
            return Err(RuleValidationError);
        }
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
pub fn matches(rule: &ResponseRule, method: &str, path: &str) -> bool {
    // The native request parser and replay client emit/accept ASCII targets
    // only.  Keep direct matcher callers fail-closed even if an invalid rule
    // is constructed outside the storage validator.
    if !rule.path.is_ascii() || !path.is_ascii() {
        return false;
    }
    if let Some(m) = &rule.method {
        if !m.eq_ignore_ascii_case(method) {
            return false;
        }
    }
    rule.path == path
        || (rule.path.ends_with('*') && path.starts_with(&rule.path[..rule.path.len() - 1]))
}

/// Stable precedence shared by the live listener, list UI, and conflict
/// preview. `Ordering::Less` means `left` is evaluated before `right`.
pub fn compare_rule_precedence(left: &ResponseRule, right: &ResponseRule) -> Ordering {
    right
        .priority
        .cmp(&left.priority)
        .then_with(|| path_is_exact(right).cmp(&path_is_exact(left)))
        .then_with(|| right.method.is_some().cmp(&left.method.is_some()))
        .then_with(|| wildcard_prefix_len(right).cmp(&wildcard_prefix_len(left)))
        .then_with(|| left.id.as_bytes().cmp(right.id.as_bytes()))
}

pub fn select_matching_rule<'a, I>(rules: I, method: &str, path: &str) -> Option<&'a ResponseRule>
where
    I: IntoIterator<Item = &'a ResponseRule>,
{
    rules
        .into_iter()
        .filter(|rule| matches(rule, method, path))
        .min_by(|left, right| compare_rule_precedence(left, right))
}

/// Assign a stable id to a new candidate, validate its projected collection,
/// and report every overlapping existing rule. Callers must hold the rule-map
/// lock from this function through the confirmed upsert.
pub fn plan_upsert(
    rules: &HashMap<String, ResponseRule>,
    mut candidate: ResponseRule,
) -> Result<RuleUpsertPlan, RuleValidationError> {
    if candidate.id.is_empty() {
        candidate.id = uuid::Uuid::new_v4().to_string();
    }
    let mut projected = rules.clone();
    upsert(&mut projected, candidate.clone())?;

    let mut conflicts = rules
        .values()
        .filter(|existing| existing.id != candidate.id && rules_overlap(&candidate, existing))
        .map(|existing| conflict(&candidate, existing))
        .collect::<Vec<_>>();
    conflicts.sort_by(|left, right| left.existing_rule_id.cmp(&right.existing_rule_id));
    let preview = RuleConflictPreview {
        candidate_id: candidate.id.clone(),
        requires_confirmation: !conflicts.is_empty(),
        conflicts,
    };
    Ok(RuleUpsertPlan { candidate, preview })
}

pub fn rules_overlap(left: &ResponseRule, right: &ResponseRule) -> bool {
    methods_overlap(left.method.as_deref(), right.method.as_deref())
        && paths_overlap(&left.path, &right.path)
}

fn methods_overlap(left: Option<&str>, right: Option<&str>) -> bool {
    left.is_none()
        || right.is_none()
        || left.is_some_and(|left| right.is_some_and(|right| left.eq_ignore_ascii_case(right)))
}

fn paths_overlap(left: &str, right: &str) -> bool {
    match (path_pattern(left), path_pattern(right)) {
        ((left, false), (right, false)) => left == right,
        ((exact, false), (prefix, true)) | ((prefix, true), (exact, false)) => {
            exact.starts_with(prefix)
        }
        ((left, true), (right, true)) => left.starts_with(right) || right.starts_with(left),
    }
}

fn rule_covers(covering: &ResponseRule, covered: &ResponseRule) -> bool {
    let method_covers = covering.method.is_none()
        || covering.method.as_deref().is_some_and(|left| {
            covered
                .method
                .as_deref()
                .is_some_and(|right| left.eq_ignore_ascii_case(right))
        });
    if !method_covers {
        return false;
    }
    match (path_pattern(&covering.path), path_pattern(&covered.path)) {
        ((left, false), (right, false)) => left == right,
        ((_, false), (_, true)) => false,
        ((prefix, true), (exact, false)) => exact.starts_with(prefix),
        ((left, true), (right, true)) => right.starts_with(left),
    }
}

fn conflict(candidate: &ResponseRule, existing: &ResponseRule) -> RuleConflict {
    let candidate_wins = compare_rule_precedence(candidate, existing) == Ordering::Less;
    let (winner, loser) = if candidate_wins {
        (candidate, existing)
    } else {
        (existing, candidate)
    };
    let kind = if candidate_wins && rule_covers(candidate, existing) {
        RuleConflictKind::CandidateShadowsExisting
    } else if !candidate_wins && rule_covers(existing, candidate) {
        RuleConflictKind::ExistingShadowsCandidate
    } else {
        RuleConflictKind::PartialOverlap
    };
    RuleConflict {
        existing_rule_id: existing.id.clone(),
        winner_rule_id: winner.id.clone(),
        loser_rule_id: loser.id.clone(),
        kind,
        reason: precedence_reason(winner, loser),
    }
}

fn precedence_reason(winner: &ResponseRule, loser: &ResponseRule) -> RulePrecedenceReason {
    if winner.priority != loser.priority {
        RulePrecedenceReason::Priority
    } else if path_is_exact(winner) != path_is_exact(loser) {
        RulePrecedenceReason::ExactPath
    } else if winner.method.is_some() != loser.method.is_some() {
        RulePrecedenceReason::MethodSpecific
    } else if wildcard_prefix_len(winner) != wildcard_prefix_len(loser) {
        RulePrecedenceReason::LongerWildcardPrefix
    } else {
        RulePrecedenceReason::IdTieBreak
    }
}

fn path_pattern(path: &str) -> (&str, bool) {
    path.strip_suffix('*')
        .map_or((path, false), |prefix| (prefix, true))
}

fn path_is_exact(rule: &ResponseRule) -> bool {
    !rule.path.ends_with('*')
}

fn wildcard_prefix_len(rule: &ResponseRule) -> usize {
    rule.path.strip_suffix('*').map_or(0, str::len)
}

/// 새 규칙에는 실제 ID를 부여하고 기존 규칙은 같은 ID로 교체한다.
/// 반환한 ID와 저장된 rule.id가 항상 같아야 context-menu 대상이 안정적이다.
/// 검증에 실패하면 map을 전혀 변경하지 않는다.
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
mod tests {
    use super::*;

    fn rule(id: &str, method: Option<&str>, path: &str) -> ResponseRule {
        ResponseRule {
            id: id.into(),
            priority: 0,
            method: method.map(|s| s.into()),
            path: path.into(),
            status: 200,
            headers: vec![],
            body: String::new(),
            delay_ms: 0,
            sequence: vec![],
        }
    }

    #[test]
    fn matches_exact_and_wildcard() {
        assert!(matches(&rule("r1", Some("POST"), "/hook"), "POST", "/hook"));
        assert!(!matches(&rule("r1", Some("GET"), "/hook"), "POST", "/hook"));
        assert!(matches(&rule("r2", None, "/any"), "DELETE", "/any"));
        assert!(matches(
            &rule("r3", None, "/events/*"),
            "POST",
            "/events/123"
        ));
        assert!(!matches(&rule("r3", None, "/events/*"), "POST", "/other"));
    }

    #[test]
    fn method_matching_is_case_insensitive_and_none_matches_all_methods() {
        assert!(matches(&rule("all", None, "/hook"), "PATCH", "/hook"));
        assert!(matches(
            &rule("post", Some("post"), "/hook"),
            "PoSt",
            "/hook"
        ));
        assert!(!matches(
            &rule("post", Some("post"), "/hook"),
            "PUT",
            "/hook"
        ));
    }

    #[test]
    fn path_matching_is_exact_or_prefix_only_for_a_trailing_star() {
        let exact = rule("exact", None, "/events/123");
        assert!(matches(&exact, "GET", "/events/123"));
        assert!(!matches(&exact, "GET", "/events/123/extra"));
        assert!(!matches(&exact, "GET", "/events/123?source=test"));

        let prefix = rule("prefix", None, "/events/*");
        assert!(matches(&prefix, "GET", "/events/"));
        assert!(matches(&prefix, "GET", "/events/123/extra"));
        assert!(!matches(&prefix, "GET", "/eventslater"));

        // An asterisk anywhere other than the final character is literal.
        let interior_star = rule("interior", None, "/events/*/tail");
        assert!(matches(&interior_star, "GET", "/events/*/tail"));
        assert!(!matches(&interior_star, "GET", "/events/123/tail"));
    }

    #[test]
    fn upsert_assigns_and_preserves_rule_identity() {
        let mut rules = HashMap::new();
        let generated = upsert(&mut rules, rule("", Some("POST"), "/hook")).unwrap();
        assert!(!generated.is_empty());
        assert_eq!(rules[&generated].id, generated);

        let same = upsert(&mut rules, rule(&generated, Some("GET"), "/updated")).unwrap();
        assert_eq!(same, generated);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[&generated].path, "/updated");
    }

    #[test]
    fn validates_status_and_delay_bounds() {
        assert!(validate_rule(&rule("r", None, "/hook")).is_ok());

        let mut invalid = rule("r", None, "/hook");
        invalid.status = MIN_RESPONSE_STATUS - 1;
        assert!(validate_rule(&invalid).is_err());
        invalid.status = MAX_RESPONSE_STATUS + 1;
        assert!(validate_rule(&invalid).is_err());
        invalid.status = MIN_RESPONSE_STATUS;
        invalid.delay_ms = MAX_RESPONSE_DELAY_MS + 1;
        assert!(validate_rule(&invalid).is_err());

        invalid.status = MAX_RESPONSE_STATUS;
        invalid.delay_ms = MAX_RESPONSE_DELAY_MS;
        assert!(validate_rule(&invalid).is_ok());
    }

    #[test]
    fn priority_is_bounded_and_old_json_defaults_to_zero() {
        let old: ResponseRule = serde_json::from_value(serde_json::json!({
            "id": "old",
            "method": null,
            "path": "/hook",
            "status": 200,
            "headers": [],
            "body": "",
            "delayMs": 0
        }))
        .unwrap();
        assert_eq!(old.priority, 0);

        let mut candidate = rule("priority", None, "/hook");
        candidate.priority = MIN_RULE_PRIORITY;
        assert!(validate_rule(&candidate).is_ok());
        candidate.priority = MAX_RULE_PRIORITY;
        assert!(validate_rule(&candidate).is_ok());
        candidate.priority = MAX_RULE_PRIORITY + 1;
        assert!(validate_rule(&candidate).is_err());
        candidate.priority = MIN_RULE_PRIORITY - 1;
        assert!(validate_rule(&candidate).is_err());
    }

    #[test]
    fn selector_is_insertion_order_independent_and_uses_documented_precedence() {
        let mut any_prefix = rule("z-any-prefix", None, "/events/*");
        any_prefix.priority = 5;
        let mut method_prefix = rule("y-method-prefix", Some("POST"), "/events/special/*");
        method_prefix.priority = 5;
        let mut exact = rule("x-exact", None, "/events/special/42");
        exact.priority = 5;
        let mut priority = rule("priority", None, "/events/*");
        priority.priority = 6;
        let rules = [any_prefix, method_prefix, exact, priority];

        for order in [vec![0, 1, 2, 3], vec![3, 2, 1, 0], vec![1, 3, 0, 2]] {
            let selected = select_matching_rule(
                order.iter().map(|index| &rules[*index]),
                "POST",
                "/events/special/42",
            )
            .unwrap();
            assert_eq!(selected.id, "priority");
        }

        let tied = &rules[..3];
        assert_eq!(
            select_matching_rule(tied.iter(), "POST", "/events/special/42")
                .unwrap()
                .id,
            "x-exact"
        );
        assert_eq!(
            select_matching_rule(tied.iter(), "POST", "/events/special/other")
                .unwrap()
                .id,
            "y-method-prefix"
        );
    }

    #[test]
    fn final_tie_break_is_bytewise_ascending_id() {
        let left = rule("alpha", Some("POST"), "/hook");
        let right = rule("beta", Some("POST"), "/hook");
        assert_eq!(
            select_matching_rule([&right, &left], "POST", "/hook")
                .unwrap()
                .id,
            "alpha"
        );
        assert_eq!(
            precedence_reason(&left, &right),
            RulePrecedenceReason::IdTieBreak
        );
    }

    #[test]
    fn conflict_preview_reports_full_and_partial_overlap_without_mutation() {
        let existing = rule("existing", None, "/events/*");
        let mut rules = HashMap::from([(existing.id.clone(), existing)]);
        let before = rules.clone();

        let mut exact = rule("candidate", Some("POST"), "/events/42");
        exact.priority = 1;
        let plan = plan_upsert(&rules, exact).unwrap();
        assert!(plan.preview.requires_confirmation);
        assert_eq!(plan.preview.conflicts.len(), 1);
        assert_eq!(
            plan.preview.conflicts[0].kind,
            RuleConflictKind::PartialOverlap
        );
        assert_eq!(
            plan.preview.conflicts[0].reason,
            RulePrecedenceReason::Priority
        );
        assert_eq!(rules, before);

        let mut covering = rule("covering", None, "/events/*");
        covering.priority = 2;
        let plan = plan_upsert(&rules, covering).unwrap();
        assert_eq!(
            plan.preview.conflicts[0].kind,
            RuleConflictKind::CandidateShadowsExisting
        );

        let unrelated = rule("other", Some("GET"), "/unrelated");
        let plan = plan_upsert(&rules, unrelated).unwrap();
        assert!(!plan.preview.requires_confirmation);
        assert!(plan.preview.conflicts.is_empty());

        // Keep the map mutable in this test so Clippy/rustc also exercise the
        // projected collection without relying on an immutable-only fixture.
        rules.clear();
        assert!(rules.is_empty());
    }

    #[test]
    fn overlap_matrix_handles_methods_exact_paths_and_nested_prefixes() {
        let get = rule("get", Some("GET"), "/events/42");
        let post = rule("post", Some("POST"), "/events/42");
        let any = rule("any", None, "/events/*");
        let nested = rule("nested", Some("GET"), "/events/private/*");
        let elsewhere = rule("elsewhere", None, "/other/*");
        assert!(!rules_overlap(&get, &post));
        assert!(rules_overlap(&get, &any));
        assert!(rules_overlap(&any, &nested));
        assert!(!rules_overlap(&get, &nested));
        assert!(!rules_overlap(&any, &elsewhere));
    }

    #[test]
    fn rejects_transport_response_headers_that_would_override_wire_framing() {
        for name in [
            "Connection",
            "Content-Length",
            "Transfer-Encoding",
            "Upgrade",
            "Host",
        ] {
            let mut candidate = rule("transport", None, "/hook");
            candidate.headers = vec![(name.into(), "1".into())];
            assert!(
                validate_rule(&candidate).is_err(),
                "{name} should be reserved"
            );
        }
    }

    #[test]
    fn validates_method_path_and_body_shape_and_size() {
        let mut invalid = rule("r", Some("POST"), "/hook");
        invalid.method = Some("post-json".into());
        assert!(validate_rule(&invalid).is_ok());
        // Custom HTTP methods use the RFC token grammar; matching remains
        // case-insensitive and does not invent a first-letter restriction.
        invalid.method = Some("!custom".into());
        assert!(validate_rule(&invalid).is_ok());
        invalid.method = Some(String::new());
        assert!(validate_rule(&invalid).is_err());
        invalid.method = Some("POST JSON".into());
        assert!(validate_rule(&invalid).is_err());
        invalid.method = Some("P".repeat(MAX_METHOD_CHARS + 1));
        assert!(validate_rule(&invalid).is_err());

        invalid = rule("r", None, "hook");
        assert!(validate_rule(&invalid).is_err());
        invalid.path = format!("/{}", "p".repeat(MAX_PATH_CHARS));
        assert!(validate_rule(&invalid).is_err());
        invalid.path = "/hook\u{0085}".into();
        assert!(validate_rule(&invalid).is_err());
        invalid.path = "/hook/한글".into();
        assert!(validate_rule(&invalid).is_err());

        invalid = rule("r", None, "/hook");
        invalid.body = "b".repeat(MAX_BODY_CHARS + 1);
        assert!(validate_rule(&invalid).is_err());
        invalid.body = "🙂".repeat(MAX_BODY_BYTES / 4 + 1);
        assert!(validate_rule(&invalid).is_err());
    }

    #[test]
    fn validates_rule_id_character_and_string_bounds() {
        let mut invalid = rule("r", None, "/hook");
        invalid.id = "i".repeat(MAX_RULE_ID_CHARS);
        assert!(validate_rule(&invalid).is_ok());
        invalid.id = "i".repeat(MAX_RULE_ID_CHARS + 1);
        assert!(validate_rule(&invalid).is_err());
        invalid.id = "stable\u{0000}".into();
        assert!(validate_rule(&invalid).is_err());

        invalid.id.clear();
        assert!(validate_rule(&invalid).is_ok());
    }

    #[test]
    fn validates_header_count_shape_and_aggregate_limits() {
        let mut invalid = rule("r", None, "/hook");
        invalid.headers = (0..=MAX_RULE_HEADERS)
            .map(|index| (format!("X-Test-{index}"), "ok".into()))
            .collect();
        assert!(validate_rule(&invalid).is_err());

        invalid.headers = vec![("not a header".into(), "ok".into())];
        assert!(validate_rule(&invalid).is_err());
        invalid.headers = vec![("X-Test".into(), "bad\nvalue".into())];
        assert!(validate_rule(&invalid).is_err());
        invalid.headers = vec![("X-Test".into(), "한글".into())];
        assert!(validate_rule(&invalid).is_err());

        invalid.headers = vec![("X-Test".into(), "v".repeat(MAX_HEADER_VALUE_CHARS + 1))];
        assert!(validate_rule(&invalid).is_err());

        invalid.headers = vec![("N".repeat(MAX_HEADER_NAME_CHARS), "ok".into())];
        assert!(validate_rule(&invalid).is_ok());
        invalid.headers = vec![("N".repeat(MAX_HEADER_NAME_CHARS + 1), "ok".into())];
        assert!(validate_rule(&invalid).is_err());

        invalid.headers = (0..5)
            .map(|index| (format!("X-{index}"), "v".repeat(MAX_HEADER_TOTAL_CHARS / 4)))
            .collect();
        assert!(validate_rule(&invalid).is_err());
    }

    #[test]
    fn rejects_invalid_upsert_without_mutating_storage() {
        let mut rules = HashMap::new();
        let id = upsert(&mut rules, rule("stable", Some("GET"), "/old")).unwrap();
        let before = rules.clone();

        let mut invalid = rule(&id, Some("GET"), "/new");
        invalid.status = 600;
        assert!(upsert(&mut rules, invalid).is_err());
        assert_eq!(rules, before);
    }

    #[test]
    fn enforces_rule_count_and_collection_string_limits() {
        let mut rules = HashMap::new();
        for index in 0..MAX_RULES {
            assert!(upsert(&mut rules, rule(&format!("r-{index}"), None, "/hook")).is_ok());
        }
        let before = rules.clone();
        assert!(upsert(&mut rules, rule("new", None, "/hook")).is_err());
        assert_eq!(rules, before);

        let mut large_rules = HashMap::new();
        for index in 0..(MAX_RULE_COLLECTION_CHARS / MAX_BODY_CHARS + 1) {
            let mut candidate = rule(&format!("large-{index}"), None, "/hook");
            candidate.body = "x".repeat(MAX_BODY_CHARS);
            if index == MAX_RULE_COLLECTION_CHARS / MAX_BODY_CHARS {
                assert!(upsert(&mut large_rules, candidate).is_err());
            } else {
                assert!(upsert(&mut large_rules, candidate).is_ok());
            }
        }
    }

    #[test]
    fn validates_complete_rule_collection() {
        let valid = rule("r", None, "/hook");
        assert!(validate_rule_collection([&valid]).is_ok());

        let invalid = rule("bad", None, "hook");
        assert!(validate_rule_collection([&valid, &invalid]).is_err());

        let too_many: Vec<ResponseRule> = (0..=MAX_RULES)
            .map(|index| rule(&format!("r-{index}"), None, "/hook"))
            .collect();
        assert!(validate_rule_collection(too_many.iter()).is_err());
    }

    #[test]
    fn response_sequence_consumes_in_order_and_holds_the_final_step() {
        let mut candidate = rule("sequence", Some("POST"), "/hook");
        candidate.status = 202;
        candidate.body = "first".into();
        candidate.sequence = vec![
            ResponseSequenceStep {
                status: 500,
                headers: vec![],
                body: "retry".into(),
                delay_ms: 10,
            },
            ResponseSequenceStep {
                status: 204,
                headers: vec![("X-Ready".into(), "yes".into())],
                body: String::new(),
                delay_ms: 0,
            },
        ];

        assert_eq!(candidate.response_at(0).status, 202);
        assert_eq!(candidate.response_at(1).body, "retry");
        assert_eq!(candidate.response_at(2).status, 204);
        assert_eq!(candidate.response_at(99).status, 204);
        assert!(validate_rule(&candidate).is_ok());
    }

    #[test]
    fn response_sequence_is_bounded_and_validated_like_the_base_response() {
        let mut candidate = rule("sequence", None, "/hook");
        candidate.sequence = vec![
            ResponseSequenceStep {
                status: 200,
                headers: vec![],
                body: String::new(),
                delay_ms: 0,
            };
            MAX_RESPONSE_SEQUENCE + 1
        ];
        assert!(validate_rule(&candidate).is_err());

        candidate.sequence = vec![ResponseSequenceStep {
            status: 600,
            headers: vec![],
            body: String::new(),
            delay_ms: 0,
        }];
        assert!(validate_rule(&candidate).is_err());

        candidate.sequence = vec![ResponseSequenceStep {
            status: 200,
            headers: vec![("X-Bad".into(), "line\nfeed".into())],
            body: String::new(),
            delay_ms: 0,
        }];
        assert!(validate_rule(&candidate).is_err());
    }

    #[test]
    fn response_sequence_state_reset_starts_at_the_base_response() {
        let mut candidate = rule("sequence", Some("POST"), "/hook");
        candidate.body = "first".into();
        candidate.sequence = vec![ResponseSequenceStep {
            status: 500,
            headers: vec![],
            body: "retry".into(),
            delay_ms: 0,
        }];
        let mut state = ResponseSequenceState::default();
        assert_eq!(state.next_response(&candidate).body, "first");
        assert_eq!(state.next_response(&candidate).body, "retry");
        assert_eq!(state.next_response(&candidate).body, "retry");
        state.reset(&candidate.id);
        assert_eq!(state.next_response(&candidate).body, "first");
    }

    #[test]
    fn matcher_rejects_non_ascii_paths_even_for_unvalidated_rules() {
        let candidate = rule("unicode", None, "/hook/한글");
        assert!(!matches(&candidate, "GET", "/hook/한글"));
        assert!(!matches(&rule("ascii", None, "/hook"), "GET", "/hook/한글"));
    }
}
