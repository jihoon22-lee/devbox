//! Offline-first quick-capture input and document policy.
//!
//! The UI may collect a draft, but this module is the final authority for the
//! values that can cross the native storage boundary.  It deliberately keeps
//! the policy dependency-free so it can be tested on WSL without starting a
//! Tauri window or touching the configured vault.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const INBOX_DIR: &str = "Inbox";
pub const DEFAULT_TITLE: &str = "빠른 캡처";
pub const MAX_TITLE_CHARS: usize = 200;
/// A UTF-8 scalar can occupy at most four bytes.  This is a cheap pre-check
/// before the normalized title is copied into a second allocation.
pub const MAX_TITLE_BYTES: usize = MAX_TITLE_CHARS * 4;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// CRLF normalization can reduce the input by at most one byte per pair.  The
/// raw bound keeps an untrusted command payload from being needlessly copied
/// at an unbounded size while still allowing a normalized 64 KiB body.
pub const MAX_RAW_BODY_BYTES: usize = MAX_BODY_BYTES * 2;
pub const MAX_TAGS: usize = 20;
pub const MAX_TAG_CHARS: usize = 48;
pub const MAX_TAG_ITEM_BYTES: usize = MAX_TAG_CHARS * 4;
pub const MAX_TAG_BYTES: usize = 1_024;
pub const MAX_COLLISION_ATTEMPTS: u32 = 100;

/// The request accepted by the Tauri command.  It contains no path and cannot
/// select a destination outside the fixed Inbox target.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QuickCaptureInput {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Normalized values used by both preview and save.  Returning these values to
/// the frontend makes the preview deterministic and ensures the saved file is
/// exactly what the user approved.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedCapture {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    EmptyBody,
    InvalidText,
    TitleTooLong,
    BodyTooLarge,
    TooManyTags,
    TagTooLong,
    TagsTooLarge,
    InvalidTag,
    SensitiveContent,
}

