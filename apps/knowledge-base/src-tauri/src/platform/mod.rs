//! Platform boundary for the Knowledge global quick-capture shortcut.
//!
//! The shortcut is an OS registration, not a frontend key listener.  This
//! keeps `Ctrl+Alt+K` useful while another application has focus and lets a
//! registration conflict be reported without exposing a platform error.

use serde::Serialize;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::sync::Mutex;
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

/// Internal lifecycle shared by the status command and registration worker.
/// The worker keeps this small inner value rather than the public Tauri state,
/// so dropping the app state can actually signal and join the worker.
struct ShortcutInner {
    state: AtomicU8,
    active: AtomicBool,
    #[cfg(target_os = "windows")]
    thread_id: AtomicU32,
    #[cfg(target_os = "windows")]
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// Tauri state shared by the status command and the platform registration
/// thread.  No input, path, or OS error is retained.
pub struct QuickCaptureShortcutState {
    inner: Arc<ShortcutInner>,
}

impl Default for QuickCaptureShortcutState {
    fn default() -> Self {
        Self {
            inner: Arc::new(ShortcutInner {
                state: AtomicU8::new(ShortcutRegistration::Registering as u8),
                active: AtomicBool::new(true),
                #[cfg(target_os = "windows")]
                thread_id: AtomicU32::new(0),
                #[cfg(target_os = "windows")]
                worker: Mutex::new(None),
            }),
        }
    }
}

impl QuickCaptureShortcutState {
    pub fn status(&self) -> ShortcutStatus {
        ShortcutStatus {
            shortcut: QUICK_CAPTURE_SHORTCUT.to_string(),
            state: decode_state(self.inner.state.load(Ordering::Acquire)),
        }
    }

    fn set(&self, state: ShortcutRegistration) {
        self.inner.state.store(state as u8, Ordering::Release);
    }

    #[cfg(target_os = "windows")]
    fn set_inner(inner: &ShortcutInner, state: ShortcutRegistration) {
        inner.state.store(state as u8, Ordering::Release);
    }
}

impl Drop for QuickCaptureShortcutState {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

impl ShortcutInner {
    #[cfg(target_os = "windows")]
    fn stop(&self) {
        self.active.store(false, Ordering::Release);
        let thread_id = self.thread_id.load(Ordering::Acquire);
        if thread_id != 0 {
            windows::stop(thread_id);
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn stop(&self) {
        self.active.store(false, Ordering::Release);
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
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, VK_K,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
        PM_NOREMOVE, WM_HOTKEY, WM_QUIT,
    };

    // The ID is private to the worker thread because the registration uses a
    // null HWND.  It only needs to be stable for message discrimination.
    const HOTKEY_ID: i32 = 0x4B42;

    pub(super) fn install(app: AppHandle, state: Arc<QuickCaptureShortcutState>) {
        state.set(ShortcutRegistration::Registering);
        let _ = app.emit(QUICK_CAPTURE_STATUS_EVENT, state.status());
        let (ready_sender, ready_receiver) = sync_channel(1);
        let worker_inner = Arc::clone(&state.inner);
        let worker_app = app.clone();
        let worker = thread::Builder::new()
            .name("knowledge-quick-capture-hotkey".to_string())
            .spawn(move || {
                // A thread without a window still needs a message queue before
                // another thread can post WM_QUIT during shutdown.
                unsafe {
                    let mut message = MSG::default();
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                }
                worker_inner
                    .thread_id
                    .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
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
                QuickCaptureShortcutState::set_inner(
                    &worker_inner,
                    if registered {
                        ShortcutRegistration::Registered
                    } else {
                        ShortcutRegistration::Conflict
                    },
                );
                let _ = worker_app.emit(
                    QUICK_CAPTURE_STATUS_EVENT,
                    ShortcutStatus {
                        shortcut: QUICK_CAPTURE_SHORTCUT.to_string(),
                        state: decode_state(worker_inner.state.load(Ordering::Acquire)),
                    },
                );
                let _ = ready_sender.send(());
                if !registered || !worker_inner.active.load(Ordering::Acquire) {
                    if registered {
                        let _ = unsafe { UnregisterHotKey(None, HOTKEY_ID) };
                    }
                    return;
                }

                loop {
                    let mut message = MSG::default();
                    let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if result.0 <= 0 {
                        break;
                    }
                    if message.message == WM_HOTKEY
                        && message.wParam.0 == HOTKEY_ID as usize
                        && worker_inner.active.load(Ordering::Acquire)
                    {
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

        *state.inner.worker.lock().unwrap() = worker.ok();

        // Let setup observe a conflict before the first frontend status query.
        let _ = ready_receiver.recv_timeout(Duration::from_millis(500));
    }

    pub(super) fn stop(thread_id: u32) {
        let _ = unsafe { PostThreadMessageW(thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
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
        state.inner.state.store(255, Ordering::Release);
        assert_eq!(state.status().state, ShortcutRegistration::Registering);
    }
}
