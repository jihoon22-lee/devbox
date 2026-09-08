//! Startup and migration own the only activation gate, before any engine opens.
use crate::{
    core::{import_rows, stores},
    vault_owner::{self, VaultOwner},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;
struct Startup {
    root: PathBuf,
    legacy: PathBuf,
    lease_base: PathBuf,
    active: AtomicBool,
    initialized: AtomicBool,
    operation: Arc<AtomicBool>,
    owner: Mutex<Option<VaultOwner>>,
    failure: Mutex<Option<String>>,
}
pub struct Reservation(Arc<AtomicBool>);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
pub const COMMANDS: &[&str] = &["status", "start_empty", "continue_existing"];
pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "store_unavailable")?;
    std::fs::create_dir_all(&root).map_err(|_| "store_unavailable")?;
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "store_path_invalid")?;
    let lease_base = app
        .path()
        .local_data_dir()
        .map_err(|_| "store_unavailable")?;
    let legacy = lease_base.clone();
    if !app.manage(Startup {
        root: root.clone(),
        legacy: legacy.clone(),
        lease_base,
        active: AtomicBool::new(false),
        initialized: AtomicBool::new(false),
        operation: Arc::new(AtomicBool::new(false)),
        owner: Mutex::new(None),
        failure: Mutex::new(None),
    }) {
        return Err("component_state_conflict".into());
    }
    let result = (|| {
        let paused = crate::migration::initialize(app, root.clone(), legacy)?;
        if !paused {
            if let Some(manifest) = stores::read(&root)? {
                let _reservation = reserve(app)?;
                let owner = owner(app, &manifest)?;
                activate_with_owner(app, &manifest, owner)?;
            }
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        failure(app, error)?;
    }
    Ok(())
}
fn failure(app: &tauri::AppHandle, error: String) -> Result<(), String> {
    *app.state::<Startup>()
        .failure
        .lock()
        .map_err(|_| "store_unavailable")? = Some(error);
    Ok(())
}
pub fn require_active(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<Startup>().active.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("setup_required".into())
    }
}
pub fn require_uninitialized(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Startup>();
    if state.active.load(Ordering::Acquire) {
        return Err("import_restart_required".into());
    }
    if state.initialized.load(Ordering::Acquire) {
        return Err("component_initialization_failed".into());
    }
    if let Some(error) = state
        .failure
        .lock()
        .map_err(|_| "store_unavailable")?
        .as_ref()
    {
        if !matches!(
            error.as_str(),
            "legacy_writer_active"
                | "vault_owner_busy"
                | "vault_binding_unavailable"
                | "vault_owner_unavailable"
        ) {
            return Err(error.clone());
        }
    }
    Ok(())
}
pub fn reserve(app: &tauri::AppHandle) -> Result<Reservation, String> {
    require_uninitialized(app)?;
    let operation = app.state::<Startup>().operation.clone();
    operation
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "store_busy")?;
    let reservation = Reservation(operation);
    require_uninitialized(app)?;
    Ok(reservation)
}
pub fn binding(root: &Path, manifest: &stores::Manifest) -> Result<(PathBuf, bool), String> {
    let path = stores::directory(root, manifest, "notes")?.join("data.db");
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "store_unavailable")?;
    let raw:Option<String>=connection.query_row("SELECT CASE WHEN length(CAST(value AS BLOB)) BETWEEN 1 AND 32768 THEN value ELSE NULL END FROM settings WHERE key='root'",[],|r|r.get(0)).map_err(|_|"vault_binding_invalid")?;
    let raw = raw.ok_or("vault_binding_invalid")?;
    if raw.chars().any(char::is_control) {
        return Err("vault_binding_invalid".into());
    }
    let vault = PathBuf::from(&raw);
    let legacy = vault != root.join("notes-vault");
    if legacy {
        import_rows::verify_legacy_binding(&connection, &raw)?;
    }
    Ok((vault, legacy))
}
pub fn owner(app: &tauri::AppHandle, manifest: &stores::Manifest) -> Result<VaultOwner, String> {
    let state = app.state::<Startup>();
    let (vault, legacy) = binding(&state.root, manifest)?;
    if !legacy {
        knowledge_base_lib::component::create_private_vault(&vault)?;
    }
    let legacy_path = state
        .legacy
        .join("com.devbox.knowledgebase")
        .join("data.db");
    vault_owner::acquire(
        &state.lease_base,
        &vault,
        legacy.then_some(legacy_path.as_path()),
    )
}
pub fn activate_with_owner(
    app: &tauri::AppHandle,
    manifest: &stores::Manifest,
    owner: VaultOwner,
) -> Result<(), String> {
    let state = app.state::<Startup>();
    require_uninitialized(app)?;
    *state.owner.lock().map_err(|_| "store_unavailable")? = Some(owner);
    state.initialized.store(true, Ordering::Release);
    let result = (|| {
        let integration = state.root.join("integration");
        knowledge_base_lib::component::initialize(
            app,
            &stores::directory(&state.root, manifest, "notes")?,
            Some(integration.clone()),
        )
        .map_err(|_| "component_initialization_failed")?;
        life_log_lib::component::initialize(
            app,
            &stores::directory(&state.root, manifest, "activity")?,
            integration.clone(),
        )?;
        everything_plus_lib::component::initialize(
            app,
            &stores::directory(&state.root, manifest, "search")?,
            Some(integration),
        )
        .map_err(|_| "component_initialization_failed")?;
        crate::lifecycle::load(app)?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        failure(app, "component_initialization_failed".into())?;
        return Err(error);
    }
    *state.failure.lock().map_err(|_| "store_unavailable")? = None;
    state.active.store(true, Ordering::Release);
    Ok(())
}
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    if crate::migration::METHODS.contains(&method) {
        return crate::migration::dispatch(app, method, args);
    }
    if !args.as_object().is_some_and(|object| object.is_empty()) {
        return Err("component_args_invalid".into());
    }
    let state = app.state::<Startup>();
    match method {
        "status" => {
            if let Some(error) = state
                .failure
                .lock()
                .map_err(|_| "store_unavailable")?
                .clone()
            {
                return Err(error);
            }
            Ok(
                json!({"active":state.active.load(Ordering::Acquire),"scheduled":crate::migration::scheduled(app),"hasExisting":stores::read(&state.root)?.is_some()}),
            )
        }
        "start_empty" | "continue_existing" => {
            if state.active.load(Ordering::Acquire) {
                crate::migration::finish_recovery(app)?;
                return Ok(json!({"active":true}));
            }
            let _reservation = reserve(app)?;
            let manifest = if method == "start_empty" {
                stores::create_empty(&state.root)?
            } else {
                stores::read(&state.root)?.ok_or("setup_required")?
            };
            let result = (|| {
                let owner = owner(app, &manifest)?;
                activate_with_owner(app, &manifest, owner)?;
                crate::migration::finish_recovery(app)
            })();
            if let Err(error) = result {
                failure(app, error.clone())?;
                return Err(error);
            }
            Ok(json!({"active":true}))
        }
        _ => Err("component_method_invalid".into()),
    }
}
