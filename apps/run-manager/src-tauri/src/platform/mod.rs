#[cfg(target_os = "windows")]
pub mod windows;

pub mod wsl;

#[cfg(target_os = "windows")]
pub use windows::install_session_end_hook;

#[cfg(target_os = "windows")]
pub(crate) use windows::replace_file_atomic;

#[cfg(not(target_os = "windows"))]
pub fn install_session_end_hook(
    _window: &tauri::WebviewWindow,
    _app: &tauri::AppHandle,
    _state: std::sync::Arc<crate::lifecycle::RuntimeState>,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn replace_file_atomic(
    replacement: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(replacement, destination)
}
