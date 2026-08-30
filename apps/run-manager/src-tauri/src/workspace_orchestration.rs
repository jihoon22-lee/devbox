//! Durable dependency-operation executor for trusted workspace tasks.
//!
//! Child runs always use the ordinary scheduler. This layer only owns DAG
//! ordering and records the exact run IDs that an operation may stop.

use crate::core::models::{Run, RunStatus};
use crate::core::workspace_orchestration::{
    build_workspace_task_operation_plan, WorkspaceTaskOperationPlan,
    WorkspaceTaskOperationRunStatus, WorkspaceTaskOperationStatus, WorkspaceTaskOperationView,
};
use crate::core::workspace_tasks::{
    revalidate_workspace_task_execution, verify_workspace_task_executions,
};
use crate::scheduler::SchedulerCoordinator;
use crate::storage::{current_epoch_millis, DatabaseState};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const STOP_SETTLE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

pub fn start_workspace_task_operation(
    database: Arc<DatabaseState>,
    coordinator: SchedulerCoordinator,
    root_job_id: &str,
    fail_fast: bool,
) -> Result<WorkspaceTaskOperationView, String> {
    let root = database
        .get_workspace_task_execution(root_job_id)
        .map_err(|_| "workspace-task-operation-storage".to_owned())?
        .ok_or_else(|| "workspace-task-not-found".to_owned())?;
    let executions = database
        .list_workspace_task_executions_for_source(&root.source_id)
        .map_err(|_| "workspace-task-operation-storage".to_owned())?;
    if verify_workspace_task_executions(&executions).is_err() {
        let _ =
            database.invalidate_workspace_task_source_at(&root.source_id, current_epoch_millis());
        return Err("workspace-task-source-changed".to_owned());
    }
    let plan = build_workspace_task_operation_plan(root_job_id, &executions)
        .map_err(|error| error.to_string())?;
    let operation = database
        .create_workspace_task_operation_at(&plan, fail_fast, current_epoch_millis())
        .map_err(|error| match error {
            crate::storage::StorageError::ConcurrentChange(entity)
                if entity == "workspace-task-operation-active" =>
            {
                "workspace-task-operation-active".to_owned()
            }
            crate::storage::StorageError::ConcurrentChange(_) => {
                "workspace-task-source-changed".to_owned()
            }
            _ => "workspace-task-operation-storage".to_owned(),
        })?;
    let _ = crate::integration::write_workspace_tasks(database.as_ref());
    spawn_workspace_task_operation(database, coordinator, operation.id.clone(), plan);
    Ok(operation)
}

pub fn spawn_workspace_task_operation(
    database: Arc<DatabaseState>,
    coordinator: SchedulerCoordinator,
    operation_id: String,
    plan: WorkspaceTaskOperationPlan,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(code) = execute_workspace_task_operation(
            Arc::clone(&database),
            coordinator.clone(),
            &operation_id,
            &plan,
        )
        .await
        {
            // Never terminalize the parent ahead of an owned process. First
            // close the launch gate, then prove each exact child is terminal.
            let _ = database.request_workspace_task_operation_stop(&operation_id);
            let _ = settle_owned_operation(
                &database,
                &coordinator,
                &operation_id,
                WorkspaceTaskOperationStatus::Failed,
                code,
            )
            .await;
        }
        let _ = crate::integration::write_workspace_tasks(database.as_ref());
    });
}

