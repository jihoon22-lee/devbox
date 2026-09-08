//! Reuse the existing native engine without starting its standalone application.
//! Product initialization and native caller/owner checks precede every dispatch.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, Manager};

/// Strict import/startup validation preserves unreadable user metadata instead
/// of invoking the standalone session/recovery empty-state fallback.
pub fn validate_persistent_file(name: &str, bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("invalid_files_store");
    }
    let input = std::str::from_utf8(bytes).map_err(|_| "invalid_files_store")?;
    match name {
        "session.json" => crate::core::session::Session::from_json(input)
            .map(|_| ())
            .map_err(|_| "invalid_files_store"),
        "lsp/config.json" => crate::lsp::catalog::LspConfig::from_json(input)
            .map(|_| ())
            .map_err(|_| "invalid_files_store"),
        "recovery.json" => {
            use crate::core::recovery::{
                RecoveryFile, MAX_DOC_CHARS, MAX_TOTAL_CHARS, RECOVERY_VERSION,
            };
            let value: RecoveryFile =
                serde_json::from_str(input).map_err(|_| "invalid_files_store")?;
            if value.version != RECOVERY_VERSION {
                return Err("invalid_files_store");
            }
            let mut paths = std::collections::HashSet::new();
            let mut total = 0;
            for entry in value.entries {
                let count = entry.content.chars().count();
                total += count;
                if entry.path.trim().is_empty()
                    || !paths.insert(entry.path)
                    || count > MAX_DOC_CHARS
                    || total > MAX_TOTAL_CHARS
                {
                    return Err("invalid_files_store");
                }
            }
            Ok(())
        }
        _ => Err("unknown_files_store"),
    }
}

// Immutable native-selected generation. Initialization never runs the legacy
// identifier migrator, reads a repository manifest or starts a language server.
static PRODUCT_DATA: OnceLock<PathBuf> = OnceLock::new();
pub(crate) fn product_hosted() -> bool {
    PRODUCT_DATA.get().is_some()
}
pub(crate) fn data_root(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    match PRODUCT_DATA.get() {
        Some(root) => Ok(root.clone()),
        None => app.path().app_local_data_dir(),
    }
}
pub fn initialize(app: &tauri::AppHandle, data: &Path) -> Result<(), String> {
    if !data.is_absolute()
        || !data.is_dir()
        || devbox_filesystem::ensure_no_links(data).is_err()
        || app.try_state::<crate::applink::PendingOpen>().is_some()
        || app.try_state::<Arc<crate::lsp::LspManager>>().is_some()
        || app
            .try_state::<Arc<crate::lsp::ManagedInstaller>>()
            .is_some()
        || app
            .try_state::<Arc<crate::watcher::WatcherManager>>()
            .is_some()
    {
        return Err("component_state_conflict".into());
    }
    PRODUCT_DATA
        .set(data.to_path_buf())
        .map_err(|_| "component_state_conflict")?;
    let installer = Arc::new(
        crate::lsp::ManagedInstaller::new(data).map_err(|_| "component_storage_unavailable")?,
    );
    let manager = Arc::new(crate::lsp::LspManager::with_installer(
        data.to_path_buf(),
        env!("CARGO_PKG_VERSION"),
        installer.clone(),
    ));
    let mut events = manager.subscribe_events();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(crate::lsp::LspEvent::Diagnostics(value)) => {
                    let _ = handle.emit_to("main", "lsp/diagnostics", value);
                }
                Ok(crate::lsp::LspEvent::Status(value)) => {
                    let _ = handle.emit_to("main", "lsp/status", value);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    app.manage(crate::applink::PendingOpen::new());
    app.manage(crate::watcher::WatcherManager::new(app.clone()));
    app.manage(manager);
    app.manage(installer);
    Ok(())
}

/// The product must keep its process alive until owned language-server trees
/// are termination-confirmed, matching the standalone editor's exit contract.
pub async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let manager = app
        .try_state::<Arc<crate::lsp::LspManager>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    manager
        .shutdown_for_exit()
        .await
        .map_err(|_| "component_shutdown_incomplete".into())
}

