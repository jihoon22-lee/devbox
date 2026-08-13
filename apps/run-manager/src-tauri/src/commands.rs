use crate::core::models::{EnvironmentCiphertextUpdate, EnvironmentUpdate, Job, JobInput, Run};
use crate::lifecycle::{self, RuntimeState, RuntimeStatus};
use crate::logs::{LogStream, LogStreams, TailRequest, TailResponse};
use crate::platform::environment::{EnvironmentProtectorState, SecretEnvironment};
use crate::platform::{StartupShortcut, StartupShortcutStatus};
use crate::storage::{current_epoch_millis, DatabaseState};
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
    let environment = consume_environment(&mut input, protector.inner())?;
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
    let environment = consume_environment(&mut input, protector.inner())?;
    state
        .update_job_with_ciphertext_at(&id, input, environment, current_epoch_millis())
        .map_err(|error| error.to_string())
}

fn consume_environment(
    input: &mut JobInput,
    protector: &EnvironmentProtectorState,
) -> Result<EnvironmentCiphertextUpdate, String> {
    let update = std::mem::take(&mut input.environment);
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
    state.delete_job(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_run(id: String, state: State<'_, Arc<DatabaseState>>) -> Result<Option<Run>, String> {
    state.get_run(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_runs(
    job_id: String,
    limit: Option<u32>,
    start_at: Option<i64>,
    end_at: Option<i64>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<Run>, String> {
    state
        .list_runs(&job_id, limit.unwrap_or(50), start_at, end_at)
        .map_err(|error| error.to_string())
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
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "run not found".to_string())?;
    let log_dir = run
        .log_dir
        .ok_or_else(|| "run has no log directory".to_string())?;
    let data_root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    crate::logs::resolve_run_directory(&data_root, &log_dir, &input.run_id)
        .map_err(|error| error.to_string())?;
    let streams =
        LogStreams::open_default(&data_root, &input.run_id).map_err(|error| error.to_string())?;
    streams
        .tail(TailRequest {
            stream: input.stream,
            cursor: input.cursor,
            max_bytes: input.max_bytes,
        })
        .await
        .map_err(|error| error.to_string())
}