async fn execute_workspace_task_operation(
    database: Arc<DatabaseState>,
    coordinator: SchedulerCoordinator,
    operation_id: &str,
    plan: &WorkspaceTaskOperationPlan,
) -> Result<(), &'static str> {
    if !database
        .mark_workspace_task_operation_running_at(operation_id, current_epoch_millis())
        .map_err(|_| "workspace-task-operation-storage")?
    {
        return match operation_status(&database, operation_id)? {
            WorkspaceTaskOperationStatus::Stopping => {
                settle_cancelled(&database, &coordinator, operation_id).await
            }
            status if status.is_terminal() => Ok(()),
            _ => Err("workspace-task-operation-state-changed"),
        };
    }
    let fail_fast = database
        .get_workspace_task_operation(operation_id)
        .map_err(|_| "workspace-task-operation-storage")?
        .ok_or("workspace-task-operation-not-found")?
        .fail_fast;

    for layer in &plan.layers {
        if operation_status(&database, operation_id)? == WorkspaceTaskOperationStatus::Stopping {
            settle_cancelled(&database, &coordinator, operation_id).await?;
            return Ok(());
        }

        let mut owned = BTreeMap::<String, String>::new();
        let mut layer_failed = false;
        for job_id in layer {
            if operation_status(&database, operation_id)? != WorkspaceTaskOperationStatus::Running {
                settle_cancelled(&database, &coordinator, operation_id).await?;
                return Ok(());
            }
            if layer_failed && fail_fast {
                break;
            }
            if !database
                .claim_workspace_task_operation_run(operation_id, job_id)
                .map_err(|_| "workspace-task-operation-storage")?
            {
                return Err("workspace-task-operation-state-changed");
            }

            let execution = match database.get_workspace_task_execution(job_id) {
                Ok(Some(execution)) => execution,
                _ => {
                    complete_child(
                        &database,
                        operation_id,
                        job_id,
                        WorkspaceTaskOperationRunStatus::Failed,
                        Some("workspace-task-not-found"),
                    )?;
                    layer_failed = true;
                    continue;
                }
            };
            if execution.source_id != plan.source_id
                || execution.revision != plan.revision
                || revalidate_workspace_task_execution(&execution).is_err()
            {
                let _ = database.invalidate_workspace_task_source_at(
                    &execution.source_id,
                    current_epoch_millis(),
                );
                complete_child(
                    &database,
                    operation_id,
                    job_id,
                    WorkspaceTaskOperationRunStatus::Failed,
                    Some("workspace-task-source-changed"),
                )?;
                layer_failed = true;
                continue;
            }

            match coordinator
                .trigger_manual_at(job_id, current_epoch_millis())
                .await
            {
                Ok(run) => {
                    if !database
                        .attach_workspace_task_operation_run(operation_id, job_id, &run.id)
                        .map_err(|_| "workspace-task-operation-storage")?
                    {
                        // This exact run was spawned after the durable launch
                        // reservation but could not be attached. Stop it before
                        // allowing the operation to settle.
                        let stopped =
                            stop_exact_run(&database, &coordinator, job_id, &run.id).await;
                        let (status, code) = stopped.unwrap_or((
                            WorkspaceTaskOperationRunStatus::Failed,
                            Some("workspace-task-operation-stop-failed".to_owned()),
                        ));
                        complete_child(&database, operation_id, job_id, status, code.as_deref())?;
                        return Err("workspace-task-operation-state-changed");
                    }
                    if let Some((status, code)) = terminal_operation_status(&run) {
                        complete_child(&database, operation_id, job_id, status, code.as_deref())?;
                        layer_failed |= status != WorkspaceTaskOperationRunStatus::Succeeded;
                    } else {
                        owned.insert(job_id.clone(), run.id);
                    }
                }
                Err(_) => {
                    complete_child(
                        &database,
                        operation_id,
                        job_id,
                        WorkspaceTaskOperationRunStatus::Failed,
                        Some("workspace-task-start-failed"),
                    )?;
                    layer_failed = true;
                }
            }
        }

        match wait_for_owned_layer(
            &database,
            &coordinator,
            operation_id,
            &mut owned,
            layer_failed,
            fail_fast,
        )
        .await?
        {
            LayerOutcome::Succeeded => {}
            LayerOutcome::Cancelled => return Ok(()),
            LayerOutcome::Failed => {
                database
                    .finish_workspace_task_operation_at(
                        operation_id,
                        WorkspaceTaskOperationStatus::Failed,
                        Some("workspace-task-dependency-failed"),
                        current_epoch_millis(),
                    )
                    .map_err(|_| "workspace-task-operation-storage")?;
                return Ok(());
            }
        }
    }

    database
        .finish_workspace_task_operation_at(
            operation_id,
            WorkspaceTaskOperationStatus::Succeeded,
            None,
            current_epoch_millis(),
        )
        .map_err(|_| "workspace-task-operation-storage")?;
    Ok(())
}

