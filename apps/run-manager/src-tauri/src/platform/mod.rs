#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use windows::install_session_end_hook;

#[cfg(not(target_os = "windows"))]
pub fn install_session_end_hook(
    _window: &tauri::WebviewWindow,
    _app: &tauri::AppHandle,
    _state: std::sync::Arc<crate::lifecycle::RuntimeState>,
) -> Result<(), String> {
    Ok(())
}
