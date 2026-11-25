//! Tells each window's page whether its window is on screen; the decisions are `core::presence`.
use crate::core::presence::{self, EVENT, Shown, Signal};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, Window, WindowEvent};

#[derive(Default)]
pub struct Presence(Mutex<presence::Presence>);

/// Call sites that hide or show a window report what they just did; they do not read it back.
pub fn report(app: &AppHandle, label: &str, signal: Signal) {
    let state = app.state::<Presence>();
    // Sent under the lock so that pages receive changes in the order they were recorded.
    let mut presence = state.0.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(shown) = presence.report(label, signal)
        && let Err(error) = app.emit_to(label, EVENT, Shown { label, shown })
    {
        eprintln!("Cannot report window state: {error}");
    }
}
/// Minimize and restore arrive as size events. A focused window is on screen.
pub fn window_event(window: &Window, event: &WindowEvent) {
    let signal = match event {
        WindowEvent::Resized(_) => {
            presence::observed(window.is_visible().ok(), window.is_minimized().ok())
        }
        WindowEvent::Focused(true) => Signal::Focused,
        WindowEvent::Destroyed => {
            window
                .state::<Presence>()
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .forget(window.label());
            return;
        }
        _ => return,
    };
    report(window.app_handle(), window.label(), signal);
}
/// A page asks once it listens: a window that starts in the tray gets no event to learn from.
#[tauri::command]
pub fn window_shown(window: WebviewWindow, state: State<'_, Presence>) -> bool {
    let signal = presence::observed(window.is_visible().ok(), window.is_minimized().ok());
    state
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .answer(window.label(), signal)
}
