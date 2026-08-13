//! Thin commands for the reviewed managed-server catalog and installer.
//!
//! The UI receives catalog metadata and exact install status, but it never
//! supplies a manifest, artifact URL, or destination path back to the
//! installer. Installs resolve exact keys against the process-owned catalog;
//! removals resolve exact keys against the process-owned installed index.

use crate::lsp::{
    initial_catalog, LspManager, ManagedInstallStatus, ManagedInstaller, ServerManifest,
};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn lsp_catalog() -> Result<Vec<ServerManifest>, String> {
    Ok(initial_catalog())
}

#[tauri::command]
pub fn lsp_installed(
    installer: State<'_, Arc<ManagedInstaller>>,
) -> Result<Vec<ManagedInstallStatus>, String> {
    installer
        .installed_status()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn lsp_recover_installed(installer: State<'_, Arc<ManagedInstaller>>) -> Result<(), String> {
    installer
        .recover_installed_index()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_install(
    installer: State<'_, Arc<ManagedInstaller>>,
    manifest_id: String,
    version: String,
    platform: String,
) -> Result<(), String> {
    installer
        .install_catalog(&manifest_id, &version, &platform)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn lsp_uninstall(
    manager: State<'_, Arc<LspManager>>,
    installer: State<'_, Arc<ManagedInstaller>>,
    manifest_id: String,
    version: String,
    platform: String,
) -> Result<(), String> {
    // A managed directory is never removed while a language session could be
    // using it.  stop_all is intentionally completed before any filesystem
    // mutation and an error leaves both the index and install untouched.
    manager
        .stop_all()
        .await
        .map_err(|error| error.to_string())?;
    installer
        .uninstall_indexed(&manifest_id, &version, &platform)
        .map_err(|error| error.to_string())
}
