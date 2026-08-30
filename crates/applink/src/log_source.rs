use serde::{Deserialize, Serialize};

pub const LOG_SOURCE_HANDOFF_KIND: &str = "log-source/v1";
pub const LOG_SOURCE_TARGET_APP: &str = "log-lens";
pub const LOG_SOURCE_MAX_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_RUN_ID_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogSourceStream {
    Stdout,
    Stderr,
}

impl LogSourceStream {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// Identity-only reference to Run Manager-owned retained output. Producers
/// may route this reference, but only Run Manager remains the data owner.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunLogSourceRef {
    pub kind: String,
    pub source_id: String,
    pub run_id: String,
    pub stream: LogSourceStream,
}

pub fn run_log_source_payload(
    run_id: &str,
    stream: LogSourceStream,
) -> Result<serde_json::Value, String> {
    validate_run_id(run_id)?;
    let reference = RunLogSourceRef {
        kind: LOG_SOURCE_HANDOFF_KIND.into(),
        source_id: format!("run-manager:{run_id}:{}", stream.as_str()),
        run_id: run_id.into(),
        stream,
    };
    let payload =
        serde_json::to_value(reference).map_err(|_| "log source payload is invalid".to_string())?;
    validate_run_log_source_payload(&payload)?;
    Ok(payload)
}

pub fn validate_run_log_source_payload(
    payload: &serde_json::Value,
) -> Result<RunLogSourceRef, String> {
    let bytes =
        serde_json::to_vec(payload).map_err(|_| "log source payload is invalid".to_string())?;
    if bytes.len() > LOG_SOURCE_MAX_PAYLOAD_BYTES {
        return Err("log source payload is invalid".into());
    }
    let reference: RunLogSourceRef = serde_json::from_value(payload.clone())
        .map_err(|_| "log source payload is invalid".to_string())?;
    validate_run_id(&reference.run_id)?;
    if reference.kind != LOG_SOURCE_HANDOFF_KIND
        || reference.source_id
            != format!(
                "run-manager:{}:{}",
                reference.run_id,
                reference.stream.as_str()
            )
    {
        return Err("log source payload is invalid".into());
    }
    Ok(reference)
}

fn validate_run_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_RUN_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("log source payload is invalid".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_contains_only_run_identity_and_stream() {
        let payload = run_log_source_payload("run-1", LogSourceStream::Stdout).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            })
        );
        assert!(validate_run_log_source_payload(&payload).is_ok());
    }

    #[test]
    fn payload_rejects_unknown_or_sensitive_fields() {
        for field in ["path", "command", "environment", "log"] {
            let mut payload = run_log_source_payload("run-1", LogSourceStream::Stderr).unwrap();
            payload
                .as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!("private"));
            assert!(validate_run_log_source_payload(&payload).is_err());
        }
        assert!(run_log_source_payload("../private", LogSourceStream::Stdout).is_err());
    }
}