/// Native producer delivery. The receiver still resolves the target and asks
/// about dirty documents; the event is only a hint to consume this pending item.
pub fn offer_product_open(
    app: &tauri::AppHandle,
    request: devbox_applink::OpenRequest,
) -> Result<(), String> {
    use tauri::Emitter;
    if PRODUCT_DATA.get().is_none() {
        return Err("component_state_unavailable".into());
    }
    devbox_applink::build_argv(&request).map_err(|_| "component_args_invalid")?;
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "devbox://open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

pub const COMMANDS: &[&str] = &[
    "take_pending_open",
    "open_file",
    "save_file",
    "rename_file_action",
    "delete_file_action",
    "reveal_file_action",
    "validate_encoding",
    "list_workspace_files",
    "canonicalize_workspace",
    "workspace_capabilities",
    "lsp_catalog",
    "lsp_installed",
    "lsp_recover_installed",
    "lsp_install",
    "lsp_import_archive",
    "lsp_uninstall",
    "render_preview",
    "load_session",
    "save_session",
    "save_recovery",
    "load_recovery",
    "discard_recovery",
    "apply_recovery",
    "watch_file",
    "unwatch_file",
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
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    // The legacy recovery command writes a renderer path directly. Product
    // recovery needs its own native preview and file-identity approval first.
    if PRODUCT_DATA.get().is_some() && method == "apply_recovery" {
        return Err("recovery_review_required".into());
    }
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "open_file" => crate::commands::file::__component_open_file(app, args).await,
        "save_file" => crate::commands::file::__component_save_file(app, args).await,
        "rename_file_action" => {
            crate::commands::file::__component_rename_file_action(app, args).await
        }
        "delete_file_action" => {
            crate::commands::file::__component_delete_file_action(app, args).await
        }
        "reveal_file_action" => {
            crate::commands::file::__component_reveal_file_action(app, args).await
        }
        "validate_encoding" => {
            crate::commands::file::__component_validate_encoding(app, args).await
        }
        "list_workspace_files" => {
            crate::commands::folder::__component_list_workspace_files(app, args).await
        }
        "canonicalize_workspace" => {
            crate::commands::folder::__component_canonicalize_workspace(app, args).await
        }
        "workspace_capabilities" => {
            crate::commands::folder::__component_workspace_capabilities(app, args).await
        }
        "lsp_catalog" => crate::commands::installer::__component_lsp_catalog(app, args).await,
        "lsp_installed" => crate::commands::installer::__component_lsp_installed(app, args).await,
        "lsp_recover_installed" => {
            crate::commands::installer::__component_lsp_recover_installed(app, args).await
        }
        "lsp_install" => crate::commands::installer::__component_lsp_install(app, args).await,
        "lsp_import_archive" => {
            crate::commands::installer::__component_lsp_import_archive(app, args).await
        }
        "lsp_uninstall" => crate::commands::installer::__component_lsp_uninstall(app, args).await,
        "render_preview" => crate::commands::preview::__component_render_preview(app, args).await,
        "load_session" => crate::commands::session::__component_load_session(app, args).await,
        "save_session" => crate::commands::session::__component_save_session(app, args).await,
        "save_recovery" => crate::commands::recovery::__component_save_recovery(app, args).await,
        "load_recovery" => crate::commands::recovery::__component_load_recovery(app, args).await,
        "discard_recovery" => {
            crate::commands::recovery::__component_discard_recovery(app, args).await
        }
        "apply_recovery" => crate::commands::recovery::__component_apply_recovery(app, args).await,
        "watch_file" => crate::commands::watch::__component_watch_file(app, args).await,
        "unwatch_file" => crate::commands::watch::__component_unwatch_file(app, args).await,
        "load_lsp_config" => crate::commands::lsp::__component_load_lsp_config(app, args).await,
        "save_lsp_config" => crate::commands::lsp::__component_save_lsp_config(app, args).await,
        "start_language_server" => {
            crate::commands::lsp::__component_start_language_server(app, args).await
        }
        "stop_language_server" => {
            crate::commands::lsp::__component_stop_language_server(app, args).await
        }
        "restart_language_server" => {
            crate::commands::lsp::__component_restart_language_server(app, args).await
        }
        "stop_all_language_servers" => {
            crate::commands::lsp::__component_stop_all_language_servers(app, args).await
        }
        "language_server_statuses" => {
            crate::commands::lsp::__component_language_server_statuses(app, args).await
        }
        "language_server_logs" => {
            crate::commands::lsp::__component_language_server_logs(app, args).await
        }
        "open_lsp_document" => crate::commands::lsp::__component_open_lsp_document(app, args).await,
        "change_lsp_document" => {
            crate::commands::lsp::__component_change_lsp_document(app, args).await
        }
        "reload_lsp_document" => {
            crate::commands::lsp::__component_reload_lsp_document(app, args).await
        }
        "save_lsp_document" => crate::commands::lsp::__component_save_lsp_document(app, args).await,
        "close_lsp_document" => {
            crate::commands::lsp::__component_close_lsp_document(app, args).await
        }
        "pull_lsp_diagnostics" => {
            crate::commands::lsp::__component_pull_lsp_diagnostics(app, args).await
        }
        "request_lsp_completion" => {
            crate::commands::lsp::__component_request_lsp_completion(app, args).await
        }
        "request_lsp_hover" => crate::commands::lsp::__component_request_lsp_hover(app, args).await,
        "request_lsp_definition" => {
            crate::commands::lsp::__component_request_lsp_definition(app, args).await
        }
        "request_lsp_references" => {
            crate::commands::lsp::__component_request_lsp_references(app, args).await
        }
        "request_lsp_rename" => {
            crate::commands::lsp::__component_request_lsp_rename(app, args).await
        }
        "apply_lsp_rename" => crate::commands::lsp::__component_apply_lsp_rename(app, args).await,
        "cancel_lsp_rename" => crate::commands::lsp::__component_cancel_lsp_rename(app, args).await,
        "discard_lsp_rename" => {
            crate::commands::lsp::__component_discard_lsp_rename(app, args).await
        }
        "request_lsp_formatting" => {
            crate::commands::lsp::__component_request_lsp_formatting(app, args).await
        }
        _ => Err("component_method_invalid".into()),
    }
}
