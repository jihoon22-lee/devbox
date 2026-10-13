use crate::core::{connection::IpcError, preferences, settings};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, IpcError> {
    app.path()
        .app_local_data_dir()
        .map(|path| path.join("settings.json"))
        .map_err(|error| IpcError::new(error.to_string()))
}
pub fn main_only(window: &WebviewWindow) -> Result<(), IpcError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(IpcError::new("메인 창에서만 변경할 수 있습니다"))
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    autostart_asked: bool,
    autostart: bool,
    tray_notice_shown: bool,
}
impl From<&settings::Settings> for AppPreferences {
    fn from(value: &settings::Settings) -> Self {
        Self {
            autostart_asked: value.autostart_asked,
            autostart: value.autostart,
            tray_notice_shown: value.tray_notice_shown,
        }
    }
}
#[tauri::command]
pub fn app_preferences(app: AppHandle, window: WebviewWindow) -> Result<AppPreferences, IpcError> {
    main_only(&window)?;
    let settings =
        settings::load(&settings_path(&app)?).map_err(|error| IpcError::new(error.to_string()))?;
    let mut preferences = AppPreferences::from(&settings);
    preferences.autostart = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| IpcError::new(error.to_string()))?;
    Ok(preferences)
}
#[tauri::command]
pub fn set_autostart(
    app: AppHandle,
    window: WebviewWindow,
    enabled: bool,
    first_run: bool,
) -> Result<AppPreferences, IpcError> {
    main_only(&window)?;
    let path = settings_path(&app)?;
    settings::with_settings(&path, |settings| {
        // Query only after reading valid settings; plugin setup never enables registration.
        let manager = app.autolaunch();
        let current = manager.is_enabled().map_err(std::io::Error::other)?;
        preferences::change_autostart(
            settings,
            current,
            enabled,
            first_run,
            |value| {
                if value {
                    manager.enable()
                } else {
                    manager.disable()
                }
                .map_err(std::io::Error::other)
            },
            |settings| settings::save(&path, settings),
        )?;
        Ok(AppPreferences::from(&*settings))
    })
    .map_err(|error| IpcError::new(error.to_string()))
}
