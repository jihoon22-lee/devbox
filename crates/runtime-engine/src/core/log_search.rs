//! Bounded, local-only search over Run Manager's retained log snapshots.
//!
//! The search core deliberately operates on bytes supplied by the existing
//! app-owned `logs` reader.  It does not know filesystem paths, database
//! connections, process state, or telemetry.  Search results contain only
//! opaque source identity and line metadata; log text stays in the existing
//! bounded viewer.

use crate::logs::{validate_run_id, LogStream};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum UTF-8 byte length of a search query, including a regex pattern.
pub const MAX_QUERY_BYTES: usize = 512;
/// Maximum regex program budget.  The `regex` crate is linear-time, and these
/// limits also prevent a user-controlled expression from consuming an
/// unbounded amount of compile memory.
pub const MAX_REGEX_PROGRAM_BYTES: usize = 128 * 1024;
/// Maximum bytes read from one retained stdout/stderr source for one search.
pub const MAX_SCAN_BYTES_PER_STREAM: usize = 4 * 1024 * 1024;
/// Maximum bytes read across all selected sources for one search.
pub const MAX_TOTAL_SCAN_BYTES: usize = 8 * 1024 * 1024;
/// Maximum records examined across all selected sources.
pub const MAX_SCAN_RECORDS: usize = 50_000;
/// A record longer than this is bounded before matching and is marked as a
/// truncated scan.  This protects both matching and line metadata memory.
pub const MAX_RECORD_BYTES: usize = 16 * 1024;
/// Maximum number of metadata matches returned to the frontend.
pub const MAX_RESULTS: usize = 500;
/// Maximum run identifier size accepted by the search/source boundary.
pub const MAX_RUN_ID_BYTES: usize = 128;
/// Maximum generated source identifier size.
pub const MAX_SOURCE_ID_BYTES: usize = 192;
/// Largest integer that can cross the JSON/WebView boundary without losing
/// precision in JavaScript.
pub const MAX_JS_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
/// Versioned source kind reserved for the future Log Lens handoff.
pub const LOG_SOURCE_KIND: &str = "log-source/v1";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogSearchMode {
    /// The default and safest mode: the query is treated as plain text.
    Literal,
    /// Explicit opt-in mode using Rust's linear-time regex engine.
    Regex,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    fn from_token(token: &str) -> Option<Self> {
        match token.trim_matches(|character: char| !character.is_ascii_alphabetic()) {
            value if value.eq_ignore_ascii_case("trace") => Some(Self::Trace),
            value if value.eq_ignore_ascii_case("debug") => Some(Self::Debug),
            value if value.eq_ignore_ascii_case("info") => Some(Self::Info),
            value
                if value.eq_ignore_ascii_case("warn") || value.eq_ignore_ascii_case("warning") =>
            {
                Some(Self::Warn)
            }
            value if value.eq_ignore_ascii_case("error") || value.eq_ignore_ascii_case("fatal") => {
                Some(Self::Error)
            }
            _ => None,
        }
    }
}

/// A search is always scoped to one already-selected run.  `source` is the
/// stream adapter (`stdout` or `stderr`), not an arbitrary path or external
/// source.  Time bounds are half-open epoch-millisecond bounds.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogSearchRequest {
    pub run_id: String,
    pub query: String,
    pub mode: LogSearchMode,
    #[serde(default)]
    pub source: Option<LogStream>,
    #[serde(default)]
    pub level: Option<LogLevel>,
    #[serde(default)]
    pub start_at: Option<i64>,
    #[serde(default)]
    pub end_at: Option<i64>,
}

/// A future handoff-safe source reference.  It intentionally contains no
/// absolute path, command, environment value, credential, or remote address.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogSourceRef {
    pub kind: String,
    pub source_id: String,
    pub run_id: String,
    pub stream: LogStream,
}