async fn wait_for_owned_layer(
    database: &DatabaseState,
    coordinator: &SchedulerCoordinator,
    operation_id: &str,
    owned: &mut BTreeMap<String, String>,
    mut failed: bool,
    fail_fast: bool,
) -> Result<LayerOutcome, &'static str> {
    while !owned.is_empty() {
        if operation_status(database, operation_id)? == WorkspaceTaskOperationStatus::Stopping {
            settle_cancelled(database, coordinator, operation_id).await?;
            return Ok(LayerOutcome::Cancelled);
        }
        let mut completed = Vec::new();
        for (job_id, run_id) in owned.iter() {
            let run = database
                .get_run(run_id)
                .map_err(|_| "workspace-task-operation-storage")?
                .ok_or("workspace-task-run-missing")?;
            if let Some((status, code)) = terminal_operation_status(&run) {
                complete_child(database, operation_id, job_id, status, code.as_deref())?;
                failed |= status != WorkspaceTaskOperationRunStatus::Succeeded;
                completed.push(job_id.clone());
            }
        }
        for job_id in completed {
            owned.remove(&job_id);
        }
        if failed && fail_fast && !owned.is_empty() {
            for (job_id, run_id) in owned.iter() {
                let (status, code) = stop_exact_run(database, coordinator, job_id, run_id).await?;
                complete_child(database, operation_id, job_id, status, code.as_deref())?;
            }
            owned.clear();
        }
        if !owned.is_empty() {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
    Ok(if failed {
        LayerOutcome::Failed
    } else {
        LayerOutcome::Succeeded
    })
}

pub async fn stop_workspace_task_operation_owned(
    database: &DatabaseState,
    coordinator: &SchedulerCoordinator,
    operation_id: &str,
) -> Result<WorkspaceTaskOperationView, String> {
    let operation = database
        .get_workspace_task_operation(operation_id)
        .map_err(|_| "workspace-task-operation-storage".to_owned())?
        .ok_or_else(|| "workspace-task-operation-not-found".to_owned())?;
    if operation.status.is_terminal() {
        let _ = crate::integration::write_workspace_tasks(database);
        return Ok(operation);
    }
    database
        .request_workspace_task_operation_stop(operation_id)
        .map_err(|_| "workspace-task-operation-storage".to_owned())?;
    settle_cancelled(database, coordinator, operation_id)
        .await
        .map_err(str::to_owned)?;
    let operation = database
        .get_workspace_task_operation(operation_id)
        .map_err(|_| "workspace-task-operation-storage".to_owned())?
        .ok_or_else(|| "workspace-task-operation-not-found".to_owned())?;
    let _ = crate::integration::write_workspace_tasks(database);
    Ok(operation)
}

async fn settle_cancelled(
    database: &DatabaseState,
    coordinator: &SchedulerCoordinator,
    operation_id: &str,
) -> Result<(), &'static str> {
    settle_owned_operation(
        database,
        coordinator,
        operation_id,
        WorkspaceTaskOperationStatus::Cancelled,
        "workspace-task-operation-cancelled",
    )
    .await
}

