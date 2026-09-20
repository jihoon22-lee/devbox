//! WSL Desktop's producer-side `log-source/v1` payloads.
//!
//! Only the two fixed Log Lens adapters are represented here.  The WSL path
//! uses the explicit `wslPath` field so the generic host-path validator in the
//! shared handoff store cannot mistake it for a Windows/local path.  The
//! receiver validates it again before constructing fixed adapter argv.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HANDOFF_KIND: &str = "log-source/v1";
pub const SOURCE_APP: &str = "wsl-desktop";
pub const TARGET_APP: &str = "log-lens";
pub const FILE_SOURCE_TYPE: &str = "wslFile";
pub const JOURNAL_SOURCE_TYPE: &str = "wslJournal";
pub const MAX_DISTRO_BYTES: usize = 128;
pub const MAX_PATH_BYTES: usize = 4 * 1024;
pub const MAX_UNIT_BYTES: usize = 128;
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WslFilePayload {
    pub source_type: String,
    pub distro: String,
    pub wsl_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WslJournalPayload {
    pub source_type: String,
    pub distro: String,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSourceError {
    InvalidSource,
    TooLarge,
}

fn valid_text(value: &str, max_bytes: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty())
        && value.len() <= max_bytes
        && !value.chars().any(char::is_control)
        && value.trim() == value
}

fn valid_distro(value: &str) -> bool {
    valid_text(value, MAX_DISTRO_BYTES, false)
        && devbox_wsl::distro::validate_distro_name(value).is_ok()
}

fn valid_wsl_path(value: &str) -> bool {
    valid_text(value, MAX_PATH_BYTES, false)
        && value.starts_with('/')
        && value
            .split('/')
            .skip(1)
            .all(|component| !component.is_empty() && component != "." && component != "..")
        && value.split('/').nth(1).is_some()
        && !value.chars().any(is_argv_injection_char)
}

fn is_argv_injection_char(character: char) -> bool {
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

fn valid_unit(value: &str) -> bool {
    valid_text(value, MAX_UNIT_BYTES, false)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'@')
        })
}

fn bounded_json<T: Serialize>(payload: &T) -> Result<Value, LogSourceError> {
    let bytes = serde_json::to_vec(payload).map_err(|_| LogSourceError::InvalidSource)?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(LogSourceError::TooLarge);
    }
    serde_json::from_slice(&bytes).map_err(|_| LogSourceError::InvalidSource)
}

pub fn file_payload(distro: &str, wsl_path: &str) -> Result<Value, LogSourceError> {
    if !valid_distro(distro) || !valid_wsl_path(wsl_path) {
        return Err(LogSourceError::InvalidSource);
    }
    let payload = bounded_json(&WslFilePayload {
        source_type: FILE_SOURCE_TYPE.to_string(),
        distro: distro.to_string(),
        wsl_path: wsl_path.to_string(),
    })?;
    validate_file_payload(&payload).map_err(|_| LogSourceError::InvalidSource)?;
    Ok(payload)
}

pub fn journal_payload(distro: &str, unit: Option<&str>) -> Result<Value, LogSourceError> {
    if !valid_distro(distro) || unit.is_some_and(|value| !valid_unit(value)) {
        return Err(LogSourceError::InvalidSource);
    }
    let payload = bounded_json(&WslJournalPayload {
        source_type: JOURNAL_SOURCE_TYPE.to_string(),
        distro: distro.to_string(),
        unit: unit.map(str::to_string),
    })?;
    validate_journal_payload(&payload).map_err(|_| LogSourceError::InvalidSource)?;
    Ok(payload)
}

pub fn validate_file_payload(payload: &Value) -> Result<WslFilePayload, LogSourceError> {
    let parsed: WslFilePayload =
        serde_json::from_value(payload.clone()).map_err(|_| LogSourceError::InvalidSource)?;
    if parsed.source_type != FILE_SOURCE_TYPE
        || !valid_distro(&parsed.distro)
        || !valid_wsl_path(&parsed.wsl_path)
    {
        return Err(LogSourceError::InvalidSource);
    }
    let encoded = serde_json::to_vec(&parsed).map_err(|_| LogSourceError::InvalidSource)?;
    if encoded.len() > MAX_PAYLOAD_BYTES {
        return Err(LogSourceError::TooLarge);
    }
    Ok(parsed)
}

pub fn validate_journal_payload(payload: &Value) -> Result<WslJournalPayload, LogSourceError> {
    let parsed: WslJournalPayload =
        serde_json::from_value(payload.clone()).map_err(|_| LogSourceError::InvalidSource)?;
    if parsed.source_type != JOURNAL_SOURCE_TYPE
        || !valid_distro(&parsed.distro)
        || parsed
            .unit
            .as_deref()
            .is_some_and(|value| !valid_unit(value))
    {
        return Err(LogSourceError::InvalidSource);
    }
    let encoded = serde_json::to_vec(&parsed).map_err(|_| LogSourceError::InvalidSource)?;
    if encoded.len() > MAX_PAYLOAD_BYTES {
        return Err(LogSourceError::TooLarge);
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_payload_is_strict_and_uses_wsl_path_namespace() {
        let payload = file_payload("Ubuntu", "/var/log/app.log").expect("payload");
        assert_eq!(
            payload,
            serde_json::json!({
                "sourceType": "wslFile",
                "distro": "Ubuntu",
                "wslPath": "/var/log/app.log"
            })
        );
        assert!(validate_file_payload(&payload).is_ok());
    }

    #[test]
    fn traversal_root_and_argv_injection_are_rejected() {
        for path in [
            "relative.log",
            "/var/../etc/passwd",
            "/",
            "/./",
            "//var/log/app.log",
            "/var/log/app;touch",
            "/var/log/app\\name",
        ] {
            assert_eq!(
                file_payload("Ubuntu", path),
                Err(LogSourceError::InvalidSource)
            );
        }
        assert_eq!(
            journal_payload("Ubuntu;touch", None),
            Err(LogSourceError::InvalidSource)
        );
        assert_eq!(
            journal_payload("Ubuntu", Some("--unit=evil")),
            Err(LogSourceError::InvalidSource)
        );
    }

    #[test]
    fn unknown_fields_and_oversized_paths_fail_closed() {
        let unknown = serde_json::json!({
            "sourceType": "wslFile",
            "distro": "Ubuntu",
            "wslPath": "/var/log/app.log",
            "command": "cat /secret"
        });
        assert_eq!(
            validate_file_payload(&unknown),
            Err(LogSourceError::InvalidSource)
        );
        assert_eq!(
            file_payload("Ubuntu", &format!("/{}", "x".repeat(MAX_PATH_BYTES))),
            Err(LogSourceError::InvalidSource)
        );
    }

    #[test]
    fn journal_payload_keeps_unit_optional_without_log_bytes() {
        let payload = journal_payload("Ubuntu", Some("sshd.service")).expect("payload");
        assert_eq!(payload["sourceType"], "wslJournal");
        assert_eq!(payload["unit"], "sshd.service");
        assert!(!serde_json::to_string(&payload).unwrap().contains("log"));
    }
}
