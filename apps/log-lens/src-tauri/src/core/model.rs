use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub const MAX_SOURCES: usize = 16;
pub const MAX_RECORDS: usize = 100_000;
pub const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 16 * 1024;
pub const MAX_FIELD_BYTES: usize = 4 * 1024;
pub const MAX_FIELDS: usize = 256;
pub const MAX_FILTER_BYTES: usize = 512;
pub const MAX_EXPORT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PATTERN_BYTES: usize = 128;
pub const MAX_DISTRO_BYTES: usize = 128;
pub const MAX_PATH_BYTES: usize = 4 * 1024;
pub const MAX_CURSOR_OFFSET_BYTES: usize = 24;
pub const MAX_CURSOR_HASH_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Plain,
    Jsonl,
    Logfmt,
}

impl LogFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Jsonl => "jsonl",
            Self::Logfmt => "logfmt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim_matches(|character: char| !character.is_ascii_alphabetic());
        if value.eq_ignore_ascii_case("trace") {
            Some(Self::Trace)
        } else if value.eq_ignore_ascii_case("debug") {
            Some(Self::Debug)
        } else if value.eq_ignore_ascii_case("info") {
            Some(Self::Info)
        } else if value.eq_ignore_ascii_case("warn") || value.eq_ignore_ascii_case("warning") {
            Some(Self::Warn)
        } else if value.eq_ignore_ascii_case("error") {
            Some(Self::Error)
        } else if value.eq_ignore_ascii_case("fatal") {
            Some(Self::Fatal)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub source_id: String,
    pub sequence: u64,
    pub timestamp_millis: Option<i64>,
    pub level: Option<LogLevel>,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    pub format: LogFormat,
    #[serde(default)]
    pub truncated: bool,
}

impl LogRecord {
    pub fn estimated_bytes(&self) -> usize {
        self.source_id
            .len()
            .saturating_add(self.message.len())
            .saturating_add(
                self.fields
                    .iter()
                    .map(|(key, value)| key.len().saturating_add(value.len()))
                    .fold(0_usize, usize::saturating_add),
            )
            .saturating_add(64)
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.source_id.is_empty()
            || self.source_id.len() > 192
            || self.source_id.chars().any(char::is_control)
            || self.message.len() > MAX_LINE_BYTES
            || has_disallowed_control(self.message.as_str())
            || self.sequence > 9_007_199_254_740_991_u64
            || self
                .timestamp_millis
                .is_some_and(|value| value.unsigned_abs() > 9_007_199_254_740_991_u64)
            || self.fields.len() > MAX_FIELDS
            || self.fields.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > MAX_FIELD_BYTES
                    || key.chars().any(char::is_control)
                    || value.len() > MAX_FIELD_BYTES
                    || has_disallowed_control(value.as_str())
            })
        {
            return Err(CoreError::InvalidInput);
        }
        Ok(())
    }
}

