use crate::core::preferences::{
    load_from_path, preferences_path, save_to_path, PortManagerPreferences,
};

const PREFERENCES_ERROR: &str = "Port Manager view settings are unavailable.";

fn preferences_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    crate::component::data_root(app)
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

/// Typed product adapter; native admission precedes this existing command.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_load_port_manager_preferences(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = load_port_manager_preferences(_component_app.clone()).await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; native admission precedes this existing command.
#[cfg(feature = "desktop")]
pub(crate) async fn __component_save_port_manager_preferences(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        preferences: PortManagerPreferences,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    save_port_manager_preferences(_component_app.clone(), input.preferences).await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}
