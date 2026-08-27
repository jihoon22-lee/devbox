//! Project-scoped `.env` parsing and metadata.
//!
//! The parser is deliberately independent from Tauri and from process
//! spawning.  It returns the values only in a short-lived, non-serializable
//! structure.  Profile storage and every IPC DTO contain names, source,
//! conflict state, a sealed-secret reference, and an opaque content revision;
//! they never contain an environment value.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use zeroize::Zeroizing;

pub const ENVIRONMENT_SOURCE_MAX_BYTES: usize = 256;
pub const MAX_ENV_FILE_BYTES: usize = 256 * 1024;
pub const MAX_ENV_LINE_BYTES: usize = 8 * 1024;
pub const MAX_ENV_VARIABLES: usize = 128;
pub const MAX_ENV_NAME_BYTES: usize = 128;
pub const MAX_ENV_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_ENV_TOTAL_VALUE_BYTES: usize = 128 * 1024;
pub const ENVIRONMENT_REVISION_BYTES: usize = 64;

/// A conflict is metadata, not an instruction to pick a winner.  A duplicate
/// or a reserved key therefore remains visible in the preview and blocks
/// execution while the profile is enabled.
#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentConflict {
    None,
    Duplicate,
    Reserved,
    DuplicateAndReserved,
}

impl EnvironmentConflict {
    pub fn is_blocking(self) -> bool {
        !matches!(self, Self::None)
    }

    fn from_flags(duplicate: bool, reserved: bool) -> Self {
        match (duplicate, reserved) {
            (false, false) => Self::None,
            (true, false) => Self::Duplicate,
            (false, true) => Self::Reserved,
            (true, true) => Self::DuplicateAndReserved,
        }
    }
}

/// Metadata safe to persist in `ProjectProfile` and send over IPC.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentVariableMetadata {
    pub name: String,
    /// Relative project source name, for example `.env` or `.env.local`.
    pub source: String,
    pub conflict: EnvironmentConflict,
    pub secret_reference: Option<devbox_secrets::SecretReference>,
}

/// Project environment configuration persisted in a profile.  `revision` is
/// a SHA-256 digest of the selected file bytes, not the file contents.  It is
/// used to prevent applying a preview after the source changed.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectEnvironmentConfig {
    pub enabled: bool,
    pub source: String,
    pub revision: String,
    pub variables: Vec<EnvironmentVariableMetadata>,
}

/// A preview is generated natively from the selected file.  It intentionally
/// exposes only masked values and metadata; the raw values stay in the native
/// call stack and are not serializable.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnvironmentPreview {
    pub source: String,
    pub revision: String,
    pub variables: Vec<EnvironmentVariablePreview>,
    pub has_conflicts: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariablePreview {
    pub name: String,
    pub source: String,
    pub conflict: EnvironmentConflict,
    pub masked_value: String,
    pub secret_reference: Option<devbox_secrets::SecretReference>,
}

/// Internal parse result.  It deliberately has no `Serialize` or derived
/// `Debug` implementation so accidental logging cannot disclose values.
pub struct ParsedEnvironment {
    source: String,
    revision: String,
    entries: Vec<ParsedEnvironmentEntry>,
}

pub struct ParsedEnvironmentEntry {
    pub metadata: EnvironmentVariableMetadata,
    pub value: Zeroizing<String>,
}

impl fmt::Debug for ParsedEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedEnvironment")
            .field("source", &self.source)
            .field("revision", &self.revision)
            .field("variable_count", &self.entries.len())
            .finish()
    }
}

impl fmt::Debug for ParsedEnvironmentEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedEnvironmentEntry")
            .field("metadata", &self.metadata)
            .field("value", &"<redacted>")
            .finish()
    }
}

impl ParsedEnvironment {
    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn entries(&self) -> &[ParsedEnvironmentEntry] {
        &self.entries
    }

    pub fn metadata(&self) -> Vec<EnvironmentVariableMetadata> {
        self.entries
            .iter()
            .map(|entry| entry.metadata.clone())
            .collect()
    }

