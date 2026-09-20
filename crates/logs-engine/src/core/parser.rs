use super::model::{
    CoreError, FilterSpec, LogFormat, LogLevel, LogRecord, MAX_EXPORT_BYTES, MAX_FIELDS,
    MAX_FIELD_BYTES, MAX_LINE_BYTES, MAX_RECORDS, MAX_SOURCE_BYTES,
};
use regex::RegexBuilder;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

const MAX_REGEX_PROGRAM_BYTES: usize = 128 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_NODES: usize = 1_024;
type ParsedLine = (
    Option<i64>,
    Option<LogLevel>,
    String,
    BTreeMap<String, String>,
);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseBatch {
    pub records: Vec<LogRecord>,
    pub truncated: bool,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedText {
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Default)]
pub struct MergeBuffer {
    records: BTreeMap<(i64, String, u64), LogRecord>,
    bytes: usize,
    dropped_records: usize,
    dropped_bytes: usize,
}

impl MergeBuffer {
    pub fn push(&mut self, record: LogRecord) {
        let record_bytes = record.estimated_bytes();
        if record_bytes > MAX_SOURCE_BYTES {
            self.dropped_records = self.dropped_records.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(record_bytes);
            return;
        }
        let key = (
            record.timestamp_millis.unwrap_or(i64::MAX),
            record.source_id.clone(),
            record.sequence,
        );
        if let Some(previous) = self.records.insert(key, record) {
            self.bytes = self.bytes.saturating_sub(previous.estimated_bytes());
        }
        self.bytes = self.bytes.saturating_add(record_bytes);
        while self.records.len() > MAX_RECORDS || self.bytes > MAX_SOURCE_BYTES {
            let Some((_, evicted)) = self.records.pop_first() else {
                break;
            };
            let evicted_bytes = evicted.estimated_bytes();
            self.bytes = self.bytes.saturating_sub(evicted_bytes);
            self.dropped_records = self.dropped_records.saturating_add(1);
            self.dropped_bytes = self.dropped_bytes.saturating_add(evicted_bytes);
        }
    }

    pub fn extend<I>(&mut self, records: I)
    where
        I: IntoIterator<Item = LogRecord>,
    {
        for record in records {
            self.push(record);
        }
    }

    pub fn finish(self) -> (Vec<LogRecord>, usize, usize) {
        (
            self.records.into_values().collect(),
            self.dropped_records,
            self.dropped_bytes,
        )
    }
}

/// Parse a bounded UTF-8 byte batch. Invalid UTF-8 is replaced at the source
/// boundary; a malformed JSONL/logfmt line is retained as plain text instead
/// of making the rest of a live stream disappear.
pub fn parse_bytes(
    bytes: &[u8],
    source_id: &str,
    sequence_start: u64,
) -> Result<ParseBatch, CoreError> {
    if source_id.is_empty() || source_id.len() > 192 || source_id.chars().any(char::is_control) {
        return Err(CoreError::InvalidSource);
    }
    let bounded_len = bytes.len().min(MAX_SOURCE_BYTES);
    let bounded = &bytes[..bounded_len];
    if bounded.is_empty() {
        return Ok(ParseBatch {
            records: Vec::new(),
            truncated: false,
            bytes: 0,
        });
    }
    let mut records = Vec::new();
    let mut truncated = bounded_len < bytes.len();
    let trailing_newline = bounded.last() == Some(&b'\n');
    let mut lines = bounded.split(|byte| *byte == b'\n').peekable();
    while let Some(raw_line) = lines.next() {
        if raw_line.is_empty() && trailing_newline && lines.peek().is_none() {
            break;
        }
        if records.len() >= MAX_RECORDS {
            truncated = true;
            break;
        }
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let (line, line_truncated) = bounded_line_text(line);
        truncated |= line_truncated;
        let sequence = sequence_start
            .checked_add(records.len() as u64)
            .filter(|value| *value <= 9_007_199_254_740_991_u64)
            .ok_or(CoreError::InvalidInput)?;
        records.push(parse_line_with_truncation(
            &line,
            source_id,
            sequence,
            line_truncated,
        ));
    }
    Ok(ParseBatch {
        records,
        truncated,
        bytes: bounded_len,
    })
}

/// Parse one line using JSONL, logfmt, then plain-text best effort detection.
pub fn parse_line(line: &str, source_id: &str, sequence: u64) -> LogRecord {
    parse_line_with_truncation(line, source_id, sequence, false)
}

