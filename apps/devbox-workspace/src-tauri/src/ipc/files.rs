use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveFile {
    pub path: String,
    pub expected_mtime_nanos: String,
    pub expected_size: u64,
    pub expected_content_hash: String,
    pub native_revision: String,
    pub text: String,
    pub encoding: editor_engine::core::encoding::Encoding,
    pub line_ending: editor_engine::core::line_ending::LineEnding,
    pub source_lossy: bool,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameFile {
    pub path: String,
    pub expected_mtime_nanos: String,
    pub expected_size: u64,
    pub expected_content_hash: String,
    pub native_revision: String,
    pub new_name: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteFile {
    pub path: String,
    pub expected_mtime_nanos: String,
    pub expected_size: u64,
    pub expected_content_hash: String,
    pub native_revision: String,
}
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceFilesCall {
    PickFiles {},
    ReconnectWslFiles {},
    ReadClipboardText {},
    LoadSession {},
    LoadRecovery {},
    TakePendingOpen {},
    OpenFile {
        request: editor_engine::commands::file::OpenFileRequest,
        received_reference: Option<String>,
    },
    SaveFile {
        request: SaveFile,
    },
    RenameFileAction {
        request: RenameFile,
    },
    DeleteFileAction {
        request: DeleteFile,
    },
    SyncEditorDocument {
        path: String,
        native_revision: String,
        text: String,
    },
    SendEditorSelection {
        path: String,
        native_revision: String,
        text: String,
        from: usize,
        to: usize,
    },
    ValidateEncoding {
        request: editor_engine::commands::file::ValidateEncodingRequest,
    },
    SaveSession {
        session: editor_engine::core::session::Session,
        native_revision: String,
    },
    SaveRecovery {
        entries: Vec<editor_engine::core::recovery::RecoveryEntry>,
        native_revision: String,
    },
    DiscardRecovery {
        path: Option<String>,
        native_revision: String,
    },
    RenderPreview {
        path: String,
        content: String,
        workspace_root: String,
    },
    UnwatchFile {
        path: String,
        document_context: Option<product_contract::ProjectContext>,
    },
    RevealFileAction {
        path: String,
    },
    ListWorkspaceFiles {
        path: String,
    },
    CanonicalizeWorkspace {
        path: String,
    },
    WorkspaceCapabilities {
        path: String,
    },
    PrepareRecovery {
        path: String,
    },
    WatchFile {
        path: String,
    },
    ApplyRecoveryPreview {
        preview_id: String,
    },
    CancelRecoveryPreview {
        preview_id: String,
    },
}
pub const METHODS: &[&str] = &[
    "apply_recovery_preview",
    "cancel_recovery_preview",
    "canonicalize_workspace",
    "delete_file_action",
    "discard_recovery",
    "list_workspace_files",
    "load_recovery",
    "load_session",
    "open_file",
    "pick_files",
    "prepare_recovery",
    "read_clipboard_text",
    "reconnect_wsl_files",
    "rename_file_action",
    "render_preview",
    "reveal_file_action",
    "save_file",
    "save_recovery",
    "save_session",
    "send_editor_selection",
    "sync_editor_document",
    "take_pending_open",
    "unwatch_file",
    "validate_encoding",
    "watch_file",
    "workspace_capabilities",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "apply_recovery_preview" => &["files"],
        "cancel_recovery_preview" => &["files"],
        "canonicalize_workspace" => &["files"],
        "delete_file_action" => &["files"],
        "discard_recovery" => &["files"],
        "list_workspace_files" => &["files"],
        "load_recovery" => &["files"],
        "load_session" => &["files"],
        "open_file" => &["files"],
        "pick_files" => &["files"],
        "prepare_recovery" => &["files"],
        "read_clipboard_text" => &["files"],
        "reconnect_wsl_files" => &["files"],
        "rename_file_action" => &["files"],
        "render_preview" => &["files"],
        "reveal_file_action" => &["files"],
        "save_file" => &["files"],
        "save_recovery" => &["files"],
        "save_session" => &["files"],
        "send_editor_selection" => &["files"],
        "sync_editor_document" => &["files"],
        "take_pending_open" => &["files"],
        "unwatch_file" => &["files"],
        "validate_encoding" => &["files"],
        "watch_file" => &["files"],
        "workspace_capabilities" => &["files"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceFilesCall {
    const COMPONENT: &'static str = "workspace.files";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 67108864;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let _ = method;
        let limit = Self::MAX_ARGUMENT_BYTES;
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::PickFiles { .. } => "pick_files",
            Self::ReconnectWslFiles { .. } => "reconnect_wsl_files",
            Self::ReadClipboardText { .. } => "read_clipboard_text",
            Self::LoadSession { .. } => "load_session",
            Self::LoadRecovery { .. } => "load_recovery",
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::OpenFile { .. } => "open_file",
            Self::SaveFile { .. } => "save_file",
            Self::RenameFileAction { .. } => "rename_file_action",
            Self::DeleteFileAction { .. } => "delete_file_action",
            Self::SyncEditorDocument { .. } => "sync_editor_document",
            Self::SendEditorSelection { .. } => "send_editor_selection",
            Self::ValidateEncoding { .. } => "validate_encoding",
            Self::SaveSession { .. } => "save_session",
            Self::SaveRecovery { .. } => "save_recovery",
            Self::DiscardRecovery { .. } => "discard_recovery",
            Self::RenderPreview { .. } => "render_preview",
            Self::UnwatchFile { .. } => "unwatch_file",
            Self::RevealFileAction { .. } => "reveal_file_action",
            Self::ListWorkspaceFiles { .. } => "list_workspace_files",
            Self::CanonicalizeWorkspace { .. } => "canonicalize_workspace",
            Self::WorkspaceCapabilities { .. } => "workspace_capabilities",
            Self::PrepareRecovery { .. } => "prepare_recovery",
            Self::WatchFile { .. } => "watch_file",
            Self::ApplyRecoveryPreview { .. } => "apply_recovery_preview",
            Self::CancelRecoveryPreview { .. } => "cancel_recovery_preview",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl WorkspaceFilesCall {
    pub fn lane(&self) -> Lane {
        Lane::Files
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.files", method)
}
#[tauri::command]
pub(crate) async fn files(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceFilesCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceFilesCall {
    fn into_call(self) -> super::Call {
        super::Call::Files(self)
    }
}

use super::empty;
use super::Request;
use crate::component::Runtime;
use serde_json::{json, Value};
use tauri::{Manager, WebviewWindow};

pub(crate) async fn execute_files(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    use tauri_plugin_dialog::DialogExt;
    let lane = request.typed.lane();
    // Native file/WSL ownership consumes the original argument tree. The
    // validated DTO must not retain a second large buffer in the Files queue.
    drop(request.typed);
    let queued = runtime.lanes.try_enter(lane)?;
    let mut deadline = request.header.deadline_ms;
    let chosen = if request.method == "pick_files" {
        empty(&request.args)?;
        let dialog = runtime.lanes.try_enter(Lane::Dialogs)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(window)
            .pick_files(move |paths| {
                // Retain the sole dialog slot until the native chooser closes.
                let _dialog = dialog;
                let _ = sender.send(paths);
            });
        let Some(paths) = receiver.await.map_err(|_| "file_dialog_unavailable")? else {
            return Ok(json!([]));
        };
        // The explicit native user choice can outlive the original RPC's
        // deadline. Re-admit the same installation/session/context/window with
        // a native request before using any returned path or saving a choice.
        let mut admitted = request.header.clone();
        admitted.request_id = uuid::Uuid::new_v4().to_string();
        admitted.deadline_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64
            + 5000;
        product_shell_tauri::authorize(window, &admitted, "workspace.files")
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
    let filesystem = match file_access(&request.method) {
        Some(write) => Some(runtime.filesystem_permit(write, deadline).await?),
        None => None,
    };
    // The context permit prevents a project switch while waiting. A closed or
    // replaced window must still not admit a delayed file operation.
    if product_shell_tauri::workspace_context(window).map_err(|_| "file_context_changed")?
        != request.header.context
    {
        return Err("file_context_changed");
    }
    crate::files_host::current_deadline(deadline)?;
    let worker = runtime
        .lanes
        .workers(Lane::Files)
        .acquire_owned()
        .await
        .map_err(|_| "worker_unavailable")?;
    let files = runtime.files.clone();
    let host = runtime.host()?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_queued, _worker, _context, _filesystem) =
            (queued, worker, context_permit, filesystem);
        let mut files = files.lock().map_err(|_| "files_unavailable")?;
        crate::files_host::current_deadline(deadline)?;
        files.initialize(&app, &host)?;
        if let Some(paths) = chosen {
            files.approve_native_selection(&paths)
        } else {
            files.execute(
                &app,
                &host,
                crate::files_host::Invocation {
                    context: request.header.context.as_ref(),
                    component: &request.component,
                    method: &request.method,
                    args: request.args,
                    deadline,
                },
            )
        }
    })
    .await
    .unwrap_or(Err("worker_unavailable"))
}

pub(crate) fn file_access(method: &str) -> Option<bool> {
    match method {
        "save_file" | "rename_file_action" | "delete_file_action" | "apply_recovery_preview" => {
            Some(true)
        }
        "sync_editor_document"
        | "load_session"
        | "save_session"
        | "load_recovery"
        | "save_recovery"
        | "discard_recovery"
        | "cancel_recovery_preview"
        | "take_pending_open"
        | "read_clipboard_text"
        | "validate_encoding"
        | "lsp_catalog"
        | "lsp_installed"
        | "load_lsp_config"
        | "preview_lsp_config_restore" => None,
        _ => Some(false),
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FileDocument<T> {
    #[serde(flatten)]
    pub document: T,
    pub native_revision: Option<String>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FilesSession {
    pub session: editor_engine::core::session::Session,
    pub persist_allowed: bool,
    pub native_revision: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FilesRecovery {
    pub entries: Vec<editor_engine::core::recovery::RecoveryEntry>,
    pub native_revision: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FilesRevision {
    pub native_revision: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FilesRecoveryPreview {
    pub preview_id: String,
    pub path: String,
    pub before: String,
    pub after: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSent {
    pub handoff_id: String,
    pub redacted: bool,
    pub receipt: serde_json::Value,
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<WorkspaceFilesCall>()?;
    let mut result = editor_engine::api::files_result_types(export)?;
    result.retain(|(method, _)| METHODS.contains(method));
    result.retain(|(method, _)| *method != "pick_files");
    result.push(("pick_files", export.register::<Vec<String>>()?));
    result.retain(|(method, _)| *method != "reconnect_wsl_files");
    result.push(("reconnect_wsl_files", export.register::<()>()?));
    result.retain(|(method, _)| *method != "sync_editor_document");
    result.push(("sync_editor_document", export.register::<bool>()?));
    result.retain(|(method, _)| *method != "send_editor_selection");
    result.push(("send_editor_selection", export.register::<SelectionSent>()?));
    result.retain(|(method, _)| *method != "open_file");
    result.push((
        "open_file",
        export.register::<FileDocument<editor_engine::commands::file::OpenedFileWire>>()?,
    ));
    result.retain(|(method, _)| *method != "save_file");
    result.push((
        "save_file",
        export.register::<FileDocument<editor_engine::commands::file::SavedFileWire>>()?,
    ));
    result.retain(|(method, _)| *method != "rename_file_action");
    result.push((
        "rename_file_action",
        export.register::<FileDocument<editor_engine::commands::file::RenamedFileWire>>()?,
    ));
    result.retain(|(method, _)| *method != "load_session");
    result.push(("load_session", export.register::<FilesSession>()?));
    result.retain(|(method, _)| *method != "save_session");
    result.push(("save_session", export.register::<FilesRevision>()?));
    result.retain(|(method, _)| *method != "load_recovery");
    result.push(("load_recovery", export.register::<FilesRecovery>()?));
    result.retain(|(method, _)| *method != "save_recovery");
    result.push(("save_recovery", export.register::<FilesRevision>()?));
    result.retain(|(method, _)| *method != "discard_recovery");
    result.push(("discard_recovery", export.register::<FilesRevision>()?));
    result.retain(|(method, _)| *method != "prepare_recovery");
    result.push((
        "prepare_recovery",
        export.register::<FilesRecoveryPreview>()?,
    ));
    result.retain(|(method, _)| *method != "apply_recovery_preview");
    result.push((
        "apply_recovery_preview",
        export.register::<editor_engine::commands::file::SavedFileWire>()?,
    ));
    result.retain(|(method, _)| *method != "cancel_recovery_preview");
    result.push(("cancel_recovery_preview", export.register::<()>()?));
    result.retain(|(method, _)| *method != "read_clipboard_text");
    result.push(("read_clipboard_text", export.register::<String>()?));
    result.sort_by_key(|(method, _)| *method);
    Ok(result)
}
