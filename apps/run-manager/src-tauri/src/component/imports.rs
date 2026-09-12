//! One native-owned import worker. Renderer timeout/navigation never abandons
//! acquisition or publication; shutdown cancels and joins the actual thread.
use super::{data_root, ProductRoot};
use crate::{
    core::runtime_import::{ImportSummary, PreparedImport},
    storage::DatabaseState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};
use tauri::Manager;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: String,
    phase: &'static str,
    summary: Option<ImportSummary>,
    issue: Option<String>,
    already_imported: bool,
}
#[derive(Default)]
struct State {
    job: Option<Job>,
    worker: Option<JoinHandle<()>>,
}
pub(super) struct ImportOwner {
    state: Arc<Mutex<State>>,
    cancel: Arc<AtomicBool>,
    closing: AtomicBool,
    stages: ProductRoot,
    source: PathBuf,
}
impl ImportOwner {
    pub(super) fn new(data: &Path, legacy_base: &Path) -> Result<Self, String> {
        let stages = data.join("legacy-imports");
        match std::fs::create_dir(&stages) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("runtime_import_write_failed".into()),
        }
        Ok(Self {
            state: Arc::default(),
            cancel: Arc::default(),
            closing: AtomicBool::new(false),
            stages: ProductRoot::open(&stages)?,
            source: legacy_base.join("com.devbox.runmanager"),
        })
    }
    fn start(
        &self,
        app: &tauri::AppHandle,
        operation: &'static str,
        existing: Option<String>,
    ) -> Result<Value, String> {
        if self.closing.load(Ordering::Acquire) {
            return Err("runtime_import_cancelled".into());
        }
        let destination = data_root(app)?;
        let database = app
            .try_state::<Arc<DatabaseState>>()
            .ok_or("component_state_unavailable")?
            .inner()
            .clone();
        let stages = self.stages.checked()?;
        let mut state = self.state.lock().map_err(|_| "runtime_import_busy")?;
        if state
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return Err("runtime_import_busy".into());
        }
        if let Some(worker) = state.worker.take() {
            worker.join().map_err(|_| "runtime_import_failed")?;
        }
        let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        if id.len() != 32
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("component_args_invalid".into());
        }
        if operation == "apply"
            && !state
                .job
                .as_ref()
                .is_some_and(|job| job.id == id && job.phase == "ready")
        {
            return Err("runtime_import_stale".into());
        }
        let stage = stages.join(&id);
        let source = self.source.clone();
        let job = Job {
            id,
            phase: operation,
            summary: None,
            issue: None,
            already_imported: false,
        };
        let response = serde_json::to_value(&job).map_err(|_| "runtime_import_failed")?;
        state.job = Some(job);
        self.cancel.store(false, Ordering::Release);
        let current = self.state.clone();
        let cancel = self.cancel.clone();
        let app = app.clone();
        state.worker = Some(
            std::thread::Builder::new()
                .name("workspace-runtime-import".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || -> Result<(ImportSummary, bool), String> {
                            data_root(&app)?;
                            let prepared = if operation == "prepare" {
                                PreparedImport::acquire(&source, &stage, &cancel)?
                            } else {
                                PreparedImport::reopen(&stage, &cancel)?
                            };
                            data_root(&app)?;
                            if operation == "apply" {
                                let receipt = database.import_legacy_runtime(
                                    &prepared,
                                    &destination,
                                    &cancel,
                                )?;
                                Ok((receipt.summary, receipt.already_imported))
                            } else {
                                Ok((prepared.summary().clone(), false))
                            }
                        },
                    ))
                    .unwrap_or_else(|_| Err("runtime_import_failed".into()));
                    if let Ok(mut state) = current.lock() {
                        if let Some(job) = state.job.as_mut() {
                            match result {
                                Ok((summary, already)) => {
                                    job.summary = Some(summary);
                                    job.phase = if operation == "apply" {
                                        "complete"
                                    } else {
                                        "ready"
                                    };
                                    job.already_imported = already;
                                }
                                Err(issue) => {
                                    job.phase = if cancel.load(Ordering::Acquire) {
                                        "cancelled"
                                    } else {
                                        "failed"
                                    };
                                    job.issue = Some(issue);
                                }
                            }
                        }
                    }
                })
                .map_err(|_| "runtime_import_failed")?,
        );
        Ok(response)
    }
    pub(super) fn request_shutdown(&self) {
        self.closing.store(true, Ordering::Release);
        self.cancel.store(true, Ordering::Release);
    }
    pub(super) fn join(&self) {
        self.request_shutdown();
        let worker = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.worker.take());
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}
pub(super) fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    value: Value,
) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Id {
        id: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    fn args<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
        if !value.is_object() {
            return Err("component_args_invalid".into());
        }
        serde_json::from_value(value).map_err(|_| "component_args_invalid".into())
    }
    let owner = app
        .try_state::<Arc<ImportOwner>>()
        .ok_or("component_state_unavailable")?;
    match method {
        "runtime_import_prepare" => {
            args::<Empty>(value)?;
            owner.start(app, "prepare", None)
        }
        "runtime_import_resume" => {
            let input: Id = args(value)?;
            owner.start(app, "resume", Some(input.id))
        }
        "runtime_import_apply" => {
            let input: Id = args(value)?;
            owner.start(app, "apply", Some(input.id))
        }
        "runtime_import_cancel" => {
            args::<Empty>(value)?;
            owner.cancel.store(true, Ordering::Release);
            Ok(Value::Null)
        }
        "runtime_import_status" => {
            args::<Empty>(value)?;
            serde_json::to_value(&owner.state.lock().map_err(|_| "runtime_import_busy")?.job)
                .map_err(|_| "runtime_import_failed".into())
        }
        "runtime_import_catalog" => {
            args::<Empty>(value)?;
            let mut ids = Vec::new();
            for entry in
                std::fs::read_dir(owner.stages.checked()?).map_err(|_| "runtime_import_failed")?
            {
                let entry = entry.map_err(|_| "runtime_import_failed")?;
                let id = entry.file_name().to_string_lossy().into_owned();
                if id.len() == 32
                    && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && entry.path().join("runtime-import.json").is_file()
                {
                    ids.push(id);
                }
                if ids.len() > 1000 {
                    return Err("runtime_import_limit".into());
                }
            }
            ids.sort();
            Ok(json!(ids))
        }
        "runtime_import_reviews" => {
            args::<Empty>(value)?;
            let database = app
                .try_state::<Arc<DatabaseState>>()
                .ok_or("component_state_unavailable")?;
            serde_json::to_value(
                database
                    .imported_job_reviews()
                    .map_err(|_| "runtime_import_failed")?,
            )
            .map_err(|_| "runtime_import_failed".into())
        }
        _ => Err("component_method_invalid".into()),
    }
}
