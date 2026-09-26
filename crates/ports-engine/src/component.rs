//! Native component entry points; the product host admits caller and operation.
//! Calling these does not start the standalone application or select its stores.

pub use crate::commands::ports::ListenerActionResult;
pub use crate::core::listeners::{KillListenerRequest, ListenerIdentity, ListenerSource};
pub use crate::core::{preferences::PortManagerPreferences, product_preferences};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tauri::Manager;

pub use crate::commands::correlation::{
    observe_product, resolve_product_action, CorrelationConfidence, PortCorrelation,
    PortObservationSnapshot, ProductBindings, ProductPortAction, ProductPortOwner,
    SnapshotSourceState,
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
    crate::core::product_preferences::load(&data.path).map_err(str::to_owned)?;
    data.checked()?;
    app.manage(data);
    Ok(())
}

pub async fn kill_external_listener(
    request: KillListenerRequest,
    deadline_ms: u64,
) -> Result<ListenerActionResult, String> {
    crate::commands::ports::kill_product_listener(request, deadline_ms).await
}
