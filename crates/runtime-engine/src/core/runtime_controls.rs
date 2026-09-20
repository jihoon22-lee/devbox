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

/// Private typed Session adapters share the durable receipt table, while the
/// generic renderer control endpoint keeps its original closed allowlist.
pub(crate) fn stored_control_method(method: &str) -> bool {
    legacy_control_method(method)
        || matches!(
            method,
            "session_start_job"
                | "session_acquire_service"
                | "session_stop_service_generation"
                | "session_stop_exact_run"
        )
}
