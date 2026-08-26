//! Bounded, runtime-only language-server logs for the management UI.
//!
//! Server stderr is untrusted: it can contain workspace paths, source text or
//! credentials emitted by a third-party executable. Raw chunks therefore
//! never cross the native boundary. They are assembled into bounded lines,
//! sanitized, and only then inserted into this in-memory store.

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

const MAX_ENTRIES_PER_LANGUAGE: usize = 200;
const MAX_LOG_LANGUAGES: usize = 64;
const MAX_RAW_LINE_BYTES: usize = 8 * 1_024;
const MAX_MESSAGE_CHARS: usize = 2_048;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LspLogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LspLogEntry {
    pub sequence: String,
    pub level: LspLogLevel,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LanguageServerLog {
    pub language_id: String,
    pub entries: Vec<LspLogEntry>,
    pub dropped_entries: u64,
    pub dropped_stderr_bytes: u64,
    pub stderr_truncated: bool,
}

#[derive(Debug, Default)]
struct LanguageLog {
    entries: VecDeque<LspLogEntry>,
    dropped_entries: u64,
    dropped_stderr_bytes: u64,
    stderr_truncated: bool,
}

#[derive(Debug, Default)]
pub struct LspLogStore {
    next_sequence: u64,
    languages: BTreeMap<String, LanguageLog>,
}

impl LspLogStore {
    pub fn append(
        &mut self,
        language_id: &str,
        level: LspLogLevel,
        code: &'static str,
        message: impl Into<String>,
    ) {
        if !self.languages.contains_key(language_id) && self.languages.len() >= MAX_LOG_LANGUAGES {
            return;
        }
        self.next_sequence = self.next_sequence.wrapping_add(1).max(1);
        let entry = LspLogEntry {
            sequence: self.next_sequence.to_string(),
            level,
            code: code.to_owned(),
            message: bound_message(message.into()),
        };
        let log = self.languages.entry(language_id.to_owned()).or_default();
        if log.entries.len() == MAX_ENTRIES_PER_LANGUAGE {
            log.entries.pop_front();
            log.dropped_entries = log.dropped_entries.saturating_add(1);
        }
        log.entries.push_back(entry);
    }

    pub fn record_stderr_state(&mut self, language_id: &str, dropped_bytes: u64, truncated: bool) {
        if !self.languages.contains_key(language_id) && self.languages.len() >= MAX_LOG_LANGUAGES {
            return;
        }
        let log = self.languages.entry(language_id.to_owned()).or_default();
        log.dropped_stderr_bytes = log.dropped_stderr_bytes.max(dropped_bytes);
        log.stderr_truncated |= truncated;
    }

    pub fn snapshots(&self) -> Vec<LanguageServerLog> {
        self.languages
            .iter()
            .map(|(language_id, log)| LanguageServerLog {
                language_id: language_id.clone(),
                entries: log.entries.iter().cloned().collect(),
                dropped_entries: log.dropped_entries,
                dropped_stderr_bytes: log.dropped_stderr_bytes,
                stderr_truncated: log.stderr_truncated,
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct StderrLineSanitizer {
    pending: Vec<u8>,
    overflowed: bool,
}

impl StderrLineSanitizer {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if let Some(line) = self.take_line() {
                    lines.push(line);
                }
                continue;
            }
            if self.pending.len() < MAX_RAW_LINE_BYTES {
                self.pending.push(byte);
            } else {
                self.overflowed = true;
            }
        }
        lines
    }

    pub fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() && !self.overflowed {
            None
        } else {
            self.take_line()
        }
    }

    fn take_line(&mut self) -> Option<String> {
        let overflowed = std::mem::take(&mut self.overflowed);
        let bytes = std::mem::take(&mut self.pending);
        if overflowed {
            return Some("서버 진단 한 줄이 길어 내용을 숨겼습니다".into());
        }
        let sanitized = sanitize_stderr_line(&bytes);
        (!sanitized.is_empty()).then_some(sanitized)
    }
}

pub fn sanitize_stderr_line(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let normalized: String = decoded
        .chars()
        .filter_map(|character| match character {
            '\r' => None,
            '\t' => Some(' '),
            value if value.is_control() => None,
            value => Some(value),
        })
        .collect();
    // Token-by-token path replacement can leak a suffix when a quoted path
    // contains spaces (`"C:\\Private Folder\\file"`). Fail closed for the
    // whole untrusted line whenever it contains a path or URL separator.
    if normalized.contains('/') || normalized.contains('\\') {
        return "서버 진단에 경로 또는 URL이 포함되어 내용을 숨겼습니다".into();
    }
    let mut sanitized = Vec::new();
    let mut redact_following = 0_u8;
    for token in normalized.split_whitespace() {
        if redact_following > 0 {
            sanitized.push("<redacted>".to_owned());
            redact_following -= 1;
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if let Some(redacted) = redact_sensitive_token(token, &lower) {
            redact_following = if lower.contains("authorization") || lower.contains("bearer") {
                2
            } else if !token.contains('=') && !token.contains(':') {
                1
            } else {
                0
            };
            sanitized.push(redacted);
        } else if looks_like_known_token(token) {
            sanitized.push("<redacted>".to_owned());
        } else {
            sanitized.push(token.to_owned());
        }
    }
    bound_message(sanitized.join(" "))
}

fn redact_sensitive_token(token: &str, lower: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "authorization",
        "cookie",
        "password",
        "passwd",
        "credential",
        "client_secret",
        "clientsecret",
        "access_token",
        "refresh_token",
        "api_key",
        "apikey",
        "x-api-key",
        "bearer",
        "token",
        "secret",
    ];
    if !MARKERS.iter().any(|marker| lower.contains(marker)) {
        return None;
    }
    let separator = token.find(['=', ':']);
    Some(match separator {
        Some(index) => format!("{}<redacted>", &token[..=index]),
        None => "<redacted>".into(),
    })
}

fn looks_like_known_token(token: &str) -> bool {
    token.split(['=', ':']).any(|candidate| {
        let trimmed = candidate.trim_matches(|character: char| {
            matches!(
                character,
                '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        });
        let lower = trimmed.to_ascii_lowercase();
        lower.starts_with("sk-")
            || lower.starts_with("ghp_")
            || lower.starts_with("github_pat_")
            || (trimmed.starts_with("eyJ") && trimmed.matches('.').count() >= 2)
    })
}

fn bound_message(message: String) -> String {
    let mut characters = message.chars();
    let bounded: String = characters.by_ref().take(MAX_MESSAGE_CHARS).collect();
    if characters.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_redacts_paths_urls_and_credentials_without_echoing_values() {
        let line = br#"failed C:\Users\dev\project /home/dev/project https://example.test/log path=C:\private url=https://private.test relative=src/main.rs Authorization: Bearer raw-token password=hunter2 ghp_123456"#;
        let output = sanitize_stderr_line(line);
        assert!(!output.contains("Users"));
        assert!(!output.contains("/home"));
        assert!(!output.contains("example.test"));
        assert!(!output.contains("private.test"));
        assert!(!output.contains("src/main.rs"));
        assert!(!output.contains("raw-token"));
        assert!(!output.contains("hunter2"));
        assert!(!output.contains("ghp_123456"));
        assert_eq!(
            output,
            "서버 진단에 경로 또는 URL이 포함되어 내용을 숨겼습니다"
        );
    }

    #[test]
    fn stderr_redacts_credentials_when_no_path_is_present() {
        let output = sanitize_stderr_line(
            b"Authorization: Bearer raw-token password=hunter2 value=ghp_123456 account=sk-fixture",
        );
        assert!(!output.contains("raw-token"));
        assert!(!output.contains("hunter2"));
        assert!(!output.contains("ghp_123456"));
        assert!(!output.contains("sk-fixture"));
        assert!(output.contains("<redacted>"));
    }

    #[test]
    fn quoted_path_with_spaces_hides_the_entire_line_without_leaking_a_suffix() {
        let output = sanitize_stderr_line(br#"failed at "C:\Private Folder\source.rs""#);
        assert_eq!(
            output,
            "서버 진단에 경로 또는 URL이 포함되어 내용을 숨겼습니다"
        );
        assert!(!output.contains("Private"));
        assert!(!output.contains("Folder"));
    }

    #[test]
    fn chunked_lines_are_assembled_and_invalid_control_bytes_are_removed() {
        let mut sanitizer = StderrLineSanitizer::default();
        assert!(sanitizer.push(b"warn part").is_empty());
        assert_eq!(
            sanitizer.push(b" two\nnext\0line\n"),
            ["warn part two", "nextline"]
        );
        assert!(sanitizer.finish().is_none());
    }

    #[test]
    fn oversized_line_is_replaced_instead_of_partially_exposed() {
        let mut sanitizer = StderrLineSanitizer::default();
        let mut bytes = vec![b'a'; MAX_RAW_LINE_BYTES + 1];
        bytes.push(b'\n');
        assert_eq!(
            sanitizer.push(&bytes),
            ["서버 진단 한 줄이 길어 내용을 숨겼습니다"]
        );
    }

    #[test]
    fn store_bounds_each_language_and_reports_dropped_entries() {
        let mut store = LspLogStore::default();
        for index in 0..=MAX_ENTRIES_PER_LANGUAGE {
            store.append("rust", LspLogLevel::Info, "fixture", index.to_string());
        }
        store.record_stderr_state("rust", 12, true);
        let snapshot = store.snapshots().pop().unwrap();
        assert_eq!(snapshot.entries.len(), MAX_ENTRIES_PER_LANGUAGE);
        assert_eq!(snapshot.dropped_entries, 1);
        assert_eq!(snapshot.dropped_stderr_bytes, 12);
        assert!(snapshot.stderr_truncated);
        assert_eq!(snapshot.entries[0].message, "1");
    }

    #[test]
    fn store_bounds_the_number_of_language_buckets() {
        let mut store = LspLogStore::default();
        for index in 0..=MAX_LOG_LANGUAGES {
            store.append(
                &format!("language-{index}"),
                LspLogLevel::Info,
                "fixture",
                "message",
            );
        }
        assert_eq!(store.snapshots().len(), MAX_LOG_LANGUAGES);
    }
}
