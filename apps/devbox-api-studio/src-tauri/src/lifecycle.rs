//! Interactive listener close policy. Service workers have a separate process
//! entry point and are never stopped by this interactive instance's controls.
use crate::core::{
    import_repository::read_file,
    lifecycle::{close_action, CloseAction, ClosePolicy},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::{Emitter, Manager};
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Settings {
    schema_version: u32,
    close_policy: ClosePolicy,
}
struct Storage {
    original: Option<String>,
    writable: bool,
}
struct State {
    path: PathBuf,
    storage: Mutex<Storage>,
    keep_listening: AtomicBool,
    tray: AtomicBool,
    closing: AtomicBool,
    failed: AtomicBool,
}
fn policy(state: &State) -> ClosePolicy {
    if state.keep_listening.load(Ordering::Acquire) {
        ClosePolicy::KeepListening
    } else {
        ClosePolicy::StopOnClose
    }
}
pub fn require_open(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<State>().closing.load(Ordering::Acquire) {
        Err("component_closing".into())
    } else {
        Ok(())
    }
}
fn show(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
fn stop(app: tauri::AppHandle, quit: bool) {
    if quit && app.state::<State>().closing.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = webhook_lab_lib::component::stop_owned_listener(&app);
        if quit {
            // Listener sockets/threads belong to this process. Even a poisoned
            // soft-stop state must not prevent explicit full exit; OS teardown
            // releases them and the API owner's kill-on-close process Jobs.
            app.exit(if result.is_ok() { 0 } else { 1 });
            return;
        }
        let state = app.state::<State>();
        state.failed.store(result.is_err(), Ordering::Release);
        if quit {
            state.closing.store(false, Ordering::Release);
        }
        let _ = app.emit_to("main", "api-studio://lifecycle", ());
        if result.is_err() {
            show(&app);
        }
    });
}
#[cfg(windows)]
fn install_tray(app: &tauri::AppHandle) -> tauri::Result<bool> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::TrayIconBuilder,
    };
    let open = MenuItem::with_id(
        app,
        "api-studio-open",
        "API Studio 열기",
        true,
        None::<&str>,
    )?;
    let stop_item = MenuItem::with_id(
        app,
        "api-studio-stop",
        "임시 Webhook 서버 중지",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "api-studio-quit",
        "API Studio 완전히 종료",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&open, &stop_item, &quit])?;
    let Some(icon) = app.default_window_icon().cloned() else {
        return Ok(false);
    };
    TrayIconBuilder::with_id("api-studio-listener")
        .tooltip("API Studio · Webhook 서버")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "api-studio-open" => show(app),
            "api-studio-stop" => stop(app.clone(), false),
            "api-studio-quit" => stop(app.clone(), true),
            _ => {}
        })
        .build(app)?;
    Ok(true)
}
#[cfg(not(windows))]
fn install_tray(_: &tauri::AppHandle) -> tauri::Result<bool> {
    Ok(false)
}
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::<tauri::Wry, ()>::new("api-studio-lifecycle")
        .setup(|app, _| {
            let path = app
                .path()
                .app_local_data_dir()?
                .join("listener-lifecycle.json");
            let original = read_file(&path, 4096);
            let parsed = original
                .as_ref()
                .ok()
                .and_then(|raw| raw.as_ref())
                .and_then(|raw| serde_json::from_str::<Settings>(raw).ok())
                .filter(|value| value.schema_version == 1);
            let writable = original
                .as_ref()
                .is_ok_and(|raw| raw.is_none() || parsed.is_some());
            let keep = parsed.is_some_and(|value| value.close_policy == ClosePolicy::KeepListening);
            app.manage(State {
                path,
                storage: Mutex::new(Storage {
                    original: original.unwrap_or(None),
                    writable,
                }),
                keep_listening: AtomicBool::new(keep),
                tray: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                failed: AtomicBool::new(false),
            });
            let available = install_tray(app).unwrap_or(false);
            app.state::<State>()
                .tray
                .store(available, Ordering::Release);
            Ok(())
        })
        .on_event(|app, event| {
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } = event
            {
                if label != "main" {
                    return;
                }
                api.prevent_close();
                let state = app.state::<State>();
                if state.closing.load(Ordering::Acquire) {
                    return;
                }
                match close_action(
                    policy(&state),
                    webhook_lab_lib::component::listener_running(app),
                    state.tray.load(Ordering::Acquire),
                ) {
                    CloseAction::Quit => stop(app.clone(), true),
                    CloseAction::Hide => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                }
            }
        })
        .build()
}
pub const COMMANDS: &[&str] = &[
    "lifecycle_status",
    "set_close_policy",
    "hide_main_window",
    "quit_product",
];
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    let state = app.state::<State>();
    if method == "set_close_policy" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            policy: ClosePolicy,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
        if input.policy == ClosePolicy::KeepListening && !state.tray.load(Ordering::Acquire) {
            return Err("lifecycle_tray_unavailable".into());
        }
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| "component_state_unavailable")?;
        if !storage.writable || read_file(&state.path, 4096)? != storage.original {
            return Err("lifecycle_settings_changed".into());
        }
        let raw = serde_json::to_string(&Settings {
            schema_version: 1,
            close_policy: input.policy,
        })
        .map_err(|_| "component_state_unavailable")?;
        devbox_filesystem::atomic_write(&state.path, raw.as_bytes())
            .map_err(|_| "component_storage_unavailable")?;
        storage.original = Some(raw);
        state.keep_listening.store(
            input.policy == ClosePolicy::KeepListening,
            Ordering::Release,
        );
        return Ok(Value::Null);
    }
    if args.as_object().is_none_or(|args| !args.is_empty()) {
        return Err("component_args_invalid".into());
    }
    match method {
        "lifecycle_status" => Ok(
            json!({ "policy": policy(&state), "trayAvailable": state.tray.load(Ordering::Acquire), "running": webhook_lab_lib::component::listener_running(app), "closing": state.closing.load(Ordering::Acquire), "stopFailed": state.failed.load(Ordering::Acquire), "settingsWritable": state.storage.lock().map_err(|_| "component_state_unavailable")?.writable }),
        ),
        "hide_main_window" => {
            if close_action(
                policy(&state),
                webhook_lab_lib::component::listener_running(app),
                state.tray.load(Ordering::Acquire),
            ) != CloseAction::Hide
            {
                return Err("lifecycle_hide_unavailable".into());
            }
            app.get_webview_window("main")
                .ok_or("component_state_unavailable")?
                .hide()
                .map_err(|_| "component_state_unavailable")?;
            Ok(Value::Null)
        }
        "quit_product" => {
            stop(app.clone(), true);
            Ok(Value::Null)
        }
        _ => Err("component_method_unavailable".into()),
    }
}
