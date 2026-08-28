//! Masked captured-request fixtures.
//!
//! Fixtures are deliberately an app-owned, one-file store.  The only public
//! input to the command layer is an opaque in-memory history ID; callers never
//! provide a filesystem path or a request body.  This module owns the second
//! safety boundary: every field is bounded and secrets, unsafe targets, and
//! token-shaped values are removed before bytes can reach disk.

use super::history::RequestRecord;
use super::rules::ResponseRule;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const FIXTURE_FILE_NAME: &str = "fixtures.json";
pub const FIXTURE_SCHEMA_VERSION: u32 = 1;
pub const MAX_FIXTURES: usize = 200;
pub const MAX_FIXTURE_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FIXTURE_ID_CHARS: usize = 128;
pub const MAX_FIXTURE_ID_BYTES: usize = 128;
pub const MAX_FIXTURE_METHOD_CHARS: usize = 16;
pub const MAX_FIXTURE_METHOD_BYTES: usize = 16;
pub const MAX_FIXTURE_URL_CHARS: usize = 4_096;
pub const MAX_FIXTURE_URL_BYTES: usize = 16_384;
pub const MAX_FIXTURE_HEADERS: usize = 100;
pub const MAX_FIXTURE_HEADER_NAME_CHARS: usize = 256;
pub const MAX_FIXTURE_HEADER_NAME_BYTES: usize = 256;
pub const MAX_FIXTURE_HEADER_VALUE_CHARS: usize = 16_384;
pub const MAX_FIXTURE_HEADER_VALUE_BYTES: usize = 65_536;
pub const MAX_FIXTURE_HEADER_TOTAL_CHARS: usize = 64_000;
pub const MAX_FIXTURE_HEADER_TOTAL_BYTES: usize = 256_000;
pub const MAX_FIXTURE_BODY_CHARS: usize = 256_000;
pub const MAX_FIXTURE_BODY_BYTES: usize = 1_024_000;
const MAX_REFERENCE_NAME_CHARS: usize = 128;
/// Keep persisted timestamps inside both JavaScript's safe-integer range and
/// the range accepted by `Date#toISOString`, so a validated fixture cannot
/// make the renderer throw while formatting its capture time.
pub const MIN_FIXTURE_TIMESTAMP_MS: i64 = -8_640_000_000_000_000;
pub const MAX_FIXTURE_TIMESTAMP_MS: i64 = 8_640_000_000_000_000;
pub const REDACTED: &str = "[REDACTED]";
pub const REDACTED_PATH: &str = "/[REDACTED_PATH]";

pub const FIXTURE_READ_ERROR: &str = "fixture 저장소를 읽을 수 없습니다";
pub const FIXTURE_WRITE_ERROR: &str = "fixture 저장소를 저장할 수 없습니다";
pub const FIXTURE_SIZE_ERROR: &str = "fixture 저장소 크기 제한을 초과했습니다";
pub const FIXTURE_CONFLICT_ERROR: &str =
    "fixture 저장소가 다른 작업으로 변경되었습니다. 다시 시도하세요";
pub const FIXTURE_NOT_FOUND_ERROR: &str = "fixture를 찾을 수 없습니다";
pub const FIXTURE_INPUT_ERROR: &str = "fixture 입력이 유효하지 않습니다";

static FIXTURE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureError {
    Read,
    Write,
    Size,
    Conflict,
    NotFound,
    Invalid,
    Path,
}

impl FixtureError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::Read => FIXTURE_READ_ERROR,
            Self::Write => FIXTURE_WRITE_ERROR,
            Self::Size => FIXTURE_SIZE_ERROR,
            Self::Conflict => FIXTURE_CONFLICT_ERROR,
            Self::NotFound => FIXTURE_NOT_FOUND_ERROR,
            Self::Invalid => FIXTURE_INPUT_ERROR,
            Self::Path => FIXTURE_WRITE_ERROR,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapturedFixture {
    pub id: String,
    pub method: String,
    /// A masked origin-form request target.  This field is never an absolute
    /// URL and may contain the fixed REDACTED_PATH marker.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub received_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureDocument {
    pub schema_version: u32,
    pub next_id: u64,
    pub fixtures: Vec<CapturedFixture>,
}