impl CaptureError {
    /// Stable, non-sensitive user-facing messages.  No input value or OS
    /// error is ever included in a command error.
    pub const fn message(self) -> &'static str {
        match self {
            Self::SensitiveContent => "민감한 정보가 포함되어 있어 저장하지 않았습니다",
            Self::EmptyBody => "빠른 캡처 본문을 입력하세요",
            Self::InvalidText
            | Self::TitleTooLong
            | Self::BodyTooLarge
            | Self::TooManyTags
            | Self::TagTooLong
            | Self::TagsTooLarge
            | Self::InvalidTag => "빠른 캡처 입력이 올바르지 않습니다",
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for CaptureError {}

/// Validate and normalize a draft without touching the filesystem.
pub fn normalize(input: QuickCaptureInput) -> Result<NormalizedCapture, CaptureError> {
    if input.title.len() > MAX_TITLE_BYTES {
        return Err(CaptureError::TitleTooLong);
    }
    let title_value = input.title.trim();
    if title_value.len() > MAX_TITLE_BYTES {
        return Err(CaptureError::TitleTooLong);
    }
    let title = title_value.to_string();
    if contains_single_line_forbidden(&title) {
        return Err(CaptureError::InvalidText);
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(CaptureError::TitleTooLong);
    }

    if input.body.len() > MAX_RAW_BODY_BYTES {
        return Err(CaptureError::BodyTooLarge);
    }
    let body = normalize_line_endings(&input.body);
    if body.trim().is_empty() {
        return Err(CaptureError::EmptyBody);
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(CaptureError::BodyTooLarge);
    }
    if contains_forbidden_text(&body) {
        return Err(CaptureError::InvalidText);
    }

    if input.tags.len() > MAX_TAGS {
        return Err(CaptureError::TooManyTags);
    }
    let mut tags = Vec::with_capacity(input.tags.len());
    let mut seen = BTreeSet::new();
    let mut tag_bytes = 0_usize;
    for raw in input.tags {
        if raw.len() > MAX_TAG_ITEM_BYTES {
            return Err(CaptureError::TagTooLong);
        }
        let tag_value = raw.trim();
        if tag_value.is_empty() {
            continue;
        }
        if tag_value.len() > MAX_TAG_ITEM_BYTES || tag_value.chars().count() > MAX_TAG_CHARS {
            return Err(CaptureError::TagTooLong);
        }
        let tag = tag_value.to_string();
        if contains_single_line_forbidden(&tag)
            || tag
                .chars()
                .any(|character| matches!(character, ',' | '[' | ']' | '"'))
        {
            return Err(CaptureError::InvalidTag);
        }
        tag_bytes = tag_bytes.saturating_add(tag.len());
        if tag_bytes > MAX_TAG_BYTES {
            return Err(CaptureError::TagsTooLarge);
        }
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
    }

    let title = if title.is_empty() {
        DEFAULT_TITLE.to_string()
    } else {
        title
    };
    if looks_sensitive(&title)
        || looks_sensitive(&body)
        || tags.iter().any(|tag| looks_sensitive(tag))
    {
        return Err(CaptureError::SensitiveContent);
    }

    Ok(NormalizedCapture { title, body, tags })
}

/// Render the portable Markdown source.  The output is deterministic for a
/// normalized input; timestamp and collision suffixes are intentionally kept
/// in the filename rather than in the note body.
pub fn render_markdown(capture: &NormalizedCapture) -> Result<String, CaptureError> {
    validate_normalized(capture)?;
    let mut document = String::with_capacity(capture.body.len() + 128);
    document.push_str("---\n");
    document.push_str("title: ");
    document
        .push_str(&serde_json::to_string(&capture.title).map_err(|_| CaptureError::InvalidText)?);
    document.push('\n');
    document.push_str("tags: [");
    for (index, tag) in capture.tags.iter().enumerate() {
        if index > 0 {
            document.push_str(", ");
        }
        document.push_str(&serde_json::to_string(tag).map_err(|_| CaptureError::InvalidTag)?);
    }
    document.push_str("]\n");
    document.push_str("---\n\n");
    document.push_str(&capture.body);
    if !document.ends_with('\n') {
        document.push('\n');
    }
    Ok(document)
}

/// Build a stable filename for a Unix timestamp and a collision ordinal.
/// `ordinal == 1` has no suffix; later files use `-2`, `-3`, ... .
pub fn filename_for_timestamp(unix_seconds: i64, ordinal: u32) -> String {
    let seconds = unix_seconds.max(0);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    let suffix = if ordinal <= 1 {
        String::new()
    } else {
        format!("-{}", ordinal)
    };
    format!(
        "quick-capture-{year:04}-{month:02}-{day:02}-{hour:02}-{minute:02}-{second:02}{suffix}.md"
    )
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

/// Keep the renderer safe even if a future caller constructs a
/// `NormalizedCapture` without first going through `normalize`.
fn validate_normalized(capture: &NormalizedCapture) -> Result<(), CaptureError> {
    if capture.title.is_empty() {
        return Err(CaptureError::InvalidText);
    }
    if capture.title.len() > MAX_TITLE_BYTES || capture.title.chars().count() > MAX_TITLE_CHARS {
        return Err(CaptureError::TitleTooLong);
    }
    if contains_single_line_forbidden(&capture.title) {
        return Err(CaptureError::InvalidText);
    }
    if capture.body.trim().is_empty() {
        return Err(CaptureError::EmptyBody);
    }
    if capture.body.len() > MAX_BODY_BYTES {
        return Err(CaptureError::BodyTooLarge);
    }
    if contains_forbidden_text(&capture.body) {
        return Err(CaptureError::InvalidText);
    }
    if capture.tags.len() > MAX_TAGS {
        return Err(CaptureError::TooManyTags);
    }
    let mut total_bytes = 0_usize;
    let mut seen = BTreeSet::new();
    for tag in &capture.tags {
        if tag.is_empty() {
            return Err(CaptureError::InvalidTag);
        }
        if tag.len() > MAX_TAG_ITEM_BYTES || tag.chars().count() > MAX_TAG_CHARS {
            return Err(CaptureError::TagTooLong);
        }
        if contains_single_line_forbidden(tag)
            || tag
                .chars()
                .any(|character| matches!(character, ',' | '[' | ']' | '"'))
        {
            return Err(CaptureError::InvalidTag);
        }
        total_bytes = total_bytes.saturating_add(tag.len());
        if total_bytes > MAX_TAG_BYTES || !seen.insert(tag) {
            return Err(if total_bytes > MAX_TAG_BYTES {
                CaptureError::TagsTooLarge
            } else {
                CaptureError::InvalidTag
            });
        }
    }
    if looks_sensitive(&capture.title)
        || looks_sensitive(&capture.body)
        || capture.tags.iter().any(|tag| looks_sensitive(tag))
    {
        return Err(CaptureError::SensitiveContent);
    }
    Ok(())
}

fn contains_forbidden_text(value: &str) -> bool {
    value.chars().any(|character| {
        character == '\0'
            || is_line_separator(character)
            || (character.is_control() && character != '\n' && character != '\t')
    })
}

fn contains_single_line_forbidden(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || is_line_separator(character))
}

fn is_line_separator(character: char) -> bool {
    matches!(character, '\u{2028}' | '\u{2029}')
}

fn is_boundary(value: &[u8], index: usize, length: usize) -> bool {
    let before = index
        .checked_sub(1)
        .and_then(|position| value.get(position))
        .copied();
    let after = value.get(index + length).copied();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
    !before.is_some_and(is_word) && !after.is_some_and(is_word)
}

fn has_assignment(value: &str, marker: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let marker_bytes = marker.as_bytes();
    let mut offset = 0_usize;
    while let Some(relative) = lower[offset..].find(marker) {
        let index = offset + relative;
        if is_boundary(bytes, index, marker_bytes.len()) {
            let rest = lower[index + marker_bytes.len()..].trim_start();
            if let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) {
                let candidate = rest.trim();
                if !candidate.is_empty() {
                    return true;
                }
            }
        }
        offset = index.saturating_add(marker_bytes.len());
        if offset >= lower.len() {
            break;
        }
    }
    false
}

/// Conservative local redaction gate.  A false positive is preferable to
/// writing a value that looks like an API credential into a plaintext vault.
fn looks_sensitive(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin ") && lower.contains("private key-----") {
        return true;
    }
    for marker in [
        "api_key",
        "api-key",
        "access_key",
        "access-key",
        "client_secret",
        "client-secret",
        "authorization",
        "x-api-key",
        "x_api_key",
        "password",
        "passwd",
        "private-key",
        "private_key",
        "secret",
        "token",
    ] {
        if has_assignment(value, marker) {
            return true;
        }
    }
    for prefix in [
        "ghp_",
        "github_pat_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "xoxb-",
        "xoxp-",
        "akia",
        "sk-",
        "glpat-",
        "npm_",
        "pypi-",
    ] {
        if contains_credential_prefix(&lower, prefix) {
            return true;
        }
    }
    let mut saw_bearer = false;
    for word in lower.split_whitespace() {
        if saw_bearer && word.len() >= 12 {
            return true;
        }
        saw_bearer = word == "bearer";
    }
    false
}

