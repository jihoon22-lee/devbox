//! Bounded, offline text extraction for the Everything+ content index.
//!
//! This module deliberately handles only plain text/source/Markdown.  Archive and
//! document-container extraction belongs to a later feature and must not be
//! smuggled into the base content index through a permissive file reader.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// Maximum bytes read from one content-index candidate.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Maximum Unicode scalar values retained in the FTS document.
pub const MAX_TEXT_CHARS: usize = 2_000_000;
/// Cooperative wall-clock budget for one candidate.
pub const PROCESSING_LIMIT: Duration = Duration::from_secs(10);
/// Bumped only when the plain-text extraction rules change.
pub const EXTRACTOR_VERSION: &str = "text-v1";
/// Maximum snippet size returned to the frontend for one result.
pub const MAX_SNIPPET_CHARS: usize = 4 * 1024;

const READ_CHUNK_BYTES: usize = 64 * 1024;
const UTF16_SAMPLE_BYTES: usize = 16 * 1024;

/// Extensions whose contents are useful for local developer search.  The list
/// is intentionally explicit: a content root never means "read every file".
pub const TEXT_EXTENSIONS: &[&str] = &[
    "asm",
    "bat",
    "c",
    "cc",
    "cfg",
    "clj",
    "cljs",
    "conf",
    "cpp",
    "cs",
    "css",
    "csv",
    "dart",
    "diff",
    "ex",
    "exs",
    "fish",
    "gitattributes",
    "gitignore",
    "go",
    "graphql",
    "h",
    "hpp",
    "hrl",
    "htm",
    "html",
    "ini",
    "java",
    "jl",
    "js",
    "json",
    "jsx",
    "kt",
    "kts",
    "less",
    "log",
    "lua",
    "md",
    "mdown",
    "mkd",
    "patch",
    "php",
    "pl",
    "ps1",
    "py",
    "rb",
    "rs",
    "rst",
    "scss",
    "sh",
    "sql",
    "swift",
    "tex",
    "toml",
    "ts",
    "tsx",
    "txt",
    "vue",
    "xml",
    "xsd",
    "yaml",
    "yml",
    "zsh",
];

/// Machine-readable status values persisted with each eligible file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentStatus {
    Indexed,
    TooLarge,
    UnsupportedEncoding,
    ReadError,
    Timeout,
    ChangedDuringRead,
    SkippedSensitive,
}

impl ContentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::TooLarge => "too_large",
            Self::UnsupportedEncoding => "unsupported_encoding",
            Self::ReadError => "read_error",
            Self::Timeout => "timeout",
            Self::ChangedDuringRead => "changed_during_read",
            Self::SkippedSensitive => "skipped_sensitive",
        }
    }
}

/// Result of one bounded extraction.  Failed records deliberately contain no
/// source bytes, and `error_code` is a fixed code rather than an IO detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRecord {
    pub text: String,
    pub status: ContentStatus,
    pub encoding: Option<&'static str>,
    pub truncated: bool,
    pub error_code: Option<&'static str>,
    pub text_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileFingerprint {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    fn changed_from(self, initial: Self, expected_size: u64) -> bool {
        initial.len != expected_size || self.len != expected_size || self != initial
    }
}

impl ContentRecord {
    fn failure(status: ContentStatus, error_code: &'static str) -> Self {
        Self {
            text: String::new(),
            status,
            encoding: None,
            truncated: false,
            error_code: Some(error_code),
            text_chars: 0,
        }
    }
}

/// Returns true only for a known content extension.  The comparison is
/// case-insensitive and does not inspect file bytes.
pub fn is_text_ext(ext: &str) -> bool {
    let lower = ext.trim_start_matches('.').to_ascii_lowercase();
    TEXT_EXTENSIONS.iter().any(|candidate| *candidate == lower)
}

/// Returns true for files that commonly contain credentials or private key
/// material.  These are skipped before reading, even when their extension is a
/// text extension.  Normal source files containing a word such as `secret` are
/// still indexable; this is a filename policy, not a content classifier.
pub fn is_sensitive_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower == ".env"
        || lower.starts_with(".env.")
        || lower == ".npmrc"
        || lower == ".netrc"
        || lower == "credentials"
        || lower.starts_with("credentials.")
        || lower == "secrets"
        || lower.starts_with("secrets.")
        || lower.ends_with(".secret")
        || lower.ends_with(".secrets")
        || lower.starts_with("id_rsa")
        || lower.starts_with("id_ed25519")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
}