fn parse_line_with_truncation(
    line: &str,
    source_id: &str,
    sequence: u64,
    input_truncated: bool,
) -> LogRecord {
    let mut line = line.strip_prefix('\u{feff}').unwrap_or(line);
    let mut truncated = input_truncated;
    if line.len() > MAX_LINE_BYTES {
        line = truncate_utf8(line, MAX_LINE_BYTES);
        truncated = true;
    }

    if let Some((timestamp_millis, level, message, fields)) = parse_json(line) {
        return LogRecord {
            source_id: source_id.to_string(),
            sequence,
            timestamp_millis,
            level,
            message: normalize_log_text(&message, MAX_LINE_BYTES),
            fields,
            format: LogFormat::Jsonl,
            truncated,
        };
    }
    if let Some((timestamp_millis, level, message, fields)) = parse_logfmt(line) {
        return LogRecord {
            source_id: source_id.to_string(),
            sequence,
            timestamp_millis,
            level,
            message: normalize_log_text(&message, MAX_LINE_BYTES),
            fields: fields
                .into_iter()
                .map(|(key, value)| (key, normalize_log_text(&value, MAX_FIELD_BYTES)))
                .collect(),
            format: LogFormat::Logfmt,
            truncated,
        };
    }

    let (timestamp_millis, level) = parse_plain_metadata(line);
    LogRecord {
        source_id: source_id.to_string(),
        sequence,
        timestamp_millis,
        level,
        message: normalize_log_text(line, MAX_LINE_BYTES),
        fields: BTreeMap::new(),
        format: LogFormat::Plain,
        truncated,
    }
}

/// Convert one source line without ever materializing an entire unbounded
/// line. `parse_bytes` calls this before parsing, so a file without newlines
/// cannot turn the source byte cap into an equally large temporary string.
fn bounded_line_text(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > MAX_LINE_BYTES;
    let bounded = &bytes[..bytes.len().min(MAX_LINE_BYTES)];
    (String::from_utf8_lossy(bounded).into_owned(), truncated)
}

/// Keep control characters out of records shown in a table or sent to an
/// export. Escaped newlines/tabs in JSON/logfmt are useful as content and are
/// rendered safely by React/export escaping, while other control characters
/// must not become invisible terminal/UI controls. U+FFFD is visible and
/// remains valid UTF-8.
fn normalize_log_text(value: &str, max_bytes: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect::<String>();
    truncate_utf8(&sanitized, max_bytes).to_string()
}

fn parse_json(line: &str) -> Option<ParsedLine> {
    let candidate = line.trim();
    if !json_shape_within_limits(candidate) {
        return None;
    }
    let value: Value = serde_json::from_str(candidate).ok()?;
    let object = value.as_object()?;
    let timestamp_millis = ["timestamp", "time", "@timestamp", "ts"]
        .iter()
        .find_map(|key| object.get(*key).and_then(parse_timestamp_value));
    let level = ["level", "severity", "log.level"].iter().find_map(|key| {
        object
            .get(*key)
            .and_then(Value::as_str)
            .and_then(LogLevel::parse)
    });
    let message_value = ["message", "msg", "log"]
        .iter()
        .find_map(|key| object.get(*key));
    let message = message_value
        .and_then(value_to_string)
        .unwrap_or_else(|| serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string()));
    let mut fields = BTreeMap::new();
    for (key, value) in object {
        if [
            "timestamp",
            "time",
            "@timestamp",
            "ts",
            "level",
            "severity",
            "log.level",
            "message",
            "msg",
            "log",
        ]
        .contains(&key.as_str())
        {
            continue;
        }
        if fields.len() >= MAX_FIELDS
            || key.len() > MAX_FIELD_BYTES
            || key.chars().any(char::is_control)
        {
            continue;
        }
        if let Some(value) = value_to_string(value) {
            fields.insert(key.clone(), normalize_log_text(&value, MAX_FIELD_BYTES));
        }
    }
    Some((
        timestamp_millis,
        level,
        normalize_log_text(&message, MAX_LINE_BYTES),
        fields,
    ))
}

