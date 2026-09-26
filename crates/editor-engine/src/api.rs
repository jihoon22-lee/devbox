//! Typed editor calls. Workspace retains all document and execution grants.
use product_ipc::workspace::{Lane, DEFAULT_BUDGET_MS, LONG_BUDGET_MS};
use tauri::Manager as _;
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum FilesCall {
    TakePendingOpen {},
    RenderPreview {
        path: String,
        content: String,
        workspace_root: String,
    },
    OpenFile {
        request: crate::commands::file::OpenFileRequest,
    },
    SaveFile {
        request: crate::commands::file::SaveFileRequest,
    },
    RenameFileAction {
        request: crate::commands::file::RenameFileRequest,
    },
    DeleteFileAction {
        request: crate::commands::file::FileActionRequest,
    },
    RevealFileAction {
        path: String,
    },
    ValidateEncoding {
        request: crate::commands::file::ValidateEncodingRequest,
    },
    WatchFile {
        path: String,
    },
    UnwatchFile {
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
    LoadSession {},
    SaveSession {
        session: crate::core::session::Session,
    },
    SaveRecovery {
        entries: Vec<crate::core::recovery::RecoveryEntry>,
    },
    LoadRecovery {},
    DiscardRecovery {
        path: Option<String>,
    },
    ApplyRecovery {
        path: String,
        content: String,
    },
}
impl FilesCall {
    pub const METHODS: &'static [&'static str] = &[
        "take_pending_open",
        "render_preview",
        "open_file",
        "save_file",
        "rename_file_action",
        "delete_file_action",
        "reveal_file_action",
        "validate_encoding",
        "watch_file",
        "unwatch_file",
        "list_workspace_files",
        "canonicalize_workspace",
        "workspace_capabilities",
        "load_session",
        "save_session",
        "save_recovery",
        "load_recovery",
        "discard_recovery",
        "apply_recovery",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::RenderPreview { .. } => "render_preview",
            Self::OpenFile { .. } => "open_file",
            Self::SaveFile { .. } => "save_file",
            Self::RenameFileAction { .. } => "rename_file_action",
            Self::DeleteFileAction { .. } => "delete_file_action",
            Self::RevealFileAction { .. } => "reveal_file_action",
            Self::ValidateEncoding { .. } => "validate_encoding",
            Self::WatchFile { .. } => "watch_file",
            Self::UnwatchFile { .. } => "unwatch_file",
            Self::ListWorkspaceFiles { .. } => "list_workspace_files",
            Self::CanonicalizeWorkspace { .. } => "canonicalize_workspace",
            Self::WorkspaceCapabilities { .. } => "workspace_capabilities",
            Self::LoadSession { .. } => "load_session",
            Self::SaveSession { .. } => "save_session",
            Self::SaveRecovery { .. } => "save_recovery",
            Self::LoadRecovery { .. } => "load_recovery",
            Self::DiscardRecovery { .. } => "discard_recovery",
            Self::ApplyRecovery { .. } => "apply_recovery",
        }
    }
    pub fn lane(&self) -> Lane {
        Lane::Files
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        if self.method() == "open_file" {
            LONG_BUDGET_MS
        } else {
            DEFAULT_BUDGET_MS
        }
    }
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
pub enum LspCall {
    LoadLspConfig {},
    SaveLspConfig {
        config: crate::lsp::catalog::LspConfig,
        recover_invalid: bool,
    },
    StartLanguageServer {
        language_id: String,
    },
    StopLanguageServer {
        language_id: String,
    },
    RestartLanguageServer {
        language_id: String,
    },
    StopAllLanguageServers {},
    LanguageServerStatuses {},
    LanguageServerLogs {},
    OpenLspDocument {
        language_id: String,
        path: String,
        text: String,
    },
    ChangeLspDocument {
        language_id: String,
        uri: String,
        text: String,
        dirty: bool,
    },
    ReloadLspDocument {
        language_id: String,
        uri: String,
        text: String,
    },
    SaveLspDocument {
        language_id: String,
        uri: String,
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
        position: crate::lsp::positions::LspPosition,
    },
    RequestLspHover {
        language_id: String,
        uri: String,
        position: crate::lsp::positions::LspPosition,
    },
    RequestLspDefinition {
        language_id: String,
        uri: String,
        position: crate::lsp::positions::LspPosition,
    },
    RequestLspReferences {
        language_id: String,
        uri: String,
        position: crate::lsp::positions::LspPosition,
        include_declaration: bool,
    },
    RequestLspRename {
        language_id: String,
        uri: String,
        position: crate::lsp::positions::LspPosition,
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
    RequestLspFormatting {
        language_id: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    },
    LspCatalog {},
    LspInstalled {},
    LspRecoverInstalled {},
    LspInstall {
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
    LspUninstall {
        manifest_id: String,
        version: String,
        platform: String,
    },
}
impl LspCall {
    pub const METHODS: &'static [&'static str] = &[
        "load_lsp_config",
        "save_lsp_config",
        "start_language_server",
        "stop_language_server",
        "restart_language_server",
        "stop_all_language_servers",
        "language_server_statuses",
        "language_server_logs",
        "open_lsp_document",
        "change_lsp_document",
        "reload_lsp_document",
        "save_lsp_document",
        "close_lsp_document",
        "pull_lsp_diagnostics",
        "request_lsp_completion",
        "request_lsp_hover",
        "request_lsp_definition",
        "request_lsp_references",
        "request_lsp_rename",
        "apply_lsp_rename",
        "cancel_lsp_rename",
        "discard_lsp_rename",
        "request_lsp_formatting",
        "lsp_catalog",
        "lsp_installed",
        "lsp_recover_installed",
        "lsp_install",
        "lsp_import_archive",
        "lsp_uninstall",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::LoadLspConfig { .. } => "load_lsp_config",
            Self::SaveLspConfig { .. } => "save_lsp_config",
            Self::StartLanguageServer { .. } => "start_language_server",
            Self::StopLanguageServer { .. } => "stop_language_server",
            Self::RestartLanguageServer { .. } => "restart_language_server",
            Self::StopAllLanguageServers { .. } => "stop_all_language_servers",
            Self::LanguageServerStatuses { .. } => "language_server_statuses",
            Self::LanguageServerLogs { .. } => "language_server_logs",
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
            Self::RequestLspRename { .. } => "request_lsp_rename",
            Self::ApplyLspRename { .. } => "apply_lsp_rename",
            Self::CancelLspRename { .. } => "cancel_lsp_rename",
            Self::DiscardLspRename { .. } => "discard_lsp_rename",
            Self::RequestLspFormatting { .. } => "request_lsp_formatting",
            Self::LspCatalog { .. } => "lsp_catalog",
            Self::LspInstalled { .. } => "lsp_installed",
            Self::LspRecoverInstalled { .. } => "lsp_recover_installed",
            Self::LspInstall { .. } => "lsp_install",
            Self::LspImportArchive { .. } => "lsp_import_archive",
            Self::LspUninstall { .. } => "lsp_uninstall",
        }
    }
    pub fn lane(&self) -> Lane {
        if matches!(
            self,
            Self::StopLanguageServer { .. }
                | Self::StopAllLanguageServers { .. }
                | Self::CancelLspRename { .. }
                | Self::DiscardLspRename { .. }
        ) {
            Lane::LspStop
        } else {
            Lane::Lsp
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
pub async fn dispatch_files(
    _component_app: &tauri::AppHandle,
    call: FilesCall,
) -> Result<serde_json::Value, String> {
    if crate::component::is_product() && matches!(call, FilesCall::ApplyRecovery { .. }) {
        return Err("recovery_review_required".into());
    }
    match call {
        FilesCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            );
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::RenderPreview {
            path,
            content,
            workspace_root,
        } => {
            use crate::commands::preview::*;
            let value = render_preview(path, content, workspace_root).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::OpenFile { request } => {
            use crate::commands::file::*;
            let value = open_file(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::SaveFile { request } => {
            use crate::commands::file::*;
            let value = save_file(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::RenameFileAction { request } => {
            use crate::commands::file::*;
            let value = rename_file_action(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::DeleteFileAction { request } => {
            use crate::commands::file::*;
            delete_file_action(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::RevealFileAction { path } => {
            use crate::commands::file::*;
            reveal_file_action(_component_app.clone(), path).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::ValidateEncoding { request } => {
            use crate::commands::file::*;
            validate_encoding(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::WatchFile { path } => {
            use crate::commands::watch::*;
            watch_file(
                path,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::UnwatchFile { path } => {
            use crate::commands::watch::*;
            unwatch_file(
                path,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::ListWorkspaceFiles { path } => {
            use crate::commands::folder::*;
            let value = list_workspace_files(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::CanonicalizeWorkspace { path } => {
            use crate::commands::folder::*;
            let value = canonicalize_workspace(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::WorkspaceCapabilities { path } => {
            use crate::commands::folder::*;
            let value = workspace_capabilities(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::LoadSession {} => {
            use crate::commands::session::*;
            let value = load_session(_component_app.clone()).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::SaveSession { session } => {
            use crate::commands::session::*;
            save_session(_component_app.clone(), session).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::SaveRecovery { entries } => {
            use crate::commands::recovery::*;
            save_recovery(_component_app.clone(), entries)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::LoadRecovery {} => {
            use crate::commands::recovery::*;
            let value = load_recovery(_component_app.clone());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::DiscardRecovery { path } => {
            use crate::commands::recovery::*;
            discard_recovery(_component_app.clone(), path)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        FilesCall::ApplyRecovery { path, content } => {
            use crate::commands::recovery::*;
            apply_recovery(path, content)?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub async fn dispatch_lsp(
    _component_app: &tauri::AppHandle,
    call: LspCall,
) -> Result<serde_json::Value, String> {
    match call {
        LspCall::LoadLspConfig {} => {
            use crate::commands::lsp::*;
            let value = load_lsp_config(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::SaveLspConfig {
            config,
            recover_invalid,
        } => {
            use crate::commands::lsp::*;
            save_lsp_config(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                config,
                recover_invalid,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::StartLanguageServer { language_id } => {
            use crate::commands::lsp::*;
            start_language_server(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::StopLanguageServer { language_id } => {
            use crate::commands::lsp::*;
            stop_language_server(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RestartLanguageServer { language_id } => {
            use crate::commands::lsp::*;
            restart_language_server(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::StopAllLanguageServers {} => {
            use crate::commands::lsp::*;
            stop_all_language_servers(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LanguageServerStatuses {} => {
            use crate::commands::lsp::*;
            let value = language_server_statuses(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LanguageServerLogs {} => {
            use crate::commands::lsp::*;
            let value = language_server_logs(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::OpenLspDocument {
            language_id,
            path,
            text,
        } => {
            use crate::commands::lsp::*;
            let value = open_lsp_document(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                path,
                text,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::ChangeLspDocument {
            language_id,
            uri,
            text,
            dirty,
        } => {
            use crate::commands::lsp::*;
            let value = change_lsp_document(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                text,
                dirty,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::ReloadLspDocument {
            language_id,
            uri,
            text,
        } => {
            use crate::commands::lsp::*;
            let value = reload_lsp_document(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                text,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::SaveLspDocument { language_id, uri } => {
            use crate::commands::lsp::*;
            let value = save_lsp_document(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::CloseLspDocument { language_id, uri } => {
            use crate::commands::lsp::*;
            let value = close_lsp_document(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::PullLspDiagnostics { language_id, uri } => {
            use crate::commands::lsp::*;
            let value = pull_lsp_diagnostics(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspCompletion {
            language_id,
            uri,
            position,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_completion(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                position,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspHover {
            language_id,
            uri,
            position,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_hover(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                position,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspDefinition {
            language_id,
            uri,
            position,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_definition(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                position,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspReferences {
            language_id,
            uri,
            position,
            include_declaration,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_references(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                position,
                include_declaration,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspRename {
            language_id,
            uri,
            position,
            new_name,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_rename(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                position,
                new_name,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::ApplyLspRename { plan_id } => {
            use crate::commands::lsp::*;
            let value = apply_lsp_rename(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                plan_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::CancelLspRename { plan_id } => {
            use crate::commands::lsp::*;
            let value = cancel_lsp_rename(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                plan_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::DiscardLspRename { plan_id } => {
            use crate::commands::lsp::*;
            let value = discard_lsp_rename(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                plan_id,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::RequestLspFormatting {
            language_id,
            uri,
            tab_size,
            insert_spaces,
        } => {
            use crate::commands::lsp::*;
            let value = request_lsp_formatting(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                language_id,
                uri,
                tab_size,
                insert_spaces,
            )
            .await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspCatalog {} => {
            use crate::commands::installer::*;
            let value = lsp_catalog()?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspInstalled {} => {
            use crate::commands::installer::*;
            let value = lsp_installed(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspRecoverInstalled {} => {
            use crate::commands::installer::*;
            lsp_recover_installed(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
            )?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspInstall {
            manifest_id,
            version,
            platform,
        } => {
            use crate::commands::installer::*;
            lsp_install(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                manifest_id,
                version,
                platform,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspImportArchive {
            manifest_id,
            version,
            platform,
            archive_paths,
        } => {
            use crate::commands::installer::*;
            lsp_import_archive(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                manifest_id,
                version,
                platform,
                archive_paths,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        LspCall::LspUninstall {
            manifest_id,
            version,
            platform,
        } => {
            use crate::commands::installer::*;
            lsp_uninstall(
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                _component_app
                    .try_state()
                    .ok_or("component_state_unavailable")?,
                manifest_id,
                version,
                platform,
            )
            .await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn files_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<FilesCall>()?;
    Ok(vec![
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
        (
            "render_preview",
            export.register::<crate::commands::preview::PreviewResponse>()?,
        ),
        (
            "open_file",
            export.register::<crate::commands::file::OpenedFileWire>()?,
        ),
        (
            "save_file",
            export.register::<crate::commands::file::SavedFileWire>()?,
        ),
        (
            "rename_file_action",
            export.register::<crate::commands::file::RenamedFileWire>()?,
        ),
        ("delete_file_action", export.register::<()>()?),
        ("reveal_file_action", export.register::<()>()?),
        ("validate_encoding", export.register::<()>()?),
        ("watch_file", export.register::<()>()?),
        ("unwatch_file", export.register::<()>()?),
        (
            "list_workspace_files",
            export.register::<crate::commands::folder::WorkspaceFiles>()?,
        ),
        ("canonicalize_workspace", export.register::<String>()?),
        (
            "workspace_capabilities",
            export.register::<crate::commands::folder::WorkspaceCapabilities>()?,
        ),
        (
            "load_session",
            export.register::<crate::commands::session::LoadedSession>()?,
        ),
        ("save_session", export.register::<()>()?),
        ("save_recovery", export.register::<()>()?),
        (
            "load_recovery",
            export.register::<Vec<crate::core::recovery::RecoveryEntry>>()?,
        ),
        ("discard_recovery", export.register::<()>()?),
        ("apply_recovery", export.register::<()>()?),
    ])
}
pub fn lsp_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<LspCall>()?;
    Ok(vec![
("load_lsp_config",export.register::<crate::lsp::config::LoadedLspConfig>()?),
("save_lsp_config",export.register::<()>()?),
("start_language_server",export.register::<()>()?),
("stop_language_server",export.register::<()>()?),
("restart_language_server",export.register::<()>()?),
("stop_all_language_servers",export.register::<()>()?),
("language_server_statuses",export.register::<Vec<crate::lsp::manager::LanguageServerStatus>>()?),
("language_server_logs",export.register::<Vec<crate::lsp::logs::LanguageServerLog>>()?),
("open_lsp_document",export.register::<crate::lsp::documents::DidOpen>()?),
("change_lsp_document",export.register::<crate::lsp::documents::DidChange>()?),
("reload_lsp_document",export.register::<crate::lsp::documents::DidChange>()?),
("save_lsp_document",export.register::<crate::lsp::documents::DidSave>()?),
("close_lsp_document",export.register::<crate::lsp::documents::DidClose>()?),
("pull_lsp_diagnostics",export.register::<crate::lsp::features::FeatureResponse<crate::lsp::features::DiagnosticResult>>()?),
("request_lsp_completion",export.register::<crate::lsp::features::FeatureResponse<crate::lsp::features::CompletionResult>>()?),
("request_lsp_hover",export.register::<crate::lsp::features::FeatureResponse<Option<crate::lsp::features::SanitizedHover>>>()?),
("request_lsp_definition",export.register::<crate::lsp::features::FeatureResponse<crate::lsp::features::FilteredLocations>>()?),
("request_lsp_references",export.register::<crate::lsp::features::FeatureResponse<crate::lsp::features::FilteredLocations>>()?),
("request_lsp_rename",export.register::<crate::lsp::manager::RenamePreview>()?),
("apply_lsp_rename",export.register::<crate::lsp::manager::RenameApplyResult>()?),
("cancel_lsp_rename",export.register::<bool>()?),
("discard_lsp_rename",export.register::<bool>()?),
("request_lsp_formatting",export.register::<crate::lsp::manager::AppliedDocumentEdits>()?),
("lsp_catalog",export.register::<Vec<crate::lsp::catalog::ServerManifest>>()?),
("lsp_installed",export.register::<Vec<crate::lsp::installer::ManagedInstallStatus>>()?),
("lsp_recover_installed",export.register::<()>()?),
("lsp_install",export.register::<()>()?),
("lsp_import_archive",export.register::<()>()?),
("lsp_uninstall",export.register::<()>()?),
])
}
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::workspace::Lane;
    #[test]
    fn completion_projection_accepts_the_external_wire_shape() {
        use crate::lsp::features::CompletionItemWire;
        let mut item = lsp_types::CompletionItem::new_simple("name".into(), "detail".into());
        item.documentation = Some(lsp_types::Documentation::MarkupContent(
            lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: "**name**".into(),
            },
        ));
        item.text_edit = Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
            range: lsp_types::Range::default(),
            new_text: "replacement".into(),
        }));
        let projected: CompletionItemWire =
            serde_json::from_value(serde_json::to_value(item).unwrap()).unwrap();
        assert_eq!(projected.label, "name");
        assert_eq!(projected.text_edit.unwrap()["newText"], "replacement");
        assert!(projected.documentation.is_some());
    }
    #[test]
    fn file_and_lsp_calls_keep_their_separate_execution_policy() {
        let file: FilesCall = serde_json::from_str(
            r#"{"method":"open_file","args":{"request":{"path":"fixture.txt","encoding":null}}}"#,
        )
        .unwrap();
        assert_eq!(file.lane(), Lane::Files);
        assert_eq!(file.deadline_budget_ms(), 29_000);
        let stop: LspCall = serde_json::from_str(
            r#"{"method":"stop_language_server","args":{"languageId":"rust"}}"#,
        )
        .unwrap();
        assert_eq!(stop.lane(), Lane::LspStop);
        assert_eq!(stop.deadline_budget_ms(), 29_000);
        assert!(serde_json::from_str::<FilesCall>(
            r#"{"method":"stop_language_server","args":{"languageId":"rust"}}"#
        )
        .is_err());
    }
}