/// One matching line.  The line number is 1-based within the currently
/// retained snapshot for that stream; it can start later after rotation.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchMatch {
    pub source_id: String,
    pub stream: LogStream,
    pub line_number: u32,
    pub level: Option<LogLevel>,
    pub timestamp_millis: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchResponse {
    pub matches: Vec<LogSearchMatch>,
    pub scanned_lines: usize,
    pub scanned_bytes: usize,
    pub truncated: bool,
    pub sources: Vec<LogSourceRef>,
}

/// Errors are intentionally fixed and input-independent.  Command layers can
/// safely expose their display text without reflecting a query, path, or log
/// line back to a WebView.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSearchError {
    InvalidRequest,
    InvalidPattern,
    InvalidSource,
    InvalidTimeRange,
}

impl fmt::Display for LogSearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRequest => "log search request is invalid",
            Self::InvalidPattern => "log search pattern is invalid",
            Self::InvalidSource => "log search source is invalid",
            Self::InvalidTimeRange => "log search time range is invalid",
        })
    }
}

impl std::error::Error for LogSearchError {}

/// Validate a request before it reaches storage or regex compilation.
pub fn validate_request(request: &LogSearchRequest) -> Result<(), LogSearchError> {
    if validate_run_id(&request.run_id).is_err()
        || request.run_id.len() > MAX_RUN_ID_BYTES
        || request.query.is_empty()
        || request.query.len() > MAX_QUERY_BYTES
        || request.query.chars().any(char::is_control)
    {
        return Err(LogSearchError::InvalidRequest);
    }
    if request
        .start_at
        .into_iter()
        .chain(request.end_at)
        .any(|timestamp| !is_js_safe_timestamp(timestamp))
        || request
            .start_at
            .zip(request.end_at)
            .is_some_and(|(start, end)| start > end)
    {
        return Err(LogSearchError::InvalidTimeRange);
    }
    Ok(())
}

/// Build and validate the source identity that will be offered to a future
/// `log-source/v1` consumer.  No producer/consumer handoff is performed here.
pub fn source_ref(run_id: &str, stream: LogStream) -> Result<LogSourceRef, LogSearchError> {
    if validate_run_id(run_id).is_err() || run_id.len() > MAX_RUN_ID_BYTES {
        return Err(LogSearchError::InvalidSource);
    }
    let source_id = format!("run-manager:{run_id}:{}", stream.as_str());
    if source_id.len() > MAX_SOURCE_ID_BYTES {
        return Err(LogSearchError::InvalidSource);
    }
    Ok(LogSourceRef {
        kind: LOG_SOURCE_KIND.to_string(),
        source_id,
        run_id: run_id.to_string(),
        stream,
    })
}

/// Validate an externally deserialized source reference before a later
/// handoff implementation uses it.  The current PR only exercises this local
/// contract and never opens a path from the payload.
pub fn validate_source_ref(source: &LogSourceRef) -> Result<(), LogSearchError> {
    let expected = source_ref(&source.run_id, source.stream)?;
    if source.kind != LOG_SOURCE_KIND || source.source_id != expected.source_id {
        return Err(LogSearchError::InvalidSource);
    }
    Ok(())
}

enum Matcher {
    Literal(String),
    Regex(regex::Regex),
}

impl Matcher {
    fn compile(request: &LogSearchRequest) -> Result<Self, LogSearchError> {
        match request.mode {
            LogSearchMode::Literal => Ok(Self::Literal(request.query.clone())),
            LogSearchMode::Regex => RegexBuilder::new(&request.query)
                .size_limit(MAX_REGEX_PROGRAM_BYTES)
                .dfa_size_limit(MAX_REGEX_PROGRAM_BYTES)
                .build()
                .map(Self::Regex)
                .map_err(|_| LogSearchError::InvalidPattern),
        }
    }

    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Literal(query) => line.contains(query),
            Self::Regex(regex) => regex.is_match(line),
        }
    }
}