/// Whether an extension is eligible for a content status record.  Oversized
/// files intentionally remain eligible so their `too_large` status is visible
/// instead of silently looking as if indexing was disabled.
pub fn is_content_candidate(path: &Path) -> bool {
    if is_sensitive_filename(path) {
        return true;
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                ".dockerignore"
                    | ".editorconfig"
                    | ".gitattributes"
                    | ".gitignore"
                    | "dockerfile"
                    | "makefile"
                    | "license"
                    | "readme"
            )
        })
    {
        return true;
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    is_text_ext(ext)
}

/// Extract one file while enforcing byte, character, encoding, race, and time
/// bounds.  No error returned by this function contains the source path.
pub fn extract_file(path: &Path, expected_size: u64, started: Instant) -> ContentRecord {
    if is_sensitive_filename(path) {
        return ContentRecord::failure(ContentStatus::SkippedSensitive, "sensitive_file");
    }
    if expected_size > MAX_FILE_BYTES {
        return ContentRecord::failure(ContentStatus::TooLarge, "file_too_large");
    }
    if timed_out(started) {
        return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
    }

    let before = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        _ => return ContentRecord::failure(ContentStatus::ReadError, "read_error"),
    };
    let before_fingerprint = FileFingerprint::from_metadata(&before);
    if before_fingerprint.len != expected_size {
        return ContentRecord::failure(ContentStatus::ChangedDuringRead, "changed_during_read");
    }

    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return ContentRecord::failure(ContentStatus::ReadError, "read_error"),
    };
    let mut bytes = Vec::with_capacity(expected_size.min(MAX_FILE_BYTES) as usize);
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        if timed_out(started) {
            return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
        }
        let read = match file.read(&mut chunk) {
            Ok(read) => read,
            Err(_) => return ContentRecord::failure(ContentStatus::ReadError, "read_error"),
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return ContentRecord::failure(ContentStatus::TooLarge, "file_too_large");
        }
    }

    let opened_after = match file.metadata() {
        Ok(metadata) if metadata.file_type().is_file() => FileFingerprint::from_metadata(&metadata),
        _ => {
            return ContentRecord::failure(ContentStatus::ChangedDuringRead, "changed_during_read")
        }
    };

    let after = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        _ => {
            return ContentRecord::failure(ContentStatus::ChangedDuringRead, "changed_during_read")
        }
    };
    let after_fingerprint = FileFingerprint::from_metadata(&after);
    if bytes.len() as u64 != expected_size
        || opened_after.changed_from(before_fingerprint, expected_size)
        || after_fingerprint.changed_from(before_fingerprint, expected_size)
    {
        return ContentRecord::failure(ContentStatus::ChangedDuringRead, "changed_during_read");
    }

    extract_bytes(&bytes, started)
}

/// Decode a bounded byte fixture.  Kept public for deterministic unit tests and
/// for the watcher/indexer to share exactly the same UTF-8/UTF-16 rules.
pub fn extract_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure(ContentStatus::TooLarge, "file_too_large");
    }
    if timed_out(started) {
        return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
    }

    let (text, encoding) = match decode(bytes, started) {
        Ok(decoded) => decoded,
        Err(DecodeFailure::Timeout) => {
            return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout")
        }
        Err(DecodeFailure::Unsupported) => {
            return ContentRecord::failure(
                ContentStatus::UnsupportedEncoding,
                "unsupported_encoding",
            )
        }
    };
    if text.contains('\0') {
        return ContentRecord::failure(ContentStatus::UnsupportedEncoding, "unsupported_encoding");
    }
    let (text, truncated, text_chars) = truncate_text(text, started);
    if timed_out(started) {
        return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
    }
    ContentRecord {
        text,
        status: ContentStatus::Indexed,
        encoding: Some(encoding),
        truncated,
        error_code: truncated.then_some("text_limit"),
        text_chars,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeFailure {
    Unsupported,
    Timeout,
}

fn decode(bytes: &[u8], started: Instant) -> Result<(String, &'static str), DecodeFailure> {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        let text = std::str::from_utf8(&bytes[3..])
            .map_err(|_| DecodeFailure::Unsupported)?
            .to_owned();
        return Ok((text, "utf8-bom"));
    }
    if bytes.starts_with(b"\xFF\xFE") {
        return decode_utf16(&bytes[2..], true, started).map(|text| (text, "utf16-le"));
    }
    if bytes.starts_with(b"\xFE\xFF") {
        return decode_utf16(&bytes[2..], false, started).map(|text| (text, "utf16-be"));
    }
    if let Some(little_endian) = detect_utf16_without_bom(bytes) {
        return decode_utf16(bytes, little_endian, started).map(|text| {
            (
                text,
                if little_endian {
                    "utf16-le"
                } else {
                    "utf16-be"
                },
            )
        });
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DecodeFailure::Unsupported)?
        .to_owned();
    Ok((text, "utf8"))
}