    pub fn has_conflicts(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.metadata.conflict.is_blocking())
    }

    #[cfg(test)]
    pub fn config(&self, enabled: bool) -> ProjectEnvironmentConfig {
        ProjectEnvironmentConfig {
            enabled,
            source: self.source.clone(),
            revision: self.revision.clone(),
            variables: self.metadata(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EnvironmentError {
    InvalidSource,
    FileTooLarge,
    InvalidUtf8,
    LineTooLong,
    TooManyVariables,
    TooMuchValueData,
    MalformedLine,
    InvalidName,
    InvalidValue,
    UnclosedQuote,
    InvalidEscape,
    InvalidMetadata,
    Cancelled,
    TimedOut,
}

impl fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSource => "환경 파일 이름이 올바르지 않습니다",
            Self::FileTooLarge => "환경 파일 크기 제한을 초과했습니다",
            Self::InvalidUtf8 => "환경 파일은 UTF-8이어야 합니다",
            Self::LineTooLong => "환경 파일 행이 너무 깁니다",
            Self::TooManyVariables => "환경 변수가 너무 많습니다",
            Self::TooMuchValueData => "환경 변수 값의 총 크기 제한을 초과했습니다",
            Self::MalformedLine => "환경 파일의 행 형식이 올바르지 않습니다",
            Self::InvalidName => "환경 변수 이름이 올바르지 않습니다",
            Self::InvalidValue => "환경 변수 값이 올바르지 않습니다",
            Self::UnclosedQuote => "환경 변수 따옴표가 닫히지 않았습니다",
            Self::InvalidEscape => "환경 변수 escape가 올바르지 않습니다",
            Self::InvalidMetadata => "환경 변수 메타데이터가 올바르지 않습니다",
            Self::Cancelled => "Workspace 작업이 취소되었습니다",
            Self::TimedOut => "Workspace 작업 시간이 초과되었습니다",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for EnvironmentError {}

/// Only project dotenv filenames are accepted.  In particular this function
/// rejects absolute paths, separators, traversal, and shell-looking names.
pub fn validate_source_name(source: &str) -> Result<(), EnvironmentError> {
    if source.is_empty()
        || source.len() > ENVIRONMENT_SOURCE_MAX_BYTES
        || source == ".env.."
        || source.contains('/')
        || source.contains('\\')
        || source.contains(':')
        || source.chars().any(char::is_control)
        || !(source == ".env"
            || source
                .strip_prefix(".env.")
                .is_some_and(valid_source_suffix))
    {
        return Err(EnvironmentError::InvalidSource);
    }
    Ok(())
}

fn valid_source_suffix(suffix: &str) -> bool {
    !suffix.is_empty()
        && !suffix.starts_with('.')
        && !suffix.contains("..")
        && !suffix.ends_with('.')
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn validate_config(config: &ProjectEnvironmentConfig) -> Result<(), EnvironmentError> {
    validate_source_name(&config.source)?;
    if config.revision.len() != ENVIRONMENT_REVISION_BYTES
        || !config
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(EnvironmentError::InvalidMetadata);
    }
    if config.variables.len() > MAX_ENV_VARIABLES {
        return Err(EnvironmentError::TooManyVariables);
    }

    let mut occurrences = HashMap::<String, usize>::new();
    for variable in &config.variables {
        validate_name(&variable.name)?;
        if variable.source != config.source {
            return Err(EnvironmentError::InvalidMetadata);
        }
        let folded = variable.name.to_ascii_uppercase();
        *occurrences.entry(folded).or_default() += 1;
        let should_reference = is_secret_name(&variable.name);
        if should_reference != variable.secret_reference.is_some() {
            return Err(EnvironmentError::InvalidMetadata);
        }
        if let Some(reference) = &variable.secret_reference {
            if !reference.is_project_environment() || reference.name != variable.name {
                return Err(EnvironmentError::InvalidMetadata);
            }
        }
    }
    // `parse_environment` updates the first occurrence when a later line
    // introduces a duplicate. Count all names before checking conflicts so a
    // valid parser-produced metadata list round-trips through persistence;
    // checking only a prefix would incorrectly reject that first entry.
    for variable in &config.variables {
        let duplicate = occurrences
            .get(&variable.name.to_ascii_uppercase())
            .is_some_and(|count| *count > 1);
        let expected_conflict =
            EnvironmentConflict::from_flags(duplicate, is_reserved_name(&variable.name));
        if variable.conflict != expected_conflict {
            return Err(EnvironmentError::InvalidMetadata);
        }
    }
    Ok(())
}

/// Parse a bounded UTF-8 dotenv file.  This is intentionally not a shell
/// parser: no command substitution, variable expansion, multiline values,
/// or arbitrary `export` syntax is executed.
pub fn parse_environment(
    source: &str,
    bytes: &[u8],
) -> Result<ParsedEnvironment, EnvironmentError> {
    validate_source_name(source)?;
    if bytes.len() > MAX_ENV_FILE_BYTES {
        return Err(EnvironmentError::FileTooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| EnvironmentError::InvalidUtf8)?;
    let mut entries: Vec<ParsedEnvironmentEntry> = Vec::new();
    let mut indexes = HashMap::<String, Vec<usize>>::new();
    let mut total_value_bytes = 0usize;

    for line in text.lines() {
        if line.len() > MAX_ENV_LINE_BYTES {
            return Err(EnvironmentError::LineTooLong);
        }
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            continue;
        }
        // An exact `export ` prefix is optional.  Other spellings are left
        // to the ordinary name validator, so `exported=value` is simply an
        // invalid variable name rather than shell syntax.
        let candidate = candidate.strip_prefix("export ").unwrap_or(candidate);
        if !candidate.contains('=') {
            return Err(EnvironmentError::MalformedLine);
        }
        let (raw_name, raw_value) = candidate
            .split_once('=')
            .ok_or(EnvironmentError::MalformedLine)?;
        let name = raw_name.trim();
        validate_name(name)?;
        if raw_name != name {
            return Err(EnvironmentError::InvalidName);
        }
        let value = parse_value(raw_value.trim())?;
        total_value_bytes = total_value_bytes
            .checked_add(value.len())
            .ok_or(EnvironmentError::TooMuchValueData)?;
        if total_value_bytes > MAX_ENV_TOTAL_VALUE_BYTES {
            return Err(EnvironmentError::TooMuchValueData);
        }
        if entries.len() >= MAX_ENV_VARIABLES {
            return Err(EnvironmentError::TooManyVariables);
        }

        let folded = name.to_ascii_uppercase();
        let duplicate = indexes.contains_key(&folded);
        let reserved = is_reserved_name(name);
        let conflict = EnvironmentConflict::from_flags(duplicate, reserved);
        let metadata = EnvironmentVariableMetadata {
            name: name.to_string(),
            source: source.to_string(),
            conflict,
            secret_reference: is_secret_name(name)
                .then(|| devbox_secrets::SecretReference::project_environment(name)),
        };
        let index = entries.len();
        if let Some(previous_indexes) = indexes.get(&folded) {
            for previous_index in previous_indexes {
                let previous = &mut entries[*previous_index].metadata;
                previous.conflict = EnvironmentConflict::from_flags(true, reserved);
            }
        }
        indexes.entry(folded).or_default().push(index);
        entries.push(ParsedEnvironmentEntry { metadata, value });
    }

    Ok(ParsedEnvironment {
        source: source.to_string(),
        revision: revision(bytes),
        entries,
    })
}

fn parse_value(raw: &str) -> Result<Zeroizing<String>, EnvironmentError> {
    let value = if raw.starts_with('"') {
        if raw.len() < 2 || !raw.ends_with('"') {
            return Err(EnvironmentError::UnclosedQuote);
        }
        let inner = &raw[1..raw.len() - 1];
        let mut out = Zeroizing::new(String::with_capacity(inner.len()));
        let mut chars = inner.chars();
        while let Some(character) = chars.next() {
            if character == '\\' {
                match chars.next() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(_) | None => return Err(EnvironmentError::InvalidEscape),
                }
            } else if character == '"' {
                // An unescaped quote is the closing delimiter; it must be the
                // final character of the value.
                return Err(EnvironmentError::UnclosedQuote);
            } else {
                out.push(character);
            }
        }
        out
    } else if raw.starts_with('\'') {
        if raw.len() < 2 || !raw.ends_with('\'') {
            return Err(EnvironmentError::UnclosedQuote);
        }
        let inner = &raw[1..raw.len() - 1];
        if inner.contains('\'') {
            return Err(EnvironmentError::UnclosedQuote);
        }
        Zeroizing::new(inner.to_string())
    } else {
        if raw.contains('"') || raw.contains('\'') {
            return Err(EnvironmentError::InvalidValue);
        }
        Zeroizing::new(raw.to_string())
    };

    if value.len() > MAX_ENV_VALUE_BYTES
        || value.chars().any(char::is_control)
        // The value is never passed through a shell.  Rejecting the two
        // command-substitution spellings still avoids accepting a source
        // file that looks executable to a later consumer.
        || value.contains("$(")
        || value.contains('`')
    {
        return Err(EnvironmentError::InvalidValue);
    }
    Ok(value)
}

fn validate_name(name: &str) -> Result<(), EnvironmentError> {
    if name.is_empty()
        || name.len() > MAX_ENV_NAME_BYTES
        || !name
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(EnvironmentError::InvalidName);
    }
    Ok(())
}

fn is_reserved_name(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "PATH"
            | "PATHEXT"
            | "COMSPEC"
            | "SHELL"
            | "WSLENV"
            | "LD_PRELOAD"
            | "DYLD_INSERT_LIBRARIES"
            | "PWD"
            | "OLDPWD"
            | "SHLVL"
            | "HOME"
            | "USER"
            | "USERNAME"
            | "SYSTEMROOT"
            | "WINDIR"
            | "TEMP"
            | "TMP"
            | "LOCALAPPDATA"
            | "APPDATA"
            | "PROGRAMDATA"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
            | "LANG"
            | "TERM"
    )
}

fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "PASSWORD",
        "PASSWD",
        "TOKEN",
        "SECRET",
        "APIKEY",
        "API_KEY",
        "ACCESSKEY",
        "ACCESS_KEY",
        "PRIVATEKEY",
        "PRIVATE_KEY",
        "CLIENTSECRET",
        "CLIENT_SECRET",
        "CREDENTIAL",
        "AUTH",
        "BEARER",
        "COOKIE",
        "SESSION",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

pub fn revision(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn preview(parsed: &ParsedEnvironment) -> ProjectEnvironmentPreview {
    let variables = parsed
        .entries
        .iter()
        .map(|entry| EnvironmentVariablePreview {
            name: entry.metadata.name.clone(),
            source: entry.metadata.source.clone(),
            conflict: entry.metadata.conflict,
            masked_value: if entry.metadata.secret_reference.is_some() {
                devbox_secrets::mask(entry.value.as_str(), 0)
            } else {
                devbox_secrets::mask(entry.value.as_str(), 2)
            },
            secret_reference: entry.metadata.secret_reference.clone(),
        })
        .collect();
    ProjectEnvironmentPreview {
        source: parsed.source.clone(),
        revision: parsed.revision.clone(),
        variables,
        has_conflicts: parsed.has_conflicts(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_and_export_values_without_expansion() {
        let parsed = parse_environment(
            ".env.local",
            br#"# comment
export APP_NAME="devbox"
URL=https://example.test/a=b
LITERAL='${NOT_EXPANDED}'
"#,
        )
        .unwrap();
        assert_eq!(parsed.entries().len(), 3);
        assert_eq!(parsed.entries()[0].value.as_str(), "devbox");
        assert_eq!(
            parsed.entries()[1].value.as_str(),
            "https://example.test/a=b"
        );
        assert_eq!(parsed.entries()[2].value.as_str(), "${NOT_EXPANDED}");
    }

    #[test]
    fn rejects_unescaped_quote_delimiters_and_quotes_in_unquoted_values() {
        for bytes in [
            b"VALUE=\"a\"b\"\n".as_slice(),
            b"VALUE='a'b'\n".as_slice(),
            b"VALUE=a\"b\n".as_slice(),
            b"VALUE=a'b\n".as_slice(),
        ] {
            assert!(
                matches!(
                    parse_environment(".env", bytes),
                    Err(EnvironmentError::UnclosedQuote | EnvironmentError::InvalidValue)
                ),
                "accepted malformed quote syntax: {bytes:?}"
            );
        }
        let escaped = parse_environment(".env", br#"VALUE="a\"b""#).unwrap();
        assert_eq!(escaped.entries()[0].value.as_str(), "a\"b");
    }

    #[test]
    fn duplicate_and_reserved_keys_are_metadata_and_blocking() {
        let parsed = parse_environment(".env", b"TOKEN=one\ntoken=two\nPATH=/unsafe\n").unwrap();
        assert_eq!(
            parsed.entries()[0].metadata.conflict,
            EnvironmentConflict::Duplicate
        );
        assert_eq!(
            parsed.entries()[1].metadata.conflict,
            EnvironmentConflict::Duplicate
        );
        assert_eq!(
            parsed.entries()[2].metadata.conflict,
            EnvironmentConflict::Reserved
        );
        assert!(parsed.has_conflicts());
    }

    #[test]
    fn every_launch_boundary_name_is_reserved_in_metadata() {
        let source = [
            "PATH",
            "PATHEXT",
            "COMSPEC",
            "SHELL",
            "WSLENV",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "PWD",
            "OLDPWD",
            "SHLVL",
            "SYSTEMROOT",
            "WINDIR",
            "TEMP",
            "TMP",
            "LOCALAPPDATA",
            "APPDATA",
            "PROGRAMDATA",
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "HOME",
            "USERNAME",
            "USER",
            "LANG",
            "TERM",
        ]
        .into_iter()
        .map(|name| format!("{name}=value\n"))
        .collect::<String>();
        let parsed = parse_environment(".env", source.as_bytes()).unwrap();
        assert!(parsed
            .entries()
            .iter()
            .all(|entry| entry.metadata.conflict == EnvironmentConflict::Reserved));
    }

    #[test]
    fn preview_never_contains_plaintext_or_debug_values() {
        let parsed = parse_environment(".env", b"API_TOKEN=top-secret\nNAME=devbox\n").unwrap();
        let preview = preview(&parsed);
        let json = serde_json::to_string(&preview).unwrap();
        assert!(!json.contains("top-secret"));
        assert!(json.contains("**"));
        assert!(!format!("{parsed:?}").contains("top-secret"));
        assert!(!format!("{:?}", parsed.entries()[0]).contains("top-secret"));
    }

    #[test]
    fn rejects_shell_syntax_unclosed_quotes_controls_and_bounds() {
        for (source, bytes, error) in [
            (
                "../.env",
                b"A=x".as_slice(),
                EnvironmentError::InvalidSource,
            ),
            (
                ".env",
                b"A=$(whoami)\n".as_slice(),
                EnvironmentError::InvalidValue,
            ),
            (
                ".env",
                b"A=\"unterminated\n".as_slice(),
                EnvironmentError::UnclosedQuote,
            ),
            (
                ".env",
                b"A=hello\nB".as_slice(),
                EnvironmentError::MalformedLine,
            ),
        ] {
            assert_eq!(parse_environment(source, bytes).unwrap_err(), error);
        }
        assert_eq!(
            parse_environment(
                ".env",
                format!("A={}\n", "x".repeat(MAX_ENV_VALUE_BYTES + 1)).as_bytes()
            )
            .unwrap_err(),
            EnvironmentError::LineTooLong
        );
        for source in [".env/escape", ".env.", ".env..local"] {
            assert_eq!(
                parse_environment(source, b"A=x").unwrap_err(),
                EnvironmentError::InvalidSource
            );
        }
        let too_many = (0..=MAX_ENV_VARIABLES)
            .map(|index| format!("KEY_{index}=x\n"))
            .collect::<String>();
        assert_eq!(
            parse_environment(".env", too_many.as_bytes()).unwrap_err(),
            EnvironmentError::TooManyVariables
        );
        assert_eq!(
            parse_environment(".env", &vec![b'#'; MAX_ENV_FILE_BYTES + 1]).unwrap_err(),
            EnvironmentError::FileTooLarge
        );
        let total = (0..20)
            .map(|index| format!("KEY_{index}={}\n", "x".repeat(7_000)))
            .collect::<String>();
        assert_eq!(
            parse_environment(".env", total.as_bytes()).unwrap_err(),
            EnvironmentError::TooMuchValueData
        );
    }

    #[test]
    fn config_validation_requires_source_revision_and_reference_integrity() {
        let parsed = parse_environment(".env", b"TOKEN=x\nNAME=y").unwrap();
        let config = parsed.config(true);
        validate_config(&config).unwrap();

        let mut invalid = config.clone();
        invalid.revision = "not-a-revision".into();
        assert_eq!(
            validate_config(&invalid),
            Err(EnvironmentError::InvalidMetadata)
        );

        let mut non_canonical_revision = parsed.config(true);
        non_canonical_revision.revision = "A".repeat(64);
        assert_eq!(
            validate_config(&non_canonical_revision),
            Err(EnvironmentError::InvalidMetadata)
        );

        let mut mismatched = config;
        mismatched.variables[0].secret_reference = Some(
            devbox_secrets::SecretReference::project_environment("OTHER"),
        );
        assert_eq!(
            validate_config(&mismatched),
            Err(EnvironmentError::InvalidMetadata)
        );

        let mut missing_reference = parsed.config(true);
        missing_reference.variables[0].secret_reference = None;
        assert_eq!(
            validate_config(&missing_reference),
            Err(EnvironmentError::InvalidMetadata)
        );

        let mut forged_conflict = parsed.config(true);
        forged_conflict.variables[1].conflict = EnvironmentConflict::Duplicate;
        assert_eq!(
            validate_config(&forged_conflict),
            Err(EnvironmentError::InvalidMetadata)
        );
    }

    #[test]
    fn parser_duplicate_metadata_round_trips_through_config_validation() {
        let parsed = parse_environment(".env", b"TOKEN=first\ntoken=second\nPATH=/tmp").unwrap();
        let config = parsed.config(false);
        validate_config(&config).unwrap();
    }
}