/// Do a cheap lexical preflight before serde_json allocates a `Value`. The
/// input line is already byte-bounded, but a valid JSON object can still hide
/// an excessive nesting depth or node count behind a small payload. Strings
/// are scanned with escape handling so braces and keywords inside messages do
/// not affect the limits. Syntactic validity remains serde_json's job.
fn json_shape_within_limits(input: &str) -> bool {
    let mut depth = 0_usize;
    let mut nodes = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_primitive = false;

    for byte in input.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => {
                in_string = true;
                in_primitive = false;
                nodes = nodes.saturating_add(1);
            }
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                if depth > MAX_JSON_DEPTH {
                    return false;
                }
                in_primitive = false;
                nodes = nodes.saturating_add(1);
            }
            b'}' | b']' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
                in_primitive = false;
            }
            byte if byte.is_ascii_whitespace() || matches!(byte, b',' | b':') => {
                in_primitive = false;
            }
            byte if byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E') =>
            {
                if !in_primitive {
                    nodes = nodes.saturating_add(1);
                    in_primitive = true;
                }
            }
            _ => {
                in_primitive = false;
            }
        }
        if nodes > MAX_JSON_NODES {
            return false;
        }
    }

    !in_string && depth == 0 && nodes <= MAX_JSON_NODES
}

fn parse_timestamp_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .or_else(|| value.as_str().and_then(parse_timestamp))
        .filter(|value| value.unsigned_abs() <= 9_007_199_254_740_991_u64)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Null => None,
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

fn parse_logfmt(line: &str) -> Option<ParsedLine> {
    let mut cursor = 0;
    let mut fields = BTreeMap::new();
    let mut parsed_any = false;
    while cursor < line.len() {
        while line
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            cursor += 1;
        }
        if cursor >= line.len() {
            break;
        }
        let key_start = cursor;
        while cursor < line.len()
            && !line.as_bytes()[cursor].is_ascii_whitespace()
            && line.as_bytes()[cursor] != b'='
        {
            cursor += 1;
        }
        if cursor == key_start || line.as_bytes().get(cursor) != Some(&b'=') {
            return None;
        }
        let key = &line[key_start..cursor];
        if key.len() > MAX_FIELD_BYTES || key.chars().any(char::is_control) {
            return None;
        }
        cursor += 1;
        let value = if line.as_bytes().get(cursor) == Some(&b'"') {
            cursor += 1;
            let mut output = String::new();
            let mut closed = false;
            while cursor < line.len() {
                let character = line[cursor..].chars().next()?;
                cursor += character.len_utf8();
                match character {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => {
                        let escaped = line[cursor..].chars().next()?;
                        cursor += escaped.len_utf8();
                        output.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            other => other,
                        });
                    }
                    other => output.push(other),
                }
                if output.len() > MAX_FIELD_BYTES {
                    output = truncate_utf8(&output, MAX_FIELD_BYTES).to_string();
                }
            }
            if !closed {
                return None;
            }
            output
        } else {
            let value_start = cursor;
            while cursor < line.len() && !line.as_bytes()[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            line[value_start..cursor].to_string()
        };
        if fields.len() >= MAX_FIELDS {
            break;
        }
        fields.insert(
            key.to_string(),
            truncate_utf8(&value, MAX_FIELD_BYTES).to_string(),
        );
        parsed_any = true;
    }
    if !parsed_any {
        return None;
    }
    let timestamp_millis = ["timestamp", "time", "ts"]
        .iter()
        .find_map(|key| fields.get(*key).and_then(|value| parse_timestamp(value)));
    let level = ["level", "severity"]
        .iter()
        .find_map(|key| fields.get(*key).and_then(|value| LogLevel::parse(value)));
    let message = fields
        .get("msg")
        .or_else(|| fields.get("message"))
        .cloned()
        .unwrap_or_else(|| line.to_string());
    Some((
        timestamp_millis,
        level,
        truncate_utf8(&message, MAX_LINE_BYTES).to_string(),
        fields,
    ))
}

fn parse_plain_metadata(line: &str) -> (Option<i64>, Option<LogLevel>) {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let first = tokens.first().copied().unwrap_or_default();
    let timestamp_millis = parse_timestamp(first).or_else(|| {
        tokens
            .get(1)
            .map(|second| format!("{first} {second}"))
            .and_then(|value| parse_timestamp(&value))
    });
    let level = if timestamp_millis.is_some() {
        let level_index = if parse_timestamp(first).is_some() {
            1
        } else {
            2
        };
        tokens
            .get(level_index)
            .and_then(|token| LogLevel::parse(token))
    } else {
        LogLevel::parse(first).or_else(|| {
            first
                .trim_matches(|character: char| matches!(character, '[' | ']' | '(' | ')'))
                .split_once('=')
                .and_then(|(_, value)| LogLevel::parse(value))
        })
    };
    (timestamp_millis, level)
}

