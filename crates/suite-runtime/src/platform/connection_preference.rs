//! Product-local automatic connection preference, scoped to an installation.
use crate::preference::{Preference, FILE, LEGACY_FILE};
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use std::{io::Read, path::PathBuf};
use tauri::Manager;
fn root(app: &tauri::AppHandle) -> Result<PathBuf, &'static str> {
    app.path()
        .app_local_data_dir()
        .map_err(|_| "suite_preference_unavailable")
}
pub(crate) fn read(app: &tauri::AppHandle) -> Result<Option<Preference>, &'static str> {
    let root = root(app)?;
    if !root
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        return Ok(None);
    }
    ensure_no_links(&root).map_err(|_| "suite_preference_unavailable")?;
    let _directories = super::component_scope::pin_directories(&root)?;
    let path = root.join(FILE);
    if !path
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        return Ok(None);
    }
    ensure_no_links(&path).map_err(|_| "suite_preference_unavailable")?;
    let (mut file, identity) =
        open_filesystem_object(&path, false).map_err(|_| "suite_preference_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "suite_preference_unavailable")?;
    if bytes.len() > 4096 || filesystem_identity(&path, false).ok() != Some(identity) {
        return Err("suite_preference_invalid");
    }
    let value = Preference::parse(&bytes)?;
    if !product_contract::installation::PRODUCTS.contains(&value.product.as_str())
        || !product_contract::commands::revision(&value.installation)
    {
        return Err("suite_preference_invalid");
    }
    Ok(Some(value))
}
pub(crate) fn write(app: &tauri::AppHandle, preference: &Preference) -> Result<(), &'static str> {
    let root = root(app)?;
    if !root
        .try_exists()
        .map_err(|_| "suite_preference_unavailable")?
    {
        ensure_no_links(root.parent().ok_or("suite_preference_unavailable")?)
            .map_err(|_| "suite_preference_unavailable")?;
        std::fs::create_dir(&root).map_err(|_| "suite_preference_unavailable")?;
    }
    ensure_no_links(&root).map_err(|_| "suite_preference_unavailable")?;
    let (_handle, identity) =
        open_filesystem_object(&root, true).map_err(|_| "suite_preference_unavailable")?;
    let _directories = super::component_scope::pin_directories(&root)?;
    let bytes = serde_json::to_vec(preference).map_err(|_| "suite_preference_invalid")?;
    devbox_filesystem::atomic_write(root.join(FILE), &bytes)
        .map_err(|_| "suite_preference_unavailable")?;
    match std::fs::remove_file(root.join(LEGACY_FILE)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("suite_preference_unavailable"),
    }
    if filesystem_identity(&root, true).ok() != Some(identity) {
        return Err("suite_preference_changed");
    }
    Ok(())
}
