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
    let manager = Arc::new(crate::lsp::LspManager::with_execution_authority(
        data.to_path_buf(),
        env!("CARGO_PKG_VERSION"),
        installer.clone(),
        Arc::new(crate::lsp::UnapprovedLspExecution),
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
    devbox_applink::validate_request(&request).map_err(|_| "component_args_invalid")?;
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "devbox://open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

pub(crate) fn is_product() -> bool {
    PRODUCT_DATA.get().is_some()
}
