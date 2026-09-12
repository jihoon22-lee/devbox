//! Native component entry points; the product host admits caller and operation.
//! Calling these does not start the standalone application or select its stores.

pub use crate::core::listeners::{KillListenerRequest, ListenerIdentity};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tauri::Manager;

pub use crate::commands::correlation::{
    observe_product, resolve_product_action, CorrelationConfidence, ProductBindings,
    ProductPortAction, ProductPortOwner, SnapshotSourceState,
};

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
    if app.config().identifier != "com.devbox.portmanager" {
        return Err("component_state_unavailable".into());
    }
    app.path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable".into())
}

pub fn initialize(app: &tauri::AppHandle, data: &Path) -> Result<(), String> {
    if app.try_state::<ProductData>().is_some() {
        return Err("component_state_conflict".into());
    }
    let data = ProductData::open(data)?;
    crate::core::preferences::load_from_path(crate::core::preferences::preferences_path(
        &data.path,
    ))
    .map_err(|_| "component_storage_unavailable")?;
    data.checked()?;
    app.manage(data);
    Ok(())
}

pub async fn kill_external_listener(
    request: KillListenerRequest,
    deadline_ms: u64,
) -> Result<serde_json::Value, String> {
    let result = crate::commands::ports::kill_product_listener(request, deadline_ms).await?;
    serde_json::to_value(result).map_err(|_| "component_response_invalid".into())
}

pub const COMMANDS: &[&str] = &[
    "list_port_observations",
    "open_port_owner",
    "open_port_log",
    "list_ports",
    "kill_listener",
    "handoff_container_stop",
    "get_process_info",
    "reveal_process",
    "open_browser",
    "load_port_manager_preferences",
    "save_port_manager_preferences",
];

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    data_root(app)?;
    let result = match method {
        "list_port_observations" => {
            crate::commands::correlation::__component_list_port_observations(app, args).await
        }
        "open_port_owner" => {
            crate::commands::correlation::__component_open_port_owner(app, args).await
        }
        "open_port_log" => crate::commands::correlation::__component_open_port_log(app, args).await,
        "list_ports" => crate::commands::ports::__component_list_ports(app, args).await,
        "kill_listener" => crate::commands::ports::__component_kill_listener(app, args).await,
        "handoff_container_stop" => {
            crate::commands::ports::__component_handoff_container_stop(app, args).await
        }
        "get_process_info" => crate::commands::ports::__component_get_process_info(app, args).await,
        "reveal_process" => crate::commands::ports::__component_reveal_process(app, args).await,
        "open_browser" => crate::commands::ports::__component_open_browser(app, args).await,
        "load_port_manager_preferences" => {
            crate::commands::preferences::__component_load_port_manager_preferences(app, args).await
        }
        "save_port_manager_preferences" => {
            crate::commands::preferences::__component_save_port_manager_preferences(app, args).await
        }
        _ => Err("component_method_invalid".into()),
    };
    data_root(app)?;
    result
}
