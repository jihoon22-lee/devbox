use crate::core::models::{Job, JobInput, Run};
use crate::lifecycle::{self, RuntimeState, RuntimeStatus};
use crate::storage::DatabaseState;
use chrono::Local;
use std::sync::Arc;
use tauri::{AppHandle, State};

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

#[tauri::command]
pub fn list_jobs(state: State<'_, Arc<DatabaseState>>) -> Result<Vec<Job>, String> {
    state.list_jobs().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_job(id: String, state: State<'_, Arc<DatabaseState>>) -> Result<Option<Job>, String> {
    state.get_job(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_job(input: JobInput, state: State<'_, Arc<DatabaseState>>) -> Result<Job, String> {
    state.create_job(input).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_job(
    id: String,
    input: JobInput,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Job, String> {
    state
        .update_job(&id, input)
        .map_err(|error| error.to_string())
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
