//! Closing quits by default. Continued background collection requires both
//! explicit collection consent and a separate close-to-tray preference.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{Emitter, Manager};
#[derive(Default)]
struct QuitReview {
    pending: Option<String>,
    approved: bool,
}
impl QuitReview {
    fn request(&mut self) -> String {
        self.pending
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
    }
    fn decide(&mut self, id: &str, quit: bool) -> Result<(), String> {
        if self.pending.as_deref() != Some(id) || self.approved {
            return Err("quit_review_stale".into());
        }
        self.pending = None;
        self.approved = quit;
        Ok(())
    }
}
#[derive(Default)]
struct Lifecycle {
    tray_available: AtomicBool,
    close_to_tray: AtomicBool,
    quit: Mutex<QuitReview>,
}
pub const METHODS: &[&str] = &["get_close_policy", "set_close_policy"];
pub const QUIT_METHODS: &[&str] = &["pending_quit", "decide_quit"];
fn request_quit(app: &tauri::AppHandle) {
    let state = app.state::<Lifecycle>();
    let Ok(mut review) = state.quit.lock() else {
        return;
    };
    let id = review.request();
    drop(review);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.emit("knowledge://quit-request", id);
    }
}
pub fn quit_dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let state = app.state::<Lifecycle>();
    let mut review = state.quit.lock().map_err(|_| "quit_unavailable")?;
    if method == "pending_quit" && args.as_object().is_some_and(|value| value.is_empty()) {
        return Ok(serde_json::json!(review.pending));
    }
    if method != "decide_quit" {
        return Err("component_args_invalid".into());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Decision {
        id: String,
        quit: bool,
    }
    let decision: Decision = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    review.decide(&decision.id, decision.quit)?;
    drop(review);
    if decision.quit {
        app.exit(0);
    }
    Ok(serde_json::Value::Null)
}
pub fn initialize(app: &tauri::AppHandle) {
    app.manage(Lifecycle::default());
    if install_tray(app).is_ok() {
        app.state::<Lifecycle>()
            .tray_available
            .store(true, Ordering::Release);
    }
}
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let show = MenuItem::with_id(app, "knowledge-show", "Knowledge 열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "knowledge-quit",
        "Knowledge 종료 · 수집 중지",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("knowledge-tray")
        .tooltip("Devbox Knowledge")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "knowledge-show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "knowledge-quit" => request_quit(app),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
pub fn load(app: &tauri::AppHandle) -> Result<(), String> {
    let close_to_tray = life_log_lib::component::close_to_tray(app)?;
    app.state::<Lifecycle>()
        .close_to_tray
        .store(close_to_tray, Ordering::Release);
    Ok(())
}
fn should_hide(selected: bool, available: bool) -> bool {
    selected && available
}
pub fn on_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    match event {
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            api.prevent_close();
            let lifecycle = app.state::<Lifecycle>();
            if should_hide(
                lifecycle.close_to_tray.load(Ordering::Acquire),
                lifecycle.tray_available.load(Ordering::Acquire),
            ) {
                if let Some(window) = app.get_webview_window("main") {
                    if window.hide().is_ok() {
                        return;
                    }
                }
            }
            request_quit(app);
        }
        tauri::RunEvent::ExitRequested { api, .. } => {
            if !app
                .state::<Lifecycle>()
                .quit
                .lock()
                .is_ok_and(|review| review.approved)
            {
                api.prevent_exit();
                request_quit(app);
                return;
            }
            let _ = life_log_lib::component::shutdown(app);
        }
        _ => {}
    }
}
pub fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let lifecycle = app.state::<Lifecycle>();
    match method {
        "get_close_policy" if args.as_object().is_some_and(|args| args.is_empty()) => {}
        "set_close_policy" => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Input {
                close_to_tray: bool,
            }
            let Input { close_to_tray } =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            if close_to_tray && !lifecycle.tray_available.load(Ordering::Acquire) {
                return Err("tray_unavailable".into());
            }
            life_log_lib::component::set_close_to_tray(app, close_to_tray)?;
            lifecycle
                .close_to_tray
                .store(close_to_tray, Ordering::Release);
        }
        _ => return Err("component_args_invalid".into()),
    }
    Ok(
        serde_json::json!({"closeToTray": lifecycle.close_to_tray.load(Ordering::Acquire), "trayAvailable": lifecycle.tray_available.load(Ordering::Acquire)}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_can_hide_only_with_explicit_preference_and_working_tray() {
        assert!(!should_hide(false, true));
        assert!(!should_hide(true, false));
        assert!(!should_hide(false, false));
        assert!(should_hide(true, true));
    }
    #[test]
    fn quit_requires_current_review_and_cancel_keeps_collection_running() {
        let mut review = QuitReview::default();
        let id = review.request();
        assert_eq!(review.request(), id);
        assert!(review.decide("stale", true).is_err());
        review.decide(&id, false).unwrap();
        assert!(!review.approved);
        let next = review.request();
        assert_ne!(next, id);
        assert!(review.decide(&id, true).is_err());
        review.decide(&next, true).unwrap();
        assert!(review.approved);
        assert!(review.decide(&next, true).is_err());
    }
}
