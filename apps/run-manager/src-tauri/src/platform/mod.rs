#[cfg(target_os = "windows")]
pub mod windows;

pub mod environment;
pub mod execution;
pub mod wsl;

use serde::Serialize;
use std::path::{Path, PathBuf};

pub const STARTUP_SHORTCUT_FILE_NAME: &str = "Run Manager.lnk";
#[cfg(any(target_os = "windows", test))]
pub const STARTUP_SHORTCUT_ARGUMENTS: &str = "--background";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupShortcut {
    shortcut_path: PathBuf,
    executable: PathBuf,
}

impl StartupShortcut {
    pub fn new(startup_directory: &Path, executable: &Path) -> Result<Self, String> {
        if !executable.is_absolute() {
            return Err("the Run Manager executable path must be absolute".to_string());
        }
        Ok(Self {
            shortcut_path: startup_directory.join(STARTUP_SHORTCUT_FILE_NAME),
            executable: executable.to_path_buf(),
        })
    }

    pub fn shortcut_path(&self) -> &Path {
        &self.shortcut_path
    }

    #[cfg(target_os = "windows")]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartupShortcutStatus {
    pub supported: bool,
    pub enabled: bool,
    pub shortcut_path: String,
}

#[cfg(target_os = "windows")]
pub use windows::install_session_end_hook;

#[cfg(target_os = "windows")]
pub use windows::{set_startup_shortcut_enabled, startup_shortcut_status};

#[cfg(target_os = "windows")]
pub(crate) use windows::replace_file_atomic;

#[cfg(not(target_os = "windows"))]
pub fn startup_shortcut_status(
    shortcut: &StartupShortcut,
) -> Result<StartupShortcutStatus, String> {
    Ok(StartupShortcutStatus {
        supported: false,
        enabled: false,
        shortcut_path: shortcut.shortcut_path().display().to_string(),
    })
}

#[cfg(not(target_os = "windows"))]
pub fn set_startup_shortcut_enabled(
    shortcut: &StartupShortcut,
    _enabled: bool,
) -> Result<StartupShortcutStatus, String> {
    startup_shortcut_status(shortcut)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_shortcut_uses_fixed_visible_name_and_background_argument() {
        #[cfg(target_os = "windows")]
        let (startup, executable) = (
            Path::new(r"C:\Users\test\Startup"),
            Path::new(r"C:\Program Files\Run Manager\RunManager.exe"),
        );
        #[cfg(not(target_os = "windows"))]
        let (startup, executable) = (
            Path::new("/users/test/Startup"),
            Path::new("/opt/run-manager/RunManager.exe"),
        );
        let shortcut = StartupShortcut::new(startup, executable).unwrap();
        assert_eq!(
            shortcut.shortcut_path().file_name(),
            Some(std::ffi::OsStr::new("Run Manager.lnk"))
        );
        assert_eq!(STARTUP_SHORTCUT_ARGUMENTS, "--background");
    }

    #[test]
    fn startup_shortcut_rejects_relative_executable_paths() {
        assert!(StartupShortcut::new(Path::new("/tmp"), Path::new("RunManager.exe")).is_err());
    }
}
