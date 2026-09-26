use product_ipc::workspace::Lane;
use product_ipc::{ComponentCall, ExecutionClass};
use serde::Deserialize;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceRuntimeCallHost {
    WorkspaceTaskSource {},
}
#[derive(ts_rs::TS)]
#[ts(untagged)]
pub enum WorkspaceRuntimeCall {
    Host(WorkspaceRuntimeCallHost),
    Engine(Box<runtime_engine::api::RuntimeCall>),
}
pub const METHODS: &[&str] = &[
    "accept_workspace_task_control",
    "apply_import",
    "apply_project_import",
    "apply_workspace_task_import",
    "cancel_project_import",
    "cancel_workspace_task_import",
    "create_job",
    "create_service",
    "delete_job",
    "delete_service",
    "export_definitions",
    "get_active_run",
    "get_job",
    "get_run",
    "get_service",
    "get_service_instance",
    "get_workspace_task_operation",
    "hide_main_window",
    "import_definitions",
    "list_active_runs",
    "list_jobs",
    "list_run_history",
    "list_runs",
    "list_runtime_controls",
    "list_services",
    "list_workspace_task_control_receipts",
    "list_workspace_task_diagnostics",
    "list_workspace_task_operations",
    "list_workspace_tasks",
    "open_run_log_in_log_lens",
    "open_workspace_task_diagnostic",
    "preview_cron",
    "preview_project_import",
    "preview_workspace_task_control",
    "preview_workspace_task_import",
    "quit_app",
    "reject_workspace_task_control",
    "renew_workspace_task_control",
    "review_runtime_control",
    "runtime_control",
    "runtime_control_status",
    "runtime_status",
    "search_run_logs",
    "service_observability",
    "set_job_enabled",
    "set_startup_shortcut_enabled",
    "show_main_window",
    "startup_shortcut_status",
    "tail_log",
    "take_pending_open",
    "trust_workspace_task_shell_source",
    "trust_workspace_task_source",
    "update_job",
    "update_service",
    "workspace_task_source",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
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
        "set_startup_shortcut_enabled" => &["tasks"],
        "show_main_window" => &["tasks"],
        "startup_shortcut_status" => &["tasks"],
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
impl WorkspaceRuntimeCallHost {
    fn method(&self) -> &'static str {
        match self {
            Self::WorkspaceTaskSource { .. } => "workspace_task_source",
        }
    }
}
impl ComponentCall for WorkspaceRuntimeCall {
    const COMPONENT: &'static str = "workspace.runtime";
    const MAX_ARGUMENT_BYTES: usize = 2097152;
    const SHARED_REQUEST_LIMIT: bool = false;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
    fn class(&self) -> ExecutionClass {
        if matches!(self.lane(), Lane::EngineStop | Lane::TerminalStop) {
            ExecutionClass::Control
        } else {
            ExecutionClass::Normal
        }
    }
}
impl WorkspaceRuntimeCall {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Engine(call) => call.lane(),
            Self::Host(_) => Lane::Engine,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
#[tauri::command]
pub(crate) async fn runtime(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceRuntimeCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceRuntimeCall {
    fn into_call(self) -> super::Call {
        super::Call::Runtime(self)
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    use super::results::*;
    export.register::<WorkspaceRuntimeCall>()?;
    let mut results = runtime_engine::api::result_types(export)?;
    results.retain(|(method, _)| METHODS.contains(method));
    results.retain(|(method, _)| *method != "workspace_task_source");
    results.push((
        "workspace_task_source",
        export.register::<WorkspaceTaskSourceReply>()?,
    ));
    results.retain(|(method, _)| *method != "open_run_log_in_log_lens");
    results.push((
        "open_run_log_in_log_lens",
        export.register::<HandoffReply>()?,
    ));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.runtime", method)
}

impl<'de> serde::Deserialize<'de> for WorkspaceRuntimeCall {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        product_ipc::decode_host_first(
            deserializer,
            &["workspace_task_source"],
            Self::Host,
            Self::Engine,
        )
    }
}