impl Default for FixtureDocument {
    fn default() -> Self {
        Self {
            schema_version: FIXTURE_SCHEMA_VERSION,
            next_id: 1,
            fixtures: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFixtureDocument {
    pub document: FixtureDocument,
    /// Exact bytes observed before the caller edits the document.  A writer
    /// uses this as a small compare-and-swap token for cross-process edits.
    pub raw: Option<Vec<u8>>,
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    oversized: bool,
}

impl BoundedJsonWriter {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_FIXTURE_FILE_BYTES.min(8 * 1024)),
            oversized: false,
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > MAX_FIXTURE_FILE_BYTES {
            self.oversized = true;
            return Err(io::Error::other("fixture JSON exceeds its size bound"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialize_document(document: &FixtureDocument) -> Result<Vec<u8>, FixtureError> {
    let mut writer = BoundedJsonWriter::new();
    match serde_json::to_writer(&mut writer, document) {
        Ok(()) => Ok(writer.bytes),
        Err(_) if writer.oversized => Err(FixtureError::Size),
        Err(_) => Err(FixtureError::Invalid),
    }
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn within(value: &str, max_chars: usize, max_bytes: usize) -> bool {
    value.chars().count() <= max_chars && value.len() <= max_bytes
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

fn is_reference(value: &str) -> bool {
    // A reference is safe to preserve only when the complete value is a
    // small identifier.  Merely checking the delimiters would preserve a
    // literal such as `${raw-secret}` (or an arbitrarily long value) in a
    // sensitive field and mistake it for an environment reference.
    if value.trim() != value {
        return false;
    }
    let name = if value.starts_with("${") && value.ends_with('}') && value.len() >= 4 {
        &value[2..value.len() - 1]
    } else if value.starts_with("{{") && value.ends_with("}}") && value.len() >= 5 {
        &value[2..value.len() - 2]
    } else {
        return false;
    };
    !name.is_empty()
        && name.len() <= MAX_REFERENCE_NAME_CHARS
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn contains_reference_marker(value: &str) -> bool {
    value.contains("${") || value.contains("{{")
}

fn sensitive_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let compact: String = lower
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
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

fn known_token(value: &str) -> bool {
    let candidates = value.split(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-' | '.')
    });
    for candidate in candidates.filter(|candidate| !candidate.is_empty()) {
        for prefix in [
            "sk-",
            "ghp_",
            "github_pat_",
            "glpat-",
            "xoxb-",
            "xoxa-",
            "xoxp-",
        ] {
            if let Some(suffix) = candidate.strip_prefix(prefix) {
                if suffix.len() >= 12 {
                    return true;
                }
            }
        }
        if candidate.len() == 20 && candidate.starts_with("AKIA") {
            return true;
        }
        let mut segments = candidate.split('.');
        let first = segments.next();
        let second = segments.next();
        let third = segments.next();
        if third.is_some()
            && segments.next().is_none()
            && [first, second, third].into_iter().flatten().all(|segment| {
                segment.len() >= 10
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            })
        {
            return true;
        }
    }
    value.contains("-----BEGIN ") && value.contains("PRIVATE KEY-----")
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            let high = hex_digit(bytes[index + 1])?;
            let low = hex_digit(bytes[index + 2])?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn unsafe_path(path: &str) -> bool {
    if path.is_empty()
        || path != path.trim()
        || !path.starts_with('/')
        || path.starts_with("//")
        || contains_reference_marker(path)
        || path.contains('\\')
        || path.contains('#')
        || has_control(path)
        || !within(path, MAX_FIXTURE_URL_CHARS, MAX_FIXTURE_URL_BYTES)
    {
        return true;
    }
    let Some(decoded) = percent_decode(path) else {
        return true;
    };
    if decoded.starts_with("//")
        || contains_reference_marker(&decoded)
        || decoded.contains('\\')
        || has_control(&decoded)
    {
        return true;
    }
    decoded
        .split('/')
        .any(|segment| segment == ".." || segment == ".")
        || decoded.split('/').any(known_token)
}

/// Return a safe origin-form target.  An unsafe pathname is deliberately
/// replaced with a fixed marker; preserving an untrusted local path in the
/// fixture would make the masked store itself a path disclosure surface.
pub fn sanitize_target(target: &str) -> String {
    if target.is_empty() || target.len() > MAX_FIXTURE_URL_BYTES || target != target.trim() {
        return REDACTED_PATH.to_string();
    }
    let query_start = target.find('?');
    let pathname = query_start.map_or(target, |index| &target[..index]);
    if unsafe_path(pathname) {
        return REDACTED_PATH.to_string();
    }
    let Some(query) = query_start.map(|index| &target[index + 1..]) else {
        return pathname.to_string();
    };
    if has_control(query) {
        return REDACTED_PATH.to_string();
    }

    let mut safe_query = Vec::new();
    for component in query.split('&') {
        if component.is_empty() {
            safe_query.push(String::new());
            continue;
        }
        let separator = component.find('=');
        let raw_key = separator.map_or(component, |index| &component[..index]);
        let raw_value = separator.map_or("", |index| &component[index + 1..]);
        let Some(key) = percent_decode(raw_key) else {
            return REDACTED_PATH.to_string();
        };
        let Some(value) = percent_decode(raw_value) else {
            return REDACTED_PATH.to_string();
        };
        if key.is_empty()
            || contains_reference_marker(raw_key)
            || contains_reference_marker(&key)
            || key.chars().any(char::is_control)
            || key.chars().any(char::is_whitespace)
            || value.chars().any(char::is_control)
            || value.chars().any(char::is_whitespace)
            || known_token(&key)
        {
            return REDACTED_PATH.to_string();
        }
        let masked_value = if sensitive_name(&key)
            || known_token(&value)
            || !is_reference(&value) && value.len() > 256
        {
            REDACTED.to_string()
        } else {
            raw_value.to_string()
        };
        if separator.is_some() {
            safe_query.push(format!("{raw_key}={masked_value}"));
        } else if masked_value == REDACTED {
            safe_query.push(format!("{raw_key}={REDACTED}"));
        } else {
            safe_query.push(raw_key.to_string());
        }
    }
    let result = format!("{pathname}?{}", safe_query.join("&"));
    if within(&result, MAX_FIXTURE_URL_CHARS, MAX_FIXTURE_URL_BYTES) {
        result
    } else {
        REDACTED_PATH.to_string()
    }
}

fn redact_scheme_tokens(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let remaining = &value[index..];
        let scheme_len = if remaining.len() >= 7
            && remaining.as_bytes()[..7].eq_ignore_ascii_case(b"Bearer ")
        {
            Some(7)
        } else if remaining.len() >= 6 && remaining.as_bytes()[..6].eq_ignore_ascii_case(b"Basic ")
        {
            Some(6)
        } else {
            None
        };
        if let Some(prefix_len) = scheme_len {
            output.push_str(&remaining[..prefix_len]);
            index += prefix_len;
            let token_start = index;
            while index < bytes.len()
                && !bytes[index].is_ascii_whitespace()
                && !b",;&\"'".contains(&bytes[index])
            {
                index += 1;
            }
            let token = &value[token_start..index];
            if is_reference(token) {
                output.push_str(token);
            } else if !token.is_empty() {
                output.push_str(REDACTED);
            }
            continue;
        }
        let character = value[index..].chars().next().unwrap_or_default();
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn sensitive_assignment(value: &str) -> bool {
    let bytes = value.as_bytes();
    for index in 0..bytes.len() {
        if index > 0
            && (bytes[index - 1].is_ascii_alphanumeric()
                || bytes[index - 1] == b'_'
                || bytes[index - 1] == b'-')
        {
            continue;
        }
        let mut end = index;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'-')
        {
            end += 1;
        }
        if end == index || !sensitive_name(&value[index..end]) {
            continue;
        }
        let mut separator = end;
        if bytes
            .get(separator)
            .is_some_and(|byte| matches!(byte, b'"' | b'\''))
        {
            separator += 1;
            // A quoted key is common in JSON-like text, including malformed
            // JSON that cannot go through the structured sanitizer below.
        }
        while separator < bytes.len() && bytes[separator].is_ascii_whitespace() {
            separator += 1;
        }
        if separator < bytes.len() && (bytes[separator] == b'=' || bytes[separator] == b':') {
            let mut value_start = separator + 1;
            while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                value_start += 1;
            }
            let quote = bytes
                .get(value_start)
                .copied()
                .filter(|byte| matches!(byte, b'"' | b'\''));
            if quote.is_some() {
                value_start += 1;
            }
            let mut value_end = value_start;
            while value_end < bytes.len() {
                if let Some(quote) = quote {
                    if bytes[value_end] == quote {
                        break;
                    }
                } else if b"&;, \t\r\n\"'".contains(&bytes[value_end]) {
                    break;
                }
                value_end += 1;
            }
            let assigned = &value[value_start..value_end];
            if !assigned.is_empty() && !is_reference(assigned) {
                return true;
            }
        }
    }
    false
}

fn redact_text(value: &str) -> String {
    if value.contains("-----BEGIN ") && value.contains("PRIVATE KEY-----") {
        return REDACTED.to_string();
    }
    if sensitive_assignment(value) {
        return REDACTED.to_string();
    }
    let output = redact_scheme_tokens(value);
    // Replace token-shaped segments in one pass.  Repeatedly calling
    // `String::replace` for every candidate made a body containing many
    // distinct tokens scan the whole body once per token (quadratic work) and
    // allocated an intermediate String for every pass.  The body is bounded,
    // but it is still untrusted input, so keep the redaction pass linear in
    // the number of UTF-8 bytes.
    let mut redacted = String::with_capacity(output.len());
    let mut segment_start = 0;
    for (index, character) in output.char_indices() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
            continue;
        }
        append_redacted_segment(&mut redacted, &output[segment_start..index]);
        redacted.push(character);
        segment_start = index + character.len_utf8();
    }
    append_redacted_segment(&mut redacted, &output[segment_start..]);
    redacted
}

fn append_redacted_segment(output: &mut String, segment: &str) {
    if segment.is_empty() {
        return;
    }
    if known_token(segment) {
        output.push_str(REDACTED);
    } else {
        output.push_str(segment);
    }
}

#[derive(Default)]
struct JsonBudget {
    nodes: usize,
}

fn sanitize_json(
    value: Value,
    key: &str,
    depth: usize,
    budget: &mut JsonBudget,
) -> Result<Value, FixtureError> {
    if depth > 32 || budget.nodes >= 10_000 {
        return Err(FixtureError::Size);
    }
    budget.nodes += 1;
    match value {
        Value::String(text) => {
            if text.chars().count() > MAX_FIXTURE_BODY_CHARS {
                return Err(FixtureError::Size);
            }
            if sensitive_name(key) {
                Ok(if is_reference(&text) {
                    Value::String(text)
                } else {
                    Value::String(REDACTED.to_string())
                })
            } else {
                Ok(Value::String(redact_text(&text)))
            }
        }
        Value::Array(items) => items
            .into_iter()
            .map(|item| sanitize_json(item, "", depth + 1, budget))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(entries) => entries
            .into_iter()
            .map(|(child_key, child)| {
                if child_key.chars().count() > MAX_FIXTURE_BODY_CHARS || has_control(&child_key) {
                    return Err(FixtureError::Invalid);
                }
                let child = if sensitive_name(&child_key) {
                    if let Value::String(text) = child {
                        if is_reference(&text) {
                            Value::String(text)
                        } else {
                            Value::String(REDACTED.to_string())
                        }
                    } else {
                        Value::String(REDACTED.to_string())
                    }
                } else {
                    sanitize_json(child, &child_key, depth + 1, budget)?
                };
                Ok((child_key, child))
            })
            .collect::<Result<serde_json::Map<_, _>, _>>()
            .map(Value::Object),
        other => Ok(other),
    }
}

pub fn sanitize_body(body: &str) -> Result<String, FixtureError> {
    if !within(body, MAX_FIXTURE_BODY_CHARS, MAX_FIXTURE_BODY_BYTES) {
        return Err(FixtureError::Size);
    }
    if body.is_empty() {
        return Ok(String::new());
    }
    let sanitized = match serde_json::from_str::<Value>(body) {
        Ok(value) => {
            serde_json::to_string(&sanitize_json(value, "", 0, &mut JsonBudget::default())?)
                .map_err(|_| FixtureError::Invalid)?
        }
        Err(_) => redact_text(body),
    };
    if within(&sanitized, MAX_FIXTURE_BODY_CHARS, MAX_FIXTURE_BODY_BYTES) {
        Ok(sanitized)
    } else {
        Err(FixtureError::Size)
    }
}

/// Sanitize a captured body before applying the history display cap.  A raw
/// prefix can cut a credential/token in half, so redaction must see the whole
/// bounded request body first.  The server admission layer caps the input at
/// `MAX_FIXTURE_BODY_BYTES + 1`; this helper then applies the renderer's
/// character/byte prefix bound without exposing a token-shaped suffix.
pub fn sanitize_body_for_history(body: &str) -> String {
    // `History::push` is normally fed by the listener's bounded reader, but
    // keep this public core boundary safe for direct callers too. Returning a
    // fixed marker avoids scanning or allocating in proportion to an
    // arbitrarily large input that could never fit a fixture/history entry.
    if body.len() > MAX_FIXTURE_BODY_BYTES {
        return REDACTED.to_string();
    }
    if within(body, MAX_FIXTURE_BODY_CHARS, MAX_FIXTURE_BODY_BYTES) {
        return sanitize_body(body).unwrap_or_else(|_| REDACTED.to_string());
    }

    // Once the display character cap is exceeded, parsing structured JSON is
    // still important for privacy: escaped sensitive keys (for example
    // `"\\u0070assword"`) are invisible to the text redactor.  Keep the
    // structured path bounded by the same depth/node/string limits; if it
    // cannot be sanitized safely, return one fixed marker instead of exposing
    // a prefix of a credential-bearing payload.
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            let mut budget = JsonBudget::default();
            let Ok(value) = sanitize_json(value, "", 0, &mut budget) else {
                return REDACTED.to_string();
            };
            let Ok(serialized) = serde_json::to_string(&value) else {
                return REDACTED.to_string();
            };
            return bounded_history_prefix(&serialized);
        }
        // A JSON-looking but malformed body has no reliable key/value
        // boundary.  Do not let a large malformed object bypass the
        // structured sanitizer and leak an escaped or oddly-delimited secret.
        return REDACTED.to_string();
    }

    let redacted = redact_text(body);
    bounded_history_prefix(&redacted)
}

fn bounded_history_prefix(value: &str) -> String {
    let mut bounded = String::new();
    for character in value.chars().take(MAX_FIXTURE_BODY_CHARS) {
        if bounded.len().saturating_add(character.len_utf8()) > MAX_FIXTURE_BODY_BYTES {
            break;
        }
        bounded.push(character);
    }
    bounded
}

pub fn sanitize_headers(
    headers: &[(String, String)],
) -> Result<Vec<(String, String)>, FixtureError> {
    if headers.len() > MAX_FIXTURE_HEADERS {
        return Err(FixtureError::Size);
    }
    let mut total_chars = 0usize;
    let mut total_bytes = 0usize;
    let mut sanitized = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        if !within(
            name,
            MAX_FIXTURE_HEADER_NAME_CHARS,
            MAX_FIXTURE_HEADER_NAME_BYTES,
        ) || !is_token(name)
            || has_control(name)
            || !within(
                value,
                MAX_FIXTURE_HEADER_VALUE_CHARS,
                MAX_FIXTURE_HEADER_VALUE_BYTES,
            )
            || (!value.is_ascii() && !sensitive_name(name))
            || has_control(value)
        {
            return Err(FixtureError::Invalid);
        }
        let safe_value = if sensitive_name(name) {
            REDACTED.to_string()
        } else {
            redact_text(value)
        };
        total_chars = total_chars
            .checked_add(name.chars().count())
            .and_then(|total| total.checked_add(safe_value.chars().count()))
            .ok_or(FixtureError::Size)?;
        total_bytes = total_bytes
            .checked_add(name.len())
            .and_then(|total| total.checked_add(safe_value.len()))
            .ok_or(FixtureError::Size)?;
        if total_chars > MAX_FIXTURE_HEADER_TOTAL_CHARS
            || total_bytes > MAX_FIXTURE_HEADER_TOTAL_BYTES
        {
            return Err(FixtureError::Size);
        }
        sanitized.push((name.clone(), safe_value));
    }
    Ok(sanitized)
}

fn valid_id(id: &str) -> bool {
    if !within(id, MAX_FIXTURE_ID_CHARS, MAX_FIXTURE_ID_BYTES) || id.is_empty() || has_control(id) {
        return false;
    }
    let Some(suffix) = id.strip_prefix("fixture-") else {
        return false;
    };
    !suffix.is_empty()
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
        && suffix.parse::<u64>().is_ok_and(|number| number > 0)
}

fn valid_method(method: &str) -> bool {
    within(method, MAX_FIXTURE_METHOD_CHARS, MAX_FIXTURE_METHOD_BYTES)
        && is_token(method)
        && !has_control(method)
}

pub fn validate_fixture(fixture: &CapturedFixture) -> Result<(), FixtureError> {
    if !valid_id(&fixture.id) || !valid_method(&fixture.method) {
        return Err(FixtureError::Invalid);
    }
    if !(MIN_FIXTURE_TIMESTAMP_MS..=MAX_FIXTURE_TIMESTAMP_MS).contains(&fixture.received_at_ms) {
        return Err(FixtureError::Invalid);
    }
    if sanitize_target(&fixture.url) != fixture.url {
        return Err(FixtureError::Invalid);
    }
    if sanitize_headers(&fixture.headers)? != fixture.headers {
        return Err(FixtureError::Invalid);
    }
    if sanitize_body(&fixture.body)? != fixture.body {
        return Err(FixtureError::Invalid);
    }
    Ok(())
}

pub fn validate_document(document: &FixtureDocument) -> Result<(), FixtureError> {
    if document.schema_version != FIXTURE_SCHEMA_VERSION
        || document.next_id == 0
        || document.fixtures.len() > MAX_FIXTURES
    {
        return Err(FixtureError::Invalid);
    }
    let mut ids = HashSet::with_capacity(document.fixtures.len());
    let mut max_id = 0u64;
    for fixture in &document.fixtures {
        validate_fixture(fixture)?;
        if !ids.insert(fixture.id.clone()) {
            return Err(FixtureError::Invalid);
        }
        if let Ok(id) = fixture.id[8..].parse::<u64>() {
            max_id = max_id.max(id);
        }
    }
    if document.next_id <= max_id {
        return Err(FixtureError::Invalid);
    }
    let _ = serialize_document(document)?;
    Ok(())
}

pub fn fixture_from_request(
    id: String,
    request: &RequestRecord,
) -> Result<CapturedFixture, FixtureError> {
    if !valid_id(&id) {
        return Err(FixtureError::Invalid);
    }
    let method = if valid_method(&request.method) {
        request.method.to_ascii_uppercase()
    } else {
        "POST".to_string()
    };
    let fixture = CapturedFixture {
        id,
        method,
        url: sanitize_target(&request.url),
        headers: sanitize_headers(&request.headers)?,
        body: sanitize_body(&request.body)?,
        received_at_ms: request.received_at_ms,
    };
    validate_fixture(&fixture)?;
    Ok(fixture)
}

/// Convert a safe fixture into an editable response-rule draft.  This only
/// fills a local editor draft; it does not persist a rule or send a handoff.
pub fn response_rule_draft(fixture: &CapturedFixture) -> Result<ResponseRule, FixtureError> {
    validate_fixture(fixture)?;
    Ok(ResponseRule {
        id: String::new(),
        method: Some(fixture.method.clone()),
        path: fixture.url.clone(),
        status: 200,
        headers: Vec::new(),
        body: String::new(),
        delay_ms: 0,
        sequence: Vec::new(),
    })
}

pub fn fixture_path_from_dir(directory: &Path) -> PathBuf {
    directory.join(FIXTURE_FILE_NAME)
}

fn is_link(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

/// Open the final store component without following a symlink/reparse point.
/// The metadata check in `read_raw` is useful for diagnostics, but it is not a
/// sufficient TOCTOU defence on its own: a path can be swapped between that
/// check and `File::open`. Keep the no-follow flag on the actual read too.
fn open_store_read(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        #[cfg(target_os = "linux")]
        const NO_FOLLOW: i32 = 0x20000; // O_NOFOLLOW
        #[cfg(target_os = "macos")]
        const NO_FOLLOW: i32 = 0x100; // O_NOFOLLOW
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        const NO_FOLLOW: i32 = 0;

        OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW)
            .open(path)
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        // FILE_FLAG_OPEN_REPARSE_POINT asks CreateFileW to open the final
        // reparse point itself instead of following it.
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        OpenOptions::new()
            .read(true)
            .custom_flags(OPEN_REPARSE_POINT)
            .open(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        File::open(path)
    }
}

fn read_raw(path: &Path) -> Result<Option<Vec<u8>>, FixtureError> {
    validate_parent(path, false)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(FixtureError::Read),
    };
    if is_link(&metadata) || !metadata.file_type().is_file() {
        return Err(FixtureError::Read);
    }
    if metadata.len() > MAX_FIXTURE_FILE_BYTES as u64 {
        return Err(FixtureError::Size);
    }
    let file = open_store_read(path).map_err(|_| FixtureError::Read)?;
    let opened_metadata = file.metadata().map_err(|_| FixtureError::Read)?;
    if is_link(&opened_metadata) || !opened_metadata.file_type().is_file() {
        return Err(FixtureError::Read);
    }
    let mut bytes = Vec::new();
    file.take((MAX_FIXTURE_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| FixtureError::Read)?;
    if bytes.len() > MAX_FIXTURE_FILE_BYTES {
        return Err(FixtureError::Size);
    }
    Ok(Some(bytes))
}

fn parse_raw(raw: Option<Vec<u8>>) -> Result<LoadedFixtureDocument, FixtureError> {
    let Some(bytes) = raw else {
        return Ok(LoadedFixtureDocument {
            document: FixtureDocument::default(),
            raw: None,
        });
    };
    let document =
        serde_json::from_slice::<FixtureDocument>(&bytes).map_err(|_| FixtureError::Read)?;
    validate_document(&document)?;
    Ok(LoadedFixtureDocument {
        document,
        raw: Some(bytes),
    })
}

pub fn load_document_with_raw(path: &Path) -> Result<LoadedFixtureDocument, FixtureError> {
    parse_raw(read_raw(path)?)
}

#[cfg(test)]
pub fn load_document(path: &Path) -> Result<FixtureDocument, FixtureError> {
    load_document_with_raw(path).map(|loaded| loaded.document)
}

fn validate_parent(path: &Path, create_missing: bool) -> Result<(), FixtureError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(FixtureError::Path);
    }

    let parent = path.parent().ok_or(FixtureError::Path)?;
    let mut current = PathBuf::new();
    let mut missing = false;

    for component in parent.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                current.push(component.as_os_str());
            }
            Component::CurDir | Component::ParentDir => return Err(FixtureError::Path),
        }

        // A Windows drive prefix is not a complete absolute path until its
        // root component is appended.
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        if missing && !create_missing {
            continue;
        }

        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound && create_missing => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(FixtureError::Path),
                }
                fs::symlink_metadata(&current).map_err(|_| FixtureError::Path)?
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing = true;
                continue;
            }
            Err(_) => return Err(FixtureError::Path),
        };

        if is_link(&metadata) || !metadata.file_type().is_dir() {
            return Err(FixtureError::Path);
        }
    }

    Ok(())
}

