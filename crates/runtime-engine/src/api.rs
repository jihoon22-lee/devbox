//! Closed typed engine calls. Product ownership checks remain at the host boundary.
use product_ipc::workspace::{Lane, LONG_BUDGET_MS};
use serde::Deserialize;
use std::sync::Arc;
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum RuntimeCall {
    RuntimeStatus {},
    ShowMainWindow {},
    HideMainWindow {},
    QuitApp {},
    StartupShortcutStatus {},
    SetStartupShortcutEnabled {
        enabled: bool,
    },
    ListJobs {},
    GetJob {
        id: String,
    },
    CreateJob {
        input: crate::core::models::JobInput,
    },
    UpdateJob {
        id: String,
        input: crate::core::models::JobInput,
    },
    SetJobEnabled {
        id: String,
        enabled: bool,
    },
    DeleteJob {
        id: String,
    },
    ListServices {},
    GetService {
        id: String,
    },
    ServiceObservability {
        id: String,
    },
    ImportDefinitions {
        json: String,
    },
    ApplyImport {
        json: String,
        selected: Vec<String>,
        revision: Option<String>,
    },
    PreviewWorkspaceTaskImport {
        path: String,
        target_kind: crate::core::models::TargetKind,
        target_distro: Option<String>,
        operation_id: String,
    },
    CancelWorkspaceTaskImport {
        operation_id: String,
    },
    ApplyWorkspaceTaskImport {
        path: String,
        source_root: String,
        project_identity: String,
        revision: String,
        target_kind: crate::core::models::TargetKind,
        target_distro: Option<String>,
        selected: Vec<String>,
        operation_id: String,
    },
    ListWorkspaceTasks {},
    TrustWorkspaceTaskSource {
        source_id: String,
        revision: String,
    },
    TrustWorkspaceTaskShellSource {
        source_id: String,
        revision: String,
        acknowledgement: String,
    },
    GetWorkspaceTaskOperation {
        operation_id: String,
    },
    ListWorkspaceTaskOperations {
        limit: Option<usize>,
    },
    ListWorkspaceTaskDiagnostics {
        run_id: String,
    },
    OpenWorkspaceTaskDiagnostic {
        run_id: String,
        diagnostic_index: u32,
    },
    PreviewProjectImport {
        path: String,
        operation_id: String,
    },
    CancelProjectImport {
        operation_id: String,
    },
    ApplyProjectImport {
        path: String,
        source_root: String,
        revision: String,
        selected: Vec<String>,
        operation_id: String,
    },
    CreateService {
        input: crate::core::models::ServiceInput,
    },
    UpdateService {
        id: String,
        input: crate::core::models::ServiceInput,
    },
    DeleteService {
        id: String,
    },
    ExportDefinitions {},
    GetServiceInstance {
        id: String,
    },
    GetRun {
        id: String,
    },
    ListRuns {
        job_id: Option<String>,
        limit: Option<u32>,
        start_at: Option<i64>,
        end_at: Option<i64>,
        status: Option<crate::core::models::RunStatus>,
        kind: Option<crate::core::models::JobKind>,
        min_duration_ms: Option<i64>,
        max_duration_ms: Option<i64>,
    },
    ListRunHistory {
        input: crate::core::models::RunHistoryFilter,
    },
    GetActiveRun {
        id: String,
    },
    ListActiveRuns {},
    PreviewCron {
        input: crate::commands::CronPreviewInput,
    },
    TailLog {
        input: crate::commands::TailLogInput,
    },
    SearchRunLogs {
        input: crate::core::log_search::LogSearchRequest,
    },
    PreviewWorkspaceTaskControl {
        handoff_id: String,
    },
    RenewWorkspaceTaskControl {
        request_id: String,
    },
    RejectWorkspaceTaskControl {
        request_id: String,
    },
    AcceptWorkspaceTaskControl {
        request_id: String,
    },
    ListWorkspaceTaskControlReceipts {
        limit: Option<usize>,
    },
    TakePendingOpen {},
    OpenRunLogInLogLens {
        run_id: String,
        stream: crate::logs::LogStream,
    },
    RuntimeControl(crate::api_control::Control),
    RuntimeControlStatus {
        operation_id: String,
    },
    ListRuntimeControls {},
    ReviewRuntimeControl {
        operation_id: String,
    },
}
pub const METHODS: &[&str] = &[
    "review_runtime_control",
    "list_runtime_controls",
    "runtime_control_status",
    "runtime_control",
    "runtime_status",
    "show_main_window",
    "hide_main_window",
    "quit_app",
    "startup_shortcut_status",
    "set_startup_shortcut_enabled",
    "list_jobs",
    "get_job",
    "create_job",
    "update_job",
    "set_job_enabled",
    "delete_job",
    "list_services",
    "get_service",
    "service_observability",
    "import_definitions",
    "apply_import",
    "preview_workspace_task_import",
    "cancel_workspace_task_import",
    "apply_workspace_task_import",
    "list_workspace_tasks",
    "trust_workspace_task_source",
    "trust_workspace_task_shell_source",
    "get_workspace_task_operation",
    "list_workspace_task_operations",
    "list_workspace_task_diagnostics",
    "open_workspace_task_diagnostic",
    "preview_project_import",
    "cancel_project_import",
    "apply_project_import",
    "create_service",
    "update_service",
    "delete_service",
    "export_definitions",
    "get_service_instance",
    "get_run",
    "list_runs",
    "list_run_history",
    "get_active_run",
    "list_active_runs",
    "preview_cron",
    "tail_log",
    "search_run_logs",
    "preview_workspace_task_control",
    "renew_workspace_task_control",
    "reject_workspace_task_control",
    "accept_workspace_task_control",
    "list_workspace_task_control_receipts",
    "take_pending_open",
    "open_run_log_in_log_lens",
];
impl RuntimeCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ReviewRuntimeControl { .. } => "review_runtime_control",
            Self::ListRuntimeControls { .. } => "list_runtime_controls",
            Self::RuntimeControlStatus { .. } => "runtime_control_status",
            Self::RuntimeControl(..) => "runtime_control",
            Self::RuntimeStatus { .. } => "runtime_status",
            Self::ShowMainWindow { .. } => "show_main_window",
            Self::HideMainWindow { .. } => "hide_main_window",
            Self::QuitApp { .. } => "quit_app",
            Self::StartupShortcutStatus { .. } => "startup_shortcut_status",
            Self::SetStartupShortcutEnabled { .. } => "set_startup_shortcut_enabled",
            Self::ListJobs { .. } => "list_jobs",
            Self::GetJob { .. } => "get_job",
            Self::CreateJob { .. } => "create_job",
            Self::UpdateJob { .. } => "update_job",
            Self::SetJobEnabled { .. } => "set_job_enabled",
            Self::DeleteJob { .. } => "delete_job",
            Self::ListServices { .. } => "list_services",
            Self::GetService { .. } => "get_service",
            Self::ServiceObservability { .. } => "service_observability",
            Self::ImportDefinitions { .. } => "import_definitions",
            Self::ApplyImport { .. } => "apply_import",
            Self::PreviewWorkspaceTaskImport { .. } => "preview_workspace_task_import",
            Self::CancelWorkspaceTaskImport { .. } => "cancel_workspace_task_import",
            Self::ApplyWorkspaceTaskImport { .. } => "apply_workspace_task_import",
            Self::ListWorkspaceTasks { .. } => "list_workspace_tasks",
            Self::TrustWorkspaceTaskSource { .. } => "trust_workspace_task_source",
            Self::TrustWorkspaceTaskShellSource { .. } => "trust_workspace_task_shell_source",
            Self::GetWorkspaceTaskOperation { .. } => "get_workspace_task_operation",
            Self::ListWorkspaceTaskOperations { .. } => "list_workspace_task_operations",
            Self::ListWorkspaceTaskDiagnostics { .. } => "list_workspace_task_diagnostics",
            Self::OpenWorkspaceTaskDiagnostic { .. } => "open_workspace_task_diagnostic",
            Self::PreviewProjectImport { .. } => "preview_project_import",
            Self::CancelProjectImport { .. } => "cancel_project_import",
            Self::ApplyProjectImport { .. } => "apply_project_import",
            Self::CreateService { .. } => "create_service",
            Self::UpdateService { .. } => "update_service",
            Self::DeleteService { .. } => "delete_service",
            Self::ExportDefinitions { .. } => "export_definitions",
            Self::GetServiceInstance { .. } => "get_service_instance",
            Self::GetRun { .. } => "get_run",
            Self::ListRuns { .. } => "list_runs",
            Self::ListRunHistory { .. } => "list_run_history",
            Self::GetActiveRun { .. } => "get_active_run",
            Self::ListActiveRuns { .. } => "list_active_runs",
            Self::PreviewCron { .. } => "preview_cron",
            Self::TailLog { .. } => "tail_log",
            Self::SearchRunLogs { .. } => "search_run_logs",
            Self::PreviewWorkspaceTaskControl { .. } => "preview_workspace_task_control",
            Self::RenewWorkspaceTaskControl { .. } => "renew_workspace_task_control",
            Self::RejectWorkspaceTaskControl { .. } => "reject_workspace_task_control",
            Self::AcceptWorkspaceTaskControl { .. } => "accept_workspace_task_control",
            Self::ListWorkspaceTaskControlReceipts { .. } => "list_workspace_task_control_receipts",
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::OpenRunLogInLogLens { .. } => "open_run_log_in_log_lens",
        }
    }
    pub fn lane(&self) -> Lane {
        match self {
            Self::RuntimeControl(input) => input.action.lane(),
            Self::QuitApp { .. }
            | Self::CancelWorkspaceTaskImport { .. }
            | Self::CancelProjectImport { .. } => Lane::EngineStop,
            _ => Lane::Engine,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
#[cfg(feature = "desktop")]
async fn execute(
    _component_app: &tauri::AppHandle,
    call: RuntimeCall,
) -> Result<serde_json::Value, String> {
    match call {
        RuntimeCall::ReviewRuntimeControl { operation_id } => {
            crate::component::control::review(_component_app, &operation_id)
        }
        RuntimeCall::ListRuntimeControls {} => crate::component::control::list(_component_app),
        RuntimeCall::RuntimeControlStatus { operation_id } => {
            crate::component::control::status(_component_app, &operation_id)
        }
        RuntimeCall::RuntimeControl(input) => {
            crate::component::control::execute_typed(_component_app, input).await
        }
        RuntimeCall::RuntimeStatus {} => {
            use crate::commands::*;
            let value = runtime_status(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            );
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ShowMainWindow {} => {
            use crate::commands::*;
            show_main_window(_component_app.clone())?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::HideMainWindow {} => {
            use crate::commands::*;
            hide_main_window(_component_app.clone())?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::QuitApp {} => {
            use crate::commands::*;
            quit_app(
                _component_app.clone(),
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            );
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::StartupShortcutStatus {} => {
            use crate::commands::*;
            let value = startup_shortcut_status(_component_app.clone())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::SetStartupShortcutEnabled { enabled } => {
            use crate::commands::*;
            let value = set_startup_shortcut_enabled(_component_app.clone(), enabled)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListJobs {} => {
            use crate::commands::*;
            let value = list_jobs(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetJob { id } => {
            use crate::commands::*;
            let value = get_job(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::CreateJob { input } => {
            use crate::commands::*;
            let value = create_job(
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::UpdateJob { id, input } => {
            use crate::commands::*;
            let value = update_job(
                id,
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::SetJobEnabled { id, enabled } => {
            use crate::commands::*;
            let value = set_job_enabled(
                id,
                enabled,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::DeleteJob { id } => {
            use crate::commands::*;
            let value = delete_job(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListServices {} => {
            use crate::commands::*;
            let value = list_services(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetService { id } => {
            use crate::commands::*;
            let value = get_service(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ServiceObservability { id } => {
            use crate::commands::*;
            let value = service_observability(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ImportDefinitions { json } => {
            use crate::commands::*;
            let value = import_definitions(
                json,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ApplyImport {
            json,
            selected,
            revision,
        } => {
            use crate::commands::*;
            let value = apply_import(
                json,
                selected,
                revision,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::PreviewWorkspaceTaskImport {
            path,
            target_kind,
            target_distro,
            operation_id,
        } => {
            use crate::commands::*;
            let value = preview_workspace_task_import(
                path,
                target_kind,
                target_distro,
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::CancelWorkspaceTaskImport { operation_id } => {
            use crate::commands::*;
            let value = cancel_workspace_task_import(
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ApplyWorkspaceTaskImport {
            path,
            source_root,
            project_identity,
            revision,
            target_kind,
            target_distro,
            selected,
            operation_id,
        } => {
            use crate::commands::*;
            let value = apply_workspace_task_import(
                path,
                source_root,
                project_identity,
                revision,
                target_kind,
                target_distro,
                selected,
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListWorkspaceTasks {} => {
            use crate::commands::*;
            let value = list_workspace_tasks(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::TrustWorkspaceTaskSource {
            source_id,
            revision,
        } => {
            use crate::commands::*;
            let value = trust_workspace_task_source(
                source_id,
                revision,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::TrustWorkspaceTaskShellSource {
            source_id,
            revision,
            acknowledgement,
        } => {
            use crate::commands::*;
            let value = trust_workspace_task_shell_source(
                source_id,
                revision,
                acknowledgement,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetWorkspaceTaskOperation { operation_id } => {
            use crate::commands::*;
            let value = get_workspace_task_operation(
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListWorkspaceTaskOperations { limit } => {
            use crate::commands::*;
            let value = list_workspace_task_operations(
                limit,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListWorkspaceTaskDiagnostics { run_id } => {
            use crate::commands::*;
            let value = list_workspace_task_diagnostics(
                run_id,
                _component_app.clone(),
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::OpenWorkspaceTaskDiagnostic {
            run_id,
            diagnostic_index,
        } => {
            use crate::commands::*;
            let value = open_workspace_task_diagnostic(
                run_id,
                diagnostic_index,
                _component_app.clone(),
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::PreviewProjectImport { path, operation_id } => {
            use crate::commands::*;
            let value = preview_project_import(
                path,
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::CancelProjectImport { operation_id } => {
            use crate::commands::*;
            let value = cancel_project_import(
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ApplyProjectImport {
            path,
            source_root,
            revision,
            selected,
            operation_id,
        } => {
            use crate::commands::*;
            let value = apply_project_import(
                path,
                source_root,
                revision,
                selected,
                operation_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::CreateService { input } => {
            use crate::commands::*;
            let value = create_service(
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::UpdateService { id, input } => {
            use crate::commands::*;
            let value = update_service(
                id,
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::DeleteService { id } => {
            use crate::commands::*;
            let value = delete_service(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ExportDefinitions {} => {
            use crate::commands::*;
            let value = export_definitions(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetServiceInstance { id } => {
            use crate::commands::*;
            let value = get_service_instance(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetRun { id } => {
            use crate::commands::*;
            let value = get_run(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListRuns {
            job_id,
            limit,
            start_at,
            end_at,
            status,
            kind,
            min_duration_ms,
            max_duration_ms,
        } => {
            use crate::commands::*;
            let value = list_runs(
                job_id,
                limit,
                start_at,
                end_at,
                status,
                kind,
                min_duration_ms,
                max_duration_ms,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListRunHistory { input } => {
            use crate::commands::*;
            let value = list_run_history(
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::GetActiveRun { id } => {
            use crate::commands::*;
            let value = get_active_run(
                id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListActiveRuns {} => {
            use crate::commands::*;
            let value = list_active_runs(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::PreviewCron { input } => {
            use crate::commands::*;
            let value = preview_cron(input)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::TailLog { input } => {
            use crate::commands::*;
            let value = tail_log(
                _component_app.clone(),
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::SearchRunLogs { input } => {
            use crate::commands::*;
            let value = search_run_logs(
                _component_app.clone(),
                input,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::PreviewWorkspaceTaskControl { handoff_id } => {
            use crate::task_control::*;
            let value = preview_workspace_task_control(
                handoff_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::RenewWorkspaceTaskControl { request_id } => {
            use crate::task_control::*;
            let value = renew_workspace_task_control(
                request_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::RejectWorkspaceTaskControl { request_id } => {
            use crate::task_control::*;
            let value = reject_workspace_task_control(
                request_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::AcceptWorkspaceTaskControl { request_id } => {
            use crate::task_control::*;
            let value = accept_workspace_task_control(
                request_id,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::ListWorkspaceTaskControlReceipts { limit } => {
            use crate::task_control::*;
            let value = list_workspace_task_control_receipts(
                limit,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            );
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        RuntimeCall::OpenRunLogInLogLens { run_id, stream } => {
            use crate::log_lens::*;
            let value = open_run_log_in_log_lens(
                run_id,
                stream,
                _component_app.clone(),
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
("review_runtime_control",export.register::<()>()?),
("list_runtime_controls",export.register::<Vec<crate::storage::RuntimeControlReceipt>>()?),
("runtime_control_status",export.register::<serde_json::Value>()?),
("runtime_control",export.register::<serde_json::Value>()?),
("runtime_status",export.register::<crate::lifecycle::RuntimeStatus>()?),
("show_main_window",export.register::<()>()?),
("hide_main_window",export.register::<()>()?),
("quit_app",export.register::<()>()?),
("startup_shortcut_status",export.register::<crate::platform::StartupShortcutStatus>()?),
("set_startup_shortcut_enabled",export.register::<crate::platform::StartupShortcutStatus>()?),
("list_jobs",export.register::<Vec<crate::core::models::Job>>()?),
("get_job",export.register::<Option<crate::core::models::Job>>()?),
("create_job",export.register::<crate::core::models::Job>()?),
("update_job",export.register::<crate::core::models::Job>()?),
("set_job_enabled",export.register::<crate::core::models::Job>()?),
("delete_job",export.register::<bool>()?),
("list_services",export.register::<Vec<crate::core::models::Job>>()?),
("get_service",export.register::<Option<crate::core::models::Job>>()?),
("service_observability",export.register::<Option<crate::commands::ServiceObservability>>()?),
("import_definitions",export.register::<crate::commands::ImportPlan>()?),
("apply_import",export.register::<usize>()?),
("preview_workspace_task_import",export.register::<crate::core::workspace_tasks::WorkspaceTaskPlan>()?),
("cancel_workspace_task_import",export.register::<bool>()?),
("apply_workspace_task_import",export.register::<crate::core::workspace_tasks::WorkspaceTaskApplyResult>()?),
("list_workspace_tasks",export.register::<Vec<crate::core::workspace_tasks::WorkspaceTaskState>>()?),
("trust_workspace_task_source",export.register::<bool>()?),
("trust_workspace_task_shell_source",export.register::<bool>()?),
("get_workspace_task_operation",export.register::<Option<crate::core::workspace_orchestration::WorkspaceTaskOperationView>>()?),
("list_workspace_task_operations",export.register::<Vec<crate::core::workspace_orchestration::WorkspaceTaskOperationView>>()?),
("list_workspace_task_diagnostics",export.register::<crate::core::workspace_diagnostics::WorkspaceTaskDiagnostics>()?),
("open_workspace_task_diagnostic",export.register::<bool>()?),
("preview_project_import",export.register::<crate::core::imports::ProjectImportPlan>()?),
("cancel_project_import",export.register::<bool>()?),
("apply_project_import",export.register::<crate::core::imports::ProjectImportApplyResult>()?),
("create_service",export.register::<crate::core::models::Job>()?),
("update_service",export.register::<crate::core::models::Job>()?),
("delete_service",export.register::<bool>()?),
("export_definitions",export.register::<crate::commands::DefinitionExport>()?),
("get_service_instance",export.register::<Option<crate::core::models::ServiceInstanceView>>()?),
("get_run",export.register::<Option<crate::core::models::RunView>>()?),
("list_runs",export.register::<Vec<crate::core::models::RunView>>()?),
("list_run_history",export.register::<Vec<crate::core::models::RunView>>()?),
("get_active_run",export.register::<Option<crate::core::models::RunView>>()?),
("list_active_runs",export.register::<Vec<crate::core::models::RunView>>()?),
("preview_cron",export.register::<Vec<crate::commands::CronPreviewItem>>()?),
("tail_log",export.register::<crate::logs::TailResponse>()?),
("search_run_logs",export.register::<crate::core::log_search::LogSearchResponse>()?),
("preview_workspace_task_control",export.register::<crate::core::workspace_task_control::WorkspaceTaskControlPreview>()?),
("renew_workspace_task_control",export.register::<u64>()?),
("reject_workspace_task_control",export.register::<crate::core::workspace_task_control::WorkspaceTaskControlReceipt>()?),
("accept_workspace_task_control",export.register::<crate::core::workspace_task_control::WorkspaceTaskControlReceipt>()?),
("list_workspace_task_control_receipts",export.register::<Vec<crate::core::workspace_task_control::WorkspaceTaskControlReceipt>>()?),
("take_pending_open",export.register::<Option<devbox_applink::OpenRequest>>()?),
("open_run_log_in_log_lens",export.register::<crate::log_lens::LogLensDispatch>()?),
])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_methods_and_extra_arguments_are_rejected() {
        assert!(serde_json::from_str::<RuntimeCall>(r#"{"method":"unknown","args":{}}"#).is_err());
    }
    #[test]
    fn method_names_are_unique() {
        let mut names = METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), METHODS.len());
    }
}

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    call: RuntimeCall,
) -> Result<serde_json::Value, String> {
    crate::component::data_root(app)?;
    crate::component::common_root(app)?;
    let method = call.method();
    let result = execute(app, call).await;
    crate::component::data_root(app)?;
    crate::component::common_root(app)?;
    let mut value = result?;
    if matches!(
        method,
        "list_jobs"
            | "list_services"
            | "get_job"
            | "get_service"
            | "update_job"
            | "update_service"
            | "create_job"
            | "create_service"
            | "set_job_enabled"
    ) {
        let database = app
            .try_state::<Arc<crate::storage::DatabaseState>>()
            .ok_or("component_state_unavailable")?;
        let annotate = |value: &mut serde_json::Value| -> Result<(), String> {
            if let Some(object) = value.as_object_mut() {
                let id = object
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("component_response_invalid")?;
                let required = database
                    .requires_secret_review(id)
                    .map_err(|_| "runtime_import_failed")?;
                object.insert(
                    "envReconnectRequired".into(),
                    serde_json::Value::Bool(required),
                );
            }
            Ok(())
        };
        if let Some(values) = value.as_array_mut() {
            for value in values {
                annotate(value)?;
            }
        } else {
            annotate(&mut value)?;
        }
    }
    Ok(value)
}

#[cfg(test)]
mod control_tests {
    use super::*;
    #[test]
    fn stop_is_nested_and_keeps_its_reserved_lane() {
        let call:RuntimeCall=serde_json::from_str(r#"{"method":"runtime_control","args":{"operationId":"op","method":"stop_service","args":{"id":"service"}}}"#).unwrap();
        assert_eq!(call.lane(), Lane::EngineStop);
        assert_eq!(call.deadline_budget_ms(), 29_000);
        for wire in [
            r#"{"method":"stop_service","args":{"id":"service"}}"#,
            r#"{"method":"runtime_control","args":{"operationId":"op","method":"unknown","args":{}}}"#,
            r#"{"method":"runtime_control","args":{"operationId":"op","method":"stop_service","args":{"id":"service","extra":true}}}"#,
        ] {
            assert!(serde_json::from_str::<RuntimeCall>(wire).is_err());
        }
    }
}
