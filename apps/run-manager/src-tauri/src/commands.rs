use crate::core::models::{
    EnvironmentCiphertextUpdate, EnvironmentUpdate, Job, JobInput, RunView, ServiceInput,
    ServiceInstanceView,
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

/// 서비스 관찰성 요약 (§13.1). definition과 runtime instance를 명확히 분리한다.
/// DB state는 실제 프로세스 생존을 단정하지 않는다 — PID는 DB 기록 기준 표시.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceObservability {
    pub id: String,
    pub definition: Job,
    pub instance: Option<ServiceInstanceView>,
    /// 활성 run (DB 기준). PID는 여기서 안전하게 노출한다 (재사용 위험 주의 주석은 프론트).
    pub current: Option<RunView>,
    /// 현재 run의 프로세스 PID (DB 기록 기준 — 재사용 위험이 있으므로 표시만).
    pub current_pid: Option<i64>,
    pub recent: Vec<RunView>,
    pub restart_count: i64,
    /// 다음 retry 시각 (instance.next_retry_at) — backoff 단계는 consecutive_failures로 유추
    pub next_retry_at: Option<i64>,
}

#[tauri::command]
pub fn service_observability(
    id: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Option<ServiceObservability>, String> {
    let Some(definition) = state.get_service(&id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let instance = state.get_service_instance(&id).map_err(|e| e.to_string())?;
    let instance_view = instance.as_ref().map(ServiceInstanceView::from_instance);
    let restart_count = instance_view
        .as_ref()
        .map(|i| i.consecutive_failures)
        .unwrap_or(0);
    let next_retry_at = instance_view.as_ref().and_then(|i| i.next_retry_at);

    let current_full = instance
        .as_ref()
        .and_then(|i| i.active_run_id.clone())
        .and_then(|rid| state.get_run(&rid).ok().flatten());
    let current = current_full.as_ref().map(RunView::from_run);
    let current_pid = current_full.as_ref().and_then(|r| r.target_pid);
    let recent = state
        .list_runs(&id, 8, None, None)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| RunView::from_run(&r))
        .collect();

    Ok(Some(ServiceObservability {
        id,
        definition,
        instance: instance_view,
        current,
        current_pid,
        recent,
        restart_count,
        next_retry_at,
    }))
}

/// import 항목 하나.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// "new" | "conflict"
    pub status: String,
    pub detail: String,
}

/// import 계획 — 실제로 생성하지 않고 충돌 여부만 판단한다.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub schema_version: u32,
    pub items: Vec<ImportItem>,
}

