use crate::core::models::{
    EnvironmentCiphertextUpdate, EnvironmentUpdate, Job, JobInput, RunView, ServiceInput,
};
use crate::lifecycle::{self, RuntimeState, RuntimeStatus};
use crate::logs::{LogStream, LogStreams, TailRequest, TailResponse};
use crate::platform::environment::{EnvironmentProtectorState, SecretEnvironment};
use crate::platform::{StartupShortcut, StartupShortcutStatus};
use crate::scheduler::SchedulerError;
use crate::storage::{current_epoch_millis, DatabaseState, StorageError};
use chrono::Local;
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailLogInput {
    pub run_id: String,
    pub stream: LogStream,
    pub cursor: Option<String>,
    pub max_bytes: usize,
}

#[tauri::command]
pub fn runtime_status(state: State<'_, Arc<RuntimeState>>) -> RuntimeStatus {
    state.status()
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
    lifecycle::show_main_window(&app)
}

#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<(), String> {
    lifecycle::hide_main_window(&app)
}

#[tauri::command]
pub fn quit_app(app: AppHandle, state: State<'_, Arc<RuntimeState>>) {
    lifecycle::request_orderly_exit(&app, state.inner().clone());
}

fn startup_shortcut(app: &AppHandle) -> Result<StartupShortcut, String> {
    let startup_directory = app
        .path()
        .data_dir()
        .map_err(|error| error.to_string())?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Startup");
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    StartupShortcut::new(&startup_directory, &executable)
}

#[tauri::command]
pub fn startup_shortcut_status(app: AppHandle) -> Result<StartupShortcutStatus, String> {
    crate::platform::startup_shortcut_status(&startup_shortcut(&app)?)
}

#[tauri::command]
pub fn set_startup_shortcut_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<StartupShortcutStatus, String> {
    crate::platform::set_startup_shortcut_enabled(&startup_shortcut(&app)?, enabled)
}

