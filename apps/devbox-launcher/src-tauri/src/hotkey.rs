//! Small, allow-listed global shortcut adapter.
//!
//! The shortcut is the only global input this app owns. The persisted file
//! contains no query, clipboard, selection, or snapshot data. Windows uses
//! RegisterHotKey; other platforms keep the browser/embedded key handler and
//! expose the same validated configuration for deterministic tests.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const DEFAULT_SHORTCUT: &str = "Ctrl+Alt+Space";
pub const SHORTCUTS: [&str; 3] = ["Ctrl+Alt+Space", "Ctrl+Alt+L", "Ctrl+Alt+J"];
const MAX_CONFIG_BYTES: u64 = 4 * 1024;
#[cfg(windows)]
const HOTKEY_ID: i32 = 0xD0B0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShortcutConfig {
    pub accelerator: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationState {
    Registered,
    Unavailable,
    Unsupported,
    Disabled,
    Pending,
}

#[derive(Debug, Clone)]
pub struct RuntimeState {
    registration: Arc<Mutex<RegistrationState>>,
    #[cfg(windows)]
    worker: Arc<Mutex<Option<HotkeyWorker>>>,
}

#[cfg(windows)]
#[derive(Debug)]
struct HotkeyWorker {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(windows)]
impl HotkeyWorker {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            // The worker is only stopped from the Tauri/main thread. Keep the
            // guard anyway so a future callback cannot deadlock itself during
            // shutdown.
            if join.thread().id() != std::thread::current().id() {
                let _ = join.join();
            }
        }
    }
}

#[cfg(windows)]
impl Drop for HotkeyWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            registration: Arc::new(Mutex::new(RegistrationState::Pending)),
            #[cfg(windows)]
            worker: Arc::new(Mutex::new(None)),
        }
    }
}

impl RuntimeState {
    pub fn registration(&self) -> RegistrationState {
        self.registration
            .lock()
            .map(|state| *state)
            .unwrap_or(RegistrationState::Unavailable)
    }

    fn set_registration(&self, state: RegistrationState) {
        if let Ok(mut current) = self.registration.lock() {
            *current = state;
        }
    }

    /// Stop and join the native listener before the Tauri event loop exits.
    ///
    /// The listener owns the Windows hotkey registration. Keeping its join
    /// handle in application state makes shutdown deterministic and prevents a
    /// detached worker from outliving the webview or retaining the shortcut.
    pub fn stop_global_listener(&self) {
        #[cfg(windows)]
        {
            let worker = self.worker.lock().ok().and_then(|mut slot| slot.take());
            if let Some(mut worker) = worker {
                worker.stop();
            }
        }
    }

    #[cfg(windows)]
    fn install_worker(&self, worker: HotkeyWorker) {
        if let Ok(mut slot) = self.worker.lock() {
            if let Some(mut previous) = slot.take() {
                previous.stop();
            }
            *slot = Some(worker);
        }
    }
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            accelerator: DEFAULT_SHORTCUT.into(),
            enabled: true,
        }
    }
}

pub fn validate(config: &ShortcutConfig) -> Result<(), String> {
    if config.accelerator.len() > 64 || !SHORTCUTS.contains(&config.accelerator.as_str()) {
        return Err("지원하지 않는 Launcher 단축키입니다".into());
    }
    Ok(())
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join("shortcut.json"))
        .map_err(|_| "Launcher 설정 경로를 확인할 수 없습니다".into())
}

pub fn load(app: &AppHandle) -> ShortcutConfig {
    let Ok(path) = config_path(app) else {
        return ShortcutConfig::default();
    };
    load_from_path(&path).unwrap_or_default()
}

fn load_from_path(path: &Path) -> Result<ShortcutConfig, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "missing")?;
    if metadata.file_type().is_symlink()
        || !metadata.file_type().is_file()
        || metadata.len() > MAX_CONFIG_BYTES
    {
        return Err("invalid".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "read")?;
    let config: ShortcutConfig = serde_json::from_slice(&bytes).map_err(|_| "parse")?;
    validate(&config)?;
    Ok(config)
}