/// Search already-bounded stream snapshots.  The function is synchronous and
/// allocation-bounded so it can be unit-tested without Tauri or filesystem
/// state.  The command layer yields between bounded file-tail reads before
/// calling it, so a running writer is never held behind the complete scan.
pub fn search_streams(
    request: &LogSearchRequest,
    streams: &[(LogStream, Vec<u8>)],
    fallback_timestamp_millis: Option<i64>,
) -> Result<LogSearchResponse, LogSearchError> {
    validate_request(request)?;
    let matcher = Matcher::compile(request)?;

    let selected_streams: Vec<LogStream> = match request.source {
        Some(stream) => vec![stream],
        None => vec![LogStream::Stdout, LogStream::Stderr],
    };
    let sources = selected_streams
        .iter()
        .copied()
        .map(|stream| source_ref(&request.run_id, stream))
        .collect::<Result<Vec<_>, _>>()?;
    for source in &sources {
        validate_source_ref(source)?;
    }

    let mut response = LogSearchResponse {
        matches: Vec::new(),
        scanned_lines: 0,
        scanned_bytes: 0,
        truncated: false,
        sources: sources.clone(),
    };

    'streams: for stream in selected_streams {
        let Some((_, bytes)) = streams.iter().find(|(candidate, _)| *candidate == stream) else {
            continue;
        };
        if response.scanned_bytes >= MAX_TOTAL_SCAN_BYTES {
            response.truncated = true;
            break;
        }
        let remaining_total = MAX_TOTAL_SCAN_BYTES - response.scanned_bytes;
        let scan_len = bytes
            .len()
            .min(MAX_SCAN_BYTES_PER_STREAM)
            .min(remaining_total);
        if scan_len < bytes.len() {
            response.truncated = true;
        }
        let bounded = &bytes[..scan_len];
        response.scanned_bytes = response.scanned_bytes.saturating_add(scan_len);
        if bounded.is_empty() {
            continue;
        }

        let text = String::from_utf8_lossy(bounded);
        for (index, raw_line) in text.split_terminator('\n').enumerate() {
            if response.scanned_lines >= MAX_SCAN_RECORDS {
                response.truncated = true;
                break 'streams;
            }
            response.scanned_lines += 1;
            let line_number = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line_bytes = line.as_bytes();
            let match_line = if line_bytes.len() > MAX_RECORD_BYTES {
                response.truncated = true;
                String::from_utf8_lossy(&line_bytes[..MAX_RECORD_BYTES]).into_owned()
            } else {
                line.to_string()
            };
            let level = detect_level(&match_line);
            let timestamp_millis = parse_line_timestamp_millis(&match_line)
                .or_else(|| fallback_timestamp_millis.filter(|value| is_js_safe_timestamp(*value)));

            if request
                .level
                .is_some_and(|expected| level != Some(expected))
                || request
                    .start_at
                    .is_some_and(|start| timestamp_millis.is_none_or(|timestamp| timestamp < start))
                || request
                    .end_at
                    .is_some_and(|end| timestamp_millis.is_none_or(|timestamp| timestamp >= end))
                || !matcher.is_match(&match_line)
            {
                continue;
            }
            if response.matches.len() >= MAX_RESULTS {
                response.truncated = true;
                break 'streams;
            }
            let source_id = sources
                .iter()
                .find(|source| source.stream == stream)
                .map(|source| source.source_id.clone())
                .ok_or(LogSearchError::InvalidSource)?;
            response.matches.push(LogSearchMatch {
                source_id,
                stream,
                line_number,
                level,
                timestamp_millis,
            });
        }
    }

    Ok(response)
}

fn detect_level(line: &str) -> Option<LogLevel> {
    let trimmed = line.trim_start();
    let mut tokens = trimmed.split_whitespace();
    let first = tokens.next()?;
    if let Some(level) = LogLevel::from_token(first) {
        return Some(level);
    }
    // A conventional timestamp prefix may precede the level token.
    if parse_timestamp_millis(first).is_some() {
        return tokens.next().and_then(LogLevel::from_token);
    }
    None
}

fn parse_line_timestamp_millis(line: &str) -> Option<i64> {
    line.split_whitespace()
        .next()
        .and_then(parse_timestamp_millis)
}

