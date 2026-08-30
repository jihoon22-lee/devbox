//! Run Manager consumer for explicit Workbench task-control handoffs.

use crate::core::workspace_task_control::{
    WorkspaceTaskControlPreview, WorkspaceTaskControlReceipt, WorkspaceTaskControlReceiptStatus,
};
use crate::core::workspace_tasks::verify_workspace_task_execution;
use crate::lifecycle::RuntimeState;
use crate::storage::{current_epoch_millis, DatabaseState};
use devbox_applink::{
    HandoffClaim, HandoffStore, TaskControlAction, TaskControlRequest, TASK_CONTROL_HANDOFF_KIND,
    TASK_CONTROL_TARGET_APP,
};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::State;

struct ClaimedTaskControl {
    claim: HandoffClaim,
    request: TaskControlRequest,
}

pub struct PendingTaskControl(Mutex<Option<ClaimedTaskControl>>);

impl PendingTaskControl {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn slot(&self) -> MutexGuard<'_, Option<ClaimedTaskControl>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
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

fn valid_handoff_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_workspace_task_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
}

fn restore_claim(claim: &HandoffClaim) {
    let _ = handoff_store().restore(claim, TASK_CONTROL_TARGET_APP, now_ms());
}

fn finalize_rejected_claim(
    claim: &HandoffClaim,
    request: &TaskControlRequest,
    database: &DatabaseState,
    code: &'static str,
) -> Result<WorkspaceTaskControlReceipt, String> {
    if database
        .create_workspace_task_control_receipt_at(
            request,
            WorkspaceTaskControlReceiptStatus::Accepted,
            None,
            current_epoch_millis(),
        )
        .is_err()
    {
        restore_claim(claim);
        return Err("task-control-receipt-storage".to_owned());
    }
    if handoff_store()
        .ack(claim, TASK_CONTROL_TARGET_APP, now_ms())
        .is_err()
    {
        let _ = database.finish_workspace_task_control_receipt_at(
            &request.request_id,
            WorkspaceTaskControlReceiptStatus::Failed,
            None,
            Some("task-control-claim-failed"),
            current_epoch_millis(),
        );
        let _ = crate::integration::write_task_control_receipts(database);
        restore_claim(claim);
        return Err("task-control-claim-failed".to_owned());
    }
    let receipt = match database.finish_workspace_task_control_receipt_at(
        &request.request_id,
        WorkspaceTaskControlReceiptStatus::Rejected,
        None,
        Some(code),
        current_epoch_millis(),
    ) {
        Ok(receipt) => receipt,
        Err(_) => {
            let _ = database.finish_workspace_task_control_receipt_at(
                &request.request_id,
                WorkspaceTaskControlReceiptStatus::Failed,
                None,
                Some("task-control-receipt-storage"),
                current_epoch_millis(),
            );
            let _ = crate::integration::write_task_control_receipts(database);
            return Err("task-control-receipt-storage".to_owned());
        }
    };
    let _ = crate::integration::write_task_control_receipts(database);
    Ok(receipt)
}

