use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceLspCall {
    LspCatalog {},
    LspInstalled {},
    LspRecoverInstalled {},
    PickLspArchives {},
    LoadLspConfig {},
    LspRecoveryList {},
    LspExecutionPreview {},
    LspExecutionRevoke {},
    LanguageServerStatuses {},
    LanguageServerLogs {},
    StartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    RestartLanguageServer {
        language_id: String,
        operation_id: String,
    },
    StopLanguageServer {
        language_id: String,
        operation_id: Option<String>,
    },
    StopAllLanguageServers {
        #[serde(default)]
        #[ts(as = "Option<Vec<String>>", optional)]
        operation_ids: Vec<String>,
    },
    SaveLspConfig {
        #[ts(as = "editor_engine::lsp::LspConfig")]
        config: serde_json::Value,
        recover_invalid: bool,
        native_revision: String,
    },
    LspInstall {
        manifest_id: String,
        version: String,
        platform: String,
    },
    LspUninstall {
        manifest_id: String,
        version: String,
        platform: String,
    },
    LspImportArchive {
        manifest_id: String,
        version: String,
        platform: String,
        archive_paths: Vec<String>,
    },
    DiscardLspArchives {
        archive_paths: Vec<String>,
    },
    LspRecoveryPreview {
        journal_id: String,
    },
    LspRecoveryApply {
        preview_id: String,
    },
    LspRecoveryCancel {
        preview_id: String,
    },
    LspExecutionApprove {
        preview_id: String,
    },
    LspExecutionCancel {
        preview_id: String,
    },

    RequestLspRename {
        language_id: String,
        uri: String,
        position: editor_engine::lsp::LspPosition,
        new_name: String,
    },
    ApplyLspRename {
        plan_id: String,
    },
    CancelLspRename {
        plan_id: String,
    },
    DiscardLspRename {
        plan_id: String,
    },
    OpenLspDocument {
        language_id: String,
        path: String,
        text: String,
        native_revision: String,
    },
    ChangeLspDocument {
        language_id: String,
        uri: String,
        text: String,
        dirty: bool,
        native_revision: String,
    },
    ReloadLspDocument {
        language_id: String,
        uri: String,
        text: String,
        native_revision: String,
    },
    SaveLspDocument {
        language_id: String,
        uri: String,
        native_revision: String,
        #[serde(default)]
        text: Option<String>,
    },
    CloseLspDocument {
        language_id: String,
        uri: String,
    },
    PullLspDiagnostics {
        language_id: String,
        uri: String,
    },
    RequestLspCompletion {
        language_id: String,
        uri: String,
        position: editor_engine::lsp::LspPosition,
    },
    RequestLspHover {
        language_id: String,
        uri: String,
        position: editor_engine::lsp::LspPosition,
    },
    RequestLspDefinition {
        language_id: String,
        uri: String,
        position: editor_engine::lsp::LspPosition,
    },
    RequestLspReferences {
        language_id: String,
        uri: String,
        position: editor_engine::lsp::LspPosition,
        include_declaration: bool,
    },
    RequestLspFormatting {
        language_id: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    },
}
pub const METHODS: &[&str] = &[
    "apply_lsp_rename",
    "cancel_lsp_rename",
    "change_lsp_document",
    "close_lsp_document",
    "discard_lsp_archives",
    "discard_lsp_rename",
    "language_server_logs",
    "language_server_statuses",
    "load_lsp_config",
    "lsp_catalog",
    "lsp_execution_approve",
    "lsp_execution_cancel",
    "lsp_execution_preview",
    "lsp_execution_revoke",
    "lsp_import_archive",
    "lsp_install",
    "lsp_installed",
    "lsp_recover_installed",
    "lsp_recovery_apply",
    "lsp_recovery_cancel",
    "lsp_recovery_list",
    "lsp_recovery_preview",
    "lsp_uninstall",
    "open_lsp_document",
    "pick_lsp_archives",
    "pull_lsp_diagnostics",
    "reload_lsp_document",
    "request_lsp_completion",
    "request_lsp_definition",
    "request_lsp_formatting",
    "request_lsp_hover",
    "request_lsp_references",
    "request_lsp_rename",
    "restart_language_server",
    "save_lsp_config",
    "save_lsp_document",
    "start_language_server",
    "stop_all_language_servers",
    "stop_language_server",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "apply_lsp_rename" => &["files"],
        "cancel_lsp_rename" => &["files"],
        "change_lsp_document" => &["files"],
        "close_lsp_document" => &["files"],
        "discard_lsp_archives" => &["files"],
        "discard_lsp_rename" => &["files"],
        "language_server_logs" => &["files"],
        "language_server_statuses" => &["files"],
        "load_lsp_config" => &["files"],
        "lsp_catalog" => &["files"],
        "lsp_execution_approve" => &["files"],
        "lsp_execution_cancel" => &["files"],
        "lsp_execution_preview" => &["files"],
        "lsp_execution_revoke" => &["files"],
        "lsp_import_archive" => &["files"],
        "lsp_install" => &["files"],
        "lsp_installed" => &["files"],
        "lsp_recover_installed" => &["files"],
        "lsp_recovery_apply" => &["files"],
        "lsp_recovery_cancel" => &["files"],
        "lsp_recovery_list" => &["files"],
        "lsp_recovery_preview" => &["files"],
        "lsp_uninstall" => &["files"],
        "open_lsp_document" => &["files"],
        "pick_lsp_archives" => &["files"],
        "pull_lsp_diagnostics" => &["files"],
        "reload_lsp_document" => &["files"],
        "request_lsp_completion" => &["files"],
        "request_lsp_definition" => &["files"],
        "request_lsp_formatting" => &["files"],
        "request_lsp_hover" => &["files"],
        "request_lsp_references" => &["files"],
        "request_lsp_rename" => &["files"],
        "restart_language_server" => &["files"],
        "save_lsp_config" => &["files"],
        "save_lsp_document" => &["files"],
        "start_language_server" => &["files"],
        "stop_all_language_servers" => &["files"],
        "stop_language_server" => &["files"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceLspCall {
    const COMPONENT: &'static str = "workspace.lsp";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let limit = if crate::lsp_host::text_request(method) {
            64 * 1024 * 1024
        } else {
            Self::MAX_ARGUMENT_BYTES
        };
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::LspCatalog { .. } => "lsp_catalog",
            Self::LspInstalled { .. } => "lsp_installed",
            Self::LspRecoverInstalled { .. } => "lsp_recover_installed",
            Self::PickLspArchives { .. } => "pick_lsp_archives",
            Self::LoadLspConfig { .. } => "load_lsp_config",
            Self::LspRecoveryList { .. } => "lsp_recovery_list",
            Self::LspExecutionPreview { .. } => "lsp_execution_preview",
            Self::LspExecutionRevoke { .. } => "lsp_execution_revoke",
            Self::LanguageServerStatuses { .. } => "language_server_statuses",
            Self::LanguageServerLogs { .. } => "language_server_logs",
            Self::StartLanguageServer { .. } => "start_language_server",
            Self::RestartLanguageServer { .. } => "restart_language_server",
            Self::StopLanguageServer { .. } => "stop_language_server",
            Self::StopAllLanguageServers { .. } => "stop_all_language_servers",
            Self::SaveLspConfig { .. } => "save_lsp_config",
            Self::LspInstall { .. } => "lsp_install",
            Self::LspUninstall { .. } => "lsp_uninstall",
            Self::LspImportArchive { .. } => "lsp_import_archive",
            Self::DiscardLspArchives { .. } => "discard_lsp_archives",
            Self::LspRecoveryPreview { .. } => "lsp_recovery_preview",
            Self::LspRecoveryApply { .. } => "lsp_recovery_apply",
            Self::LspRecoveryCancel { .. } => "lsp_recovery_cancel",
            Self::LspExecutionApprove { .. } => "lsp_execution_approve",
            Self::LspExecutionCancel { .. } => "lsp_execution_cancel",
            Self::RequestLspRename { .. } => "request_lsp_rename",
            Self::ApplyLspRename { .. } => "apply_lsp_rename",
            Self::CancelLspRename { .. } => "cancel_lsp_rename",
            Self::DiscardLspRename { .. } => "discard_lsp_rename",
            Self::OpenLspDocument { .. } => "open_lsp_document",
            Self::ChangeLspDocument { .. } => "change_lsp_document",
            Self::ReloadLspDocument { .. } => "reload_lsp_document",
            Self::SaveLspDocument { .. } => "save_lsp_document",
            Self::CloseLspDocument { .. } => "close_lsp_document",
            Self::PullLspDiagnostics { .. } => "pull_lsp_diagnostics",
            Self::RequestLspCompletion { .. } => "request_lsp_completion",
            Self::RequestLspHover { .. } => "request_lsp_hover",
            Self::RequestLspDefinition { .. } => "request_lsp_definition",
            Self::RequestLspReferences { .. } => "request_lsp_references",
            Self::RequestLspFormatting { .. } => "request_lsp_formatting",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl WorkspaceLspCall {
    pub fn lane(&self) -> Lane {
        if crate::lsp_host::stops(self.method()) {
            Lane::LspStop
        } else {
            Lane::Lsp
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.lsp", method)
}
#[tauri::command]
pub(crate) async fn lsp(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceLspCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceLspCall {
    fn into_call(self) -> super::Call {
        super::Call::Lsp(self)
    }
}

use super::empty;
use super::Request;
use crate::component::Runtime;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{Manager, WebviewWindow};

pub(crate) async fn execute_lsp(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: Option<crate::core::context_activity::ContextPermit>,
) -> Result<Value, &'static str> {
    use tauri_plugin_dialog::DialogExt;
    let lane = request.typed.lane();
    // The established LSP owner consumes the wire value after context/grant
    // checks. Retain only that copy while it waits for an actor or worker.
    drop(request.typed);
    let queued = runtime.lanes.try_enter(lane)?;
    let (operation_id, cancelled) =
        crate::lsp_host::admission(&request.method, request.args.clone())?;
    let context_key =
        serde_json::to_string(&request.header.context).map_err(|_| "invalid_request")?;
    for id in cancelled {
        runtime.lsp_operations.cancel(&context_key, &id)?;
    }
    let start = operation_id
        .as_deref()
        .map(|id| runtime.lsp_operations.register(&context_key, Some(id)))
        .transpose()
        .map_err(|issue| {
            if issue == "source_cancelled" {
                "lsp_operation_cancelled"
            } else {
                issue
            }
        })?;
    let _cancel = start.as_ref().map(|start| start.cancel_on_drop());
    if runtime.lsp_shutdown.is_cancelled() {
        return Err("lsp_operation_cancelled");
    }
    let mut deadline = request.header.deadline_ms;
    let chosen = if request.method == "pick_lsp_archives" {
        empty(&request.args)?;
        let dialog = runtime.lanes.try_enter(Lane::Dialogs)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(window)
            .set_title("관리형 LSP archive 선택")
            .add_filter("LSP archive", &["zip", "tgz", "gz"])
            .pick_files(move |paths| {
                let _dialog = dialog;
                let _ = sender.send(paths);
            });
        let Some(paths) = receiver.await.map_err(|_| "file_dialog_unavailable")? else {
            return Ok(json!([]));
        };
        let mut admitted = request.header.clone();
        admitted.request_id = uuid::Uuid::new_v4().to_string();
        admitted.deadline_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64
            + 29_000;
        product_shell_tauri::authorize(window, &admitted, "workspace.lsp")
            .map_err(|_| "file_selection_expired")?;
        deadline = admitted.deadline_ms;
        Some(
            paths
                .into_iter()
                .map(|path| path.into_path().map_err(|_| "invalid_file_path"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let worker = if crate::lsp_host::worker_required(&request.method) {
        Some(
            runtime
                .lanes
                .workers(Lane::Lsp)
                .acquire_owned()
                .await
                .map_err(|_| "worker_unavailable")?,
        )
    } else {
        None
    };
    let runtime = runtime.clone();
    let app = window.app_handle().clone();
    let host = runtime.host()?;
    tauri::async_runtime::spawn_blocking(move || {
        let (_queued, _worker, _context) = (queued, worker, context_permit);
        crate::files_host::current_deadline(deadline)?;
        if runtime.lsp_shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        let owner = {
            let mut slot = runtime.lsp.lock().map_err(|_| "lsp_unavailable")?;
            if slot.is_none() {
                // Files and LSP share only initialization. No installer/server
                // wait holds the editor's mutex or filesystem/context permit.
                let mut files = runtime.files.try_lock().map_err(|_| "files_unavailable")?;
                files.initialize(&app, &host)?;
                *slot = Some(Arc::new(crate::lsp_host::LspHost::new(
                    &app,
                    &host,
                    runtime.context_activity.clone(),
                    runtime.filesystem_activity.clone(),
                    runtime.files.clone(),
                )?));
            }
            slot.as_ref().ok_or("lsp_unavailable")?.clone()
        };
        crate::files_host::current_deadline(deadline)?;
        if runtime.lsp_shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        if let Some(paths) = chosen {
            owner.choose(&host, &paths, deadline)
        } else if crate::lsp_host::review_method(&request.method) {
            owner.execute_review(
                &app,
                &host,
                &request.method,
                request.args,
                request.header.context.as_ref(),
                deadline,
            )
        } else {
            tauri::async_runtime::block_on(owner.execute(
                &app,
                &host,
                crate::lsp_host::Invocation {
                    method: &request.method,
                    args: request.args,
                    context: request.header.context.as_ref(),
                    deadline,
                    start,
                },
                &runtime.lsp_shutdown,
            ))
        }
    })
    .await
    .unwrap_or(Err("worker_unavailable"))
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct WorkspaceLspConfig {
    pub config: editor_engine::lsp::LspConfig,
    pub persist_allowed: bool,
    #[serde(rename = "recoveryAllowed")]
    pub recovery_allowed: bool,
    pub error: Option<String>,
    #[serde(rename = "nativeRevision")]
    pub native_revision: Option<String>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LspRecoveryPreview {
    pub preview_id: String,
    pub journal_id: String,
    pub files: Vec<editor_engine::lsp::manager::recovery::RenameRecoveryFile>,
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<WorkspaceLspCall>()?;
    let mut result = editor_engine::api::lsp_result_types(export)?;
    result.retain(|(method, _)| METHODS.contains(method));
    result.retain(|(method, _)| *method != "load_lsp_config");
    result.push(("load_lsp_config", export.register::<WorkspaceLspConfig>()?));
    result.retain(|(method, _)| *method != "pick_lsp_archives");
    result.push(("pick_lsp_archives", export.register::<Vec<String>>()?));
    result.retain(|(method, _)| *method != "discard_lsp_archives");
    result.push(("discard_lsp_archives", export.register::<()>()?));
    result.retain(|(method, _)| *method != "lsp_recovery_list");
    result.push((
        "lsp_recovery_list",
        export.register::<editor_engine::lsp::manager::recovery::RenameRecoveryListing>()?,
    ));
    result.retain(|(method, _)| *method != "lsp_recovery_preview");
    result.push((
        "lsp_recovery_preview",
        export.register::<LspRecoveryPreview>()?,
    ));
    result.retain(|(method, _)| *method != "lsp_recovery_apply");
    result.push((
        "lsp_recovery_apply",
        export.register::<editor_engine::lsp::manager::recovery::RenameRecoveryResult>()?,
    ));
    result.retain(|(method, _)| *method != "lsp_recovery_cancel");
    result.push(("lsp_recovery_cancel", export.register::<()>()?));
    result.retain(|(method, _)| *method != "lsp_execution_preview");
    result.push((
        "lsp_execution_preview",
        export.register::<LspExecutionPreview>()?,
    ));
    result.retain(|(method, _)| *method != "lsp_execution_approve");
    result.push(("lsp_execution_approve", export.register::<()>()?));
    result.retain(|(method, _)| *method != "lsp_execution_cancel");
    result.push(("lsp_execution_cancel", export.register::<()>()?));
    result.retain(|(method, _)| *method != "lsp_execution_revoke");
    result.push(("lsp_execution_revoke", export.register::<()>()?));
    result.sort_by_key(|(method, _)| *method);
    Ok(result)
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LspExecutionPreview {
    pub preview_id: String,
    pub approved: bool,
    pub workspace_root: String,
    pub config_revision: String,
    pub commands: Vec<LspCommandReview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub environment: Option<std::collections::BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub environment_keys: Option<Vec<String>>,
    pub definitions_digest: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LspCommandReview {
    pub language_id: String,
    pub executable: String,
    pub args: Vec<String>,
    pub runtime: Option<String>,
}
