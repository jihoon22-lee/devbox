//! Startup owns the only activation gate, before any engine opens.
use crate::{
    core::stores,
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
    lease_base: PathBuf,
    active: AtomicBool,
    prepared: AtomicBool,
    initialized: AtomicBool,
    operation: Arc<AtomicBool>,
    owner: Mutex<Option<VaultOwner>>,
    failure: Mutex<Option<String>>,
    configuration: Mutex<()>,
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
    if !app.manage(Startup {
        root: root.clone(),
        lease_base: lease_base.clone(),
        active: AtomicBool::new(false),
        prepared: AtomicBool::new(false),
        initialized: AtomicBool::new(false),
        operation: Arc::new(AtomicBool::new(false)),
        owner: Mutex::new(None),
        failure: Mutex::new(None),
        configuration: Mutex::new(()),
    }) {
        return Err("component_state_conflict".into());
    }
    let result = (|| {
        let binding = crate::vault_binding::initialize(app, root.clone(), lease_base);
        let paused_binding = binding?;
        if !paused_binding {
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
pub fn configure<T>(
    app: &tauri::AppHandle,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let state = app.state::<Startup>();
    let _guard = state.configuration.lock().map_err(|_| "store_busy")?;
    action()
}
pub fn require_active(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<Startup>().active.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err("setup_required".into())
    }
}
pub(crate) fn readiness(app: &tauri::AppHandle) -> (bool, bool) {
    let state = app.state::<Startup>();
    (
        state.active.load(Ordering::Acquire),
        state.prepared.load(Ordering::Acquire),
    )
}
pub(crate) fn require_ready(app: &tauri::AppHandle) -> Result<(), String> {
    let (active, prepared) = readiness(app);
    if active || prepared {
        Ok(())
    } else {
        Err("setup_required".into())
    }
}
fn validate_prepared_stores(root: &Path, manifest: &stores::Manifest) -> Result<(), String> {
    for source in [
        stores::StoreKind::Notes,
        stores::StoreKind::Activity,
        stores::StoreKind::Search,
    ] {
        let connection = Connection::open_with_flags(
            stores::directory(root, manifest, source.key())?.join("data.db"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|_| "store_unavailable")?;
        connection
            .busy_timeout(std::time::Duration::from_millis(100))
            .map_err(|_| "store_unavailable")?;
        stores::validate_store(&connection, source)?;
    }
    Ok(())
}
pub fn require_uninitialized(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Startup>();
    if state.active.load(Ordering::Acquire) || state.prepared.load(Ordering::Acquire) {
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
            "vault_owner_busy" | "vault_binding_unavailable" | "vault_owner_unavailable"
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
    let external = !vault_owner::same_vault(&vault, &root.join("notes-vault"));
    let approval = crate::core::vault_binding::approval_id(&connection, &raw)?;
    if external && approval.is_none() {
        return Err("vault_binding_invalid".into());
    }
    Ok((vault, external))
}
pub fn owner(app: &tauri::AppHandle, manifest: &stores::Manifest) -> Result<VaultOwner, String> {
    let state = app.state::<Startup>();
    let (vault, external) = binding(&state.root, manifest)?;
    if !external {
        knowledge_base_lib::component::create_private_vault(&vault)?;
    }
    vault_owner::acquire(&state.lease_base, &vault)
}

pub fn activate_with_owner(
    app: &tauri::AppHandle,
    manifest: &stores::Manifest,
    owner: VaultOwner,
) -> Result<(), String> {
    let state = app.state::<Startup>();
    require_uninitialized(app)?;
    if product_shell_tauri::suite_import_only(app).map_err(str::to_owned)? {
        validate_prepared_stores(&state.root, manifest)?;
        *state.owner.lock().map_err(|_| "store_unavailable")? = Some(owner);
        *state.failure.lock().map_err(|_| "store_unavailable")? = None;
        state.prepared.store(true, Ordering::Release);
        return Ok(());
    }
    product_shell_tauri::require_suite_writable(app).map_err(str::to_owned)?;
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
        crate::search::initialize(app, &state.root, manifest)?;
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
    if crate::vault_binding::METHODS.contains(&method) {
        return crate::vault_binding::dispatch(app, method, args);
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
                if matches!(
                    error.as_str(),
                    "vault_binding_unavailable" | "vault_binding_invalid"
                ) {
                    return Ok(
                        json!({"active":false,"hasExisting":stores::read(&state.root)?.is_some(),"bindingUnavailable":true,"vaultChange":crate::vault_binding::pending(app)}),
                    );
                }
                return Err(error);
            }
            Ok(
                json!({"active":state.active.load(Ordering::Acquire),"prepared":state.prepared.load(Ordering::Acquire),"hasExisting":stores::read(&state.root)?.is_some(),"vaultChange":crate::vault_binding::pending(app)}),
            )
        }
        "start_empty" | "continue_existing" => {
            if state.active.load(Ordering::Acquire) || state.prepared.load(Ordering::Acquire) {
                return Ok(
                    json!({"active":state.active.load(Ordering::Acquire),"prepared":state.prepared.load(Ordering::Acquire)}),
                );
            }
            if crate::vault_binding::pending(app) {
                return Err("vault_change_conflict".into());
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
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                failure(app, error.clone())?;
                return Err(error);
            }
            Ok(
                json!({"active":state.active.load(Ordering::Acquire),"prepared":state.prepared.load(Ordering::Acquire)}),
            )
        }
        _ => Err("component_method_invalid".into()),
    }
}

pub(crate) fn integration_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    require_active(app)?;
    Ok(app.state::<Startup>().root.join("integration"))
}

#[cfg(test)]
mod suite_preparation_tests {
    #[test]
    fn health_summary_reflects_store_readiness_only() {
        assert_eq!(
            super::summary_flags(false, false, false),
            super::Flags {
                busy: false,
                setup_selected: false,
                review_required: false
            }
        );
        assert_eq!(
            super::summary_flags(true, true, false),
            super::Flags {
                busy: true,
                setup_selected: true,
                review_required: true
            }
        );
        assert_eq!(
            super::summary_flags(false, true, true),
            super::Flags {
                busy: false,
                setup_selected: true,
                review_required: false
            }
        );
    }

    use super::*;
    #[test]
    fn import_only_store_readiness_checks_schema_without_opening_engines() {
        let root = tempfile::tempdir().unwrap();
        let manifest = stores::create_empty(root.path()).unwrap();
        validate_prepared_stores(root.path(), &manifest).unwrap();
        let path = stores::directory(root.path(), &manifest, "search")
            .unwrap()
            .join("data.db");
        Connection::open(path)
            .unwrap()
            .execute_batch("PRAGMA user_version=999")
            .unwrap();
        assert!(validate_prepared_stores(root.path(), &manifest).is_err());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Flags {
    busy: bool,
    setup_selected: bool,
    review_required: bool,
}
fn summary_flags(busy: bool, store_exists: bool, ready: bool) -> Flags {
    Flags {
        busy,
        setup_selected: store_exists,
        review_required: store_exists && !ready,
    }
}
pub(crate) fn suite_status(app: &tauri::AppHandle) -> Result<Value, &'static str> {
    let state = app.try_state::<Startup>().ok_or("migration_unavailable")?;
    let exists = stores::read(&state.root)
        .map_err(|_| "migration_unavailable")?
        .is_some();
    let flags = summary_flags(
        state.operation.load(Ordering::Acquire),
        exists,
        require_ready(app).is_ok(),
    );
    let native = serde_json::to_vec(&(flags.busy, flags.setup_selected, flags.review_required))
        .map_err(|_| "migration_unavailable")?;
    let summary = product_contract::migration_status::Summary::new(
        "knowledge",
        env!("CARGO_PKG_VERSION"),
        flags.busy,
        flags.setup_selected,
        flags.review_required,
        &native,
    )?;
    serde_json::to_value(summary).map_err(|_| "migration_unavailable")
}

#[cfg(test)]
mod external_binding_tests {
    #[test]
    fn external_folder_requires_explicit_approval() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let manifest = super::stores::create_empty(root.path()).unwrap();
        let db = super::stores::directory(root.path(), &manifest, "notes")
            .unwrap()
            .join("data.db");
        let connection = super::Connection::open(db).unwrap();
        connection
            .execute(
                "UPDATE settings SET value=?1 WHERE key='root'",
                [external.path().to_string_lossy().as_ref()],
            )
            .unwrap();
        assert_eq!(
            super::binding(root.path(), &manifest).unwrap_err(),
            "vault_binding_invalid"
        );
    }
}