fn target_is_owned_file(path: &Path) -> Result<(), FixtureError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link(&metadata) || !metadata.file_type().is_file() => {
            Err(FixtureError::Path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(FixtureError::Path),
    }
}

pub fn save_document_if_current(
    path: &Path,
    expected_raw: Option<&[u8]>,
    document: &FixtureDocument,
) -> Result<(), FixtureError> {
    // The compare-and-swap check and atomic rename must be one process-local
    // critical section. The byte token still detects edits from another
    // process, while this gate prevents two commands in this process from
    // both observing the same old bytes and subsequently overwriting each
    // other.
    let _write_guard = FIXTURE_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| FixtureError::Write)?;
    validate_document(document)?;
    let current = read_raw(path)?;
    if current.as_deref() != expected_raw {
        return Err(FixtureError::Conflict);
    }
    validate_parent(path, true)?;
    target_is_owned_file(path)?;
    let bytes = serialize_document(document)?;
    devbox_filesystem::atomic_write(path, &bytes).map_err(|_| FixtureError::Write)
}

#[cfg(test)]
pub fn save_document(path: &Path, document: &FixtureDocument) -> Result<(), FixtureError> {
    let loaded = load_document_with_raw(path)?;
    save_document_if_current(path, loaded.raw.as_deref(), document)
}