fn parse_timestamp_millis(value: &str) -> Option<i64> {
    let value = value.trim_matches(|character| matches!(character, '[' | ']' | '(' | ')'));
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
        .filter(|timestamp| is_js_safe_timestamp(*timestamp))
}

const fn is_js_safe_timestamp(timestamp: i64) -> bool {
    timestamp >= -MAX_JS_SAFE_INTEGER && timestamp <= MAX_JS_SAFE_INTEGER
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(query: &str, mode: LogSearchMode) -> LogSearchRequest {
        LogSearchRequest {
            run_id: "run-1".to_string(),
            query: query.to_string(),
            mode,
            source: None,
            level: None,
            start_at: None,
            end_at: None,
        }
    }

    fn streams(stdout: &str, stderr: &str) -> Vec<(LogStream, Vec<u8>)> {
        vec![
            (LogStream::Stdout, stdout.as_bytes().to_vec()),
            (LogStream::Stderr, stderr.as_bytes().to_vec()),
        ]
    }

    #[test]
    fn literal_is_the_default_safe_matcher_and_keeps_stream_order() {
        let response = search_streams(
            &request("a+b", LogSearchMode::Literal),
            &streams("INFO a+b\nINFO aab\n", "ERROR a+b\n"),
            None,
        )
        .unwrap();
        assert_eq!(response.matches.len(), 2);
        assert_eq!(response.matches[0].stream, LogStream::Stdout);
        assert_eq!(response.matches[0].line_number, 1);
        assert_eq!(response.matches[1].stream, LogStream::Stderr);
        assert_eq!(response.matches[1].line_number, 1);
    }

    #[test]
    fn regex_requires_explicit_mode_and_compile_errors_are_fixed() {
        let mut regex_request = request(r"a+b", LogSearchMode::Regex);
        let response = search_streams(&regex_request, &streams("aaab\naab\n", ""), None).unwrap();
        assert_eq!(response.matches.len(), 2);
        regex_request.query = "[".to_string();
        assert_eq!(
            search_streams(&regex_request, &streams("secret [", ""), None),
            Err(LogSearchError::InvalidPattern)
        );
        assert!(!LogSearchError::InvalidPattern
            .to_string()
            .contains("secret"));
    }

    #[test]
    fn level_source_and_time_filters_are_applied_without_copying_line_text() {
        let mut request = request("failure", LogSearchMode::Literal);
        request.source = Some(LogStream::Stderr);
        request.level = Some(LogLevel::Error);
        request.start_at = Some(1_700_000_000_000);
        request.end_at = Some(1_800_000_000_000);
        let response = search_streams(
            &request,
            &streams(
                "2020-01-01T00:00:00Z ERROR failure\n",
                "2024-01-01T00:00:00Z ERROR failure\n2024-01-01T00:00:00Z INFO failure\n",
            ),
            None,
        )
        .unwrap();
        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].stream, LogStream::Stderr);
        assert_eq!(response.matches[0].level, Some(LogLevel::Error));
        assert_eq!(
            response.matches[0].timestamp_millis,
            Some(1_704_067_200_000)
        );
        assert!(response.matches[0]
            .source_id
            .starts_with("run-manager:run-1:"));
    }

    #[test]
    fn missing_line_timestamp_uses_run_time_for_range_filter() {
        let mut request = request("plain", LogSearchMode::Literal);
        request.start_at = Some(10);
        request.end_at = Some(20);
        let included = search_streams(&request, &streams("plain\n", ""), Some(15)).unwrap();
        assert_eq!(included.matches.len(), 1);
        let excluded = search_streams(&request, &streams("plain\n", ""), Some(30)).unwrap();
        assert!(excluded.matches.is_empty());
    }

    #[test]
    fn scan_and_result_bounds_are_deterministic() {
        let mut request = request("hit", LogSearchMode::Literal);
        let bytes = vec![b'h'; MAX_RECORD_BYTES + 10];
        let mut hit_bytes = b"hit\n".to_vec();
        hit_bytes.extend(bytes);
        let response = search_streams(
            &request,
            &[
                (LogStream::Stdout, hit_bytes),
                (LogStream::Stderr, Vec::new()),
            ],
            None,
        )
        .unwrap();
        assert_eq!(response.matches.len(), 1);
        assert!(response.truncated);
        request.query = "hit".to_string();
        let many = (0..=MAX_RESULTS)
            .map(|_| "hit")
            .collect::<Vec<_>>()
            .join("\n");
        let response = search_streams(
            &request,
            &[
                (LogStream::Stdout, many.into_bytes()),
                (LogStream::Stderr, Vec::new()),
            ],
            None,
        )
        .unwrap();
        assert_eq!(response.matches.len(), MAX_RESULTS);
        assert!(response.truncated);
    }

    #[test]
    fn invalid_input_and_source_never_echo_user_values() {
        let mut request = request("\n/path/to/secret", LogSearchMode::Literal);
        request.run_id = "../outside".to_string();
        let error = validate_request(&request).unwrap_err();
        assert_eq!(error, LogSearchError::InvalidRequest);
        assert!(!error.to_string().contains("outside"));

        let source = source_ref("run-1", LogStream::Stdout).unwrap();
        assert!(validate_source_ref(&source).is_ok());
        let mut unsafe_source = source;
        unsafe_source.source_id = "/tmp/secret".to_string();
        assert_eq!(
            validate_source_ref(&unsafe_source),
            Err(LogSearchError::InvalidSource)
        );

        let payload = r#"{
            "kind":"log-source/v1",
            "sourceId":"run-manager:run-1:stdout",
            "runId":"run-1",
            "stream":"stdout",
            "absolutePath":"/tmp/secret"
        }"#;
        assert!(serde_json::from_str::<LogSourceRef>(payload).is_err());

        let request_payload = r#"{
            "runId":"run-1",
            "query":"needle",
            "mode":"literal",
            "absolutePath":"/tmp/secret"
        }"#;
        assert!(serde_json::from_str::<LogSearchRequest>(request_payload).is_err());
    }

    #[test]
    fn request_bounds_and_time_order_are_rejected_before_scan() {
        let mut oversized = request("x", LogSearchMode::Literal);
        oversized.query = "x".repeat(MAX_QUERY_BYTES + 1);
        assert_eq!(
            validate_request(&oversized),
            Err(LogSearchError::InvalidRequest)
        );

        let mut reversed = request("x", LogSearchMode::Literal);
        reversed.start_at = Some(20);
        reversed.end_at = Some(10);
        assert_eq!(
            validate_request(&reversed),
            Err(LogSearchError::InvalidTimeRange)
        );

        let mut imprecise = request("x", LogSearchMode::Literal);
        imprecise.start_at = Some(MAX_JS_SAFE_INTEGER + 1);
        assert_eq!(
            validate_request(&imprecise),
            Err(LogSearchError::InvalidTimeRange)
        );

        let mut control = request("x", LogSearchMode::Literal);
        control.query.push('\0');
        assert_eq!(
            validate_request(&control),
            Err(LogSearchError::InvalidRequest)
        );
    }

    #[test]
    fn timestamps_cross_the_webview_boundary_only_when_js_safe() {
        let response = search_streams(
            &request("plain", LogSearchMode::Literal),
            &streams("plain\n", ""),
            Some(MAX_JS_SAFE_INTEGER + 1),
        )
        .unwrap();
        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].timestamp_millis, None);
    }

    #[test]
    fn regex_engine_handles_nested_quantifier_shape_without_catastrophic_scan() {
        let request = request(r"(a+)+$", LogSearchMode::Regex);
        let bytes = format!("{}b\n", "a".repeat(MAX_RECORD_BYTES.min(4_096)));
        let response = search_streams(
            &request,
            &[
                (LogStream::Stdout, bytes.into_bytes()),
                (LogStream::Stderr, Vec::new()),
            ],
            None,
        )
        .unwrap();
        assert!(response.matches.is_empty());
    }
}