fn parse_timestamp(value: &str) -> Option<i64> {
    let value = value.trim_matches(|character: char| matches!(character, '[' | ']' | '(' | ')'));
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        return bounded_timestamp(timestamp.timestamp_millis());
    }
    // `journalctl --output=short-iso` commonly emits offsets without the
    // RFC3339 colon (`+0900`). Keep both fractional and non-fractional forms
    // bounded and timezone-aware before falling back to a naive UTC value.
    for format in ["%Y-%m-%dT%H:%M:%S%.f%z", "%Y-%m-%dT%H:%M:%S%z"] {
        if let Ok(timestamp) = chrono::DateTime::parse_from_str(value, format) {
            return bounded_timestamp(timestamp.timestamp_millis());
        }
    }
    for format in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(timestamp) = chrono::NaiveDateTime::parse_from_str(value, format) {
            return bounded_timestamp(timestamp.and_utc().timestamp_millis());
        }
    }
    None
}

fn bounded_timestamp(timestamp: i64) -> Option<i64> {
    (timestamp.unsigned_abs() <= 9_007_199_254_740_991_u64).then_some(timestamp)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    &value[..end]
}

/// Merge batches with a total order. Timestamp ties and timestamp-less lines
/// use source id then sequence, preserving per-source sequence regardless of
/// filesystem/adapter completion order.
pub fn merge_records(batches: Vec<Vec<LogRecord>>) -> Vec<LogRecord> {
    merge_records_with_stats(batches).0
}

pub fn merge_records_with_stats(batches: Vec<Vec<LogRecord>>) -> (Vec<LogRecord>, usize, usize) {
    let mut buffer = MergeBuffer::default();
    for batch in batches {
        buffer.extend(batch);
    }
    buffer.finish()
}

pub fn filter_records(
    records: &[LogRecord],
    filter: &FilterSpec,
) -> Result<Vec<LogRecord>, CoreError> {
    if records.len() > MAX_RECORDS {
        return Err(CoreError::OutputLimit);
    }
    let mut input_bytes = 0_usize;
    for record in records {
        record.validate()?;
        input_bytes = input_bytes.saturating_add(record.estimated_bytes());
        if input_bytes > MAX_SOURCE_BYTES {
            return Err(CoreError::OutputLimit);
        }
    }
    filter.validate()?;
    let matcher = if filter.text.is_empty() {
        None
    } else if filter.regex {
        Some(
            RegexBuilder::new(&filter.text)
                .size_limit(MAX_REGEX_PROGRAM_BYTES)
                .dfa_size_limit(MAX_REGEX_PROGRAM_BYTES)
                .build()
                .map_err(|_| CoreError::InvalidFilter)?,
        )
    } else {
        None
    };
    let mut result = Vec::new();
    for record in records {
        if filter
            .source_id
            .as_ref()
            .is_some_and(|source| source != &record.source_id)
            || filter
                .level
                .is_some_and(|level| record.level != Some(level))
            || filter.start_at.is_some_and(|start| {
                record
                    .timestamp_millis
                    .is_none_or(|timestamp| timestamp < start)
            })
            || filter.end_at.is_some_and(|end| {
                record
                    .timestamp_millis
                    .is_none_or(|timestamp| timestamp >= end)
            })
            || filter.field.as_ref().is_some_and(|field| {
                filter
                    .field_value
                    .as_ref()
                    .is_some_and(|expected| record.fields.get(field) != Some(expected))
            })
        {
            continue;
        }
        let matches_text = if filter.text.is_empty() {
            true
        } else if let Some(regex) = &matcher {
            regex.is_match(&record.message)
                || record
                    .fields
                    .iter()
                    .any(|(key, value)| regex.is_match(key) || regex.is_match(value))
        } else {
            record.message.contains(&filter.text)
                || record
                    .fields
                    .iter()
                    .any(|(key, value)| key.contains(&filter.text) || value.contains(&filter.text))
        };
        if matches_text {
            result.push(record.clone());
            if result.len() >= MAX_RECORDS {
                break;
            }
        }
    }
    Ok(result)
}

