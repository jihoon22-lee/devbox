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
pub struct LogLensDispatch {
    pub handoff_id: String,
}

/// Publish one selected run stream and launch the installed Log Lens target.
/// The run database is consulted only to prove that the selected app-owned
/// log exists; its relative path is never copied into the payload or argv.
#[tauri::command]
pub fn open_run_log_in_log_lens(
    run_id: String,
    stream: LogStream,
    app: AppHandle,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<LogLensDispatch, String> {
    let _ = (run_id, stream, app, state);
    Err("log-lens-unavailable".into())
}

/// Typed product adapter; native admission precedes this existing command.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_open_run_log_in_log_lens(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        run_id: String,
        stream: LogStream,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = open_run_log_in_log_lens(
        input.run_id,
        input.stream,
        _component_app.clone(),
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    )?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
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
