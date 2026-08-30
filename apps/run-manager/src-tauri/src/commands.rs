use crate::core::imports::{
    definition_revision, imported_job_input, normalize_import_cwd, preview_project_with_control,
    validate_import_cwd, verify_preview_revision_with_control, ImportOperationRegistry,
    ProjectImportApplyResult, ProjectImportError, ProjectImportPlan, MAX_DEFINITION_JSON_BYTES,
    MAX_ITEMS,
};
use crate::core::log_search::{
    search_streams, validate_request, LogSearchError, LogSearchRequest, LogSearchResponse,
    MAX_SCAN_BYTES_PER_STREAM,
};
use crate::core::models::{
    EnvironmentCiphertextUpdate, EnvironmentUpdate, Job, JobInput, JobKind, RunHistoryFilter,
    RunStatus, RunView, ServiceInput, ServiceInstanceView,
};
use crate::core::workspace_tasks::{
    preview_workspace_tasks, revalidate_workspace_task_execution, verify_workspace_task_execution,
    verify_workspace_task_plan, WorkspaceTaskApplyResult, WorkspaceTaskExecution,
    WorkspaceTaskPlan, WorkspaceTaskState, MAX_TASKS,
};
use crate::lifecycle::{self, RuntimeState, RuntimeStatus};
use crate::logs::{LogStream, LogStreams, TailRequest, TailResponse, MAX_TAIL_BYTES};
use crate::platform::environment::{EnvironmentProtectorState, SecretEnvironment};
use crate::platform::{StartupShortcut, StartupShortcutStatus};
use crate::scheduler::SchedulerError;
use crate::storage::{current_epoch_millis, DatabaseState, StorageError};
use chrono::Local;
use serde::Deserialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TailLogInput {
    pub run_id: String,
    pub stream: LogStream,
    pub cursor: Option<String>,
    pub max_bytes: usize,
}

fn log_search_command_error(error: LogSearchError) -> String {
    match error {
        LogSearchError::InvalidRequest => "log-search-invalid-request".to_string(),
        LogSearchError::InvalidPattern => "log-search-invalid-pattern".to_string(),
        LogSearchError::InvalidSource => "log-search-invalid-source".to_string(),
        LogSearchError::InvalidTimeRange => "log-search-invalid-time-range".to_string(),
    }
}