pub fn export_records(records: &[LogRecord]) -> Result<ExportedText, CoreError> {
    if records.len() > MAX_RECORDS {
        return Err(CoreError::ExportTooLarge);
    }
    let mut input_bytes = 0_usize;
    for record in records {
        record.validate().map_err(|_| CoreError::ExportTooLarge)?;
        input_bytes = input_bytes.saturating_add(record.estimated_bytes());
        if input_bytes > MAX_SOURCE_BYTES {
            return Err(CoreError::ExportTooLarge);
        }
    }
    let mut text = String::new();
    let mut truncated = false;
    for record in records.iter().take(MAX_RECORDS) {
        let mut line = String::new();
        if let Some(timestamp) = record.timestamp_millis {
            line.push_str(&timestamp.to_string());
            line.push(' ');
        }
        append_export_message(&mut line, &record.message);
        for (key, value) in &record.fields {
            if matches!(
                key.as_str(),
                "timestamp" | "time" | "ts" | "level" | "severity" | "msg" | "message"
            ) {
                continue;
            }
            line.push(' ');
            line.push_str(key);
            line.push('=');
            append_logfmt_value(&mut line, value);
        }
        line.push('\n');
        if text.len().saturating_add(line.len()) > MAX_EXPORT_BYTES {
            truncated = true;
            break;
        }
        text.push_str(&line);
    }
    Ok(ExportedText { text, truncated })
}

fn append_export_message(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            character if character.is_control() => {
                let _ = write!(output, "\\u{{{:x}}}", character as u32);
            }
            _ => output.push(character),
        }
    }
}

