use crate::core::preferences::{
    load_from_path, preferences_path, save_to_path, PortManagerPreferences,
};
use tauri::Manager;

const PREFERENCES_ERROR: &str = "Port Manager view settings are unavailable.";

fn preferences_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(preferences_path)
        .map_err(|_| PREFERENCES_ERROR.to_owned())
}

/// Load bounded, app-owned view state. Invalid or corrupt state is rejected
/// by the core parser; the frontend can then use its safe defaults.
#[tauri::command]
pub async fn load_port_manager_preferences(
    app: tauri::AppHandle,
) -> Result<PortManagerPreferences, String> {
    let path = preferences_file(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        load_from_path(path).map_err(|_| PREFERENCES_ERROR.to_owned())
    })
    .await
    .map_err(|_| PREFERENCES_ERROR.to_owned())?
}

/// Persist only the strict preference DTO. The shared writer makes the file
/// replacement atomic and the core validator rejects paths/secrets/unknown
/// control fields before any bytes reach disk.
#[tauri::command]
pub async fn save_port_manager_preferences(
    app: tauri::AppHandle,
    preferences: PortManagerPreferences,
) -> Result<(), String> {
    let path = preferences_file(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        save_to_path(path, &preferences).map_err(|_| PREFERENCES_ERROR.to_owned())
    })
    .await
    .map_err(|_| PREFERENCES_ERROR.to_owned())?
}
