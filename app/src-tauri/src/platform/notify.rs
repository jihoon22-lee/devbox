use std::sync::{Mutex, MutexGuard, Once};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::core::notify::{Navigation, PendingNavigation, route_for};

#[derive(Default)]
pub struct Notifications(Mutex<PendingNavigation>);
impl Notifications {
    fn queue(&self) -> MutexGuard<'_, PendingNavigation> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Windows protocol activation is delivered only through argv (cold and single-instance).
/// The deep-link plugin's automatic new-url event is intentionally not also subscribed.
pub fn handle_arguments(app: &AppHandle, args: &[String], focus: bool) -> bool {
    let [_, uri] = args else { return false };
    let Some(intent) = app.state::<Notifications>().queue().enqueue(uri) else {
        return false;
    };
    if focus {
        super::window::focus_main(app);
    }
    // Emitting is an optimization; pending_navigation preserves activation before Shell mounts.
    if let Err(error) = app.emit_to("main", "devbox://navigate", intent) {
        eprintln!("Cannot emit pending navigation: {error}");
    }
    true
}

#[tauri::command]
pub fn pending_navigation(
    window: WebviewWindow,
    state: State<'_, Notifications>,
) -> Option<Navigation> {
    if window.label() != "main" {
        return None;
    }
    state.queue().pending()
}
#[tauri::command]
pub fn acknowledge_navigation(
    window: WebviewWindow,
    state: State<'_, Notifications>,
    sequence: u64,
) {
    if window.label() == "main" {
        state.queue().acknowledge(sequence);
    }
}
#[derive(serde::Deserialize)]
pub struct Attention {
    id: String,
    title: String,
    body: String,
    visible: bool,
}

// Synchronous Tauri commands run on the UI thread, keeping WinRT/COM and HWND work there.
#[tauri::command]
pub fn notify_attention(app: AppHandle, window: WebviewWindow, a: Attention) {
    if window.label() != "main" {
        return;
    }
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    let uri = format!("devbox://agents/{}", a.id);
    if route_for(&uri).is_none() || (a.visible && main.is_focused().unwrap_or(false)) {
        return;
    }
    let spec = devbox_win::toast::ToastSpec {
        title: a.title,
        body: a.body,
        launch_uri: uri.clone(),
        actions: vec![("열기".into(), uri)],
    };
    if let Err(error) = devbox_win::toast::show(
        &app.config().identifier,
        &spec,
        &devbox_win::toast::tag_for(&a.id),
        "agents",
    ) {
        static REPORTED: Once = Once::new();
        REPORTED.call_once(|| {
            eprintln!("Windows toast unavailable (a registered installation is required): {error}")
        });
    }
    // Also flash on toast success, for Focus Assist / Do Not Disturb.
    if let Ok(hwnd) = main.hwnd() {
        devbox_win::taskbar::flash(windows::Win32::Foundation::HWND(hwnd.0));
    }
}
#[tauri::command]
pub fn set_badge(app: AppHandle, window: WebviewWindow, attention: u32, open: u32) {
    if window.label() != "main" {
        return;
    }
    // Task 23 adds the tray summary using both counts here.
    let _ = open;
    if let Some(main) = app.get_webview_window("main")
        && let Ok(hwnd) = main.hwnd()
        && let Err(error) =
            devbox_win::taskbar::set_overlay(windows::Win32::Foundation::HWND(hwnd.0), attention)
    {
        eprintln!("Cannot update taskbar badge: {error}");
    }
}