#[tauri::command]
pub fn preview_workspace_task_control(
    handoff_id: String,
    pending: State<'_, PendingTaskControl>,
    database: State<'_, Arc<DatabaseState>>,
) -> Result<WorkspaceTaskControlPreview, String> {
    if !valid_handoff_id(&handoff_id) {
        return Err("task-control-invalid".to_owned());
    }
    let mut slot = pending.slot();
    if slot.is_some() {
        return Err("task-control-busy".to_owned());
    }
    let claim = handoff_store()
        .claim(
            &handoff_id,
            TASK_CONTROL_HANDOFF_KIND,
            TASK_CONTROL_TARGET_APP,
            now_ms(),
        )
        .map_err(|_| "task-control-unavailable".to_owned())?;
    let request = match TaskControlRequest::from_claim(&claim) {
        Ok(request) => request,
        Err(code) => {
            restore_claim(&claim);
            return Err(code.to_owned());
        }
    };
    // Workspace-imported jobs use canonical UUID identities. Reject a generic
    // but attacker-chosen task token before it can be persisted and republished
    // in a receipt snapshot.
    if !valid_workspace_task_id(&request.task_id) {
        if handoff_store()
            .ack(&claim, TASK_CONTROL_TARGET_APP, now_ms())
            .is_err()
        {
            restore_claim(&claim);
            return Err("task-control-claim-failed".to_owned());
        }
        return Err("task-control-invalid".to_owned());
    }
    match database.get_workspace_task_control_receipt(&request.request_id) {
        Ok(Some(_)) => {
            if handoff_store()
                .ack(&claim, TASK_CONTROL_TARGET_APP, now_ms())
                .is_err()
            {
                restore_claim(&claim);
                return Err("task-control-claim-failed".to_owned());
            }
            let _ = crate::integration::write_task_control_receipts(database.inner().as_ref());
            return Err("task-control-request-replayed".to_owned());
        }
        Ok(None) => {}
        Err(_) => {
            restore_claim(&claim);
            return Err("task-control-receipt-storage".to_owned());
        }
    }
    let execution = match database.get_workspace_task_execution(&request.task_id) {
        Ok(Some(execution)) => execution,
        _ => {
            finalize_rejected_claim(
                &claim,
                &request,
                database.inner().as_ref(),
                "task-control-task-not-found",
            )?;
            return Err("task-control-task-not-found".to_owned());
        }
    };
    let source_changed = execution.revision != request.expected_revision
        || (request.action == TaskControlAction::Start
            && verify_workspace_task_execution(&execution).is_err());
    if source_changed {
        finalize_rejected_claim(
            &claim,
            &request,
            database.inner().as_ref(),
            "task-control-source-changed",
        )?;
        return Err("task-control-source-changed".to_owned());
    }
    let preview = WorkspaceTaskControlPreview::from_request(
        &request,
        execution.label,
        execution.task_kind.as_str().to_owned(),
    );
    *slot = Some(ClaimedTaskControl { claim, request });
    Ok(preview)
}

#[tauri::command]
pub fn renew_workspace_task_control(
    request_id: String,
    pending: State<'_, PendingTaskControl>,
) -> Result<u64, String> {
    let mut slot = pending.slot();
    let current = slot
        .as_mut()
        .filter(|current| current.request.request_id == request_id)
        .ok_or_else(|| "task-control-not-open".to_owned())?;
    let renewed = handoff_store()
        .renew(
            &current.claim,
            TASK_CONTROL_TARGET_APP,
            now_ms(),
            devbox_applink::DEFAULT_CLAIM_LEASE_MS,
        )
        .map_err(|_| "task-control-unavailable".to_owned())?;
    current.claim = renewed.clone();
    Ok(renewed.lease_until_ms)
}

#[tauri::command]
pub fn reject_workspace_task_control(
    request_id: String,
    pending: State<'_, PendingTaskControl>,
    database: State<'_, Arc<DatabaseState>>,
) -> Result<WorkspaceTaskControlReceipt, String> {
    let mut slot = pending.slot();
    let Some(current) = slot.take() else {
        return Err("task-control-not-open".to_owned());
    };
    if current.request.request_id != request_id {
        *slot = Some(current);
        return Err("task-control-not-open".to_owned());
    }
    drop(slot);
    finalize_rejected_claim(
        &current.claim,
        &current.request,
        database.inner().as_ref(),
        "task-control-user-rejected",
    )
}

