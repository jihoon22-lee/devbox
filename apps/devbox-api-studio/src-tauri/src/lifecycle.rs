//! Interactive listener close policy. Service workers have a separate process
//! entry point and are never stopped by this interactive instance's controls.
use crate::core::{
    lifecycle::{close_action, CloseAction, ClosePolicy},
    store_file::read_file,
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
    running: AtomicBool,
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
fn close(app: tauri::AppHandle) {
    if app.state::<State>().closing.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let installed = crate::webhook_owner::installed(&app);
        let result = match installed {
            Ok(installed) => match close_action(policy(&app.state::<State>()), installed) {
                CloseAction::KeepOwnerAndQuit => Ok(()),
                CloseAction::StopOwnerAndQuit => {
                    crate::webhook_owner::call(&app, "stop_server", json!({}), u64::MAX)
                        .await
                        .map(|_| ())
                }
            },
            Err(error) => Err(error),
        };
        if result.is_ok() || matches!(crate::webhook_owner::installed(&app), Ok(false)) {
            app.exit(if result.is_ok() { 0 } else { 1 });
        } else {
            let state = app.state::<State>();
            state.closing.store(false, Ordering::Release);
            state.failed.store(true, Ordering::Release);
            let _ = app.emit_to("main", "api-studio://lifecycle", ());
            show(&app);
        }
    });
}
pub(crate) fn observe(app: &tauri::AppHandle, value: &Value) {
    if let Some(state) = app.try_state::<State>() {
        if let Some(running) = value.get("running").and_then(Value::as_bool) {
            state.running.store(running, Ordering::Release);
        }
    }
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
            let keep = parsed
                .map(|value| value.close_policy == ClosePolicy::KeepListening)
                .unwrap_or_else(|| original.as_ref().is_ok_and(|raw| raw.is_none()));
            app.manage(State {
                path,
                storage: Mutex::new(Storage {
                    original: original.unwrap_or(None),
                    writable,
                }),
                keep_listening: AtomicBool::new(keep),
                running: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                failed: AtomicBool::new(false),
            });
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
                close(app.clone());
            }
        })
        .build()
}
pub async fn dispatch_typed(
    app: &tauri::AppHandle,
    call: crate::ipc::lifecycle::LifecycleCall,
) -> Result<Value, String> {
    let state = app.state::<State>();
    use crate::ipc::lifecycle::LifecycleCall;
    if let LifecycleCall::SetClosePolicy { policy: selected } = call {
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| "component_state_unavailable")?;
        if !storage.writable || read_file(&state.path, 4096)? != storage.original {
            return Err("lifecycle_settings_changed".into());
        }
        let raw = serde_json::to_string(&Settings {
            schema_version: 1,
            close_policy: selected,
        })
        .map_err(|_| "component_state_unavailable")?;
        devbox_filesystem::atomic_write(&state.path, raw.as_bytes())
            .map_err(|_| "component_storage_unavailable")?;
        storage.original = Some(raw);
        state
            .keep_listening
            .store(selected == ClosePolicy::KeepListening, Ordering::Release);
        return Ok(Value::Null);
    }
    match call {
        LifecycleCall::LifecycleStatus {} => {
            let installed = crate::webhook_owner::installed(app)?;
            let running =
                match crate::webhook_owner::call(app, "server_status", json!({}), u64::MAX).await {
                    Ok(value) => {
                        observe(app, &value);
                        value["running"].as_bool().unwrap_or(false)
                    }
                    Err(error) => {
                        state.running.store(false, Ordering::Release);
                        return Err(error);
                    }
                };
            Ok(
                json!({ "mainWindowVisible": app.get_webview_window("main").and_then(|window| window.is_visible().ok()), "policy": policy(&state), "trayAvailable": false, "backgroundAvailable": installed, "running": running, "closing": state.closing.load(Ordering::Acquire), "stopFailed": state.failed.load(Ordering::Acquire), "settingsWritable": state.storage.lock().map_err(|_| "component_state_unavailable")?.writable }),
            )
        }
        LifecycleCall::HideMainWindow {} => Err("lifecycle_hide_unavailable".into()),
        LifecycleCall::QuitProduct {} => {
            close(app.clone());
            Ok(Value::Null)
        }
        LifecycleCall::SetClosePolicy { .. } => unreachable!("policy was handled above"),
    }
}

pub(crate) fn operation_rows(
    app: &tauri::AppHandle,
) -> Result<Vec<product_contract::operations::Row>, &'static str> {
    use product_contract::operations::{Phase, Row};
    let Some(state) = app.try_state::<State>() else {
        return Ok(vec![]);
    };
    let running = state.running.load(Ordering::Acquire);
    let closing = state.closing.load(Ordering::Acquire);
    let failed = state.failed.load(Ordering::Acquire);
    if !running && !closing && !failed {
        return Ok(vec![]);
    };
    let phase = if failed {
        Phase::Failed
    } else if closing {
        Phase::Uncancellable
    } else {
        Phase::Running
    };
    Ok(vec![Row::new(
        "api-studio",
        "api-studio.webhooks",
        "webhooks",
        "temporary-listener",
        "임시 Webhook 서버",
        phase,
        &(running, closing, failed),
    )?])
}
