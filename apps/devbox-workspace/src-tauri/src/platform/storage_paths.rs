//! The Tauri host supplies its protected storage roots to the shared file owner.
use crate::host::Host;
use tauri::Manager;
pub(crate) use workspace_wsl::storage_paths::{display, ProtectedStorage};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) fn from_host(app: &tauri::AppHandle, host: &Host) -> Result<ProtectedStorage> {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../../../products.json"))
            .map_err(|_| "invalid_files_store")?;
    let identifiers = catalog["products"]
        .as_array()
        .ok_or("invalid_files_store")?
        .iter()
        .map(|product| {
            product["identifier"]
                .as_str()
                .map(str::to_owned)
                .ok_or("invalid_files_store")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut protected = ProtectedStorage::new(host.storage_root(), identifiers)?;
    let roaming = app
        .path()
        .app_data_dir()
        .map_err(|_| "invalid_files_store")?;
    protected.add_parent(roaming.parent().ok_or("invalid_files_store")?)?;
    Ok(protected)
}