/// 정의 export JSON을 파싱하고, 기존 정의와 충돌하는지 계획을 만든다.
#[tauri::command]
pub fn import_definitions(
    json: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<ImportPlan, String> {
    let doc: DefinitionExport =
        serde_json::from_str(&json).map_err(|e| format!("정의 JSON을 읽을 수 없습니다: {e}"))?;
    if doc.schema_version != 1 {
        return Err(format!(
            "지원하지 않는 정의 스키마 버전: {}",
            doc.schema_version
        ));
    }
    let existing_jobs = state.list_jobs().map_err(|e| e.to_string())?;
    let existing_services = state.list_services().map_err(|e| e.to_string())?;
    let job_ids: std::collections::HashSet<String> =
        existing_jobs.iter().map(|j| j.id.clone()).collect();
    let service_ids: std::collections::HashSet<String> =
        existing_services.iter().map(|s| s.id.clone()).collect();

    let mut items = Vec::new();
    for job in &doc.jobs {
        items.push(ImportItem {
            id: job.id.clone(),
            name: job.name.clone(),
            kind: "job".into(),
            status: if job_ids.contains(&job.id) {
                "conflict"
            } else {
                "new"
            }
            .into(),
            detail: job_enabled_draft(job),
        });
    }
    for service in &doc.services {
        items.push(ImportItem {
            id: service.id.clone(),
            name: service.name.clone(),
            kind: "service".into(),
            status: if service_ids.contains(&service.id) {
                "conflict"
            } else {
                "new"
            }
            .into(),
            detail: job_enabled_draft(service),
        });
    }
    Ok(ImportPlan {
        schema_version: doc.schema_version,
        items,
    })
}

/// WSL distro·cwd가 현재 PC에 없을 수 있으므로 draft(disabled)로 들여오는
/// 안내를 위한 상세 문자열. 실제 적용 시 enabled는 보존하되, 요구 사항이
/// 없으면 사용자가 직접 활성화한다 — [설계] "disabled draft로 들어옴".
fn job_enabled_draft(job: &Job) -> String {
    if job.enabled {
        "활성(사용자 확인 필요)".into()
    } else {
        "비활성 draft".into()
    }
}

/// 선택한 항목을 실제로 생성한다. 충돌 항목·미선택 항목은 건너뛴다.
#[tauri::command]
pub fn apply_import(
    json: String,
    selected: Vec<String>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<usize, String> {
    let doc: DefinitionExport =
        serde_json::from_str(&json).map_err(|e| format!("정의 JSON을 읽을 수 없습니다: {e}"))?;
    let selected_set: std::collections::HashSet<String> = selected.into_iter().collect();
    let mut created = 0usize;
    let now = current_epoch_millis();

    for job in &doc.jobs {
        if !selected_set.contains(&job.id) {
            continue;
        }
        // 충돌은 건너뛴다 (apply 전 계획에서 확인됨)
        if state.get_job(&job.id).map_err(|e| e.to_string())?.is_some() {
            continue;
        }
        let input = JobInput {
            name: job.name.clone(),
            command: job.command.clone(),
            cwd: job.cwd.clone(),
            target_kind: job.target_kind,
            target_distro: job.target_distro.clone(),
            environment: crate::core::models::EnvironmentUpdate::Clear,
            cron_expr: job.cron_expr.clone().unwrap_or_default(),
            enabled: job.enabled,
            overlap_policy: job.overlap_policy,
            catch_up: job.catch_up,
        };
        state
            .create_job_with_ciphertext_at(input, None, now)
            .map_err(|e| e.to_string())?;
        created += 1;
    }
    for service in &doc.services {
        if !selected_set.contains(&service.id) {
            continue;
        }
        if state
            .get_service(&service.id)
            .map_err(|e| e.to_string())?
            .is_some()
        {
            continue;
        }
        let input = crate::core::models::ServiceInput {
            name: service.name.clone(),
            command: service.command.clone(),
            cwd: service.cwd.clone(),
            target_kind: service.target_kind,
            target_distro: service.target_distro.clone(),
            environment: crate::core::models::EnvironmentUpdate::Clear,
            restart_policy: service.restart_policy.unwrap_or_default(),
            auto_start: service.auto_start.unwrap_or(false),
            health_tcp_address: service.health_tcp_address.clone(),
            health_tcp_port: service.health_tcp_port,
        };
        state
            .create_service_with_ciphertext_at(
                input,
                crate::core::models::EnvironmentCiphertextUpdate::Clear,
                now,
            )
            .map_err(|e| e.to_string())?;
        created += 1;
    }
    Ok(created)
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

/// 정의 export 문서 (schema version 포함). secret 값은 절대 포함하지 않는다 —
/// Job의 `envConfigured`(존재 여부)만 나간다 (ciphertext는 read DTO에 원천 차단).
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionExport {
    pub schema_version: u32,
    pub exported_at: String,
    pub jobs: Vec<Job>,
    pub services: Vec<Job>,
}

#[tauri::command]
pub fn export_definitions(
    state: State<'_, Arc<DatabaseState>>,
) -> Result<DefinitionExport, String> {
    let jobs = state.list_jobs().map_err(|e| e.to_string())?;
    let services = state.list_services().map_err(|e| e.to_string())?;
    Ok(DefinitionExport {
        schema_version: 1,
        exported_at: current_epoch_millis().to_string(),
        jobs,
        services,
    })
}

#[tauri::command]
pub fn get_service_instance(
    id: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Option<ServiceInstanceView>, String> {
    state
        .get_service_instance(&id)
        .map(|instance| instance.as_ref().map(ServiceInstanceView::from_instance))
        .map_err(|_| "run-storage-failed".to_string())
}

fn service_command_error(error: SchedulerError) -> String {
    match error {
        SchedulerError::Adapter { source, .. } => source.message,
        SchedulerError::Storage(StorageError::NotFound(_)) => "service-not-found".to_string(),
        SchedulerError::Storage(_) => "run-storage-failed".to_string(),
        SchedulerError::Cron(_) | SchedulerError::Join(_) => "scheduler-unavailable".to_string(),
    }
}

#[tauri::command]
pub async fn start_service(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<ServiceInstanceView, String> {
    state
        .coordinator()
        .start_service_at(&id, current_epoch_millis())
        .await
        .map(|instance| ServiceInstanceView::from_instance(&instance))
        .map_err(service_command_error)
}

#[tauri::command]
pub async fn stop_service(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<Option<ServiceInstanceView>, String> {
    state
        .coordinator()
        .stop_service_at(&id, current_epoch_millis())
        .await
        .map(|instance| instance.as_ref().map(ServiceInstanceView::from_instance))
        .map_err(service_command_error)
}

#[tauri::command]
pub async fn restart_service(
    id: String,
    state: State<'_, Arc<RuntimeState>>,
) -> Result<ServiceInstanceView, String> {
    state
        .coordinator()
        .restart_service_at(&id, current_epoch_millis())
        .await
        .map(|instance| ServiceInstanceView::from_instance(&instance))
        .map_err(service_command_error)
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
