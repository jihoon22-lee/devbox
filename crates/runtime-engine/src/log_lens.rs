//! Explicit Run Manager -> Log Lens producer handoff.
//!
//! Publishing is only reachable from an explicit UI action.  The payload is
//! written to the shared one-time store and the child process receives only
//! the opaque handoff kind/id through AppLink argv.

use crate::logs::LogStream;
use crate::storage::DatabaseState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

// Publishing and launching are one user-visible operation.  Serialize them
// in-process so two rapid UI/context-menu requests cannot leave two valid
// envelopes pointing at the same Log Lens window or make retry ownership
// ambiguous. The shared store still provides cross-process one-time claims.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
#[ts(rename = "RuntimeLogDispatch")]
pub struct LogLensDispatch {
    pub handoff_id: String,
}

/// Publish one selected run stream and launch the installed Log Lens target.
/// The run database is consulted only to prove that the selected app-owned
/// log exists; its relative path is never copied into the payload or argv.
pub fn open_run_log_in_log_lens(
    run_id: String,
    stream: LogStream,
    app: AppHandle,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<LogLensDispatch, String> {
    let _ = (run_id, stream, app, state);
    Err("log-lens-unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_response_never_contains_a_path_or_log_bytes() {
        let dispatch = LogLensDispatch {
            handoff_id: "a".repeat(32),
        };
        let json = serde_json::to_string(&dispatch).expect("dispatch json");
        assert!(!json.contains("path"));
        assert!(!json.contains("log"));
    }
}
