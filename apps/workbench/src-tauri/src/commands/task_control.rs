//! Explicit typed Workbench -> Run Manager task-control dispatch.

use devbox_applink::{
    CreateHandoff, HandoffError, HandoffStore, OpenRequest, TaskControlAction, TaskControlRequest,
    TASK_CONTROL_HANDOFF_KIND, TASK_CONTROL_SCHEMA_VERSION, TASK_CONTROL_SOURCE_APP,
    TASK_CONTROL_TARGET_APP,
};
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

fn handoff_store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn dispatch_workspace_task_control(
    task_id: String,
    action: TaskControlAction,
    expected_revision: String,
) -> Result<TaskControlDispatch, String> {
    // Treat the renderer selection as a hint. Re-read the strict native
    // snapshot before publishing so an injected renderer value cannot become
    // durable handoff/receipt metadata, even though Run Manager also verifies
    // the request independently.
    let controls = list_workspace_task_controls()?;
    authorize_dispatch(&controls, &task_id, action, &expected_revision).map_err(str::to_owned)?;
    let request = TaskControlRequest {
        schema_version: TASK_CONTROL_SCHEMA_VERSION,
        request_id: uuid::Uuid::new_v4().simple().to_string(),
        task_id,
        action,
        expected_revision,
    };
    let payload = request.to_payload().map_err(str::to_owned)?;
    let now = now_ms();
    if now == 0 {
        return Err("task-control-unavailable".to_owned());
    }
    let store = handoff_store();
    let publication = store
        .create_with_publication(
            CreateHandoff {
                kind: TASK_CONTROL_HANDOFF_KIND.to_owned(),
                source_app: TASK_CONTROL_SOURCE_APP.to_owned(),
                target_app: Some(TASK_CONTROL_TARGET_APP.to_owned()),
                payload,
            },
            now,
        )
        .map_err(|_| "task-control-unavailable".to_owned())?;
    let open = OpenRequest {
        target: publication.descriptor.clone().into(),
        from: Some(TASK_CONTROL_SOURCE_APP.to_owned()),
    };
    if devbox_launch::launch_open(TASK_CONTROL_TARGET_APP, &open).is_err() {
        match store.remove_pending(&publication) {
            Ok(()) | Err(HandoffError::Missing) => {}
            Err(_) => return Err("task-control-cleanup-failed".to_owned()),
        }
        return Err("task-control-run-manager-unavailable".to_owned());
    }
    Ok(TaskControlDispatch {
        request_id: request.request_id,
        handoff_id: publication.descriptor.id,
    })
}

fn authorize_dispatch(
    controls: &[WorkspaceTaskControlItem],
    task_id: &str,
    action: TaskControlAction,
    expected_revision: &str,
) -> Result<(), &'static str> {
    let task = controls
        .iter()
        .find(|task| task.id == task_id)
        .ok_or("task-control-task-not-found")?;
    if task.revision != expected_revision {
        return Err("task-control-source-changed");
    }
    match action {
        TaskControlAction::Start if task.operation_active => Err("workspace-task-operation-active"),
        TaskControlAction::Start if !task.available => Err("workspace-task-unavailable"),
        TaskControlAction::Start if !task.trusted => Err("workspace-task-source-untrusted"),
        TaskControlAction::Start if task.task_kind == "shell" && !task.shell_trusted => {
            Err("workspace-task-shell-untrusted")
        }
        TaskControlAction::Stop if !task.operation_active => {
            Err("task-control-operation-not-active")
        }
        TaskControlAction::Start | TaskControlAction::Stop => Ok(()),
    }
}

#[tauri::command]
pub fn list_workspace_task_controls() -> Result<Vec<WorkspaceTaskControlItem>, String> {
    let envelope = devbox_integration::read_named_view_snapshot_in(
        &devbox_integration::integration_root(),
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
        &devbox_integration::integration_root(),
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

    #[test]
    fn dispatch_authorization_rechecks_the_exact_native_snapshot_state() {
        let task = WorkspaceTaskControlItem {
            id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            label: "Build".to_owned(),
            revision: "a".repeat(64),
            task_kind: "process".to_owned(),
            trusted: true,
            shell_trusted: false,
            available: true,
            has_dependencies: false,
            operation_active: false,
        };
        assert_eq!(
            authorize_dispatch(
                std::slice::from_ref(&task),
                &task.id,
                TaskControlAction::Start,
                &task.revision,
            ),
            Ok(())
        );
        assert_eq!(
            authorize_dispatch(
                std::slice::from_ref(&task),
                "arbitrary-renderer-value",
                TaskControlAction::Start,
                &task.revision,
            ),
            Err("task-control-task-not-found")
        );
        assert_eq!(
            authorize_dispatch(
                std::slice::from_ref(&task),
                &task.id,
                TaskControlAction::Start,
                &"b".repeat(64),
            ),
            Err("task-control-source-changed")
        );
        assert_eq!(
            authorize_dispatch(
                std::slice::from_ref(&task),
                &task.id,
                TaskControlAction::Stop,
                &task.revision,
            ),
            Err("task-control-operation-not-active")
        );
    }
}