pub fn sorted_fixtures(document: &FixtureDocument) -> Vec<CapturedFixture> {
    let mut fixtures = document.fixtures.clone();
    fixtures.sort_by(|left, right| {
        right
            .received_at_ms
            .cmp(&left.received_at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    fixtures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::history::History;
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn request(url: &str, body: &str) -> RequestRecord {
        RequestRecord {
            id: 1,
            method: "post".into(),
            url: url.into(),
            headers: vec![
                ("Authorization".into(), "•••••".into()),
                ("X-Trace".into(), "trace-123".into()),
            ],
            body: body.into(),
            received_at_ms: 100,
        }
    }

    fn fixture(number: u64, received_at_ms: i64) -> CapturedFixture {
        CapturedFixture {
            id: format!("fixture-{number}"),
            method: "POST".into(),
            url: "/hook".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: "{\"ok\":true}".into(),
            received_at_ms,
        }
    }

    #[test]
    fn capture_masks_headers_query_body_and_unsafe_path() {
        let captured = fixture_from_request(
            "fixture-1".into(),
            &RequestRecord {
                id: 1,
                method: "post".into(),
                url: "/hooks/../secret?token=top-secret&ok=yes".into(),
                headers: vec![
                    ("Authorization".into(), "•••••".into()),
                    ("X-Trace-Token".into(), "trace-secret".into()),
                ],
                body: r#"{"event":"push","password":"body-secret","ok":true}"#.into(),
                received_at_ms: 1,
            },
        )
        .unwrap();
        assert_eq!(captured.url, REDACTED_PATH);
        assert_eq!(captured.headers[0].1, REDACTED);
        assert_eq!(captured.headers[1].1, REDACTED);
        assert!(!serde_json::to_string(&captured)
            .unwrap()
            .contains("body-secret"));
        assert!(captured.body.contains(REDACTED));
    }

    #[test]
    fn safe_queries_are_preserved_but_sensitive_values_are_masked() {
        let captured = fixture_from_request(
            "fixture-1".into(),
            &request("/hook?event=push&access_token=secret", "plain body"),
        )
        .unwrap();
        assert_eq!(captured.url, "/hook?event=push&access_token=[REDACTED]");
        assert_eq!(captured.body, "plain body");
    }

    #[test]
    fn path_and_query_keys_never_preserve_placeholder_markers() {
        assert_eq!(sanitize_target("/hook/${TOKEN}"), REDACTED_PATH);
        assert_eq!(sanitize_target("/hook?${TOKEN}=value"), REDACTED_PATH);
        assert_eq!(
            sanitize_target("/hook?value=${TOKEN}"),
            "/hook?value=${TOKEN}"
        );
    }

    #[test]
    fn sensitive_fields_preserve_only_bounded_reference_names() {
        let invalid = sanitize_body(r#"{"password":"${bad value}"}"#).unwrap();
        assert_eq!(invalid, r#"{"password":"[REDACTED]"}"#);

        let long_name = format!("${{{}}}", "A".repeat(MAX_REFERENCE_NAME_CHARS + 1));
        let body = format!(r#"{{"password":"{long_name}"}}"#);
        assert_eq!(
            sanitize_body(&body).unwrap(),
            r#"{"password":"[REDACTED]"}"#
        );

        assert_eq!(
            sanitize_body(r#"{"password":"{{A}}"}"#).unwrap(),
            r#"{"password":"{{A}}"}"#
        );
        // The redactor must not slice a UTF-8 string at a byte offset while
        // looking for an ASCII auth scheme.
        assert_eq!(
            sanitize_body("🙂 ordinary body").unwrap(),
            "🙂 ordinary body"
        );
    }

    #[test]
    fn encoded_control_query_keys_fail_closed() {
        assert_eq!(sanitize_target("/hook?%00=present"), REDACTED_PATH);
    }

    #[test]
    fn malformed_json_still_redacts_quoted_sensitive_assignments() {
        let sanitized = sanitize_body(r#"{"token":"raw-body-secret""#).unwrap();
        assert_eq!(sanitized, REDACTED);
        assert!(!sanitized.contains("raw-body-secret"));
    }

    #[test]
    fn malformed_and_oversized_input_fails_without_a_partial_fixture() {
        let mut too_big = request("/hook", "");
        too_big.body = "x".repeat(MAX_FIXTURE_BODY_CHARS + 1);
        assert_eq!(
            fixture_from_request("fixture-1".into(), &too_big),
            Err(FixtureError::Size)
        );

        let mut too_many = request("/hook", "");
        too_many.headers = (0..MAX_FIXTURE_HEADERS + 1)
            .map(|index| (format!("X-{index}"), "ok".into()))
            .collect();
        assert_eq!(
            fixture_from_request("fixture-1".into(), &too_many),
            Err(FixtureError::Size)
        );
    }

    #[test]
    fn timestamp_is_bounded_for_safe_renderer_formatting() {
        let mut outside = fixture(1, MAX_FIXTURE_TIMESTAMP_MS + 1);
        assert_eq!(validate_fixture(&outside), Err(FixtureError::Invalid));

        outside.received_at_ms = MIN_FIXTURE_TIMESTAMP_MS - 1;
        assert_eq!(validate_fixture(&outside), Err(FixtureError::Invalid));
    }

    #[test]
    fn fixture_ids_require_a_positive_numeric_suffix() {
        for id in [
            "fixture-",
            "fixture-0",
            "fixture-1x",
            "fixture-999999999999999999999999",
        ] {
            let mut candidate = fixture(1, 1);
            candidate.id = id.to_string();
            assert_eq!(validate_fixture(&candidate), Err(FixtureError::Invalid));
        }
    }

    #[test]
    fn fixture_to_rule_draft_is_local_and_has_empty_response_metadata() {
        let draft = response_rule_draft(&fixture(1, 1)).unwrap();
        assert_eq!(draft.id, "");
        assert_eq!(draft.method.as_deref(), Some("POST"));
        assert_eq!(draft.path, "/hook");
        assert_eq!(draft.status, 200);
        assert!(draft.headers.is_empty());
        assert!(draft.body.is_empty());
        assert_eq!(draft.delay_ms, 0);
    }

    #[test]
    fn corrupt_and_oversized_files_are_rejected_without_rewrite() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FIXTURE_FILE_NAME);
        fs::write(&path, b"{not-json").unwrap();
        let before = fs::read(&path).unwrap();
        assert_eq!(load_document(&path), Err(FixtureError::Read));
        assert_eq!(
            save_document(&path, &FixtureDocument::default()),
            Err(FixtureError::Read)
        );
        assert_eq!(fs::read(&path).unwrap(), before);

        let oversized = vec![b'x'; MAX_FIXTURE_FILE_BYTES + 1];
        fs::write(&path, &oversized).unwrap();
        assert_eq!(load_document(&path), Err(FixtureError::Size));
        assert_eq!(fs::read(&path).unwrap(), oversized);
    }

    #[cfg(unix)]
    #[test]
    fn link_backed_store_is_rejected_without_following_the_target() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let target = directory.path().join("outside.json");
        let path = directory.path().join(FIXTURE_FILE_NAME);
        fs::write(&target, b"outside bytes").unwrap();
        symlink(&target, &path).unwrap();

        assert_eq!(load_document(&path), Err(FixtureError::Read));
        assert_eq!(
            save_document(&path, &FixtureDocument::default()),
            Err(FixtureError::Read)
        );
        assert_eq!(fs::read(&target).unwrap(), b"outside bytes");
    }

    #[cfg(unix)]
    #[test]
    fn link_backed_parent_is_rejected_without_creating_outside_files() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let linked_parent = directory.path().join("linked-parent");
        symlink(outside.path(), &linked_parent).unwrap();
        let path = linked_parent.join("new").join(FIXTURE_FILE_NAME);

        assert_eq!(load_document(&path), Err(FixtureError::Path));
        assert_eq!(
            save_document(&path, &FixtureDocument::default()),
            Err(FixtureError::Path)
        );
        assert!(!outside.path().join("new").exists());
    }

    #[test]
    fn relative_store_path_is_rejected() {
        let path = Path::new("relative").join(FIXTURE_FILE_NAME);
        assert_eq!(load_document(&path), Err(FixtureError::Path));
        assert_eq!(
            save_document(&path, &FixtureDocument::default()),
            Err(FixtureError::Path)
        );
    }

    #[test]
    fn storage_is_atomic_bounded_and_deterministically_sorted() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FIXTURE_FILE_NAME);
        let document = FixtureDocument {
            schema_version: FIXTURE_SCHEMA_VERSION,
            next_id: 3,
            fixtures: vec![fixture(1, 1), fixture(2, 2)],
        };
        save_document(&path, &document).unwrap();
        assert_eq!(load_document(&path).unwrap(), document);
        assert_eq!(sorted_fixtures(&document)[0].id, "fixture-2");
        let names: Vec<String> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![FIXTURE_FILE_NAME.to_string()]);
    }

    #[test]
    fn compare_and_swap_rejects_competing_writer_without_losing_bytes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(FIXTURE_FILE_NAME);
        let initial = FixtureDocument::default();
        save_document(&path, &initial).unwrap();
        let loaded = load_document_with_raw(&path).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let path_one = path.clone();
        let path_two = path.clone();
        let barrier_one = Arc::clone(&barrier);
        let barrier_two = Arc::clone(&barrier);
        let expected_raw_one = loaded.raw.clone();
        let expected_raw_two = loaded.raw;
        let first = thread::spawn(move || {
            barrier_one.wait();
            let mut next = initial.clone();
            next.next_id = 2;
            next.fixtures.push(fixture(1, 1));
            save_document_if_current(&path_one, expected_raw_one.as_deref(), &next)
        });
        let second = thread::spawn(move || {
            barrier_two.wait();
            let mut next = FixtureDocument {
                next_id: 2,
                ..FixtureDocument::default()
            };
            next.fixtures.push(fixture(1, 2));
            // The second read uses the same original bytes, so exactly one
            // writer may commit and the other must observe a conflict.
            save_document_if_current(&path_two, expected_raw_two.as_deref(), &next)
        });
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Err(FixtureError::Conflict))
                .count(),
            1
        );
        let final_document = load_document(&path).unwrap();
        assert_eq!(final_document.fixtures.len(), 1);
    }

    #[test]
    fn history_snapshot_is_masked_before_fixture_creation() {
        let mut history = History::default();
        history.push(
            "POST".into(),
            "/hook".into(),
            vec![("Authorization".into(), "raw-secret".into())],
            r#"{"token":"raw-body-secret"}"#.into(),
            1,
        );
        let fixture =
            fixture_from_request("fixture-1".into(), &history.masked_record(1).unwrap()).unwrap();
        let encoded = serde_json::to_string(&fixture).unwrap();
        assert!(!encoded.contains("raw-secret"));
        assert!(!encoded.contains("raw-body-secret"));
    }
}
