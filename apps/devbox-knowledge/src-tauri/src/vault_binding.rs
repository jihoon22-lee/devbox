//! Reviewed next-start rebinding keeps live editors and old watcher ownership
//! untouched. Remote filesystem work precedes the short local activation gate.
use crate::core::retirement::{Lease, Pool};
use crate::core::{stores, vault_binding as data};
use knowledge_base_lib::component::ProductVault;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::Manager;
const TIMEOUT: Duration = Duration::from_secs(5);
const PREVIEW_TTL: Duration = Duration::from_secs(300);
struct Job {
    id: String,
    cancel: Arc<AtomicBool>,
    started: Instant,
    committed: Arc<AtomicBool>,
    result: Option<Result<Value, String>>,
}
struct Preview {
    id: String,
    schedule: data::Schedule,
    vault: Lease<ProductVault>,
    created: Instant,
}
struct Binding {
    root: PathBuf,
    lease_base: PathBuf,
    legacy: PathBuf,
    job: Arc<Mutex<Option<Job>>>,
    preview: Arc<Mutex<Option<Preview>>>,
    retirement: Mutex<Option<Arc<Pool<ProductVault>>>>,
}
pub const METHODS: &[&str] = &[
    "vault_change_status",
    "schedule_vault_change",
    "cancel_vault_change",
    "prepare_vault_change",
    "apply_vault_change",
    "discard_vault_preview",
    "vault_change_job",
];
pub fn initialize(
    app: &tauri::AppHandle,
    root: PathBuf,
    lease_base: PathBuf,
) -> Result<bool, String> {
    let pending = data::read(&root);
    if !app.manage(Binding {
        root,
        legacy: lease_base.join("com.devbox.knowledgebase/data.db"),
        lease_base,
        job: Arc::default(),
        preview: Arc::default(),
        retirement: Mutex::default(),
    }) {
        return Err("component_state_conflict".into());
    }
    Ok(pending?.is_some())
}
pub fn pending(app: &tauri::AppHandle) -> bool {
    app.try_state::<Binding>()
        .is_some_and(|state| data::read(&state.root).map_or(true, |value| value.is_some()))
}
fn database(
    root: &Path,
    manifest: &stores::Manifest,
    writable: bool,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Connection, String> {
    let path = stores::directory(root, manifest, "notes")?.join("data.db");
    open_database(&path, writable, cancel, deadline)
}
fn open_database(
    path: &Path,
    writable: bool,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Connection, String> {
    // This is the already-selected product-owned database. Even a read-only
    // preview must let SQLite recover a hot journal left by process exit.
    // No CREATE flag, schema migration, legacy source write, or row mutation.
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| "vault_change_invalid")?;
    conn.busy_timeout(Duration::from_millis(50))
        .map_err(|_| "vault_change_invalid")?;
    conn.progress_handler(
        1000,
        Some(move || cancel.load(Ordering::Acquire) || Instant::now() >= deadline),
    );
    if !writable {
        conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON")
            .map_err(|_| "vault_change_invalid")?;
    }
    crate::core::import_rows::validate_owned_store(&conn, crate::core::import_rows::Source::Notes)?;
    Ok(conn)
}

fn boundary(cancel: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("vault_change_cancelled".into())
    } else if Instant::now() >= deadline {
        Err("vault_change_timeout".into())
    } else {
        Ok(())
    }
}
fn current_schedule(state: &Binding, schedule: &data::Schedule) -> Result<(), String> {
    if data::read(&state.root)?.as_ref() != Some(schedule)
        || stores::read(&state.root)?.as_ref() != Some(&schedule.base)
    {
        return Err("vault_change_stale".into());
    }
    Ok(())
}
fn legacy_guard_path(state: &Binding) -> Option<&Path> {
    state.legacy.exists().then_some(state.legacy.as_path())
}
fn prepare(
    app: &tauri::AppHandle,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<Value, String> {
    let state = app.state::<Binding>();
    crate::startup::require_uninitialized(app)?;
    let schedule = data::read(&state.root)?.ok_or("vault_change_stale")?;
    current_schedule(&state, &schedule)?;
    let conn = database(&state.root, &schedule.base, false, cancel.clone(), deadline)?;
    let already_applied = data::committed(&conn, &schedule)?;
    let bound_root = data::current_root(&conn)?;
    if !already_applied && bound_root != schedule.previous_root {
        return Err("vault_change_stale".into());
    }
    drop(conn);
    let pool = {
        let mut slot = state.retirement.lock().map_err(|_| "store_busy")?;
        if slot.is_none() {
            *slot = Some(Pool::new(2)?);
        }
        slot.as_ref().ok_or("store_busy")?.clone()
    };
    if !pool.available() {
        return Err("store_busy".into());
    }
    // A preview only inspects an existing directory; it creates no layout/files.
    let vault = ProductVault::inspect(Path::new(&schedule.target))?;
    if already_applied && Path::new(&bound_root) != vault.path() {
        return Err("vault_change_stale".into());
    }
    let vault = pool.hold(vault).ok_or("store_busy")?;
    let owner =
        crate::vault_owner::acquire(&state.lease_base, vault.path(), legacy_guard_path(&state))?;
    vault.revalidate()?;
    drop(owner);
    boundary(&cancel, deadline)?;
    crate::startup::require_uninitialized(app)?;
    current_schedule(&state, &schedule)?;
    let id = uuid::Uuid::new_v4().to_string();
    let result = json!({"previewId":id,"target":schedule.target,"previousRoot":schedule.previous_root,"alreadyApplied":already_applied,"expiresInSeconds":300});
    let mut slot = state.preview.lock().map_err(|_| "store_busy")?;
    boundary(&cancel, deadline)?;
    crate::startup::require_uninitialized(app)?;
    current_schedule(&state, &schedule)?;
    *slot = Some(Preview {
        id,
        schedule,
        vault,
        created: Instant::now(),
    });
    Ok(result)
}
fn apply(
    app: &tauri::AppHandle,
    preview_id: &str,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    committed: Arc<AtomicBool>,
) -> Result<Value, String> {
    let state = app.state::<Binding>();
    crate::startup::require_uninitialized(app)?;
    let preview = {
        let mut slot = state.preview.lock().map_err(|_| "store_busy")?;
        if !slot.as_ref().is_some_and(|preview| {
            preview.id == preview_id && preview.created.elapsed() < PREVIEW_TTL
        }) {
            return Err("vault_change_stale".into());
        }
        slot.take().ok_or("vault_change_stale")?
    };
    current_schedule(&state, &preview.schedule)?;
    boundary(&cancel, deadline)?;
    let owner = crate::vault_owner::acquire(
        &state.lease_base,
        preview.vault.path(),
        legacy_guard_path(&state),
    )?;
    preview.vault.revalidate()?;
    // Approved creation affects only fixed empty layout folders. Cancellation
    // or a later DB failure never deletes those folders or existing vault bytes.
    preview.vault.prepare_layout()?;
    boundary(&cancel, deadline)?;
    // No remote filesystem operation follows acquisition of this gate. A slow
    // preview/layout probe cannot prevent cancelling and using the old store.
    let _reservation = crate::startup::reserve(app)?;
    crate::startup::configure(app, || {
        current_schedule(&state, &preview.schedule)?;
        boundary(&cancel, deadline)?;
        let conn = database(
            &state.root,
            &preview.schedule.base,
            true,
            cancel.clone(),
            deadline,
        )?;
        data::apply(
            &conn,
            &preview.schedule,
            preview
                .vault
                .path()
                .to_str()
                .ok_or("vault_change_invalid")?,
        )?;
        committed.store(true, Ordering::Release);
        // If this write fails, the DB receipt recognizes the committed plan on
        // restart. Clearing the schedule is not the configuration commit point.
        data::write(&state.root, None)
    })?;
    crate::startup::activate_with_owner(app, &preview.schedule.base, owner)?;
    Ok(json!({"active":true}))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Id {
    id: String,
}
fn id(args: Value) -> Result<String, String> {
    let value: Id = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    if uuid::Uuid::parse_str(&value.id).is_err() {
        return Err("component_args_invalid".into());
    }
    Ok(value.id)
}
fn empty(args: &Value) -> Result<(), String> {
    if args.as_object().is_some_and(|args| args.is_empty()) {
        Ok(())
    } else {
        Err("component_args_invalid".into())
    }
}
pub fn dispatch(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value, String> {
    let state = app.try_state::<Binding>().ok_or("vault_change_invalid")?;
    match method {
        "vault_change_status" => {
            empty(&args)?;
            Ok(json!({"schedule":data::read(&state.root)?}))
        }
        "schedule_vault_change" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                path: String,
            }
            let input: Input =
                serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            let target = data::target(&input.path)?;
            if crate::startup::require_active(app).is_err() {
                crate::startup::require_uninitialized(app)?;
            }
            crate::startup::configure(app, || {
                if crate::migration::scheduled(app) {
                    return Err("vault_change_conflict".into());
                }
                let base = stores::read(&state.root)?.ok_or("setup_required")?;
                let conn = database(
                    &state.root,
                    &base,
                    false,
                    Arc::new(AtomicBool::new(false)),
                    Instant::now() + TIMEOUT,
                )?;
                let previous_root = data::current_root(&conn)?;
                if crate::vault_owner::same_vault(Path::new(&target), Path::new(&previous_root)) {
                    return Err("vault_change_same".into());
                }
                let schedule = data::Schedule {
                    schema_version: 1,
                    id: uuid::Uuid::new_v4().to_string(),
                    target,
                    base,
                    previous_root,
                };
                data::write(&state.root, Some(&schedule))?;
                Ok(json!({"schedule":schedule}))
            })
        }
        "cancel_vault_change" => {
            empty(&args)?;
            if let Some(job) = state.job.lock().map_err(|_| "store_busy")?.as_ref() {
                job.cancel.store(true, Ordering::Release);
            }
            *state.preview.lock().map_err(|_| "store_busy")? = None;
            crate::startup::configure(app, || {
                data::read(&state.root)?;
                data::write(&state.root, None)?;
                Ok(json!({"schedule":null}))
            })
        }
        "discard_vault_preview" => {
            let id = id(args)?;
            let mut slot = state.preview.lock().map_err(|_| "store_busy")?;
            if slot.as_ref().is_some_and(|preview| preview.id == id) {
                *slot = None;
            }
            Ok(Value::Null)
        }
        "vault_change_job" => {
            let id = id(args)?;
            let slot = state.job.lock().map_err(|_| "store_busy")?;
            let job = slot
                .as_ref()
                .filter(|job| job.id == id)
                .ok_or("vault_change_stale")?;
            match &job.result {
                Some(Ok(value)) => Ok(json!({"state":"succeeded","value":value})),
                Some(Err(error)) => Ok(
                    json!({"state":"failed","issue":crate::component::issue(error),"committed":job.committed.load(Ordering::Acquire)}),
                ),
                None if job.started.elapsed() >= TIMEOUT
                    && !job.committed.load(Ordering::Acquire) =>
                {
                    job.cancel.store(true, Ordering::Release);
                    Ok(json!({"state":"failed","issue":"vault_change_timeout"}))
                }
                None => {
                    Ok(json!({"state":"running","committed":job.committed.load(Ordering::Acquire)}))
                }
            }
        }
        "prepare_vault_change" | "apply_vault_change" => {
            crate::startup::require_uninitialized(app)?;
            let preview_id = if method == "apply_vault_change" {
                Some(id(args)?)
            } else {
                empty(&args)?;
                None
            };
            let mut slot = state.job.lock().map_err(|_| "store_busy")?;
            if slot.as_ref().is_some_and(|job| job.result.is_none()) {
                return Err("store_busy".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let cancel = Arc::new(AtomicBool::new(false));
            let started = Instant::now();
            let committed = Arc::new(AtomicBool::new(false));
            *slot = Some(Job {
                id: id.clone(),
                cancel: cancel.clone(),
                started,
                committed: committed.clone(),
                result: None,
            });
            let jobs = state.job.clone();
            let app = app.clone();
            let job_id = id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let result = if let Some(preview_id) = preview_id {
                    apply(&app, &preview_id, cancel, started + TIMEOUT, committed)
                } else {
                    prepare(&app, cancel, started + TIMEOUT)
                };
                if let Ok(mut slot) = jobs.lock() {
                    if let Some(job) = slot.as_mut().filter(|job| job.id == job_id) {
                        job.result = Some(result);
                    }
                }
            });
            Ok(json!({"jobId":id}))
        }
        _ => Err("component_method_invalid".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_writer_fixture() {
        let Some(path) = std::env::var_os("DEVBOX_VAULT_JOURNAL_FIXTURE") else {
            return;
        };
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        conn.execute_batch("PRAGMA cache_size=1; PRAGMA synchronous=FULL; BEGIN IMMEDIATE;")
            .unwrap();
        for index in 0..32 {
            conn.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2)",
                rusqlite::params![format!("uncommitted-{index}"), "x".repeat(8192)],
            )
            .unwrap();
        }
        // Simulate process exit while a derived-index writer is still active.
        // SQLite, not the fixture, must recover its hot rollback journal.
        std::process::exit(0);
    }

    #[test]
    fn preview_recovers_owned_hot_journal_without_allowing_setting_writes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("notes.db");
        knowledge_base_lib::component::create_empty_store(&path, root.path()).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vault_binding::tests::interrupted_writer_fixture",
                "--nocapture",
            ])
            .env("DEVBOX_VAULT_JOURNAL_FIXTURE", &path)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(path.with_extension("db-journal").metadata().unwrap().len() > 0);
        let conn = open_database(
            &path,
            false,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + TIMEOUT,
        )
        .unwrap();
        assert_eq!(
            data::current_root(&conn).unwrap(),
            root.path().to_string_lossy()
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM settings WHERE key LIKE 'uncommitted-%'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert!(conn
            .execute("UPDATE settings SET value='changed' WHERE key='root'", [])
            .is_err());
        drop(conn);
        assert!(!path.with_extension("db-journal").exists());
        assert!(open_database(
            &root.path().join("missing.db"),
            false,
            Arc::new(AtomicBool::new(false)),
            Instant::now() + TIMEOUT
        )
        .is_err());
        assert!(!root.path().join("missing.db").exists());
    }
}
