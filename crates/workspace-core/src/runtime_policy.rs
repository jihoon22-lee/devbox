//! The UI host and agent use the same closed route allowlists.
pub fn runtime_routes(method: &str) -> &'static [&'static str] {
    match method {
        "accept_workspace_task_control" => &["tasks"],
        "apply_import" => &["tasks"],
        "apply_project_import" => &["tasks"],
        "apply_workspace_task_import" => &["tasks"],
        "cancel_project_import" => &["tasks"],
        "cancel_workspace_task_import" => &["tasks"],
        "create_job" => &["tasks"],
        "create_service" => &["tasks"],
        "delete_job" => &["tasks"],
        "delete_service" => &["tasks"],
        "export_definitions" => &["tasks"],
        "get_active_run" => &["tasks"],
        "get_job" => &["tasks"],
        "get_run" => &["tasks"],
        "get_service" => &["tasks"],
        "get_service_instance" => &["tasks"],
        "get_workspace_task_operation" => &["tasks"],
        "hide_main_window" => &["tasks"],
        "import_definitions" => &["tasks"],
        "list_active_runs" => &["tasks"],
        "list_jobs" => &["tasks"],
        "list_run_history" => &["tasks"],
        "list_runs" => &["tasks"],
        "list_runtime_controls" => &["tasks"],
        "list_services" => &["tasks"],
        "list_workspace_task_control_receipts" => &["tasks"],
        "list_workspace_task_diagnostics" => &["tasks"],
        "list_workspace_task_operations" => &["tasks"],
        "list_workspace_tasks" => &["tasks"],
        "open_run_log_in_log_lens" => &["tasks"],
        "open_workspace_task_diagnostic" => &["tasks"],
        "preview_cron" => &["tasks"],
        "preview_project_import" => &["tasks"],
        "preview_workspace_task_control" => &["tasks"],
        "preview_workspace_task_import" => &["tasks"],
        "quit_app" => &["tasks"],
        "reject_workspace_task_control" => &["tasks"],
        "renew_workspace_task_control" => &["tasks"],
        "review_runtime_control" => &["tasks"],
        "runtime_control" => &["tasks"],
        "runtime_control_status" => &["tasks"],
        "runtime_status" => &["tasks"],
        "search_run_logs" => &["tasks"],
        "service_observability" => &["tasks"],
        "set_job_enabled" => &["tasks"],
        "show_main_window" => &["tasks"],
        "tail_log" => &["tasks"],
        "take_pending_open" => &["tasks"],
        "trust_workspace_task_shell_source" => &["tasks"],
        "trust_workspace_task_source" => &["tasks"],
        "update_job" => &["tasks"],
        "update_service" => &["tasks"],
        "workspace_task_source" => &["tasks"],
        _ => &[],
    }
}

pub fn processes_routes(method: &str) -> &'static [&'static str] {
    match method {
        "apply_legacy_runtime_settings" => &["runtime"],
        "get_process_info" => &["runtime"],
        "handoff_container_stop" => &["runtime"],
        "list_port_observations" => &["runtime"],
        "list_ports" => &["runtime"],
        "load_port_manager_preferences" => &["runtime"],
        "open_browser" => &["runtime"],
        "open_port_log" => &["runtime"],
        "open_port_owner" => &["runtime"],
        "preview_legacy_runtime_settings" => &["runtime"],
        "reveal_process" => &["runtime"],
        "save_port_manager_preferences" => &["runtime"],
        _ => &[],
    }
}

pub fn process_actions_routes(method: &str) -> &'static [&'static str] {
    match method {
        "kill_listener" => &["runtime"],
        _ => &[],
    }
}

pub fn logs_routes(method: &str) -> &'static [&'static str] {
    match method {
        "accept_log_source" => &["logs"],
        "apply_legacy_runtime_settings" => &["logs"],
        "cancel_read" => &["logs"],
        "delete_saved_view" => &["logs"],
        "discard_log_source" => &["logs"],
        "export_log_records" => &["logs"],
        "filter_log_records" => &["logs"],
        "fixed_adapter" => &["logs"],
        "list_saved_views" => &["logs"],
        "open_webhook_log" => &["logs"],
        "preview_legacy_runtime_settings" => &["logs"],
        "preview_log_source" => &["logs"],
        "read_source" => &["logs"],
        "read_sources" => &["logs"],
        "receive_log_source" => &["logs"],
        "reconnect_runtime_sources" => &["logs"],
        "renew_log_source" => &["logs"],
        "save_saved_view" => &["logs"],
        "send_selection_to_toolbox" => &["logs"],
        "summarize_source" => &["logs"],
        "take_pending_open" => &["logs"],
        _ => &[],
    }
}

pub fn issue(error: String) -> &'static str {
    // Existing engines also return OS/SQLite diagnostics. Only fixed public
    // codes can cross this product boundary; no path/SQL/environment is echoed.
    match error.as_str() {
        "runtime_control_in_progress" => "runtime_control_in_progress",
        "runtime_control_recovery_required" => "runtime_control_recovery_required",
        "runtime_control_failed" => "runtime_control_failed",
        "runtime_control_unavailable" => "runtime_control_unavailable",
        "runtime_control_owner_unsettled" => "runtime_control_owner_unsettled",
        "runtime_settings_unavailable" => "runtime_settings_unavailable",
        "runtime_settings_invalid" => "runtime_settings_invalid",
        "runtime_settings_conflict" => "runtime_settings_conflict",
        "runtime_import_busy" => "runtime_import_busy",
        "runtime_import_destination_conflict" => "runtime_import_destination_conflict",
        "runtime_import_stale" => "runtime_import_stale",
        "component_args_invalid" => "invalid_request",
        "component_storage_changed" => "runtime_store_changed",
        "runtime_log_changed" => "runtime_log_changed",
        "runtime_log_unavailable" | "runtime_log_identity_invalid" => "runtime_log_unavailable",
        "process_action_stale" => "process_action_stale",
        "process_owner_unsettled" => "process_owner_unsettled",
        "process_action_invalid" => "invalid_request",
        "process_observation_unavailable" => "process_observation_unavailable",
        "workspace-task-source-changed" => "runtime_task_source_changed",
        "workspace-task-source-untrusted" | "workspace-task-shell-untrusted" => {
            "runtime_task_review_required"
        }
        _ => "runtime_operation_unavailable",
    }
}