fn decode_utf16(
    bytes: &[u8],
    little_endian: bool,
    started: Instant,
) -> Result<String, DecodeFailure> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeFailure::Unsupported);
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        if index % 8192 == 0 && timed_out(started) {
            return Err(DecodeFailure::Timeout);
        }
        let unit = if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        };
        units.push(unit);
    }
    String::from_utf16(&units).map_err(|_| DecodeFailure::Unsupported)
}

fn detect_utf16_without_bom(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let sample = &bytes[..bytes.len().min(UTF16_SAMPLE_BYTES)];
    let odd_zero = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    let even_zero = sample.iter().step_by(2).filter(|byte| **byte == 0).count();
    let pairs = sample.len() / 2;
    if odd_zero * 4 >= pairs * 3 {
        Some(true)
    } else if even_zero * 4 >= pairs * 3 {
        Some(false)
    } else {
        None
    }
}

fn truncate_text(text: String, started: Instant) -> (String, bool, usize) {
    let mut output = String::with_capacity(text.len().min(MAX_TEXT_CHARS * 4));
    let mut chars = 0;
    let mut truncated = false;
    for (index, character) in text.chars().enumerate() {
        if index == MAX_TEXT_CHARS {
            truncated = true;
            break;
        }
        if index % 8192 == 0 && timed_out(started) {
            // The caller performs the final timeout check.  Returning the
            // bounded prefix keeps this helper allocation-safe; the record is
            // discarded as timeout by `extract_bytes` immediately afterwards.
            break;
        }
        output.push(character);
        chars += 1;
    }
    if text.chars().nth(MAX_TEXT_CHARS).is_some() {
        truncated = true;
    }
    (output, truncated, chars)
}

fn timed_out(started: Instant) -> bool {
    started.elapsed() >= PROCESSING_LIMIT
}

/// Redacts common credential values from snippets before they cross the UI
/// boundary.  The full content remains local to the app-owned index; this is
/// intentionally conservative and deterministic rather than a claim of secret
/// detection completeness.
pub fn redact_snippet(input: &str) -> String {
    let lower_input = input.to_ascii_lowercase();
    let redacted = if lower_input.contains("-----begin") {
        "[REDACTED PRIVATE KEY]".to_string()
    } else {
        let lower = lower_input;
        let keys = [
            "authorization:",
            "\"authorization\":",
            "authorization=",
            "bearer ",
            "cookie:",
            "\"cookie\":",
            "set-cookie:",
            "\"set-cookie\":",
            "password=",
            "password:",
            "\"password\":",
            "token=",
            "token:",
            "\"token\":",
            "secret=",
            "secret:",
            "\"secret\":",
            "api_key=",
            "api_key:",
            "\"api_key\":",
            "api-key=",
            "api-key:",
            "\"api-key\":",
            "apikey=",
            "apikey:",
            "\"apikey\":",
            "access_token=",
            "access_token:",
            "\"access_token\":",
            "refresh_token=",
            "refresh_token:",
            "\"refresh_token\":",
            "client_secret=",
            "client_secret:",
            "\"client_secret\":",
            "private_key:",
            "\"private_key\":",
        ];
        let mut spans = Vec::new();
        for key in keys {
            let mut offset = 0;
            while let Some(relative) = lower[offset..].find(key) {
                let start = offset + relative;
                let mut value_start = start + key.len();
                while let Some(character) = input[value_start..].chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    value_start += character.len_utf8();
                }
                let (value_start, value_end) = match input[value_start..].chars().next() {
                    Some(quote @ ('"' | '\'')) => {
                        let content_start = value_start + quote.len_utf8();
                        let mut escaped = false;
                        let mut end = input.len();
                        for (index, character) in input[content_start..].char_indices() {
                            if escaped {
                                escaped = false;
                            } else if character == '\\' {
                                escaped = true;
                            } else if character == quote {
                                end = content_start + index;
                                break;
                            }
                        }
                        (content_start, end)
                    }
                    Some(_) => {
                        let end = input[value_start..]
                            .char_indices()
                            .find(|(_, character)| {
                                character.is_whitespace()
                                    || matches!(*character, ',' | ';' | ']' | '}' | ')')
                            })
                            .map(|(index, _)| value_start + index)
                            .unwrap_or(input.len());
                        (value_start, end)
                    }
                    None => (value_start, value_start),
                };
                if value_start < value_end {
                    spans.push((value_start, value_end));
                }
                offset = value_end.max(start + key.len());
                if offset >= lower.len() {
                    break;
                }
            }
        }
        if spans.is_empty() {
            input.to_string()
        } else {
            spans.sort_unstable();
            let mut output = String::with_capacity(input.len());
            let mut cursor = 0;
            for (start, end) in spans {
                if start < cursor {
                    continue;
                }
                output.push_str(&input[cursor..start]);
                output.push_str("[REDACTED]");
                cursor = end;
            }
            output.push_str(&input[cursor..]);
            output
        }
    };
    let redacted = redact_known_token_patterns(&redacted);
    if redacted.chars().count() <= MAX_SNIPPET_CHARS {
        return redacted;
    }
    let mut bounded: String = redacted
        .chars()
        .take(MAX_SNIPPET_CHARS.saturating_sub(1))
        .collect();
    bounded.push('…');
    bounded
}