fn contains_credential_prefix(value: &str, prefix: &str) -> bool {
    let mut offset = 0_usize;
    while let Some(relative) = value[offset..].find(prefix) {
        let index = offset + relative;
        let tail = &value[index + prefix.len()..];
        if tail
            .chars()
            .take_while(|character| !character.is_whitespace())
            .count()
            >= 12
        {
            return true;
        }
        offset = index.saturating_add(prefix.len());
        if offset >= value.len() {
            break;
        }
    }
    false
}

/// Howard Hinnant's civil_from_days algorithm (epoch 1970-01-01).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_part + 2) / 5 + 1) as i64;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    } as i64;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(title: &str, body: &str, tags: &[&str]) -> QuickCaptureInput {
        QuickCaptureInput {
            title: title.to_string(),
            body: body.to_string(),
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    #[test]
    fn normalizes_input_and_renders_deterministically() {
        let normalized = normalize(input(
            "  Idea  ",
            "line 1\r\nline 2",
            &["rust", "rust", "tauri"],
        ))
        .unwrap();
        assert_eq!(normalized.title, "Idea");
        assert_eq!(normalized.body, "line 1\nline 2");
        assert_eq!(normalized.tags, ["rust", "tauri"]);
        assert_eq!(
            render_markdown(&normalized).unwrap(),
            "---\ntitle: \"Idea\"\ntags: [\"rust\", \"tauri\"]\n---\n\nline 1\nline 2\n"
        );
    }

    #[test]
    fn quotes_frontmatter_values_without_losing_punctuation() {
        let normalized = normalize(input(
            r#"Release: "stable" \ path"#,
            "body",
            &[r"C:\tools", "#rust"],
        ))
        .unwrap();
        assert_eq!(
            render_markdown(&normalized).unwrap(),
            r##"---
title: "Release: \"stable\" \\ path"
tags: ["C:\\tools", "#rust"]
---

body
"##
        );
    }

    #[test]
    fn request_wire_shape_rejects_unknown_control_fields() {
        let result = serde_json::from_value::<QuickCaptureInput>(serde_json::json!({
            "title": "Idea",
            "body": "body",
            "tags": [],
            "path": "outside.md"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn blank_title_uses_fixed_default_and_blank_body_is_rejected() {
        let normalized = normalize(input("  ", "note", &[])).unwrap();
        assert_eq!(normalized.title, DEFAULT_TITLE);
        assert_eq!(
            normalize(input("title", " \n\t", &[])),
            Err(CaptureError::EmptyBody)
        );
    }

    #[test]
    fn enforces_text_tag_and_body_bounds() {
        assert_eq!(
            normalize(input(&"x".repeat(MAX_TITLE_CHARS + 1), "body", &[])),
            Err(CaptureError::TitleTooLong)
        );
        assert_eq!(
            normalize(input("title", &"x".repeat(MAX_BODY_BYTES + 1), &[])),
            Err(CaptureError::BodyTooLarge)
        );
        assert_eq!(
            normalize(input("title", "body", &vec!["x"; MAX_TAGS + 1])),
            Err(CaptureError::TooManyTags)
        );
        assert_eq!(
            normalize(input("title", "body", &[&"x".repeat(MAX_TAG_CHARS + 1)])),
            Err(CaptureError::TagTooLong)
        );
        assert!(normalize(input("😀".repeat(MAX_TITLE_CHARS).as_str(), "body", &[])).is_ok());
        assert_eq!(
            normalize(input(&"😀".repeat(MAX_TITLE_CHARS + 1), "body", &[],)),
            Err(CaptureError::TitleTooLong)
        );
        assert!(normalize(input("title", &"😀".repeat(MAX_BODY_BYTES / 4), &[])).is_ok());
        assert_eq!(
            normalize(input("title", &"😀".repeat(MAX_BODY_BYTES / 4 + 1), &[])),
            Err(CaptureError::BodyTooLarge)
        );
        let raw_at_limit = format!("{}x", "\r\n".repeat(MAX_BODY_BYTES - 1));
        assert!(normalize(input("title", &raw_at_limit, &[])).is_ok());
        let raw_over_limit = format!("{}x", "\r\n".repeat(MAX_BODY_BYTES));
        assert_eq!(
            normalize(input("title", &raw_over_limit, &[])),
            Err(CaptureError::BodyTooLarge)
        );
        assert!(normalize(input("title", "body", &[&"😀".repeat(MAX_TAG_CHARS)])).is_ok());
    }

    #[test]
    fn rejects_control_injection_and_unsafe_tags() {
        assert_eq!(
            normalize(input("bad\nname", "body", &[])),
            Err(CaptureError::InvalidText)
        );
        assert_eq!(
            normalize(input("title\u{2028}name", "body", &[])),
            Err(CaptureError::InvalidText)
        );
        assert_eq!(
            normalize(input("title", "body\u{2029}text", &[])),
            Err(CaptureError::InvalidText)
        );
        assert_eq!(
            normalize(input("title", "body", &["safe,unsafe"])),
            Err(CaptureError::InvalidTag)
        );
        assert_eq!(
            normalize(input("title", "body", &["bad\u{0007}"])),
            Err(CaptureError::InvalidTag)
        );
    }

    #[test]
    fn rejects_credential_like_values_without_echoing_them() {
        for body in [
            "api_key=super-secret-value",
            "X-API-Key: super-secret-value",
            "Authorization: Bearer abcdefghijklmnop",
            "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----",
            "ghp_abcdefghijklmnop",
            "ghp_short ghp_abcdefghijklmnop",
            "sk-abcdefghijklmnop",
        ] {
            let error = normalize(input("title", body, &[])).unwrap_err();
            assert_eq!(error, CaptureError::SensitiveContent);
            assert!(!error.to_string().contains(body));
        }
    }

    #[test]
    fn renderer_rechecks_the_normalized_boundary() {
        let oversized = NormalizedCapture {
            title: "title".to_string(),
            body: "x".repeat(MAX_BODY_BYTES + 1),
            tags: Vec::new(),
        };
        assert_eq!(render_markdown(&oversized), Err(CaptureError::BodyTooLarge));

        let sensitive = NormalizedCapture {
            title: "title".to_string(),
            body: "X-API-Key: hidden-value".to_string(),
            tags: Vec::new(),
        };
        assert_eq!(
            render_markdown(&sensitive),
            Err(CaptureError::SensitiveContent)
        );
    }

    #[test]
    fn filename_is_utc_deterministic_and_collision_suffix_is_bounded() {
        assert_eq!(
            filename_for_timestamp(0, 1),
            "quick-capture-1970-01-01-00-00-00.md"
        );
        assert_eq!(
            filename_for_timestamp(0, 3),
            "quick-capture-1970-01-01-00-00-00-3.md"
        );
        assert_eq!(
            filename_for_timestamp(1_754_952_000, 2),
            "quick-capture-2025-08-11-22-40-00-2.md"
        );
        assert_eq!(
            filename_for_timestamp(-1, 0),
            "quick-capture-1970-01-01-00-00-00.md"
        );
    }
}
