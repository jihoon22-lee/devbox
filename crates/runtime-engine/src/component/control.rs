//! Closed Runtime controls with durable at-most-once submission. A receipt is
//! committed before invoking the existing owner. An interrupted submission
//! requires state review; retry never guesses that no side effect occurred.
use crate::storage::{ControlReservation, DatabaseState, RuntimeControlReceipt};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::Manager;

use crate::api_control::Control;
fn replay(receipt: RuntimeControlReceipt) -> Result<Value, String> {
    match receipt.state.as_str() {
        "completed" => receipt
            .result
            .ok_or("runtime_control_recovery_required".into()),
        "failed" => Err("runtime_control_failed".into()),
        "pending" => Err("runtime_control_in_progress".into()),
        _ => Err("runtime_control_recovery_required".into()),
    }
}
pub(super) async fn execute(app: &tauri::AppHandle, args: Value) -> Result<Value, String> {
    execute_typed(
        app,
        serde_json::from_value(args).map_err(|_| "component_args_invalid")?,
    )
    .await
}
pub(crate) async fn execute_typed(app: &tauri::AppHandle, input: Control) -> Result<Value, String> {
    let method = input.action.method();
    let wire = serde_json::to_value(&input.action).map_err(|_| "component_args_invalid")?;
    let args = &wire["args"];
    let database = app
        .try_state::<Arc<DatabaseState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    let key = if method == "stop_workspace_task_operation" {
        "operationId"
    } else {
        "id"
    };
    let id = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or("component_args_invalid")?;
    let cached = database
        .runtime_control(&input.operation_id)
        .map_err(|_| "runtime_control_unavailable")?;
    let target_id = if let Some(receipt) = cached {
        receipt.target_id
    } else if method == "stop_workspace_task_operation" {
        database
            .get_workspace_task_operation(id)
            .map_err(|_| "runtime_control_unavailable")?
            .ok_or("runtime_control_unavailable")?
            .root_job_id
    } else {
        id.to_owned()
    };
    let fingerprint: String =
        Sha256::digest(serde_json::to_vec(args).map_err(|_| "component_args_invalid")?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    match database
        .reserve_runtime_control(
            &input.operation_id,
            method,
            &target_id,
            &fingerprint,
            crate::storage::current_epoch_millis(),
        )
        .map_err(|_| "runtime_control_unavailable")?
    {
        ControlReservation::Existing(receipt) => return replay(receipt),
        ControlReservation::New => {}
    }
    let result = input.action.execute(app).await;
    database
        .finish_runtime_control(&input.operation_id, result.as_ref().ok())
        .map_err(|_| "runtime_control_recovery_required")?;
    result.map_err(|_| "runtime_control_failed".into())
}
pub(crate) fn metadata(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Id {
        operation_id: String,
    }
    let database = app
        .try_state::<Arc<DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    match method {
        "list_runtime_controls" => {
            if args.as_object().is_none_or(|value| !value.is_empty()) {
                return Err("component_args_invalid".into());
            }
            Ok(json!(database
                .unresolved_runtime_controls()
                .map_err(|_| "runtime_control_unavailable")?))
        }
        "runtime_control_status" => {
            let input: Id = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            let receipt = database
                .runtime_control(&input.operation_id)
                .map_err(|_| "runtime_control_unavailable")?
                .ok_or("runtime_control_unavailable")?;
            replay(receipt)
        }
        "review_runtime_control" => {
            let input: Id = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            database
                .review_runtime_control(&input.operation_id)
                .map_err(|_| "runtime_control_owner_unsettled")?;
            Ok(Value::Null)
        }
        _ => Err("component_method_invalid".into()),
    }
}
