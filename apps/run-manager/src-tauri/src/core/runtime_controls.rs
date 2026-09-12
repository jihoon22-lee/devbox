//! The closed set of side effects covered by durable product request receipts.
pub fn legacy_control_method(method: &str) -> bool {
    matches!(
        method,
        "run_job_now"
            | "stop_active_run"
            | "start_service"
            | "stop_service"
            | "restart_service"
            | "run_workspace_task_operation"
            | "stop_workspace_task_operation"
    )
}