fn has_disallowed_control(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilterSpec {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub level: Option<LogLevel>,
    #[serde(default)]
    pub start_at: Option<i64>,
    #[serde(default)]
    pub end_at: Option<i64>,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub field_value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerEngine {
    Docker,
    Podman,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SourceSpec {
    LocalFile {
        path: String,
    },
    Directory {
        path: String,
        pattern: String,
    },
    WslFile {
        distro: String,
        path: String,
    },
    WslJournal {
        distro: String,
        #[serde(default)]
        unit: Option<String>,
    },
    Run {
        source_id: String,
    },
    Container {
        engine: ContainerEngine,
        container_id: String,
    },
}

/// The receiver-side portion of the existing `log-source/v1` contract. It
/// carries identity only; no producer path, command, environment, or log
/// bytes cross the handoff boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogSourceRef {
    pub kind: String,
    pub source_id: String,
    pub run_id: String,
    pub stream: String,
}

impl LogSourceRef {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.kind != "log-source/v1"
            || !matches!(self.stream.as_str(), "stdout" | "stderr")
            || self.run_id.is_empty()
            || self.run_id.len() > 128
            || self
                .run_id
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
            || self.source_id != format!("run-manager:{}:{}", self.run_id, self.stream)
        {
            return Err(CoreError::InvalidSource);
        }
        validate_run_source_id(&self.source_id)
    }

    pub fn into_source(self) -> Result<SourceSpec, CoreError> {
        self.validate()?;
        Ok(SourceSpec::Run {
            source_id: self.source_id,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub source_id: String,
    pub kind: SourceKind,
    pub display_name: String,
    pub read_only: bool,
    pub handoff: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    LocalFile,
    Directory,
    WslFile,
    WslJournal,
    Run,
    Container,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedView {
    pub name: String,
    pub sources: Vec<SourceSpec>,
    pub filter: FilterSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadStatus {
    Initial,
    Advanced,
    Rotated,
    Truncated,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIdentity {
    pub device: Option<u64>,
    pub inode: Option<u64>,
    pub size: u64,
    pub modified_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCursor {
    pub identity: Option<FileIdentity>,
    /// Decimal text avoids JavaScript precision loss for a long-lived file.
    pub offset: String,
    /// A bounded hash of the bytes immediately before `offset`. It lets the
    /// reader distinguish an append from a truncate-and-regrow that reused
    /// the same inode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_hash: Option<String>,
}

impl FileCursor {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.offset.is_empty()
            || self.offset.len() > MAX_CURSOR_OFFSET_BYTES
            || self.offset.parse::<u64>().is_err()
            || self.anchor_hash.as_ref().is_some_and(|hash| {
                hash.len() != MAX_CURSOR_HASH_BYTES
                    || hash.bytes().any(|byte| !byte.is_ascii_hexdigit())
            })
        {
            return Err(CoreError::InvalidInput);
        }
        if let Some(identity) = &self.identity {
            if identity
                .modified_millis
                .is_some_and(|value| value > 9_007_199_254_740_991_u64)
            {
                return Err(CoreError::InvalidInput);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSnapshot {
    pub operation_id: String,
    pub generation: u64,
    pub source: SourceSummary,
    pub records: Vec<LogRecord>,
    pub next_cursor: Option<FileCursor>,
    pub status: ReadStatus,
    pub truncated: bool,
    pub dropped_records: usize,
    pub dropped_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidInput,
    InvalidSource,
    InvalidPath,
    InvalidPattern,
    InvalidFilter,
    UnsupportedSource,
    OperationCancelled,
    StaleOperation,
    Timeout,
    OutputLimit,
    Io,
    AdapterUnavailable,
    ExportTooLarge,
}

impl fmt::Display for CoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "log input is invalid",
            Self::InvalidSource => "log source is invalid",
            Self::InvalidPath => "log path is invalid",
            Self::InvalidPattern => "log pattern is invalid",
            Self::InvalidFilter => "log filter is invalid",
            Self::UnsupportedSource => "log source is not supported",
            Self::OperationCancelled => "log operation was cancelled",
            Self::StaleOperation => "log operation is stale",
            Self::Timeout => "log source timed out",
            Self::OutputLimit => "log source output exceeded the limit",
            Self::Io => "log source could not be read",
            Self::AdapterUnavailable => "log adapter is unavailable",
            Self::ExportTooLarge => "selected export is too large",
        })
    }
}

impl std::error::Error for CoreError {}

pub fn validate_text(value: &str, max_bytes: usize, allow_empty: bool) -> Result<(), CoreError> {
    if (!allow_empty && value.is_empty())
        || value.len() > max_bytes
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::InvalidInput);
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<(), CoreError> {
    validate_text(path, MAX_PATH_BYTES, false).map_err(|_| CoreError::InvalidPath)?;
    let trimmed = path.trim();
    if trimmed != path {
        return Err(CoreError::InvalidPath);
    }
    let bytes = trimmed.as_bytes();
    let drive_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if !(Path::new(trimmed).is_absolute()
        || trimmed.starts_with("\\\\")
        || trimmed.starts_with("//")
        || drive_path)
        || trimmed.starts_with("\\\\?\\")
        || trimmed.starts_with("//?/")
        || trimmed.starts_with("\\\\.\\")
        || trimmed.starts_with("//./")
    {
        return Err(CoreError::InvalidPath);
    }
    Ok(())
}

fn validate_wsl_path(path: &str) -> Result<(), CoreError> {
    validate_text(path, MAX_PATH_BYTES, false).map_err(|_| CoreError::InvalidPath)?;
    let trimmed = path.trim();
    if trimmed != path {
        return Err(CoreError::InvalidPath);
    }
    if !trimmed.starts_with('/')
        || trimmed
            .split('/')
            .skip(1)
            .any(|component| component.is_empty() || component == "." || component == "..")
        || trimmed.split('/').nth(1).is_none()
        || trimmed.contains("\0")
        || trimmed.chars().any(is_wsl_path_injection_char)
    {
        return Err(CoreError::InvalidPath);
    }
    Ok(())
}

fn is_wsl_path_injection_char(character: char) -> bool {
    matches!(
        character,
        ';' | '&'
            | '|'
            | '<'
            | '>'
            | '`'
            | '$'
            | '"'
            | '\''
            | '\\'
            | '('
            | ')'
            | '{'
            | '}'
            | '*'
            | '?'
            | '['
            | ']'
            | '!'
            | '~'
            | '#'
            | '%'
    )
}

fn validate_pattern(pattern: &str) -> Result<(), CoreError> {
    validate_text(pattern, MAX_PATTERN_BYTES, false).map_err(|_| CoreError::InvalidPattern)?;
    if pattern == "."
        || pattern == ".."
        || pattern.contains('/')
        || pattern.contains('\\')
        || pattern.contains("..")
        || pattern
            .chars()
            .any(|character| !character.is_ascii() && character.is_control())
    {
        return Err(CoreError::InvalidPattern);
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return Ok(());
    }
    Ok(())
}

fn validate_identifier(value: &str, max_bytes: usize) -> Result<(), CoreError> {
    validate_text(value, max_bytes, false)?;
    if value.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'@'))
    }) {
        return Err(CoreError::InvalidInput);
    }
    Ok(())
}

fn validate_run_source_id(source_id: &str) -> Result<(), CoreError> {
    validate_identifier(source_id, 192)?;
    let Some(rest) = source_id.strip_prefix("run-manager:") else {
        return Err(CoreError::InvalidSource);
    };
    let Some((run_id, stream)) = rest.rsplit_once(':') else {
        return Err(CoreError::InvalidSource);
    };
    if run_id.is_empty()
        || run_id
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        || !matches!(stream, "stdout" | "stderr")
    {
        return Err(CoreError::InvalidSource);
    }
    Ok(())
}

fn validate_container_id(container_id: &str) -> Result<(), CoreError> {
    validate_identifier(container_id, 128).map_err(|_| CoreError::InvalidSource)?;
    if !container_id
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
    {
        return Err(CoreError::InvalidSource);
    }
    Ok(())
}

impl SourceSpec {
    pub fn validate(&self) -> Result<(), CoreError> {
        match self {
            Self::LocalFile { path } => validate_path(path),
            Self::Directory { path, pattern } => {
                validate_path(path)?;
                validate_pattern(pattern)
            }
            Self::WslFile { distro, path } => {
                validate_text(distro, MAX_DISTRO_BYTES, false)
                    .map_err(|_| CoreError::InvalidSource)?;
                if distro.trim() != distro {
                    return Err(CoreError::InvalidSource);
                }
                wsl::distro::validate_distro_name(distro).map_err(|_| CoreError::InvalidSource)?;
                validate_wsl_path(path)
            }
            Self::WslJournal { distro, unit } => {
                validate_text(distro, MAX_DISTRO_BYTES, false)
                    .map_err(|_| CoreError::InvalidSource)?;
                if distro.trim() != distro {
                    return Err(CoreError::InvalidSource);
                }
                wsl::distro::validate_distro_name(distro).map_err(|_| CoreError::InvalidSource)?;
                if let Some(unit) = unit {
                    validate_identifier(unit, 128).map_err(|_| CoreError::InvalidSource)?;
                }
                Ok(())
            }
            Self::Run { source_id } => validate_run_source_id(source_id),
            Self::Container {
                engine: _,
                container_id,
            } => validate_container_id(container_id),
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            Self::LocalFile { .. } => SourceKind::LocalFile,
            Self::Directory { .. } => SourceKind::Directory,
            Self::WslFile { .. } => SourceKind::WslFile,
            Self::WslJournal { .. } => SourceKind::WslJournal,
            Self::Run { .. } => SourceKind::Run,
            Self::Container { .. } => SourceKind::Container,
        }
    }

    /// Stable opaque identifier.  It is intentionally not a path or a
    /// container name, so status/snapshot payloads do not echo source inputs.
    pub fn opaque_id(&self) -> String {
        let mut hash = 0xcbf29ce484222325_u64;
        let encoded = serde_json::to_vec(self).unwrap_or_default();
        for byte in encoded {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        format!("log-source:{:016x}", hash)
    }

    pub fn summary(&self) -> Result<SourceSummary, CoreError> {
        self.validate()?;
        let display_name = match self {
            Self::LocalFile { .. } => "Local file",
            Self::Directory { .. } => "Local directory",
            Self::WslFile { .. } => "WSL file",
            Self::WslJournal { .. } => "WSL journal",
            Self::Run { .. } => "Run Manager handoff",
            Self::Container { .. } => "Container logs",
        };
        Ok(SourceSummary {
            source_id: self.opaque_id(),
            kind: self.kind(),
            display_name: display_name.to_string(),
            read_only: true,
            // Every source accepted by the #366/#367 receiver is a fixed
            // handoff adapter. Keep this flag aligned with the source
            // contract instead of marking only the original Run variant.
            handoff: matches!(
                self,
                Self::Run { .. } | Self::WslFile { .. } | Self::WslJournal { .. }
            ),
        })
    }
}

/// Validate a source list before any member is opened. Two equal descriptors
/// would otherwise receive the same opaque source ID and sequence namespace,
/// making their rows indistinguishable in the merged view.
pub fn validate_source_list(sources: &[SourceSpec]) -> Result<(), CoreError> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(CoreError::InvalidSource);
    }
    for (index, source) in sources.iter().enumerate() {
        source.validate()?;
        if sources[..index].iter().any(|previous| previous == source) {
            return Err(CoreError::InvalidSource);
        }
    }
    Ok(())
}

impl FilterSpec {
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_text(&self.text, MAX_FILTER_BYTES, true).map_err(|_| CoreError::InvalidFilter)?;
        if let Some(source_id) = &self.source_id {
            validate_identifier(source_id, 192).map_err(|_| CoreError::InvalidFilter)?;
        }
        if let Some(field) = &self.field {
            validate_identifier(field, MAX_FIELD_BYTES).map_err(|_| CoreError::InvalidFilter)?;
        }
        if let Some(field_value) = &self.field_value {
            validate_text(field_value, MAX_FIELD_BYTES, true)
                .map_err(|_| CoreError::InvalidFilter)?;
        }
        if self
            .start_at
            .zip(self.end_at)
            .is_some_and(|(start, end)| start > end)
        {
            return Err(CoreError::InvalidFilter);
        }
        if self
            .start_at
            .into_iter()
            .chain(self.end_at)
            .any(|value| value.unsigned_abs() > 9_007_199_254_740_991_u64)
        {
            return Err(CoreError::InvalidFilter);
        }
        Ok(())
    }
}

impl SavedView {
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_text(&self.name, 128, false)?;
        validate_source_list(&self.sources)?;
        self.filter.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_source_ref_is_identity_only_and_strict() {
        let reference = LogSourceRef {
            kind: "log-source/v1".to_string(),
            source_id: "run-manager:run_1:stderr".to_string(),
            run_id: "run_1".to_string(),
            stream: "stderr".to_string(),
        };
        assert!(reference.validate().is_ok());
        assert!(serde_json::from_str::<LogSourceRef>(
            r#"{"kind":"log-source/v1","sourceId":"run-manager:run_1:stdout","runId":"run_1","stream":"stdout","absolutePath":"/secret"}"#
        )
        .is_err());
        let mut wrong = reference;
        wrong.source_id = "/secret/path".to_string();
        assert_eq!(wrong.validate(), Err(CoreError::InvalidSource));
    }

    #[test]
    fn wsl_distro_name_is_bounded_before_adapter_argv() {
        let source = SourceSpec::WslFile {
            distro: "d".repeat(MAX_DISTRO_BYTES + 1),
            path: "/var/log/app.log".to_string(),
        };
        assert_eq!(source.validate(), Err(CoreError::InvalidSource));
    }

    #[test]
    fn container_id_cannot_be_interpreted_as_an_option() {
        let source = SourceSpec::Container {
            engine: ContainerEngine::Docker,
            container_id: "--help".to_string(),
        };
        assert_eq!(source.validate(), Err(CoreError::InvalidSource));
    }

    #[test]
    fn local_device_namespace_paths_are_rejected() {
        for path in [
            r"\\.\pipe\devbox",
            r"//./pipe/devbox",
            r"\\?\C:\logs\app.log",
        ] {
            assert_eq!(
                SourceSpec::LocalFile {
                    path: path.to_string(),
                }
                .validate(),
                Err(CoreError::InvalidPath)
            );
        }
    }

    #[test]
    fn source_paths_and_distro_do_not_change_at_the_adapter_boundary() {
        assert_eq!(
            SourceSpec::LocalFile {
                path: " /var/log/app.log".to_string(),
            }
            .validate(),
            Err(CoreError::InvalidPath)
        );
        assert_eq!(
            SourceSpec::WslFile {
                distro: " Ubuntu ".to_string(),
                path: "/var/log/app.log".to_string(),
            }
            .validate(),
            Err(CoreError::InvalidSource)
        );
        assert_eq!(
            SourceSpec::WslFile {
                distro: "Ubuntu".to_string(),
                path: "/".to_string(),
            }
            .validate(),
            Err(CoreError::InvalidPath)
        );
        assert_eq!(
            SourceSpec::WslFile {
                distro: "Ubuntu".to_string(),
                path: "/./".to_string(),
            }
            .validate(),
            Err(CoreError::InvalidPath)
        );
        assert_eq!(
            SourceSpec::WslFile {
                distro: "Ubuntu".to_string(),
                path: "/var/log/app;touch".to_string(),
            }
            .validate(),
            Err(CoreError::InvalidPath)
        );
    }

    #[test]
    fn duplicate_source_descriptors_are_rejected_before_reading() {
        let source = SourceSpec::LocalFile {
            path: r"C:\logs\app.log".to_string(),
        };
        assert_eq!(
            validate_source_list(&[source.clone(), source]),
            Err(CoreError::InvalidSource)
        );
    }
}