#[tauri::command]
pub fn list_jobs(state: State<'_, Arc<DatabaseState>>) -> Result<Vec<Job>, String> {
    state.list_jobs().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_job(id: String, state: State<'_, Arc<DatabaseState>>) -> Result<Option<Job>, String> {
    state.get_job(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_job(
    input: JobInput,
    protector: State<'_, EnvironmentProtectorState>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    let mut input = input;
    let environment = consume_environment(&mut input.environment, protector.inner())?;
    let ciphertext = match environment {
        EnvironmentCiphertextUpdate::Replace(ciphertext) => Some(ciphertext),
        EnvironmentCiphertextUpdate::Keep | EnvironmentCiphertextUpdate::Clear => None,
    };
    state
        .create_job_with_ciphertext_at(input, ciphertext, current_epoch_millis())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_job(
    id: String,
    input: JobInput,
    protector: State<'_, EnvironmentProtectorState>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    let mut input = input;
    let environment = consume_environment(&mut input.environment, protector.inner())?;
    state
        .update_job_with_ciphertext_at(&id, input, environment, current_epoch_millis())
        .map_err(|error| error.to_string())
}

fn consume_environment(
    update: &mut EnvironmentUpdate,
    protector: &EnvironmentProtectorState,
) -> Result<EnvironmentCiphertextUpdate, String> {
    let update = std::mem::take(update);
    match update {
        EnvironmentUpdate::Keep => Ok(EnvironmentCiphertextUpdate::Keep),
        EnvironmentUpdate::Clear => Ok(EnvironmentCiphertextUpdate::Clear),
        EnvironmentUpdate::Replace { values } => {
            let environment = SecretEnvironment::new(values);
            if environment.is_empty() {
                drop(environment);
                return Ok(EnvironmentCiphertextUpdate::Clear);
            }
            protector
                .encrypt_owned(environment)
                .map(EnvironmentCiphertextUpdate::Replace)
                .map_err(|error| error.to_string())
        }
    }
}

#[tauri::command]
pub fn set_job_enabled(
    id: String,
    enabled: bool,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    state
        .set_job_enabled(&id, enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_job(id: String, state: State<'_, Arc<DatabaseState>>) -> Result<bool, String> {
    state.delete_job(&id).map_err(storage_command_error)
}

fn storage_command_error(error: StorageError) -> String {
    match error {
        StorageError::Validation(code) if code == "active-run-must-stop" => code,
        StorageError::NotFound(_) => "job-not-found".to_string(),
        _ => "run-storage-failed".to_string(),
    }
}

#[tauri::command]
pub fn list_services(state: State<'_, Arc<DatabaseState>>) -> Result<Vec<Job>, String> {
    state.list_services().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_service(
    id: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Option<Job>, String> {
    state.get_service(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_service(
    input: ServiceInput,
    protector: State<'_, EnvironmentProtectorState>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    let mut input = input;
    let environment = consume_environment(&mut input.environment, protector.inner())?;
    state
        .create_service_with_ciphertext_at(input, environment, current_epoch_millis())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_service(
    id: String,
    input: ServiceInput,
    protector: State<'_, EnvironmentProtectorState>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    let mut input = input;
    let environment = consume_environment(&mut input.environment, protector.inner())?;
    state
        .update_service_with_ciphertext_at(&id, input, environment, current_epoch_millis())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_service(id: String, state: State<'_, Arc<DatabaseState>>) -> Result<bool, String> {
    state.delete_service(&id).map_err(storage_command_error)
}

#[tauri::command]
pub fn get_run(
    id: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Option<RunView>, String> {
    state
        .get_run(&id)
        .map(|run| run.as_ref().map(RunView::from_run))
        .map_err(|_| "run-storage-failed".to_string())
}

#[tauri::command]
pub fn list_runs(
    job_id: String,
    limit: Option<u32>,
    start_at: Option<i64>,
    end_at: Option<i64>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<RunView>, String> {
    state
        .list_runs(&job_id, limit.unwrap_or(50), start_at, end_at)
        .map(|runs| runs.iter().map(RunView::from_run).collect())
        .map_err(|_| "run-storage-failed".to_string())
}

fn scheduler_command_error(error: SchedulerError) -> String {
    match error {
        SchedulerError::Storage(_) => "run-storage-failed".to_string(),
        SchedulerError::Cron(_) => "job-schedule-invalid".to_string(),
        SchedulerError::Adapter { .. } => "run-execution-failed".to_string(),
        SchedulerError::Join(_) => "scheduler-unavailable".to_string(),
    }
}

/// Start one explicit run through the same overlap policy, protected
/// environment boundary, logs, and process adapter as scheduled work.
#[tauri::command]
pub async fn run_job_now(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<RunView, String> {
    state
        .coordinator()
        .trigger_manual_at(&id, current_epoch_millis())
        .await
        .map(|run| RunView::from_run(&run))
        .map_err(scheduler_command_error)
}

/// Stop the active process tree for one job. A null result means the job has
/// no active process run; durable queued intents are left untouched.
#[tauri::command]
pub async fn stop_active_run(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<Option<RunView>, String> {
    state
        .coordinator()
        .stop_active_at(&id, current_epoch_millis())
        .await
        .map(|run| run.as_ref().map(RunView::from_run))
        .map_err(scheduler_command_error)
}

#[tauri::command]
pub fn get_active_run(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<Option<RunView>, String> {
    state
        .coordinator()
        .active_process_run(&id)
        .map(|run| run.as_ref().map(RunView::from_run))
        .map_err(scheduler_command_error)
}

#[tauri::command]
pub fn list_active_runs(state: State<'_, Arc<RuntimeState>>) -> Result<Vec<RunView>, String> {
    state
        .coordinator()
        .active_process_runs()
        .map(|runs| runs.iter().map(RunView::from_run).collect())
        .map_err(scheduler_command_error)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronPreviewInput {
    pub cron_expr: String,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CronPreviewItem {
    pub timestamp_millis: i64,
    pub datetime: String,
    pub wall_time: String,
    pub wall_key: String,
}

/// Return the next five system-local occurrences from the shared cron core.
/// The command deliberately accepts only an expression; the reference clock
/// remains the daemon's local system clock so preview and scheduling use the
/// same timezone semantics.
#[tauri::command]
pub fn preview_cron(input: CronPreviewInput) -> Result<Vec<CronPreviewItem>, String> {
    let after = Local::now();
    crate::core::cron::preview_occurrences(&input.cron_expr, after)
        .map(|occurrences| {
            occurrences
                .into_iter()
                .map(|occurrence| CronPreviewItem {
                    timestamp_millis: occurrence.timestamp_millis(),
                    datetime: occurrence.datetime.to_rfc3339(),
                    wall_time: occurrence.wall_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    wall_key: occurrence.wall_key,
                })
                .collect()
        })
        .map_err(|error| format!("cron_expr: invalid cron expression ({error})"))
}

/// Read one bounded snapshot from an app-owned run log.
///
/// The database value is only a relative identifier. Resolve it against the
/// current app-local root before opening the stream so a stale or tampered row
/// cannot turn this command into an arbitrary file reader.
#[tauri::command]
pub async fn tail_log(
    app: AppHandle,
    input: TailLogInput,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<TailResponse, String> {
    let run = state
        .get_run(&input.run_id)
        .map_err(|_| "run-storage-failed".to_string())?
        .ok_or_else(|| "run-not-found".to_string())?;
    let log_dir = run.log_dir.ok_or_else(|| "logs-unavailable".to_string())?;
    let data_root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "logs-unavailable".to_string())?;
    crate::logs::resolve_run_directory(&data_root, &log_dir, &input.run_id)
        .map_err(|_| "logs-unavailable".to_string())?;
    let streams = LogStreams::open_default(&data_root, &input.run_id)
        .map_err(|_| "logs-unavailable".to_string())?;
    streams
        .tail(TailRequest {
            stream: input.stream,
            cursor: input.cursor,
            max_bytes: input.max_bytes,
        })
        .await
        .map_err(|_| "logs-read-failed".to_string())
}
