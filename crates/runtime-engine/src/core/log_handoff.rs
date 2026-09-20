//! Run Manager's producer-side `log-source/v1` contract.
//!
//! The existing log-search source reference is deliberately reused as the
//! handoff payload.  It contains only an app-owned run identity and stream;
//! the retained log directory, command, environment, and log bytes stay in
//! Run Manager.  The one-time envelope and its opaque id are owned by the
//! command layer in `log_lens.rs`.

use super::log_search::{source_ref, validate_source_ref, LogSearchError, LogSourceRef};
use crate::logs::LogStream;
use serde_json::Value;

pub const HANDOFF_KIND: &str = "log-source/v1";
pub const SOURCE_APP: &str = "run-manager";
pub const TARGET_APP: &str = "log-lens";
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

/// Build the strict, identity-only payload that is placed in the shared
/// one-time handoff store.  `source_ref` validates the run id and generated
/// source id before any JSON is created.
pub fn payload_for_run(run_id: &str, stream: LogStream) -> Result<Value, LogSearchError> {
    let reference = source_ref(run_id, stream)?;
    let payload = serde_json::to_value(reference).map_err(|_| LogSearchError::InvalidSource)?;
    devbox_applink::validate_run_log_source_payload(&payload)
        .map_err(|_| LogSearchError::InvalidSource)?;
    validate_payload(&payload)?;
    Ok(payload)
}

/// Re-validate a payload at the producer boundary.  This is intentionally a
/// second check before publishing so future callers cannot accidentally add a
/// path, command, credential, or raw log field to this contract.
pub fn validate_payload(payload: &Value) -> Result<LogSourceRef, LogSearchError> {
    let bytes = serde_json::to_vec(payload).map_err(|_| LogSearchError::InvalidSource)?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(LogSearchError::InvalidSource);
    }
    devbox_applink::validate_run_log_source_payload(payload)
        .map_err(|_| LogSearchError::InvalidSource)?;
    let reference: LogSourceRef =
        serde_json::from_value(payload.clone()).map_err(|_| LogSearchError::InvalidSource)?;
    if reference.kind != HANDOFF_KIND {
        return Err(LogSearchError::InvalidSource);
    }
    validate_source_ref(&reference)?;
    Ok(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_is_the_existing_identity_only_reference() {
        let payload = payload_for_run("run-1", LogStream::Stdout).expect("payload");
        assert_eq!(
            payload,
            serde_json::json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            })
        );
        assert!(validate_payload(&payload).is_ok());
    }

    #[test]
    fn unknown_path_and_raw_log_fields_are_rejected() {
        let payload = serde_json::json!({
            "kind": "log-source/v1",
            "sourceId": "run-manager:run-1:stdout",
            "runId": "run-1",
            "stream": "stdout",
            "absolutePath": "/secret/run.log",
            "log": "raw output"
        });
        assert_eq!(
            validate_payload(&payload),
            Err(LogSearchError::InvalidSource)
        );
    }

    #[test]
    fn generated_source_ids_never_accept_path_or_command_input() {
        assert_eq!(
            payload_for_run("../outside", LogStream::Stdout),
            Err(LogSearchError::InvalidSource)
        );
        assert_eq!(
            payload_for_run("run-1", LogStream::Stdout).and_then(|_payload| validate_payload(
                &serde_json::json!({
                    "kind": HANDOFF_KIND,
                    "sourceId": "run-manager:run-1:stdout",
                    "runId": "run-1",
                    "stream": "stdout",
                    "command": "cat /secret"
                })
            )),
            Err(LogSearchError::InvalidSource)
        );
    }
}