/// Read one bounded source through the existing decimal-cursor tail API.
/// Each chunk releases the stream lock before yielding to the scheduler, so a
/// running writer is never held behind the complete search scan. A rotation
/// observed between chunks causes one safe restart from the current retained
/// boundary rather than returning duplicate or path-derived data.
async fn read_search_snapshot(
    streams: &LogStreams,
    stream: LogStream,
) -> Result<(Vec<u8>, bool), String> {
    let mut bytes = Vec::with_capacity(MAX_SCAN_BYTES_PER_STREAM);
    let mut cursor: Option<String> = None;
    let mut restarted = false;
    let mut truncated = false;

    loop {
        let request_cursor = cursor.clone();
        let response = streams
            .tail_log(stream, request_cursor.as_deref(), MAX_TAIL_BYTES)
            .await
            .map_err(|_| "log-search-read-failed".to_string())?;

        if response.truncated && request_cursor.is_some() {
            if !restarted {
                restarted = true;
                bytes.clear();
                cursor = None;
                tokio::task::yield_now().await;
                continue;
            }
            return Err("log-search-read-failed".to_string());
        }

        let remaining = MAX_SCAN_BYTES_PER_STREAM.saturating_sub(bytes.len());
        if response.data.len() > remaining {
            bytes.extend_from_slice(&response.data[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&response.data);
        if response.data.is_empty() {
            break;
        }

        let next_cursor = response.next_cursor;
        if request_cursor.as_deref() == Some(next_cursor.as_str()) {
            truncated = true;
            break;
        }
        cursor = Some(next_cursor);
        if bytes.len() >= MAX_SCAN_BYTES_PER_STREAM {
            // The exact end may be equal to the cap, so conservatively mark
            // the result as bounded rather than probing with an extra read.
            truncated = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    Ok((bytes, truncated))
}

/// Resolve and reconstruct retained segment metadata away from Tauri's async
/// command executor. Both operations perform bounded synchronous filesystem
/// work and keep returning fixed, path-independent failures.
async fn open_search_streams(
    data_root: PathBuf,
    log_dir: String,
    run_id: String,
) -> Result<LogStreams, String> {
    tokio::task::spawn_blocking(move || {
        crate::logs::resolve_run_directory(&data_root, &log_dir, &run_id)
            .map_err(|_| "logs-unavailable".to_string())?;
        LogStreams::open_default(&data_root, run_id).map_err(|_| "logs-unavailable".to_string())
    })
    .await
    .map_err(|_| "logs-unavailable".to_string())?
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
    validate_workspace_task_update(&id, &input, state.inner().as_ref())?;
    if input.enabled {
        revalidate_workspace_task_action(&id, state.inner().as_ref())?;
    }
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
    if enabled {
        revalidate_workspace_task_action(&id, state.inner().as_ref())?;
    }
    state
        .set_job_enabled(&id, enabled)
        .map_err(|error| error.to_string())
}

fn workspace_task_storage_error(error: StorageError) -> String {
    match error {
        StorageError::Validation(code)
            if matches!(
                code.as_str(),
                "workspace-task-source-untrusted" | "workspace-task-unavailable"
            ) =>
        {
            code
        }
        StorageError::NotFound(_) => "workspace-task-not-found".to_owned(),
        StorageError::ConcurrentChange(_) => "workspace-task-source-changed".to_owned(),
        _ => "run-storage-failed".to_owned(),
    }
}

fn workspace_task_operation_error(error: ProjectImportError) -> String {
    let value = error.to_string();
    let suffix = value
        .strip_prefix("project-import-")
        .unwrap_or("operation-failed");
    format!("workspace-task-import-{suffix}")
}

fn workspace_task_apply_error(error: StorageError) -> String {
    match error {
        StorageError::Validation(_) => "workspace-task-import-invalid".to_owned(),
        other => workspace_task_storage_error(other),
    }
}

fn invalidate_workspace_task(
    state: &DatabaseState,
    execution: &WorkspaceTaskExecution,
) -> Result<(), String> {
    state
        .invalidate_workspace_task_source_at(&execution.source_id, current_epoch_millis())
        .map(|_| ())
        .map_err(workspace_task_storage_error)
}

fn revalidate_workspace_task_action(
    job_id: &str,
    state: &DatabaseState,
) -> Result<Option<WorkspaceTaskExecution>, String> {
    state
        .ensure_workspace_task_can_enable(job_id)
        .map_err(workspace_task_storage_error)?;
    let Some(execution) = state
        .get_workspace_task_execution(job_id)
        .map_err(workspace_task_storage_error)?
    else {
        return Ok(None);
    };
    if revalidate_workspace_task_execution(&execution).is_err() {
        invalidate_workspace_task(state, &execution)?;
        return Err("workspace-task-source-changed".to_owned());
    }
    Ok(Some(execution))
}

fn validate_workspace_task_update(
    job_id: &str,
    input: &JobInput,
    state: &DatabaseState,
) -> Result<(), String> {
    let Some(execution) = state
        .get_workspace_task_execution(job_id)
        .map_err(workspace_task_storage_error)?
    else {
        return Ok(());
    };
    if input.name != execution.label
        || input.command != execution.command
        || input.cwd.as_deref() != Some(execution.cwd.as_str())
        || input.target_kind != execution.target_kind
        || input.target_distro != execution.target_distro
    {
        return Err("workspace-task-managed-fields-locked".to_owned());
    }
    if let EnvironmentUpdate::Replace { values } = &input.environment {
        if values.keys().any(|key| {
            !execution
                .environment_keys
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(key))
        }) {
            return Err("workspace-task-environment-key-not-declared".to_owned());
        }
    }
    Ok(())
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

/// Definition import item.  The preview exposes only masked environment
/// metadata; ciphertext and values are never deserialized from the document.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// "new" | "conflict"
    pub status: String,
    pub detail: String,
    pub cwd: Option<String>,
    pub environment_keys: Vec<String>,
    pub requires_confirmation: bool,
}

/// import 계획 — 실제로 생성하지 않고 충돌 여부만 판단한다.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub schema_version: u32,
    pub revision: String,
    pub items: Vec<ImportItem>,
}

const MAX_IMPORT_DEFINITIONS: usize = MAX_ITEMS;
const MAX_SELECTION_ID_BYTES: usize = 256;

fn definition_import_error() -> String {
    "definition-import-invalid".to_owned()
}

fn parse_definition_export(json: &str) -> Result<(DefinitionExport, String), String> {
    if json.len() > MAX_DEFINITION_JSON_BYTES {
        return Err("definition-import-too-large".to_owned());
    }
    let revision = definition_revision(json).map_err(|_| "definition-import-invalid".to_owned())?;
    let mut doc: DefinitionExport =
        serde_json::from_str(json).map_err(|_| definition_import_error())?;
    if doc.schema_version != 1
        || doc.jobs.len().saturating_add(doc.services.len()) > MAX_IMPORT_DEFINITIONS
    {
        return Err("definition-import-invalid".to_owned());
    }

    let mut ids = HashSet::new();
    for job in &mut doc.jobs {
        job.cwd =
            normalize_import_cwd(job.cwd.as_deref()).map_err(|_| definition_import_error())?;
        validate_import_definition(job, JobKind::Job)?;
        if !ids.insert(job.id.clone()) {
            return Err("definition-import-duplicate".to_owned());
        }
    }
    for service in &mut doc.services {
        service.cwd =
            normalize_import_cwd(service.cwd.as_deref()).map_err(|_| definition_import_error())?;
        validate_import_definition(service, JobKind::Service)?;
        if !ids.insert(service.id.clone()) {
            return Err("definition-import-duplicate".to_owned());
        }
    }
    Ok((doc, revision))
}

fn validate_selection_ids(selected: &[String], error_code: &'static str) -> Result<(), String> {
    if selected.iter().any(|id| {
        id.is_empty() || id.len() > MAX_SELECTION_ID_BYTES || id.chars().any(char::is_control)
    }) {
        return Err(error_code.to_owned());
    }
    Ok(())
}

fn selected_definition_ids(
    selected: &[String],
    jobs: &[Job],
    services: &[Job],
    error_code: &'static str,
) -> Result<HashSet<String>, String> {
    validate_selection_ids(selected, error_code)?;
    let available = jobs
        .iter()
        .chain(services.iter())
        .map(|job| job.id.as_str())
        .collect::<HashSet<_>>();
    if selected.iter().any(|id| !available.contains(id.as_str())) {
        return Err(error_code.to_owned());
    }
    Ok(selected.iter().cloned().collect())
}

fn selected_project_ids(
    selected: &[String],
    plan: &ProjectImportPlan,
) -> Result<HashSet<String>, String> {
    validate_selection_ids(selected, "project-import-invalid")?;
    let available = plan
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<HashSet<_>>();
    if selected.iter().any(|id| !available.contains(id.as_str())) {
        return Err("project-import-invalid".to_owned());
    }
    Ok(selected.iter().cloned().collect())
}

fn validate_import_definition(job: &Job, expected_kind: JobKind) -> Result<(), String> {
    if job.kind != expected_kind
        || job.id.len() > 128
        || job.name.is_empty()
        || job.name.len() > 512
        || job.command.is_empty()
        || job.command.len() > 16 * 1024
        || job.name.chars().any(char::is_control)
        || job.command.chars().any(char::is_control)
        || !validate_import_cwd(job.cwd.as_deref())
        || job.target_distro.as_deref().is_some_and(|distro| {
            distro.is_empty() || distro.len() > 128 || distro.chars().any(char::is_control)
        })
        || crate::core::shell::validate_uuid("definition id", &job.id).is_err()
    {
        return Err("definition-import-invalid".to_owned());
    }
    match expected_kind {
        JobKind::Job => {
            let input = JobInput {
                name: job.name.clone(),
                command: job.command.clone(),
                cwd: job.cwd.clone(),
                target_kind: job.target_kind,
                target_distro: job.target_distro.clone(),
                environment: crate::core::models::EnvironmentUpdate::Keep,
                cron_expr: job.cron_expr.clone().unwrap_or_default(),
                enabled: false,
                overlap_policy: job.overlap_policy,
                catch_up: false,
            };
            input
                .validate()
                .map_err(|_| "definition-import-invalid".to_owned())?;
        }
        JobKind::Service => {
            let input = ServiceInput {
                name: job.name.clone(),
                command: job.command.clone(),
                cwd: job.cwd.clone(),
                target_kind: job.target_kind,
                target_distro: job.target_distro.clone(),
                environment: crate::core::models::EnvironmentUpdate::Keep,
                restart_policy: job.restart_policy.unwrap_or_default(),
                auto_start: false,
                health_tcp_address: job.health_tcp_address.clone(),
                health_tcp_port: job.health_tcp_port,
            };
            input
                .validate()
                .map_err(|_| "definition-import-invalid".to_owned())?;
            if job.cron_expr.is_some() {
                return Err("definition-import-invalid".to_owned());
            }
        }
    }
    Ok(())
}

/// 정의 export JSON을 파싱하고, 기존 정의와 충돌하는지 계획을 만든다.
#[tauri::command]
pub fn import_definitions(
    json: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<ImportPlan, String> {
    let (doc, revision) = parse_definition_export(&json)?;

    let mut items = Vec::new();
    for job in &doc.jobs {
        let conflict = state
            .has_import_definition_id(&job.id)
            .map_err(|_| "definition-import-storage-failed".to_owned())?;
        items.push(ImportItem {
            id: job.id.clone(),
            name: job.name.clone(),
            kind: "job".into(),
            status: if conflict { "conflict" } else { "new" }.into(),
            detail: job_enabled_draft(job),
            cwd: job.cwd.clone(),
            environment_keys: Vec::new(),
            requires_confirmation: job.enabled || job.env_configured || job.cwd.is_some(),
        });
    }
    for service in &doc.services {
        let conflict = state
            .has_import_definition_id(&service.id)
            .map_err(|_| "definition-import-storage-failed".to_owned())?;
        items.push(ImportItem {
            id: service.id.clone(),
            name: service.name.clone(),
            kind: "service".into(),
            status: if conflict { "conflict" } else { "new" }.into(),
            detail: job_enabled_draft(service),
            cwd: service.cwd.clone(),
            environment_keys: Vec::new(),
            requires_confirmation: service.enabled
                || service.env_configured
                || service.cwd.is_some(),
        });
    }
    Ok(ImportPlan {
        schema_version: doc.schema_version,
        revision,
        items,
    })
}

/// WSL distro·cwd가 현재 PC에 없을 수 있으므로 draft(disabled)로 들여오는
/// 안내를 위한 상세 문자열. 실제 적용은 항상 비활성 상태로 저장하고,
/// 사용자가 환경과 작업 디렉터리를 검토한 뒤 직접 활성화한다.
fn job_enabled_draft(job: &Job) -> String {
    if job.enabled {
        "비활성 draft · 환경변수와 작업 디렉터리를 확인한 뒤 활성화하세요".into()
    } else {
        "비활성 draft · 환경변수와 작업 디렉터리를 확인하세요".into()
    }
}

/// 선택한 항목을 실제로 생성한다. 충돌 항목·미선택 항목은 건너뛴다.
#[tauri::command]
pub fn apply_import(
    json: String,
    selected: Vec<String>,
    revision: Option<String>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<usize, String> {
    let (doc, expected_revision) = parse_definition_export(&json)?;
    if revision.as_deref() != Some(expected_revision.as_str()) {
        return Err("import-preview-stale".to_owned());
    }
    if selected.len() > MAX_IMPORT_DEFINITIONS {
        return Err("definition-import-too-many-items".to_owned());
    }
    let selected_set = selected_definition_ids(
        &selected,
        &doc.jobs,
        &doc.services,
        "definition-import-invalid",
    )?;
    let mut jobs = Vec::new();
    for job in &doc.jobs {
        if !selected_set.contains(&job.id) {
            continue;
        }
        let input = JobInput {
            name: job.name.clone(),
            command: job.command.clone(),
            cwd: job.cwd.clone(),
            target_kind: job.target_kind,
            target_distro: job.target_distro.clone(),
            environment: crate::core::models::EnvironmentUpdate::Keep,
            cron_expr: job.cron_expr.clone().unwrap_or_default(),
            // An import must never create a scheduler side effect.  The user
            // explicitly enables the draft after reviewing it.
            enabled: false,
            overlap_policy: job.overlap_policy,
            catch_up: job.catch_up,
        };
        jobs.push((job.id.clone(), input));
    }

    let mut services = Vec::new();
    for service in &doc.services {
        if !selected_set.contains(&service.id) {
            continue;
        }
        let input = crate::core::models::ServiceInput {
            name: service.name.clone(),
            command: service.command.clone(),
            cwd: service.cwd.clone(),
            target_kind: service.target_kind,
            target_distro: service.target_distro.clone(),
            environment: crate::core::models::EnvironmentUpdate::Keep,
            restart_policy: service.restart_policy.unwrap_or_default(),
            auto_start: false,
            health_tcp_address: service.health_tcp_address.clone(),
            health_tcp_port: service.health_tcp_port,
        };
        services.push((service.id.clone(), input));
    }

    let (created, _skipped) = state
        .create_definition_import_at(jobs, services, current_epoch_millis())
        .map_err(|_| "definition-import-save-failed".to_owned())?;
    Ok(created)
}

fn project_import_error(error: ProjectImportError) -> String {
    error.to_string()
}

fn project_plan_with_conflicts(
    mut plan: ProjectImportPlan,
    state: &DatabaseState,
    control: &crate::core::imports::ImportControl,
) -> Result<ProjectImportPlan, String> {
    control.check().map_err(project_import_error)?;
    for item in &mut plan.items {
        control.check().map_err(project_import_error)?;
        let conflict = state
            .has_import_definition_conflict(item.kind, &item.name, Some(&item.cwd))
            .map_err(|_| "run-storage-failed".to_owned())?;
        if conflict {
            item.status = "conflict".to_owned();
            item.detail = "충돌 — 같은 작업 디렉터리의 정의가 있어 건너뜁니다".to_owned();
            item.requires_confirmation = true;
        }
    }
    control.check().map_err(project_import_error)?;
    Ok(plan)
}

fn workspace_plan_with_conflicts(
    mut plan: WorkspaceTaskPlan,
    state: &DatabaseState,
    control: &crate::core::imports::ImportControl,
) -> Result<WorkspaceTaskPlan, String> {
    control.check().map_err(workspace_task_operation_error)?;
    for item in &mut plan.items {
        control.check().map_err(workspace_task_operation_error)?;
        if !item.is_ready_process() {
            continue;
        }
        let conflict = state
            .has_import_definition_conflict(JobKind::Job, &item.label, item.cwd.as_deref())
            .map_err(|_| "run-storage-failed".to_owned())?;
        if conflict {
            item.status = "conflict".to_owned();
            item.blocked_reason = Some("definition-conflict".to_owned());
        }
    }
    control.check().map_err(workspace_task_operation_error)?;
    Ok(plan)
}

/// Read one bounded `.vscode/tasks.json` source. Preview is offline and never
/// executes a task, extension, command variable, shell, or package manager.
#[tauri::command]
pub fn preview_workspace_task_import(
    path: String,
    target_kind: crate::core::models::TargetKind,
    target_distro: Option<String>,
    operation_id: String,
    state: State<'_, Arc<DatabaseState>>,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<WorkspaceTaskPlan, String> {
    let operation = operations
        .begin(&operation_id)
        .map_err(workspace_task_operation_error)?;
    operation
        .control()
        .check()
        .map_err(workspace_task_operation_error)?;
    let plan = preview_workspace_tasks(Path::new(&path), target_kind, target_distro.as_deref())
        .map_err(|error| error.to_string())?;
    workspace_plan_with_conflicts(plan, state.inner().as_ref(), operation.control())
}

#[tauri::command]
pub fn cancel_workspace_task_import(
    operation_id: String,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<bool, String> {
    operations
        .cancel(&operation_id)
        .map_err(workspace_task_operation_error)
}

/// Re-read the exact source revision and atomically materialize only selected
/// process tasks as disabled, untrusted drafts.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn apply_workspace_task_import(
    path: String,
    source_root: String,
    project_identity: String,
    revision: String,
    target_kind: crate::core::models::TargetKind,
    target_distro: Option<String>,
    selected: Vec<String>,
    operation_id: String,
    state: State<'_, Arc<DatabaseState>>,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<WorkspaceTaskApplyResult, String> {
    if selected.is_empty() || selected.len() > MAX_TASKS {
        return Err("workspace-task-selection-invalid".to_owned());
    }
    validate_selection_ids(&selected, "workspace-task-selection-invalid")?;
    let operation = operations
        .begin(&operation_id)
        .map_err(workspace_task_operation_error)?;
    operation
        .control()
        .check()
        .map_err(workspace_task_operation_error)?;
    let plan = verify_workspace_task_plan(
        Path::new(&path),
        target_kind,
        target_distro.as_deref(),
        &source_root,
        &project_identity,
        &revision,
    )
    .map_err(|error| error.to_string())?;
    operation
        .control()
        .check()
        .map_err(workspace_task_operation_error)?;
    state
        .apply_workspace_task_import_at(&plan, &selected, current_epoch_millis())
        .map_err(workspace_task_apply_error)
}

#[tauri::command]
pub fn list_workspace_tasks(
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<WorkspaceTaskState>, String> {
    state
        .list_workspace_task_states()
        .map_err(workspace_task_storage_error)
}

/// Authorize one exact source revision. The filesystem claim is verified both
/// before and after the durable CAS; a racing change clears trust and disables
/// every task in the source before this command can report success.
#[tauri::command]
pub fn trust_workspace_task_source(
    source_id: String,
    revision: String,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<bool, String> {
    let candidate = state
        .list_workspace_task_states()
        .map_err(workspace_task_storage_error)?
        .into_iter()
        .find(|item| item.source_id == source_id)
        .ok_or_else(|| "workspace-task-not-found".to_owned())?;
    let candidate = state
        .get_workspace_task_state(&candidate.job_id)
        .map_err(workspace_task_storage_error)?
        .ok_or_else(|| "workspace-task-not-found".to_owned())?;
    if candidate.revision != revision {
        return Err("workspace-task-source-changed".to_owned());
    }
    let execution = state
        .get_workspace_task_execution(&candidate.job_id)
        .map_err(workspace_task_storage_error)?
        .ok_or_else(|| "workspace-task-not-found".to_owned())?;
    if verify_workspace_task_execution(&execution).is_err() {
        invalidate_workspace_task(state.inner().as_ref(), &execution)?;
        return Err("workspace-task-source-changed".to_owned());
    }
    state
        .trust_workspace_task_source_at(&source_id, &revision, current_epoch_millis())
        .map_err(workspace_task_storage_error)?;
    let refreshed = state
        .get_workspace_task_execution(&candidate.job_id)
        .map_err(workspace_task_storage_error)?
        .ok_or_else(|| "workspace-task-not-found".to_owned())?;
    if revalidate_workspace_task_execution(&refreshed).is_err() {
        invalidate_workspace_task(state.inner().as_ref(), &refreshed)?;
        return Err("workspace-task-source-changed".to_owned());
    }
    Ok(true)
}

/// Preview package scripts and Cargo targets from a local project root.  The
/// operation is read-only and offline; no package manager or Cargo process is
/// started.
#[tauri::command]
pub fn preview_project_import(
    path: String,
    operation_id: String,
    state: State<'_, Arc<DatabaseState>>,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<ProjectImportPlan, String> {
    let operation = operations
        .begin(&operation_id)
        .map_err(project_import_error)?;
    let plan = preview_project_with_control(Path::new(&path), operation.control())
        .map_err(project_import_error)?;
    project_plan_with_conflicts(plan, state.inner().as_ref(), operation.control())
}

/// Cancel one in-flight bounded preview/apply operation. Cancellation is
/// cooperative and never rolls back an already committed database transaction;
/// project apply builds one atomic batch and checks the flag before saving.
#[tauri::command]
pub fn cancel_project_import(
    operation_id: String,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<bool, String> {
    operations
        .cancel(&operation_id)
        .map_err(project_import_error)
}

/// Re-read and apply only the selected preview items.  Source revision and
/// canonical root are checked first, and every resulting definition is
/// disabled with a fixed manual-review schedule.
#[tauri::command]
pub fn apply_project_import(
    path: String,
    source_root: String,
    revision: String,
    selected: Vec<String>,
    operation_id: String,
    state: State<'_, Arc<DatabaseState>>,
    operations: State<'_, Arc<ImportOperationRegistry>>,
) -> Result<ProjectImportApplyResult, String> {
    if selected.len() > MAX_ITEMS {
        return Err("project-import-too-many-items".to_owned());
    }
    validate_selection_ids(&selected, "project-import-invalid")?;
    let operation = operations
        .begin(&operation_id)
        .map_err(project_import_error)?;
    let plan = verify_preview_revision_with_control(
        Path::new(&path),
        &source_root,
        &revision,
        operation.control(),
    )
    .map_err(project_import_error)?;
    let selected = selected_project_ids(&selected, &plan)?;
    let mut imported_inputs = Vec::new();
    for item in plan.items.iter().filter(|item| selected.contains(&item.id)) {
        imported_inputs.push(imported_job_input(item, &plan.source_root));
    }
    operation.control().check().map_err(project_import_error)?;
    let control = operation.control();
    let (created, skipped_in_transaction) = state
        .create_import_jobs_at_with_cancel(imported_inputs, current_epoch_millis(), || {
            control
                .check()
                .map_err(|error| StorageError::Validation(error.to_string()))
        })
        .map_err(|error| match error {
            StorageError::Validation(code)
                if matches!(
                    code.as_str(),
                    "project-import-cancelled" | "project-import-timeout"
                ) =>
            {
                code
            }
            _ => "project-import-save-failed".to_owned(),
        })?;
    Ok(ProjectImportApplyResult {
        created,
        skipped_conflicts: skipped_in_transaction,
    })
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
// Kept as a compatibility command for the existing positional IPC contract;
// new callers use `list_run_history` with one structured filter object.
#[allow(clippy::too_many_arguments)]
pub fn list_runs(
    job_id: Option<String>,
    limit: Option<u32>,
    start_at: Option<i64>,
    end_at: Option<i64>,
    status: Option<RunStatus>,
    kind: Option<JobKind>,
    min_duration_ms: Option<i64>,
    max_duration_ms: Option<i64>,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<RunView>, String> {
    let filter = RunHistoryFilter {
        job_id,
        kind,
        status,
        start_at,
        end_at,
        min_duration_ms,
        max_duration_ms,
        limit,
    };
    state
        .list_run_history(&filter, current_epoch_millis())
        .map(|runs| runs.iter().map(RunView::from_run).collect())
        .map_err(|error| history_command_error(&filter, error))
}

fn history_command_error(filter: &RunHistoryFilter, _error: StorageError) -> String {
    if filter.validate().is_err() {
        "run-history-invalid-filter".to_owned()
    } else {
        "run-storage-failed".to_owned()
    }
}

/// Explicit alias for callers that prefer a single filter object.  The
/// legacy `list_runs` command above remains available for existing clients.
#[tauri::command]
pub fn list_run_history(
    input: RunHistoryFilter,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<Vec<RunView>, String> {
    state
        .list_run_history(&input, current_epoch_millis())
        .map(|runs| runs.iter().map(RunView::from_run).collect())
        .map_err(|error| history_command_error(&input, error))
}

fn scheduler_command_error(error: SchedulerError) -> String {
    match error {
        SchedulerError::Storage(_) => "run-storage-failed".to_string(),
        SchedulerError::Cron(_) => "job-schedule-invalid".to_string(),
        SchedulerError::Adapter { source, .. }
            if matches!(
                source.message.as_str(),
                "workspace-task-source-changed" | "workspace-task-configuration-invalid"
            ) =>
        {
            source.message
        }
        SchedulerError::Adapter { .. } => "run-execution-failed".to_string(),
        SchedulerError::Join(_) => "scheduler-unavailable".to_string(),
    }
}

/// Start one explicit run through the same overlap policy, protected
/// environment boundary, logs, and process adapter as scheduled work.
#[tauri::command]
pub async fn run_job_now(
    id: String,
    runtime: State<'_, Arc<RuntimeState>>,
    database: State<'_, Arc<DatabaseState>>,
) -> Result<RunView, String> {
    revalidate_workspace_task_action(&id, database.inner().as_ref())?;
    runtime
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

/// Search the currently retained app-owned stdout/stderr snapshots for one
/// run. The command returns only bounded line metadata; it never stores,
/// forwards, or re-emits matching log text.
#[tauri::command]
pub async fn search_run_logs(
    app: AppHandle,
    input: LogSearchRequest,
    state: State<'_, Arc<DatabaseState>>,
) -> Result<LogSearchResponse, String> {
    validate_request(&input).map_err(log_search_command_error)?;
    let run = state
        .get_run(&input.run_id)
        .map_err(|_| "run-storage-failed".to_string())?
        .ok_or_else(|| "run-not-found".to_string())?;
    let log_dir = run.log_dir.ok_or_else(|| "logs-unavailable".to_string())?;
    let data_root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "logs-unavailable".to_string())?;
    let streams =
        open_search_streams(data_root.clone(), log_dir.clone(), input.run_id.clone()).await?;

    let selected = input.source.map_or_else(
        || vec![LogStream::Stdout, LogStream::Stderr],
        |stream| vec![stream],
    );
    let mut snapshots = Vec::with_capacity(selected.len());
    let mut read_truncated = false;
    for stream in selected {
        let (bytes, truncated) = match read_search_snapshot(&streams, stream).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                // A writer may rotate a segment between two cursor reads. A
                // fresh metadata snapshot is safe to retry once; repeated
                // failure remains the same fixed read error.
                let fresh =
                    open_search_streams(data_root.clone(), log_dir.clone(), input.run_id.clone())
                        .await
                        .map_err(|_| "log-search-read-failed".to_string())?;
                read_search_snapshot(&fresh, stream).await?
            }
        };
        snapshots.push((stream, bytes));
        read_truncated |= truncated;
    }
    let fallback_timestamp = run.started_at.or(Some(run.created_at));
    let mut response =
        tokio::task::spawn_blocking(move || search_streams(&input, &snapshots, fallback_timestamp))
            .await
            .map_err(|_| "log-search-failed".to_string())?
            .map_err(log_search_command_error)?;
    response.truncated |= read_truncated;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn bounded_search_reads_yield_to_a_running_writer() {
        let root = tempfile::tempdir().expect("temporary log root");
        let streams = LogStreams::open(
            root.path(),
            "run-1",
            crate::logs::LogLimits {
                segment_bytes: MAX_TAIL_BYTES as u64,
                max_segments: 8,
            },
        )
        .expect("log streams");
        streams
            .append(LogStream::Stdout, &vec![b'a'; MAX_TAIL_BYTES * 2])
            .await
            .expect("seed log");

        let reader = streams.clone();
        let writer = streams.clone();
        let search =
            tokio::spawn(async move { read_search_snapshot(&reader, LogStream::Stdout).await });
        tokio::task::yield_now().await;
        let append = tokio::time::timeout(
            Duration::from_secs(2),
            writer.append(LogStream::Stdout, b"writer\n"),
        )
        .await;
        assert!(append.is_ok(), "writer should not wait for the full search");
        let snapshot = search.await.expect("search task").expect("search snapshot");
        assert!(!snapshot.0.is_empty());
    }

    #[test]
    fn import_selection_ids_are_bounded_before_hashing() {
        assert!(validate_selection_ids(&["safe-id".to_owned()], "invalid").is_ok());
        assert_eq!(
            validate_selection_ids(&["\n".to_owned()], "invalid"),
            Err("invalid".to_owned())
        );
        assert_eq!(
            validate_selection_ids(&["x".repeat(MAX_SELECTION_ID_BYTES + 1)], "invalid"),
            Err("invalid".to_owned())
        );
    }

    #[test]
    fn import_selection_ids_must_belong_to_the_preview_plan() {
        assert_eq!(
            selected_definition_ids(&["unknown".to_owned()], &[], &[], "invalid"),
            Err("invalid".to_owned())
        );

        let plan = ProjectImportPlan {
            schema_version: 1,
            source_root: "/work/demo".to_owned(),
            revision: "a".repeat(64),
            files: Vec::new(),
            items: vec![crate::core::imports::ProjectImportItem {
                id: "npm:script:build".to_owned(),
                name: "npm · build".to_owned(),
                status: "new".to_owned(),
                command: "npm run -- build".to_owned(),
                kind: JobKind::Job,
                source: crate::core::imports::ProjectImportSource::PackageScript,
                source_name: "scripts.build".to_owned(),
                source_path: "package.json".to_owned(),
                cwd: "/work/demo".to_owned(),
                environment_keys: Vec::new(),
                requires_confirmation: true,
                detail: "fixture".to_owned(),
            }],
        };
        assert!(selected_project_ids(&["npm:script:build".to_owned()], &plan).is_ok());
        assert_eq!(
            selected_project_ids(&["npm:script:missing".to_owned()], &plan),
            Err("project-import-invalid".to_owned())
        );
    }

    #[test]
    fn workspace_import_operation_codes_do_not_reuse_project_prefix() {
        assert_eq!(
            workspace_task_operation_error(ProjectImportError::Cancelled),
            "workspace-task-import-cancelled"
        );
        assert_eq!(
            workspace_task_operation_error(ProjectImportError::TimedOut),
            "workspace-task-import-timeout"
        );
    }

    #[test]
    fn workspace_adapter_codes_remain_actionable_at_the_command_boundary() {
        assert_eq!(
            scheduler_command_error(SchedulerError::Adapter {
                run_id: "opaque-run".to_owned(),
                source: crate::scheduler::AdapterError::new("workspace-task-source-changed",),
            }),
            "workspace-task-source-changed"
        );
    }
}
