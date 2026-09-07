//! Startup activation precedes all product feature commands and workers.
use crate::core::stores;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::Manager;
struct Startup {
    root: PathBuf,
    active: AtomicBool,
    activation: Mutex<()>,
    failure: Mutex<Option<String>>,
}
pub const COMMANDS: &[&str] = &["status", "start_empty"];
pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "store_unavailable")?;
    std::fs::create_dir_all(&root).map_err(|_| "store_unavailable")?;
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "store_path_invalid")?;
    let manifest = stores::read(&root);
    if !app.manage(Startup {
        root,
        active: AtomicBool::new(false),
        activation: Mutex::new(()),
        failure: Mutex::new(None),
    }) {
        return Err("component_state_conflict".into());
    }
    let result = match manifest {
        Ok(Some(manifest)) => activate(app, &manifest),
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        *app.state::<Startup>()
            .failure
            .lock()
            .map_err(|_| "store_unavailable")? = Some(error);
    }
    Ok(())
}
fn activate(app: &tauri::AppHandle, manifest: &stores::Manifest) -> Result<(), String> {
    let startup = app.state::<Startup>();
    let _activation = startup.activation.try_lock().map_err(|_| "store_busy")?;
    if startup.active.load(Ordering::Acquire) {
        return Ok(());
    }
    let root = &startup.root;
    let integration = root.join("integration");
    knowledge_base_lib::component::create_private_vault(&root.join("notes-vault"))?;
    knowledge_base_lib::component::initialize(
        app,
        &stores::directory(root, manifest, "notes")?,
        Some(integration.clone()),
    )
    .map_err(|_| "component_initialization_failed")?;
    life_log_lib::component::initialize(
        app,
        &stores::directory(root, manifest, "activity")?,
        integration.clone(),
    )?;
    everything_plus_lib::component::initialize(
        app,
        &stores::directory(root, manifest, "search")?,
        Some(integration),
    )
    .map_err(|_| "component_initialization_failed")?;
    crate::lifecycle::load(app)?;
    startup.active.store(true, Ordering::Release);
    Ok(())
}
pub fn require_active(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<Startup>().active.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("setup_required".into())
    }
}
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    if !args.as_object().is_some_and(|object| object.is_empty()) {
        return Err("component_args_invalid".into());
    }
    if let Some(error) = app
        .state::<Startup>()
        .failure
        .lock()
        .map_err(|_| "store_unavailable")?
        .clone()
    {
        return Err(error);
    }
    match method {
        "status" => Ok(json!({"active": app.state::<Startup>().active.load(Ordering::Acquire)})),
        "start_empty" => {
            let manifest = stores::create_empty(&app.state::<Startup>().root)?;
            if let Err(error) = activate(app, &manifest) {
                *app.state::<Startup>()
                    .failure
                    .lock()
                    .map_err(|_| "store_unavailable")? = Some(error.clone());
                return Err(error);
            }
            Ok(json!({"active":true}))
        }
        _ => Err("component_method_invalid".into()),
    }
}
