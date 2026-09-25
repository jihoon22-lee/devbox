//! Explicit typed Workbench -> Run Manager task-control dispatch.

use devbox_applink::{TaskControlAction, TaskControlRequest, TASK_CONTROL_SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const RUN_MANAGER_SCHEMA_VERSION: u32 = 1;
const WORKSPACE_TASKS_VIEW_KIND: &str = "workspace-tasks";
const TASK_CONTROL_RECEIPTS_VIEW_KIND: &str = "task-control-receipts";
const MAX_WORKSPACE_TASKS: usize = 128;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskControlItem {
    pub id: String,
    pub label: String,
    pub revision: String,
    pub task_kind: String,
    pub trusted: bool,
    pub shell_trusted: bool,
    pub available: bool,
    pub has_dependencies: bool,
    pub operation_active: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskControlReceipt {
    pub schema_version: u32,
    pub request_id: String,
    pub task_id: String,
    pub action: TaskControlAction,
    pub status: String,
    pub operation_id: Option<String>,
    pub failure_code: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskControlDispatch {
    pub request_id: String,
    pub handoff_id: String,
}

#[tauri::command]
pub fn dispatch_workspace_task_control(
    task_id: String,
    action: TaskControlAction,
    expected_revision: String,
) -> Result<TaskControlDispatch, String> {
    let _ = (task_id, action, expected_revision);
    Err("task-control-run-manager-unavailable".into())
}

#[tauri::command]
pub fn list_workspace_task_controls() -> Result<Vec<WorkspaceTaskControlItem>, String> {
    let envelope = devbox_integration::read_named_view_snapshot_in(
        &crate::component::integration_root(),
        "run-manager",
        RUN_MANAGER_SCHEMA_VERSION,
        WORKSPACE_TASKS_VIEW_KIND,
    )
    .map_err(|_| "task-control-snapshot-unavailable".to_owned())?
    .ok_or_else(|| "task-control-snapshot-missing".to_owned())?;
    let views = envelope
        .views()
        .map_err(|_| "task-control-snapshot-invalid".to_owned())?;
    let view = views
        .get(WORKSPACE_TASKS_VIEW_KIND)
        .ok_or_else(|| "task-control-snapshot-invalid".to_owned())?;
    if view.schema_version != 1 || view.entries.len() > MAX_WORKSPACE_TASKS {
        return Err("task-control-snapshot-invalid".to_owned());
    }
    let mut ids = BTreeSet::new();
    let mut tasks = Vec::with_capacity(view.entries.len());
    for value in &view.entries {
        let task: WorkspaceTaskControlItem = serde_json::from_value(value.clone())
            .map_err(|_| "task-control-snapshot-invalid".to_owned())?;
        let probe = TaskControlRequest {
            schema_version: TASK_CONTROL_SCHEMA_VERSION,
            request_id: "a".repeat(32),
            task_id: task.id.clone(),
            action: TaskControlAction::Start,
            expected_revision: task.revision.clone(),
        };
        if probe.validate().is_err()
            || !ids.insert(task.id.clone())
            || !matches!(task.task_kind.as_str(), "process" | "shell")
            || task.label.is_empty()
            || task.label.len() > 256
            || task.label.chars().any(char::is_control)
        {
            return Err("task-control-snapshot-invalid".to_owned());
        }
        tasks.push(task);
    }
    tasks.sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
    Ok(tasks)
}

#[tauri::command]
pub fn get_workspace_task_control_receipt(
    request_id: String,
) -> Result<Option<TaskControlReceipt>, String> {
    let request_probe = TaskControlRequest {
        schema_version: TASK_CONTROL_SCHEMA_VERSION,
        request_id: request_id.clone(),
        task_id: "probe".to_owned(),
        action: TaskControlAction::Start,
        expected_revision: "a".repeat(64),
    };
    request_probe
        .validate()
        .map_err(|_| "task-control-request-invalid".to_owned())?;
    let Some(envelope) = devbox_integration::read_named_view_snapshot_in(
        &crate::component::integration_root(),
        "run-manager",
        RUN_MANAGER_SCHEMA_VERSION,
        TASK_CONTROL_RECEIPTS_VIEW_KIND,
    )
    .map_err(|_| "task-control-receipt-unavailable".to_owned())?
    else {
        return Ok(None);
    };
    let views = envelope
        .views()
        .map_err(|_| "task-control-receipt-invalid".to_owned())?;
    let view = views
        .get(TASK_CONTROL_RECEIPTS_VIEW_KIND)
        .ok_or_else(|| "task-control-receipt-invalid".to_owned())?;
    if view.schema_version != 1 || view.entries.len() > 100 {
        return Err("task-control-receipt-invalid".to_owned());
    }
    let mut found = None;
    let mut ids = BTreeSet::new();
    for value in &view.entries {
        let receipt: TaskControlReceipt = serde_json::from_value(value.clone())
            .map_err(|_| "task-control-receipt-invalid".to_owned())?;
        if !valid_receipt(&receipt) || !ids.insert(receipt.request_id.clone()) {
            return Err("task-control-receipt-invalid".to_owned());
        }
        if receipt.request_id == request_id {
            found = Some(receipt);
        }
    }
    Ok(found)
}

fn valid_receipt(receipt: &TaskControlReceipt) -> bool {
    let request_probe = TaskControlRequest {
        schema_version: receipt.schema_version,
        request_id: receipt.request_id.clone(),
        task_id: receipt.task_id.clone(),
        action: receipt.action,
        // Receipts deliberately do not disclose the expected source revision.
        // A fixed valid digest lets the shared contract validate every other
        // request field without reconstructing that private input.
        expected_revision: "a".repeat(64),
    };
    if request_probe.validate().is_err()
        || receipt.created_at <= 0
        || receipt.updated_at < receipt.created_at
    {
        return false;
    }

    let operation_valid = receipt.operation_id.as_deref().is_some_and(|value| {
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
    });
    let failure_valid = receipt.failure_code.as_deref().is_some_and(|value| {
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });

    match receipt.status.as_str() {
        "accepted" => receipt.operation_id.is_none() && receipt.failure_code.is_none(),
        "rejected" | "failed" => receipt.operation_id.is_none() && failure_valid,
        "started" | "stopped" => operation_valid && receipt.failure_code.is_none(),
        _ => false,
    }
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_dispatch_workspace_task_control(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        task_id: String,
        action: TaskControlAction,
        expected_revision: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value =
        dispatch_workspace_task_control(input.task_id, input.action, input.expected_revision)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_list_workspace_task_controls(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = list_workspace_task_controls()?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_get_workspace_task_control_receipt(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        request_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = get_workspace_task_control_receipt(input.request_id)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_shape_has_only_opaque_correlators() {
        let dispatch = TaskControlDispatch {
            request_id: "a".repeat(32),
            handoff_id: "b".repeat(32),
        };
        let value = serde_json::to_value(dispatch).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["handoffId", "requestId"]
        );
    }

    #[test]
    fn receipt_validation_rejects_untrusted_snapshot_fields() {
        let mut receipt = TaskControlReceipt {
            schema_version: TASK_CONTROL_SCHEMA_VERSION,
            request_id: "a".repeat(32),
            task_id: "task:build".to_owned(),
            action: TaskControlAction::Start,
            status: "started".to_owned(),
            operation_id: Some("550e8400-e29b-41d4-a716-446655440000".to_owned()),
            failure_code: None,
            created_at: 1,
            updated_at: 2,
        };
        assert!(valid_receipt(&receipt));

        receipt.operation_id = Some("../operation".to_owned());
        assert!(!valid_receipt(&receipt));
        receipt.operation_id = None;
        receipt.status = "failed".to_owned();
        receipt.failure_code = Some("FAILED_WITH_RAW_TEXT".to_owned());
        assert!(!valid_receipt(&receipt));
        receipt.failure_code = Some("task-control-interrupted".to_owned());
        assert!(valid_receipt(&receipt));
        receipt.updated_at = 0;
        assert!(!valid_receipt(&receipt));
    }
}
