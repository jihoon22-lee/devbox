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
fn startup_issue(error: &str) -> &'static str {
    match error {
        "import_database_invalid" => "import_database_invalid",
        "import_schema_unsupported" => "import_schema_unsupported",
        _ => crate::ipc::setup::classify(error),
    }
}
fn failure(app: &tauri::AppHandle, error: String) -> Result<(), String> {
    // Health intentionally denies setup IPC. The owned launcher can still retain
    // this fixed issue token; never print a path, database message, or raw error.
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "knowledge_startup_{}",
        startup_issue(&error)
    );
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
// Only existing selected product stores reach this metadata reader. SQLite must
// roll back interrupted owned writes before reading; queries cannot change rows,
// migrate schemas, or create a missing database during Health preparation.
fn selected_store_metadata(database: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "store_unavailable")?;
    connection
        .busy_timeout(std::time::Duration::from_millis(100))
        .map_err(|_| "store_unavailable")?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON")
        .map_err(|_| "store_unavailable")?;
    Ok(connection)
}

fn validate_prepared_stores(root: &Path, manifest: &stores::Manifest) -> Result<(), String> {
    for source in [
        stores::StoreKind::Notes,
        stores::StoreKind::Activity,
        stores::StoreKind::Search,
    ] {
        let connection = selected_store_metadata(
            &stores::directory(root, manifest, source.key())?.join("data.db"),
        )?;
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
    let connection = selected_store_metadata(&path)?;
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
        knowledge_vault_engine::component::create_private_vault(
            &vault,
            &stores::directory(&state.root, manifest, "notes")?.join("data.db"),
        )?;
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
        knowledge_vault_engine::component::initialize(
            app,
            &stores::directory(&state.root, manifest, "notes")?,
            Some(integration.clone()),
        )
        .map_err(|_| "component_initialization_failed")?;
        if !crate::collector_owner::installed(app)? {
            activity_engine::component::initialize(
                app,
                &stores::directory(&state.root, manifest, "activity")?,
                integration.clone(),
            )?;
            content_index_engine::component::initialize(
                app,
                &stores::directory(&state.root, manifest, "search")?,
                Some(integration),
            )
            .map_err(|_| "component_initialization_failed")?;
        }
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
pub fn dispatch_typed(
    app: &tauri::AppHandle,
    call: crate::ipc::setup::StartupCall,
) -> Result<Value, String> {
    use crate::ipc::setup::StartupCall;
    let state = app.state::<Startup>();
    match call {
        StartupCall::LoadRecovery {} => {
            // Selection is native manifest-owned, and this path grants local journal reads only.
            let manifest = stores::read(&state.root)?.ok_or("store_invalid")?;
            let directory = stores::directory(&state.root, &manifest, "notes")?;
            let database = directory.join("data.db");
            devbox_filesystem::ensure_no_links(&database).map_err(|_| "store_invalid")?;
            let connection =
                Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(|_| "store_unavailable")?;
            stores::validate_store(&connection, stores::StoreKind::Notes)?;
            let view = knowledge_vault_engine::component::load_offline_recovery(&directory)?;
            serde_json::to_value(view).map_err(|_| "component_response_invalid".into())
        }
        StartupCall::Status {} => {
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
        call @ (StartupCall::StartEmpty {} | StartupCall::ContinueExisting {}) => {
            if state.active.load(Ordering::Acquire) || state.prepared.load(Ordering::Acquire) {
                return Ok(
                    json!({"active":state.active.load(Ordering::Acquire),"prepared":state.prepared.load(Ordering::Acquire)}),
                );
            }
            if crate::vault_binding::pending(app) {
                return Err("vault_change_conflict".into());
            }
            let _reservation = reserve(app)?;
            let manifest = if matches!(call, StartupCall::StartEmpty {}) {
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
    fn interrupted_owned_store_writer_fixture() {
        let Some(path) = std::env::var_os("DEVBOX_STARTUP_JOURNAL_FIXTURE") else {
            return;
        };
        let path = PathBuf::from(path);
        let table = if path.parent().unwrap().file_name().unwrap() == "search" {
            "meta"
        } else {
            "settings"
        };
        let connection =
            Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        connection
            .execute_batch("PRAGMA cache_size=1; PRAGMA synchronous=FULL; BEGIN IMMEDIATE;")
            .unwrap();
        for index in 0..32 {
            connection
                .execute(
                    &format!("INSERT INTO {table}(key,value) VALUES(?1,?2)"),
                    rusqlite::params![format!("uncommitted-{index}"), "x".repeat(8192)],
                )
                .unwrap();
        }
        std::process::exit(0);
    }

    fn interrupt_writer(path: &Path) {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "startup::suite_preparation_tests::interrupted_owned_store_writer_fixture",
                "--nocapture",
            ])
            .env("DEVBOX_STARTUP_JOURNAL_FIXTURE", path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(path.with_extension("db-journal").metadata().unwrap().len() > 0);
    }

    #[test]
    fn startup_binding_recovers_selected_notes_hot_journal() {
        let root = tempfile::tempdir().unwrap();
        let manifest = stores::create_empty(root.path()).unwrap();
        let path = stores::directory(root.path(), &manifest, "notes")
            .unwrap()
            .join("data.db");
        interrupt_writer(&path);
        assert_eq!(
            binding(root.path(), &manifest).unwrap(),
            (root.path().join("notes-vault"), false)
        );
        assert!(!path.with_extension("db-journal").exists());
    }

    #[test]
    fn health_preparation_recovers_selected_stores_hot_journals() {
        for component in ["notes", "activity", "search"] {
            let root = tempfile::tempdir().unwrap();
            let manifest = stores::create_empty(root.path()).unwrap();
            let path = stores::directory(root.path(), &manifest, component)
                .unwrap()
                .join("data.db");
            interrupt_writer(&path);
            validate_prepared_stores(root.path(), &manifest).unwrap();
            let connection =
                Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let table = if component == "search" {
                "meta"
            } else {
                "settings"
            };
            assert_eq!(
                connection
                    .query_row(
                        &format!("SELECT count(*) FROM {table} WHERE key LIKE 'uncommitted-%'"),
                        [],
                        |row| row.get::<_, i64>(0)
                    )
                    .unwrap(),
                0
            );
            assert!(!path.with_extension("db-journal").exists());
        }
    }

    #[test]
    fn selected_metadata_reader_cannot_create_database_or_mutate_settings() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing.db");
        assert!(selected_store_metadata(&missing).is_err());
        assert!(!missing.exists());
        let manifest = stores::create_empty(root.path()).unwrap();
        let path = stores::directory(root.path(), &manifest, "notes")
            .unwrap()
            .join("data.db");
        let connection = selected_store_metadata(&path).unwrap();
        assert!(connection
            .execute("UPDATE settings SET value='changed' WHERE key='root'", [])
            .is_err());
        assert!(connection
            .execute_batch("CREATE TABLE changed(value TEXT)")
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_selected_database_cannot_trigger_recovery_outside_its_store() {
        let root = tempfile::tempdir().unwrap();
        let manifest = stores::create_empty(root.path()).unwrap();
        let path = stores::directory(root.path(), &manifest, "notes")
            .unwrap()
            .join("data.db");
        let outside = root.path().join("outside.db");
        std::fs::rename(&path, &outside).unwrap();
        let original = std::fs::read(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(binding(root.path(), &manifest).is_err());
        assert!(validate_prepared_stores(root.path(), &manifest).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), original);
    }

    #[test]
    fn startup_diagnostic_uses_fixed_codes_without_private_error_values() {
        for issue in [
            "store_unavailable",
            "vault_binding_invalid",
            "import_database_invalid",
            "import_schema_unsupported",
        ] {
            assert_eq!(startup_issue(issue), issue);
        }
        for private in [
            "C:\\private\\notes.db",
            "password=secret",
            "store_unavailable path=private",
            "unknown_future_issue",
        ] {
            assert_eq!(startup_issue(private), "unavailable");
        }
    }

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