fn append_logfmt_value(output: &mut String, value: &str) {
    let quoted = value.chars().any(|character| {
        character.is_whitespace() || character.is_control() || character == '"' || character == '\\'
    });
    if !quoted {
        output.push_str(value);
        return;
    }
    output.push('"');
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(output, "\\u{{{:x}}}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_timestamp_and_level() {
        let record = parse_line("2026-08-27T12:00:00Z ERROR failed", "source-a", 4);
        assert_eq!(record.format, LogFormat::Plain);
        assert_eq!(record.level, Some(LogLevel::Error));
        assert_eq!(record.sequence, 4);
        assert!(record.timestamp_millis.is_some());
        let spaced = parse_line("2026-08-27 12:00:00 WARN slow", "source-a", 5);
        assert_eq!(spaced.level, Some(LogLevel::Warn));
        assert!(spaced.timestamp_millis.is_some());
    }

    #[test]
    fn parses_jsonl_and_bounds_nested_fields() {
        let record = parse_line(
            r#"{"timestamp":"2026-08-27T12:00:00Z","level":"warn","message":"slow","request":{"id":3}}"#,
            "source-a",
            1,
        );
        assert_eq!(record.format, LogFormat::Jsonl);
        assert_eq!(record.level, Some(LogLevel::Warn));
        assert_eq!(
            record.fields.get("request"),
            Some(&r#"{"id":3}"#.to_string())
        );
    }

    #[test]
    fn malformed_json_falls_back_to_plain() {
        let record = parse_line("{not json", "source-a", 1);
        assert_eq!(record.format, LogFormat::Plain);
        assert_eq!(record.message, "{not json");
    }

    #[test]
    fn parses_quoted_logfmt_values_and_metadata() {
        let record = parse_line(
            r##"ts=2026-08-27T12:00:00Z level=info msg="hello world" request_id=abc"##,
            "source-a",
            2,
        );
        assert_eq!(record.format, LogFormat::Logfmt);
        assert_eq!(record.message, "hello world");
        assert_eq!(record.fields.get("request_id"), Some(&"abc".to_string()));
    }

    #[test]
    fn mixed_fixture_keeps_all_supported_formats_in_order() {
        let input = include_bytes!("../../tests/fixtures/mixed.log");
        let batch = parse_bytes(input, "fixture", 0).unwrap();
        assert_eq!(batch.records.len(), 3);
        assert_eq!(
            batch
                .records
                .iter()
                .map(|record| record.format)
                .collect::<Vec<_>>(),
            vec![LogFormat::Plain, LogFormat::Jsonl, LogFormat::Logfmt]
        );
    }

    #[test]
    fn preserves_invalid_utf8_replacement_and_line_cap() {
        let bytes = [b'a', b'\n', 0xff, b'\n'];
        let batch = parse_bytes(&bytes, "source-a", 10).unwrap();
        assert_eq!(batch.records.len(), 2);
        assert!(batch.records[1].message.contains('\u{fffd}'));
        let long = "x".repeat(MAX_LINE_BYTES + 1);
        assert!(parse_line(&long, "source-a", 1).truncated);
        let batch = parse_bytes(long.as_bytes(), "source-a", 0).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert!(batch.truncated);
        assert!(batch.records[0].truncated);
        assert!(batch.records[0].message.len() <= MAX_LINE_BYTES);
    }

    #[test]
    fn empty_source_has_no_phantom_log_line() {
        let batch = parse_bytes(&[], "source-a", 0).unwrap();
        assert!(batch.records.is_empty());
        assert_eq!(batch.bytes, 0);
        assert!(!batch.truncated);
    }

    #[test]
    fn sequence_numbers_cannot_cross_javascript_safe_integer_boundary() {
        assert!(parse_bytes(b"line\nline\n", "source-a", 9_007_199_254_740_991,).is_err());
        assert!(parse_bytes(b"line\n", "source-a", 9_007_199_254_740_991,).is_ok());
    }

    #[test]
    fn bounds_json_depth_and_node_count_before_record_creation() {
        let deeply_nested = format!(
            "{}1{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        let depth_record = parse_line(&deeply_nested, "source-a", 1);
        assert_eq!(depth_record.format, LogFormat::Plain);

        let many_fields = (0..(MAX_JSON_NODES / 2 + 1))
            .map(|index| format!("\"k{index}\":1"))
            .collect::<Vec<_>>()
            .join(",");
        let node_record = parse_line(&format!("{{{many_fields}}}"), "source-a", 2);
        assert_eq!(node_record.format, LogFormat::Plain);
    }

    #[test]
    fn normalizes_controls_and_validation_rejects_them_at_the_model_boundary() {
        let record = parse_line("INFO bad\u{0000}\u{007f}", "source-a", 1);
        assert!(!record.message.chars().any(char::is_control));
        assert!(record.validate().is_ok());

        let json = parse_line(
            r#"{"level":"error","message":"line\nwith\u0000control","field":"ok"}"#,
            "source-a",
            2,
        );
        assert_eq!(json.format, LogFormat::Jsonl);
        assert_eq!(json.message, "line\nwith\u{fffd}control");
        assert!(json.validate().is_ok());

        let journal = parse_line(
            "2026-08-27T12:00:00.123+0900 INFO journal entry",
            "source-a",
            3,
        );
        assert_eq!(journal.level, Some(LogLevel::Info));
        assert_eq!(journal.timestamp_millis, Some(1_787_799_600_123));

        let mut invalid = record;
        invalid.message.push('\u{0000}');
        assert_eq!(invalid.validate(), Err(CoreError::InvalidInput));
    }

    #[test]
    fn deterministic_merge_orders_ties_and_missing_timestamps() {
        let a = parse_line("a", "b", 2);
        let b = parse_line("b", "a", 1);
        let merged = merge_records(vec![vec![a], vec![b]]);
        assert_eq!(merged[0].source_id, "a");
        assert_eq!(merged[1].source_id, "b");
    }

    #[test]
    fn filters_literal_regex_level_and_fields() {
        let records = vec![
            parse_line(r#"level=error msg="failed" code=500"#, "a", 0),
            parse_line(r#"level=info msg="ok" code=200"#, "a", 1),
        ];
        let mut filter = FilterSpec {
            text: "fail.*".to_string(),
            regex: true,
            level: Some(LogLevel::Error),
            field: Some("code".to_string()),
            field_value: Some("500".to_string()),
            ..Default::default()
        };
        assert_eq!(filter_records(&records, &filter).unwrap().len(), 1);
        filter.text = "[".to_string();
        assert_eq!(
            filter_records(&records, &filter),
            Err(CoreError::InvalidFilter)
        );
    }

    #[test]
    fn export_is_bounded_and_deterministic() {
        let record = parse_line("INFO hello", "a", 1);
        let exported = export_records(&[record]).unwrap();
        assert_eq!(exported.text, "INFO hello\n");
        assert!(!exported.truncated);
    }

    #[test]
    fn export_escapes_logfmt_delimiters_and_embedded_newlines() {
        let mut record = parse_line("INFO hello", "a", 1);
        record.message = "hello\nworld".to_string();
        record
            .fields
            .insert("path".to_string(), r#"C:\logs\"quoted"#.to_string());
        let exported = export_records(&[record]).unwrap();
        let expected_line = r#"hello\nworld path="C:\\logs\\\"quoted""#;
        assert_eq!(exported.text, format!("{expected_line}\n"));
    }
}
