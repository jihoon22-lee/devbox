//! Native component entry points; the product host admits caller and operation.
//! Calling these does not start the standalone application or select its stores.

use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tauri::Manager;

struct ProductData {
    path: PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    _handle: File,
}
impl ProductData {
    fn open(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("component_storage_unavailable".into());
        }
        devbox_filesystem::ensure_no_links(path).map_err(|_| "component_storage_unavailable")?;
        let (handle, identity) = devbox_filesystem::open_filesystem_object(path, true)
            .map_err(|_| "component_storage_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn checked(&self) -> Result<PathBuf, String> {
        devbox_filesystem::ensure_no_links(&self.path).map_err(|_| "component_storage_changed")?;
        if devbox_filesystem::filesystem_identity(&self.path, true)
            .map_err(|_| "component_storage_changed")?
            != self.identity
        {
            return Err("component_storage_changed".into());
        }
        Ok(self.path.clone())
    }
}
pub(crate) fn data_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(data) = app.try_state::<ProductData>() {
        return data.checked();
    }
    if app.config().identifier != "com.devbox.loglens" {
        return Err("component_state_unavailable".into());
    }
    app.path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable".into())
}

pub fn initialize(
    app: &tauri::AppHandle,
    data: &Path,
    runtime_logs: std::sync::Arc<dyn crate::core::RuntimeLogProvider>,
) -> Result<(), String> {
    if app.try_state::<ProductData>().is_some()
        || app.try_state::<crate::commands::AppState>().is_some()
        || app.try_state::<crate::applink::PendingOpen>().is_some()
        || app
            .try_state::<crate::handoff::PendingLogSource>()
            .is_some()
    {
        return Err("component_state_conflict".into());
    }
    let data = ProductData::open(data)?;
    crate::core::saved_views::list_from_dir(&data.path)
        .map_err(|_| "component_storage_unavailable")?;
    data.checked()?;
    app.manage(data);
    app.manage(crate::commands::AppState {
        runtime_logs: Some(runtime_logs),
        ..Default::default()
    });
    app.manage(crate::applink::PendingOpen::new());
    app.manage(crate::handoff::PendingLogSource::new());
    Ok(())
}

pub fn is_initialized(app: &tauri::AppHandle) -> bool {
    app.try_state::<ProductData>().is_some()
}

/// Cancel readers and retain the product until blocking readers have returned,
/// including a worker whose renderer request was dropped or superseded.
pub fn request_shutdown(app: &tauri::AppHandle) -> Result<bool, String> {
    app.try_state::<crate::commands::AppState>()
        .ok_or("component_state_unavailable")?
        .operations
        .shutdown()
        .map_err(|_| "component_shutdown_incomplete".into())
}

pub async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let operations = app
        .try_state::<crate::commands::AppState>()
        .ok_or("component_state_unavailable")?
        .operations
        .clone();
    while !operations
        .shutdown()
        .map_err(|_| "component_shutdown_incomplete")?
    {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    Ok(())
}

pub fn offer_product_open(
    app: &tauri::AppHandle,
    request: devbox_applink::OpenRequest,
) -> Result<(), String> {
    use tauri::Emitter;
    data_root(app)?;
    if !is_initialized(app) || !crate::applink::is_log_source_request(&request) {
        return Err("component_args_invalid".into());
    }
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "workspace://logs-open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

pub const COMMANDS: &[&str] = &[
    "take_pending_open",
    "send_selection_to_toolbox",
    "summarize_source",
    "list_saved_views",
    "save_saved_view",
    "delete_saved_view",
    "receive_log_source",
    "fixed_adapter",
    "read_source",
    "read_sources",
    "cancel_read",
    "filter_log_records",
    "export_log_records",
    "preview_log_source",
    "accept_log_source",
    "discard_log_source",
    "renew_log_source",
];

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    data_root(app)?;
    let result = match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "send_selection_to_toolbox" => {
            crate::commands::__component_send_selection_to_toolbox(app, args).await
        }
        "summarize_source" => crate::commands::__component_summarize_source(app, args).await,
        "list_saved_views" => crate::commands::__component_list_saved_views(app, args).await,
        "save_saved_view" => crate::commands::__component_save_saved_view(app, args).await,
        "delete_saved_view" => crate::commands::__component_delete_saved_view(app, args).await,
        "receive_log_source" => crate::commands::__component_receive_log_source(app, args).await,
        "fixed_adapter" => crate::commands::__component_fixed_adapter(app, args).await,
        "read_source" => crate::commands::__component_read_source(app, args).await,
        "read_sources" => crate::commands::__component_read_sources(app, args).await,
        "cancel_read" => crate::commands::__component_cancel_read(app, args).await,
        "filter_log_records" => crate::commands::__component_filter_log_records(app, args).await,
        "export_log_records" => crate::commands::__component_export_log_records(app, args).await,
        "preview_log_source" => crate::handoff::__component_preview_log_source(app, args).await,
        "accept_log_source" => crate::handoff::__component_accept_log_source(app, args).await,
        "discard_log_source" => crate::handoff::__component_discard_log_source(app, args).await,
        "renew_log_source" => crate::handoff::__component_renew_log_source(app, args).await,
        _ => Err("component_method_invalid".into()),
    };
    data_root(app)?;
    result
}
