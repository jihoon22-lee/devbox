//! Closing quits by default. Continued background collection requires both
//! explicit collection consent and a separate close-to-tray preference.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
#[derive(Default)]
struct Lifecycle {
    tray_available: AtomicBool,
    close_to_tray: AtomicBool,
}
pub const METHODS: &[&str] = &["get_close_policy", "set_close_policy"];
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
            "knowledge-quit" => app.exit(0),
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
            let lifecycle = app.state::<Lifecycle>();
            if should_hide(
                lifecycle.close_to_tray.load(Ordering::Acquire),
                lifecycle.tray_available.load(Ordering::Acquire),
            ) {
                if let Some(window) = app.get_webview_window("main") {
                    if window.hide().is_ok() {
                        api.prevent_close();
                    }
                }
            }
        }
        tauri::RunEvent::ExitRequested { .. } => {
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
}