#[tauri::command]
pub async fn accept_workspace_task_control(
    request_id: String,
    pending: State<'_, PendingTaskControl>,
    runtime: State<'_, Arc<RuntimeState>>,
    database: State<'_, Arc<DatabaseState>>,
) -> Result<WorkspaceTaskControlReceipt, String> {
    let request = {
        let mut slot = pending.slot();
        let current = slot
            .as_ref()
            .filter(|current| current.request.request_id == request_id)
            .ok_or_else(|| "task-control-not-open".to_owned())?;
        database
            .create_workspace_task_control_receipt_at(
                &current.request,
                WorkspaceTaskControlReceiptStatus::Accepted,
                None,
                current_epoch_millis(),
            )
            .map_err(|_| "task-control-request-replayed".to_owned())?;
        if handoff_store()
            .ack(&current.claim, TASK_CONTROL_TARGET_APP, now_ms())
            .is_err()
        {
            let _ = database.finish_workspace_task_control_receipt_at(
                &request_id,
                WorkspaceTaskControlReceiptStatus::Failed,
                None,
                Some("task-control-claim-failed"),
                current_epoch_millis(),
            );
            let failed = slot.take().expect("checked task-control slot");
            restore_claim(&failed.claim);
            return Err("task-control-claim-failed".to_owned());
        }
        slot.take().expect("checked task-control slot").request
    };

    let action_result = perform_action(&request, runtime.inner(), database.inner()).await;
    let (status, operation_id, failure_code) = match action_result {
        Ok((status, operation_id)) => (status, Some(operation_id), None),
        Err(code) => (WorkspaceTaskControlReceiptStatus::Failed, None, Some(code)),
    };
    let receipt = database
        .finish_workspace_task_control_receipt_at(
            &request.request_id,
            status,
            operation_id.as_deref(),
            failure_code.as_deref(),
            current_epoch_millis(),
        )
        .map_err(|_| "task-control-receipt-storage".to_owned())?;
    let _ = crate::integration::write_task_control_receipts(database.inner().as_ref());
    if let Some(code) = failure_code {
        return Err(code);
    }
    Ok(receipt)
}

async fn perform_action(
    request: &TaskControlRequest,
    runtime: &Arc<RuntimeState>,
    database: &Arc<DatabaseState>,
) -> Result<(WorkspaceTaskControlReceiptStatus, String), String> {
    let execution = database
        .get_workspace_task_execution(&request.task_id)
        .map_err(|_| "task-control-storage".to_owned())?
        .ok_or_else(|| "task-control-task-not-found".to_owned())?;
    if execution.revision != request.expected_revision {
        return Err("task-control-source-changed".to_owned());
    }
    match request.action {
        TaskControlAction::Start => {
            verify_workspace_task_execution(&execution)
                .map_err(|_| "task-control-source-changed".to_owned())?;
            let operation = crate::workspace_orchestration::start_workspace_task_operation(
                Arc::clone(database),
                runtime.coordinator(),
                &request.task_id,
                true,
            )?;
            Ok((WorkspaceTaskControlReceiptStatus::Started, operation.id))
        }
        TaskControlAction::Stop => {
            let operation = database
                .get_active_workspace_task_operation_for_root(&request.task_id)
                .map_err(|_| "task-control-storage".to_owned())?
                .ok_or_else(|| "task-control-operation-not-active".to_owned())?;
            crate::workspace_orchestration::stop_workspace_task_operation_owned(
                database.as_ref(),
                &runtime.coordinator(),
                &operation.id,
            )
            .await?;
            Ok((WorkspaceTaskControlReceiptStatus::Stopped, operation.id))
        }
    }
}

#[tauri::command]
pub fn list_workspace_task_control_receipts(
    limit: Option<usize>,
    database: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<WorkspaceTaskControlReceipt>, String> {
    database
        .list_workspace_task_control_receipts(limit.unwrap_or(20))
        .map_err(|_| "task-control-receipt-storage".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_identity_is_strictly_opaque() {
        assert!(valid_handoff_id(&"a".repeat(32)));
        assert!(!valid_handoff_id("../request"));
        assert!(!valid_handoff_id(&"A".repeat(32)));
        assert!(valid_workspace_task_id(
            "550e8400-e29b-41d4-a716-446655440000"
        ));
        assert!(!valid_workspace_task_id("secret-looking-task-id"));
    }
}
