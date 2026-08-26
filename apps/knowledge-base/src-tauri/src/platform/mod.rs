//! Platform boundary for the Knowledge global quick-capture shortcut.
//!
//! The shortcut is an OS registration, not a frontend key listener.  This
//! keeps `Ctrl+Alt+K` useful while another application has focus and lets a
//! registration conflict be reported without exposing a platform error.

use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub const QUICK_CAPTURE_SHORTCUT: &str = "Ctrl+Alt+K";
#[cfg(target_os = "windows")]
pub const QUICK_CAPTURE_EVENT: &str = "knowledge://quick-capture";
pub const QUICK_CAPTURE_STATUS_EVENT: &str = "knowledge://quick-capture-shortcut-status";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutRegistration {
    Registering,
    Registered,
    Conflict,
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub shortcut: String,
    pub state: ShortcutRegistration,
}

/// Tauri state shared by the status command and the platform registration
/// thread.  The atomic contains only a small enum; no input, path, or OS
/// error is retained.
pub struct QuickCaptureShortcutState {
    state: AtomicU8,
}

impl Default for QuickCaptureShortcutState {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(ShortcutRegistration::Registering as u8),
        }
    }
}

impl QuickCaptureShortcutState {
    pub fn status(&self) -> ShortcutStatus {
        ShortcutStatus {
            shortcut: QUICK_CAPTURE_SHORTCUT.to_string(),
            state: decode_state(self.state.load(Ordering::Acquire)),
        }
    }

    fn set(&self, state: ShortcutRegistration) {
        self.state.store(state as u8, Ordering::Release);
    }
}

fn decode_state(value: u8) -> ShortcutRegistration {
    match value {
        value if value == ShortcutRegistration::Registered as u8 => {
            ShortcutRegistration::Registered
        }
        value if value == ShortcutRegistration::Conflict as u8 => ShortcutRegistration::Conflict,
        value if value == ShortcutRegistration::Unsupported as u8 => {
            ShortcutRegistration::Unsupported
        }
        value if value == ShortcutRegistration::Unavailable as u8 => {
            ShortcutRegistration::Unavailable
        }
        _ => ShortcutRegistration::Registering,
    }
}

pub fn install(app: AppHandle, state: Arc<QuickCaptureShortcutState>) {
    #[cfg(target_os = "windows")]
    windows::install(app, state);

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app.emit(QUICK_CAPTURE_STATUS_EVENT, state.status());
        state.set(ShortcutRegistration::Unsupported);
        let _ = app.emit(QUICK_CAPTURE_STATUS_EVENT, state.status());
    }
}

#[tauri::command]
pub fn shortcut_status(state: tauri::State<'_, Arc<QuickCaptureShortcutState>>) -> ShortcutStatus {
    state.status()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Duration;
    use tauri::Manager;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_K,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };

    // The ID is private to the worker thread because the registration uses a
    // null HWND.  It only needs to be stable for message discrimination.
    const HOTKEY_ID: i32 = 0x4B42;

    pub(super) fn install(app: AppHandle, state: Arc<QuickCaptureShortcutState>) {
        state.set(ShortcutRegistration::Registering);
        let _ = app.emit(QUICK_CAPTURE_STATUS_EVENT, state.status());
        let (ready_sender, ready_receiver) = sync_channel(1);
        let worker_state = Arc::clone(&state);
        let worker_app = app.clone();
        let worker = thread::Builder::new()
            .name("knowledge-quick-capture-hotkey".to_string())
            .spawn(move || {
                // RegisterHotKey returns an error for an unavailable/conflicting
                // accelerator.  Deliberately collapse it to a safe UI state.
                let registered = unsafe {
                    RegisterHotKey(
                        None,
                        HOTKEY_ID,
                        MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
                        VK_K.0 as u32,
                    )
                    .is_ok()
                };
                worker_state.set(if registered {
                    ShortcutRegistration::Registered
                } else {
                    ShortcutRegistration::Conflict
                });
                let _ = worker_app.emit(QUICK_CAPTURE_STATUS_EVENT, worker_state.status());
                let _ = ready_sender.send(());
                if !registered {
                    return;
                }

                loop {
                    let mut message = MSG::default();
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if result.0 <= 0 {
                        break;
                    }
                    if message.message == WM_HOTKEY && message.wParam.0 == HOTKEY_ID as usize {
                        if let Some(window) = worker_app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                        let _ = worker_app.emit(QUICK_CAPTURE_EVENT, ());
                    }
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                let _ = unsafe { UnregisterHotKey(None, HOTKEY_ID) };
            });

        if worker.is_err() {
            state.set(ShortcutRegistration::Unavailable);
            let _ = app.emit(QUICK_CAPTURE_STATUS_EVENT, state.status());
            return;
        }

        // Let setup observe a conflict before the first frontend status query.
        // The worker remains alive for the process lifetime; Windows releases
        // the registration when the process exits.
        let _ = ready_receiver.recv_timeout(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_is_safe_and_does_not_include_platform_details() {
        let state = QuickCaptureShortcutState::default();
        assert_eq!(state.status().shortcut, QUICK_CAPTURE_SHORTCUT);
        assert_eq!(state.status().state, ShortcutRegistration::Registering);
        assert!(!format!("{:?}", state.status()).contains("raw"));
    }

    #[test]
    fn status_state_changes_are_bounded_to_known_values() {
        let state = QuickCaptureShortcutState::default();
        state.set(ShortcutRegistration::Conflict);
        assert_eq!(state.status().state, ShortcutRegistration::Conflict);
        state.state.store(255, Ordering::Release);
        assert_eq!(state.status().state, ShortcutRegistration::Registering);
    }
}