pub fn save(app: &AppHandle, config: ShortcutConfig) -> Result<ShortcutConfig, String> {
    validate(&config)?;
    let path = config_path(app)?;
    let parent = path
        .parent()
        .ok_or("Launcher 설정 경로를 확인할 수 없습니다")?;
    std::fs::create_dir_all(parent).map_err(|_| "Launcher 설정을 저장할 수 없습니다")?;
    let bytes = serde_json::to_vec(&config).map_err(|_| "Launcher 설정을 저장할 수 없습니다")?;
    devbox_filesystem::atomic_write(path, &bytes)
        .map_err(|_| "Launcher 설정을 저장할 수 없습니다")?;
    Ok(config)
}

pub fn start_global_listener(app: &AppHandle, config: &ShortcutConfig, runtime: RuntimeState) {
    if !config.enabled {
        runtime.set_registration(RegistrationState::Disabled);
        return;
    }

    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_J, VK_L,
            VK_SPACE,
        };
        use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};
        let key = match config.accelerator.as_str() {
            "Ctrl+Alt+Space" => VK_SPACE.0 as u32,
            "Ctrl+Alt+L" => VK_L.0 as u32,
            "Ctrl+Alt+J" => VK_J.0 as u32,
            _ => {
                runtime.set_registration(RegistrationState::Unavailable);
                return;
            }
        };
        let app = app.clone();
        let thread_runtime = runtime.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (status_tx, status_rx) = mpsc::sync_channel(1);
        let worker = match std::thread::Builder::new()
            .name("devbox-launcher-hotkey".into())
            .spawn(move || {
                let registered = unsafe {
                    RegisterHotKey(None, HOTKEY_ID, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, key)
                };
                if registered.is_err() {
                    // A conflicting shortcut is a recoverable user-visible state;
                    // never fail app startup or print the requested shortcut/path.
                    thread_runtime.set_registration(RegistrationState::Unavailable);
                    let _ = status_tx.send(RegistrationState::Unavailable);
                    return;
                }
                thread_runtime.set_registration(RegistrationState::Registered);
                let _ = status_tx.send(RegistrationState::Registered);
                let mut message = MSG::default();
                while !thread_stop.load(Ordering::Acquire) {
                    let has_message = unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) };
                    if !has_message.as_bool() {
                        // A short bounded poll keeps shutdown responsive without
                        // requiring a second Win32 message just to wake the loop.
                        std::thread::sleep(Duration::from_millis(25));
                        continue;
                    }
                    if message.message == WM_HOTKEY {
                        let handle = app.clone();
                        let callback_handle = handle.clone();
                        let _ = handle.run_on_main_thread(move || {
                            if let Some(window) = callback_handle.get_webview_window("main") {
                                if window.is_visible().unwrap_or(false) {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.unminimize();
                                    let _ = window.set_focus();
                                }
                            }
                        });
                    }
                }
                unsafe {
                    let _ = UnregisterHotKey(None, HOTKEY_ID);
                }
            }) {
            Ok(worker) => worker,
            Err(_) => {
                runtime.set_registration(RegistrationState::Unavailable);
                return;
            }
        };
        runtime.install_worker(HotkeyWorker {
            stop,
            join: Some(worker),
        });
        if status_rx.recv_timeout(Duration::from_secs(1)).is_err() {
            runtime.set_registration(RegistrationState::Unavailable);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = app;
        runtime.set_registration(RegistrationState::Unsupported);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allow_listed_shortcuts_are_valid() {
        assert!(validate(&ShortcutConfig::default()).is_ok());
        assert!(validate(&ShortcutConfig {
            accelerator: "Alt+F4".into(),
            enabled: true
        })
        .is_err());
        assert!(validate(&ShortcutConfig {
            accelerator: "Ctrl+Alt+Space\nsecret".into(),
            enabled: true
        })
        .is_err());
    }
}
