//! UI jobs return immediately; bounded SQLite work runs off the IPC executor.
//! Source paths come from native configuration, never renderer-supplied paths.
use crate::core::{
    import_plan::{self, Phase, Plan},
    import_rows::Source,
    stores,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;
struct Job {
    id: String,
    cancel: Arc<AtomicBool>,
    result: Option<Result<Value, String>>,
}
struct Migration {
    root: PathBuf,
    legacy: PathBuf,
    scheduled: AtomicBool,
    job: Mutex<Option<Job>>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Schedule {
    schema_version: u32,
    requested: bool,
}
pub const METHODS: &[&str] = &[
    "list_import_sources",
    "list_imports",
    "prepare_import",
    "activate_import",
    "discard_import",
    "rollback_import",
    "import_job",
    "cancel_import_job",
    "schedule_import",
    "cancel_scheduled_import",
];
fn schedule_path(root: &Path) -> PathBuf {
    root.join("import-on-next-start.json")
}
fn read_schedule(root: &Path) -> Result<bool, String> {
    let path = schedule_path(root);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err("import_storage_unavailable".into()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > 1024 => {
            return Err("import_plan_invalid".into())
        }
        _ => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|_| "import_storage_unavailable")?
        .take(1025)
        .read_to_end(&mut bytes)
        .map_err(|_| "import_storage_unavailable")?;
    if bytes.len() > 1024 {
        return Err("import_plan_invalid".into());
    }
    let schedule: Schedule = serde_json::from_slice(&bytes).map_err(|_| "import_plan_invalid")?;
    if schedule.schema_version != 1 {
        return Err("import_schema_unsupported".into());
    }
    Ok(schedule.requested)
}
fn save_schedule(root: &Path, requested: bool) -> Result<(), String> {
    let path = schedule_path(root);
    if fs::symlink_metadata(&path).is_ok() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "import_path_invalid")?;
    }
    devbox_filesystem::atomic_write(
        path,
        &serde_json::to_vec(&Schedule {
            schema_version: 1,
            requested,
        })
        .map_err(|_| "import_plan_invalid")?,
    )
    .map_err(|_| "import_storage_unavailable".into())
}
pub fn initialize(app: &tauri::AppHandle, root: PathBuf, legacy: PathBuf) -> Result<bool, String> {
    let scheduled = read_schedule(&root)?;
    let active = stores::read(&root)?;
    let pending = import_plan::list(&root)?.iter().any(|plan| {
        (active.is_none()
            && matches!(
                plan.phase,
                Phase::Building | Phase::Prepared | Phase::Cancelling
            ))
            || plan.phase == Phase::RollingBack
            || (plan.phase == Phase::Activating && active == Some(plan.next.clone()))
    });
    if !app.manage(Migration {
        root,
        legacy,
        scheduled: AtomicBool::new(scheduled),
        job: Mutex::new(None),
    }) {
        return Err("component_state_conflict".into());
    }
    Ok(scheduled || pending)
}
pub fn scheduled(app: &tauri::AppHandle) -> bool {
    app.try_state::<Migration>()
        .is_some_and(|state| state.scheduled.load(Ordering::Acquire))
}
pub fn clear_schedule(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Migration>();
    save_schedule(&state.root, false)?;
    state.scheduled.store(false, Ordering::Release);
    Ok(())
}
pub fn cancel_on_exit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<Migration>() {
        if let Ok(job) = state.job.lock() {
            if let Some(job) = job.as_ref() {
                job.cancel.store(true, Ordering::Release);
            }
        }
    }
}
fn summary(root: &Path, plan: &Plan) -> Result<Value, String> {
    let vault = if matches!(
        plan.phase,
        Phase::Prepared | Phase::Activating | Phase::Activated
    ) {
        Some(
            crate::startup::binding(root, &plan.next)?
                .0
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        None
    };
    Ok(
        json!({"id":plan.id,"phase":plan.phase,"preparedAtMs":plan.prepared_at_ms,"hasPrevious":plan.base.is_some(),"vault":vault,"sources":plan.sources.iter().map(|source|json!({"source":source.source,"bytes":source.snapshot.bytes,"report":source.report})).collect::<Vec<_>>()}),
    )
}
fn spawn_job(
    app: &tauri::AppHandle,
    run: impl FnOnce(tauri::AppHandle, Arc<AtomicBool>) -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    let reservation = crate::startup::reserve(app)?;
    let state = app.state::<Migration>();
    let mut current = state.job.lock().map_err(|_| "store_busy")?;
    if current.as_ref().is_some_and(|job| job.result.is_none()) {
        return Err("store_busy".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let cancellation = Arc::new(AtomicBool::new(false));
    *current = Some(Job {
        id: id.clone(),
        cancel: cancellation.clone(),
        result: None,
    });
    let app = app.clone();
    let job_id = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _reservation = reservation;
        let result = run(app.clone(), cancellation);
        if let Ok(mut current) = app.state::<Migration>().job.lock() {
            if let Some(job) = current.as_mut().filter(|job| job.id == job_id) {
                job.result = Some(result);
            }
        }
    });
    Ok(json!({"jobId":id}))
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobId {
    job_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanId {
    plan_id: String,
}
fn args<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| "component_args_invalid".into())
}
fn empty(value: &Value) -> Result<(), String> {
    if value.as_object().is_some_and(|v| v.is_empty()) {
        Ok(())
    } else {
        Err("component_args_invalid".into())
    }
}
pub fn finish_recovery(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Migration>();
    let selected = stores::read(&state.root)?;
    for plan in import_plan::list(&state.root)? {
        if plan.phase == Phase::Activating && selected == Some(plan.next.clone()) {
            import_plan::commit(&state.root, &plan.id)?;
        }
    }
    clear_schedule(app)
}
pub fn dispatch(app: &tauri::AppHandle, method: &str, value: Value) -> Result<Value, String> {
    let state = app
        .try_state::<Migration>()
        .ok_or("import_storage_unavailable")?;
    match method {
        "list_import_sources" => {
            empty(&value)?;
            Ok(json!([Source::Notes,Source::Activity,Source::Search].into_iter().map(|source|{
                let path=state.legacy.join(source.identifier()).join("data.db");
                json!({"source":source,"available":path.is_file()&&devbox_filesystem::ensure_no_links(path).is_ok()})
            }).collect::<Vec<_>>()))
        }
        "list_imports" => {
            empty(&value)?;
            import_plan::list(&state.root)?
                .iter()
                .map(|plan| summary(&state.root, plan))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        "schedule_import" | "cancel_scheduled_import" => {
            empty(&value)?;
            crate::startup::require_active(app)?;
            let requested = method == "schedule_import";
            save_schedule(&state.root, requested)?;
            state.scheduled.store(requested, Ordering::Release);
            Ok(json!({"scheduled":requested}))
        }
        "import_job" | "cancel_import_job" => {
            let JobId { job_id } = args(value)?;
            let job = state.job.lock().map_err(|_| "store_busy")?;
            let job = job
                .as_ref()
                .filter(|job| job.id == job_id)
                .ok_or("import_job_stale")?;
            if method == "cancel_import_job" && job.result.is_none() {
                job.cancel.store(true, Ordering::Release);
            }
            Ok(match &job.result {
                None => {
                    json!({"state":"running","cancellationRequested":job.cancel.load(Ordering::Acquire)})
                }
                Some(Ok(value)) => json!({"state":"succeeded","value":value}),
                Some(Err(error)) => {
                    let issue = crate::component::issue(error);
                    json!({"state":if issue=="cancelled"{"cancelled"}else{"failed"},"issue":issue})
                }
            })
        }
        "prepare_import" => {
            crate::startup::require_uninitialized(app)?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                sources: Vec<Source>,
            }
            let Input { sources } = args(value)?;
            if sources.is_empty() || sources.len() > 3 {
                return Err("component_args_invalid".into());
            }
            spawn_job(app, move |app, token| {
                let state = app.state::<Migration>();
                crate::storage_space::require(
                    &state.root,
                    import_plan::required_space(&state.root, &state.legacy, &sources)?,
                )?;
                let plan = import_plan::prepare(&state.root, &state.legacy, &sources, token)?;
                Ok(json!({"plan":summary(&state.root,&plan)?,"active":false}))
            })
        }
        "activate_import" => {
            crate::startup::require_uninitialized(app)?;
            let PlanId { plan_id } = args(value)?;
            spawn_job(app, move |app, token| {
                let state = app.state::<Migration>();
                let plan = import_plan::read(&state.root, &plan_id)?;
                let owner = crate::startup::owner(&app, &plan.next)?;
                let sources = plan.sources.iter().map(|s| s.source).collect::<Vec<_>>();
                crate::storage_space::require(
                    &state.root,
                    import_plan::required_space(&state.root, &state.legacy, &sources)?,
                )?;
                let plan = import_plan::activate(&state.root, &state.legacy, &plan_id, &token)?;
                crate::startup::activate_with_owner(&app, &plan.next, owner)?;
                let plan = import_plan::commit(&state.root, &plan_id)?;
                clear_schedule(&app)?;
                Ok(json!({"plan":summary(&state.root,&plan)?,"active":true}))
            })
        }
        "discard_import" | "rollback_import" => {
            crate::startup::require_uninitialized(app)?;
            let PlanId { plan_id } = args(value)?;
            let rollback = method == "rollback_import";
            spawn_job(app, move |app, token| {
                if token.load(Ordering::Acquire) {
                    return Err("import_cancelled".into());
                }
                let state = app.state::<Migration>();
                let plan = if rollback {
                    import_plan::rollback(&state.root, &plan_id)?
                } else {
                    import_plan::cancel(&state.root, &plan_id)?
                };
                Ok(json!({"plan":summary(&state.root,&plan)?,"active":false}))
            })
        }
        _ => Err("component_method_invalid".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheduling_survives_restart_and_future_metadata_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        assert!(!read_schedule(root.path()).unwrap());
        save_schedule(root.path(), true).unwrap();
        assert!(read_schedule(root.path()).unwrap());
        save_schedule(root.path(), false).unwrap();
        assert!(!read_schedule(root.path()).unwrap());
        fs::write(
            schedule_path(root.path()),
            r#"{"schemaVersion":2,"requested":true}"#,
        )
        .unwrap();
        let before = fs::read(schedule_path(root.path())).unwrap();
        assert!(read_schedule(root.path()).is_err());
        assert_eq!(fs::read(schedule_path(root.path())).unwrap(), before);
    }
}