fn redact_known_token_patterns(input: &str) -> String {
    let mut spans = Vec::new();
    let mut candidate_start = None;
    for (index, character) in input
        .char_indices()
        .chain(std::iter::once((input.len(), ' ')))
    {
        let is_candidate =
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.');
        match (candidate_start, is_candidate) {
            (None, true) => candidate_start = Some(index),
            (Some(start), false) => {
                let candidate = &input[start..index];
                if looks_like_known_token(candidate) {
                    spans.push((start, index));
                }
                candidate_start = None;
            }
            _ => {}
        }
    }
    if spans.is_empty() {
        return input.to_string();
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for (start, end) in spans {
        output.push_str(&input[cursor..start]);
        output.push_str("[REDACTED TOKEN]");
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn looks_like_known_token(candidate: &str) -> bool {
    let prefixed = [
        "sk-",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
    ];
    if prefixed
        .iter()
        .any(|prefix| candidate.starts_with(prefix) && candidate.len() >= prefix.len() + 12)
    {
        return true;
    }
    if candidate.len() == 20
        && (candidate.starts_with("AKIA") || candidate.starts_with("ASIA"))
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return true;
    }
    let mut jwt_parts = candidate.split('.');
    let parts = (
        jwt_parts.next(),
        jwt_parts.next(),
        jwt_parts.next(),
        jwt_parts.next(),
    );
    matches!(parts, (Some(a), Some(b), Some(c), None)
    if [a, b, c].iter().all(|part| {
        part.len() >= 10
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn utf16(value: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in value.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        bytes
    }

    #[test]
    fn recognizes_developer_text_extensions_case_insensitively() {
        assert!(is_text_ext("RS"));
        assert!(is_text_ext("md"));
        assert!(is_text_ext("JSON"));
        assert!(is_text_ext("tsx"));
        assert!(!is_text_ext("png"));
        assert!(!is_text_ext("exe"));
    }

    #[test]
    fn decodes_utf8_utf16_and_empty_text() {
        let english = extract_bytes("hello world".as_bytes(), now());
        assert_eq!(english.text, "hello world");
        assert_eq!(english.encoding, Some("utf8"));

        let korean = extract_bytes("안녕하세요 devbox".as_bytes(), now());
        assert_eq!(korean.status, ContentStatus::Indexed);
        assert!(korean.text.contains("안녕하세요"));

        let little = extract_bytes(&utf16("한글 UTF-16", true), now());
        assert_eq!(little.status, ContentStatus::Indexed);
        assert_eq!(little.encoding, Some("utf16-le"));
        assert_eq!(little.text, "한글 UTF-16");

        let big = extract_bytes(&utf16("UTF-16 BE", false), now());
        assert_eq!(big.status, ContentStatus::Indexed);
        assert_eq!(big.encoding, Some("utf16-be"));
        assert_eq!(big.text, "UTF-16 BE");

        let mut no_bom = utf16("UTF-16 without BOM", true);
        no_bom.drain(..2);
        let no_bom_record = extract_bytes(&no_bom, now());
        assert_eq!(no_bom_record.status, ContentStatus::Indexed);
        assert_eq!(no_bom_record.encoding, Some("utf16-le"));
        assert_eq!(no_bom_record.text, "UTF-16 without BOM");

        let empty = extract_bytes(b"", now());
        assert_eq!(empty.status, ContentStatus::Indexed);
        assert_eq!(empty.text_chars, 0);
    }

    #[test]
    fn rejects_binary_and_invalid_utf16() {
        let binary = extract_bytes(&[0, 1, 2, 3, 4], now());
        assert_eq!(binary.status, ContentStatus::UnsupportedEncoding);
        assert_eq!(binary.error_code, Some("unsupported_encoding"));

        let invalid = extract_bytes(&[0xFF, 0xFE, 0x00], now());
        assert_eq!(invalid.status, ContentStatus::UnsupportedEncoding);
    }

    #[test]
    fn truncates_by_unicode_scalar_count_not_bytes() {
        let input = "가😊".repeat(MAX_TEXT_CHARS / 2 + 1);
        let record = extract_bytes(input.as_bytes(), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert!(record.truncated);
        assert_eq!(record.text_chars, MAX_TEXT_CHARS);
        assert_eq!(record.error_code, Some("text_limit"));
        assert!(record.text.is_char_boundary(record.text.len()));
    }

    #[test]
    fn returns_fixed_large_file_and_timeout_statuses() {
        let large = extract_bytes(&vec![b'x'; (MAX_FILE_BYTES + 1) as usize], now());
        assert_eq!(large.status, ContentStatus::TooLarge);
        assert_eq!(large.text, "");
        assert_eq!(large.error_code, Some("file_too_large"));
        let timeout = extract_bytes(b"small", Instant::now() - PROCESSING_LIMIT);
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));
    }

    #[test]
    fn detects_same_size_metadata_change_with_fingerprints() {
        let initial = FileFingerprint {
            len: 12,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let rewritten = FileFingerprint {
            len: 12,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
        };
        assert!(rewritten.changed_from(initial, 12));
        assert!(!initial.changed_from(initial, 12));
        assert!(initial.changed_from(initial, 11));
    }

    #[test]
    fn skips_credential_named_files_without_reading() {
        assert!(is_sensitive_filename(Path::new(".env")));
        assert!(is_sensitive_filename(Path::new("credentials.json")));
        assert!(is_sensitive_filename(Path::new("server.key")));
        assert!(!is_sensitive_filename(Path::new("src/secret_manager.rs")));
    }

    #[test]
    fn redacts_common_credential_values_but_preserves_normal_text() {
        let snippet = "Authorization: Bearer abc123 [review] password=letmein token: yaml-secret";
        let redacted = redact_snippet(snippet);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("letmein"));
        assert!(!redacted.contains("yaml-secret"));
        assert!(redacted.contains("[review]"));
        let json = redact_snippet(r#"{"token":"json-secret","note":"keep"}"#);
        assert!(!json.contains("json-secret"));
        assert!(json.contains("keep"));
        assert_eq!(redact_snippet("quarterly review"), "quarterly review");
        assert_eq!(
            redact_snippet("-----BEGIN PRIVATE KEY-----"),
            "[REDACTED PRIVATE KEY]"
        );
    }

    #[test]
    fn redacts_known_tokens_without_assignment_keys() {
        // Assemble fixtures at runtime so repository secret scanners do not
        // mistake deliberately fake credentials for committed live values.
        let github = ["ghp", "_1234567890abcdefghijkl"].concat();
        let aws = ["AK", "IA1234567890ABCDEF"].concat();
        let jwt = [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "abcdefghijklmnop",
        ]
        .join(".");
        let snippet = format!("use {github} and {aws} then {jwt}");
        let redacted = redact_snippet(&snippet);
        assert!(!redacted.contains(&github));
        assert!(!redacted.contains(&aws));
        assert!(!redacted.contains(&jwt));
        assert_eq!(redacted.matches("[REDACTED TOKEN]").count(), 3);
    }

    #[test]
    fn bounds_unusually_long_snippets_on_character_boundary() {
        let snippet = "가".repeat(MAX_SNIPPET_CHARS + 100);
        let redacted = redact_snippet(&snippet);
        assert_eq!(redacted.chars().count(), MAX_SNIPPET_CHARS);
        assert!(redacted.ends_with('…'));
    }
}
