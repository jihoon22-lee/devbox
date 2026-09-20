//! Closed Runtime controls with durable at-most-once submission. A receipt is
//! committed before invoking the existing owner. An interrupted submission
//! requires state review; retry never guesses that no side effect occurred.
use crate::storage::{ControlReservation, DatabaseState, RuntimeControlReceipt};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tauri::Manager;

use crate::core::runtime_controls::legacy_control_method;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Control {
    operation_id: String,
    method: String,
    args: Value,
}
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
    let input: Control = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    if !legacy_control_method(&input.method) || !input.args.is_object() {
        return Err("component_args_invalid".into());
    }
    let database = app
        .try_state::<Arc<DatabaseState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    let key = if input.method == "stop_workspace_task_operation" {
        "operationId"
    } else {
        "id"
    };
    let id = input
        .args
        .get(key)
        .and_then(Value::as_str)
        .ok_or("component_args_invalid")?;
    let cached = database
        .runtime_control(&input.operation_id)
        .map_err(|_| "runtime_control_unavailable")?;
    let target_id = if let Some(receipt) = cached {
        receipt.target_id
    } else if input.method == "stop_workspace_task_operation" {
        database
            .get_workspace_task_operation(id)
            .map_err(|_| "runtime_control_unavailable")?
            .ok_or("runtime_control_unavailable")?
            .root_job_id
    } else {
        id.to_owned()
    };
    let fingerprint: String =
        Sha256::digest(serde_json::to_vec(&input.args).map_err(|_| "component_args_invalid")?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    match database
        .reserve_runtime_control(
            &input.operation_id,
            &input.method,
            &target_id,
            &fingerprint,
            crate::storage::current_epoch_millis(),
        )
        .map_err(|_| "runtime_control_unavailable")?
    {
        ControlReservation::Existing(receipt) => return replay(receipt),
        ControlReservation::New => {}
    }
    let result = match input.method.as_str() {
        "run_job_now" => crate::commands::__component_run_job_now(app, input.args).await,
        "stop_active_run" => crate::commands::__component_stop_active_run(app, input.args).await,
        "start_service" => crate::commands::__component_start_service(app, input.args).await,
        "stop_service" => crate::commands::__component_stop_service(app, input.args).await,
        "restart_service" => crate::commands::__component_restart_service(app, input.args).await,
        "run_workspace_task_operation" => {
            crate::commands::__component_run_workspace_task_operation(app, input.args).await
        }
        "stop_workspace_task_operation" => {
            crate::commands::__component_stop_workspace_task_operation(app, input.args).await
        }
        _ => unreachable!("closed control method checked before reservation"),
    };
    database
        .finish_runtime_control(&input.operation_id, result.as_ref().ok())
        .map_err(|_| "runtime_control_recovery_required")?;
    result.map_err(|_| "runtime_control_failed".into())
}
pub(super) fn metadata(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
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