async fn settle_owned_operation(
    database: &DatabaseState,
    coordinator: &SchedulerCoordinator,
    operation_id: &str,
    terminal_status: WorkspaceTaskOperationStatus,
    failure_code: &'static str,
) -> Result<(), &'static str> {
    let deadline = tokio::time::Instant::now() + STOP_SETTLE_TIMEOUT;
    loop {
        let active = database
            .workspace_task_operation_active_runs(operation_id)
            .map_err(|_| "workspace-task-operation-storage")?;
        let mut stop_failed = false;
        for (job_id, run_id) in active {
            match stop_exact_run(database, coordinator, &job_id, &run_id).await {
                Ok((status, code)) => {
                    complete_child(database, operation_id, &job_id, status, code.as_deref())?
                }
                Err(_) => stop_failed = true,
            }
        }
        let unsettled = database
            .workspace_task_operation_has_unsettled_runs(operation_id)
            .map_err(|_| "workspace-task-operation-storage")?;
        if !unsettled {
            database
                .finish_workspace_task_operation_at(
                    operation_id,
                    terminal_status,
                    Some(failure_code),
                    current_epoch_millis(),
                )
                .map_err(|_| "workspace-task-operation-storage")?;
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(if stop_failed {
                "workspace-task-operation-stop-failed"
            } else {
                "workspace-task-operation-stop-timeout"
            });
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn stop_exact_run(
    database: &DatabaseState,
    coordinator: &SchedulerCoordinator,
    job_id: &str,
    run_id: &str,
) -> Result<(WorkspaceTaskOperationRunStatus, Option<String>), &'static str> {
    if let Some(stopped) = coordinator
        .stop_exact_active_at(job_id, run_id, current_epoch_millis())
        .await
        .map_err(|_| "workspace-task-operation-stop-failed")?
    {
        if stopped.id != run_id {
            return Err("workspace-task-operation-ownership-changed");
        }
    }
    let run = database
        .get_run(run_id)
        .map_err(|_| "workspace-task-operation-storage")?
        .ok_or("workspace-task-run-missing")?;
    terminal_operation_status(&run).ok_or("workspace-task-operation-ownership-changed")
}

fn complete_child(
    database: &DatabaseState,
    operation_id: &str,
    job_id: &str,
    status: WorkspaceTaskOperationRunStatus,
    code: Option<&str>,
) -> Result<(), &'static str> {
    database
        .complete_workspace_task_operation_run(operation_id, job_id, status, code)
        .map(|_| ())
        .map_err(|_| "workspace-task-operation-storage")
}

fn operation_status(
    database: &DatabaseState,
    operation_id: &str,
) -> Result<WorkspaceTaskOperationStatus, &'static str> {
    database
        .get_workspace_task_operation(operation_id)
        .map_err(|_| "workspace-task-operation-storage")?
        .map(|operation| operation.status)
        .ok_or("workspace-task-operation-not-found")
}

fn terminal_operation_status(
    run: &Run,
) -> Option<(WorkspaceTaskOperationRunStatus, Option<String>)> {
    match run.status {
        RunStatus::Succeeded => Some((WorkspaceTaskOperationRunStatus::Succeeded, None)),
        RunStatus::Failed => Some((
            WorkspaceTaskOperationRunStatus::Failed,
            crate::core::models::RunView::from_run(run)
                .failure_code
                .or_else(|| Some("workspace-task-run-failed".to_owned())),
        )),
        RunStatus::Cancelled => Some((
            WorkspaceTaskOperationRunStatus::Cancelled,
            Some("workspace-task-run-cancelled".to_owned()),
        )),
        RunStatus::Skipped => Some((
            WorkspaceTaskOperationRunStatus::Skipped,
            Some("workspace-task-run-skipped".to_owned()),
        )),
        RunStatus::Queued | RunStatus::Starting | RunStatus::Running | RunStatus::Stopping => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: RunStatus, failure_code: Option<&str>) -> Run {
        Run {
            id: "run".to_owned(),
            job_id: "job".to_owned(),
            scheduled_at: None,
            occurrence_wall_key: None,
            queue_sequence: 1,
            blocked_by_run_id: None,
            started_at: Some(1),
            ended_at: matches!(
                status,
                RunStatus::Succeeded
                    | RunStatus::Failed
                    | RunStatus::Cancelled
                    | RunStatus::Skipped
            )
            .then_some(2),
            exit_code: None,
            status,
            owner_instance_id: None,
            attempt_token: None,
            error_message: failure_code.map(str::to_owned),
            target_pid: None,
            target_process_created_at: None,
            target_pgid: None,
            target_sid: None,
            process_marker: None,
            log_dir: None,
            logs_deleted_at: None,
            created_at: 1,
        }
    }

    #[test]
    fn terminal_mapping_uses_only_fixed_run_failure_codes() {
        let failed = run(RunStatus::Failed, Some("spawn-failed"));
        assert_eq!(
            terminal_operation_status(&failed),
            Some((
                WorkspaceTaskOperationRunStatus::Failed,
                Some("spawn-failed".to_owned())
            ))
        );
        let succeeded = run(RunStatus::Succeeded, None);
        assert_eq!(
            terminal_operation_status(&succeeded),
            Some((WorkspaceTaskOperationRunStatus::Succeeded, None))
        );
    }
}
