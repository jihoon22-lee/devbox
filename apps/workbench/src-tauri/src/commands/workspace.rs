//! Workbench command — 프로필 CRUD, health, Start/Stop Workspace.

use crate::commands::environment::EnvironmentInjection;
use crate::commands::process_tree::ProcessTree;
use crate::core::health::{has_distro, parse_git_status};
use crate::core::operation::{
    poll_interval, wait_for_change, OperationBudget, OperationClaim, OperationError,
    OperationToken, SingleFlight,
};
use crate::core::preflight::{
    PreflightStatus, ResourceProvenance, ResourceState, WorkspacePreflight,
};
use crate::core::profile::{
    validate_profile_id, validate_service_id, ProfileStore, ProjectProfile, MAX_SERVICES,
};
use crate::core::retry::{
    can_retry, failed_step, plan_retry, RetryPlanError, RetryStep, OPEN_CODE_PAD_STEP,
    OPEN_WSL_STEP, WAIT_PORT_STEP,
};
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::Metadata;
use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

const PROFILE_FILE: &str = "project-profiles.json";
const LIFE_LOG_PRODUCER: &str = "life-log";
const LIFE_LOG_SNAPSHOT_VERSION: u32 = 1;
const LIFE_LOG_PROJECTS_VIEW: &str = "projects";
const LIFE_LOG_PROJECTS_VIEW_VERSION: u32 = 1;
const MAX_LIFE_LOG_PROJECTS: usize = 512;
const PROFILE_READ_ERROR: &str = "프로필 저장소를 읽을 수 없습니다";
const PROFILE_WRITE_ERROR: &str = "프로필 저장소를 저장할 수 없습니다";
const PROFILE_CONFLICT_ERROR: &str =
    "프로필 저장소가 다른 작업으로 변경되었습니다. 다시 시도하세요";
const PROFILE_PATH_ERROR: &str = "프로필 저장소 경로를 확인할 수 없습니다";
const WORKSPACE_START_TIMEOUT: Duration = Duration::from_secs(30);
const PROJECT_HEALTH_TIMEOUT: Duration = Duration::from_secs(10);
const WSL_COMMAND_STDOUT_BYTES: usize = 64 * 1024;
const WSL_COMMAND_STDERR_BYTES: usize = 64 * 1024;
const PORT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);
const PORT_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const PROCESS_TERMINATION_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_PROCESS_STAT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LifeLogAbsorbReport {
    pub added: usize,
    /// 파일 경과 시간과 producer가 기록한 view 경과 시간을 합친 값.
    pub freshness_ms: Option<u64>,
    /// 안전하지만 distro 정보가 없어 ProjectProfile로 표현할 수 없는 POSIX 경로 수.
    pub unsupported_paths: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LifeLogProjectEntry {
    path: String,
    activity_window_start_ms: i64,
    last_activity_at_ms: Option<i64>,
    recent_session_count: u64,
    recent_duration_ms: i64,
}

fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|_| PROFILE_PATH_ERROR.to_string())?;
    Ok(dir.join(PROFILE_FILE))
}

fn is_link_metadata(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn read_profile_file(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    read_profile_file_with_control(path, None, None)
}

fn read_profile_file_with_control(
    path: &std::path::Path,
    token: Option<&OperationToken>,
    budget: Option<OperationBudget>,
) -> Result<Option<Vec<u8>>, String> {
    if let (Some(token), Some(budget)) = (token, budget) {
        budget.check(token).map_err(OperationError::message)?;
    }
    reject_links_in_existing_path(path)?;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(PROFILE_READ_ERROR.into()),
    };
    if is_link_metadata(&metadata) || !metadata.file_type().is_file() {
        return Err(PROFILE_READ_ERROR.into());
    }
    if metadata.len() > crate::core::profile::MAX_PROFILE_FILE_BYTES as u64 {
        return Err("프로필 저장소 크기 제한을 초과했습니다".into());
    }

    let source_identity =
        crate::platform::path_identity(path, false).map_err(|_| PROFILE_PATH_ERROR.to_string())?;
    let (file, opened_identity) = crate::platform::open_readonly_with_identity(path, false)
        .map_err(|_| PROFILE_READ_ERROR.to_string())?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| PROFILE_READ_ERROR.to_string())?;
    if is_link_metadata(&opened_metadata) || source_identity != opened_identity {
        return Err(PROFILE_PATH_ERROR.into());
    }
    let mut reader = file.take((crate::core::profile::MAX_PROFILE_FILE_BYTES + 1) as u64);
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .unwrap_or(crate::core::profile::MAX_PROFILE_FILE_BYTES)
            .min(crate::core::profile::MAX_PROFILE_FILE_BYTES)
            + 1,
    );
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        if let (Some(token), Some(budget)) = (token, budget) {
            budget.check(token).map_err(OperationError::message)?;
        }
        let count = reader
            .read(&mut chunk)
            .map_err(|_| PROFILE_READ_ERROR.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > crate::core::profile::MAX_PROFILE_FILE_BYTES {
            return Err("프로필 저장소 크기 제한을 초과했습니다".into());
        }
    }

    if let (Some(token), Some(budget)) = (token, budget) {
        budget.check(token).map_err(OperationError::message)?;
    }
    // Recheck every existing parent after the handle read. A directory
    // replacement can otherwise make the same textual profile path resolve
    // through a newly-created junction/symlink while the read is in flight.
    reject_links_in_existing_path(path)?;
    let after_metadata =
        std::fs::symlink_metadata(path).map_err(|_| PROFILE_PATH_ERROR.to_string())?;
    let after_identity =
        crate::platform::path_identity(path, false).map_err(|_| PROFILE_PATH_ERROR.to_string())?;
    if is_link_metadata(&after_metadata) || source_identity != after_identity {
        return Err(PROFILE_PATH_ERROR.into());
    }
    Ok(Some(bytes))
}

fn reject_links_in_existing_path(path: &std::path::Path) -> Result<(), String> {
    // `ancestors()` keeps Windows drive/UNC prefixes intact. Component-wise
    // PathBuf construction can otherwise turn an absolute `C:\\...` path into
    // the relative drive path `C:` before the reparse check.
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => return Err(PROFILE_PATH_ERROR.into()),
        };
        if is_link_metadata(&metadata) {
            return Err(PROFILE_PATH_ERROR.into());
        }
    }
    Ok(())
}

fn ensure_profile_directory(path: &std::path::Path) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| PROFILE_PATH_ERROR.to_string())?;
    reject_links_in_existing_path(directory)?;
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if is_link_metadata(&metadata) || !metadata.file_type().is_dir() => {
            Err(PROFILE_PATH_ERROR.into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(directory).map_err(|_| PROFILE_WRITE_ERROR.to_string())?;
            reject_links_in_existing_path(directory)?;
            match std::fs::symlink_metadata(directory) {
                Ok(metadata) if !is_link_metadata(&metadata) && metadata.file_type().is_dir() => {
                    Ok(())
                }
                _ => Err(PROFILE_PATH_ERROR.into()),
            }
        }
        Err(_) => Err(PROFILE_PATH_ERROR.into()),
    }
}

#[derive(Debug)]
pub(crate) struct ProfileStoreDocument {
    pub(crate) store: ProfileStore,
    raw: Option<Vec<u8>>,
}

pub(crate) fn load_store_document(app: &AppHandle) -> Result<ProfileStoreDocument, String> {
    let path = profile_path(app)?;
    load_store_document_at_path(&path, None, None)
}

fn load_store_document_at_path(
    path: &std::path::Path,
    token: Option<&OperationToken>,
    budget: Option<OperationBudget>,
) -> Result<ProfileStoreDocument, String> {
    let Some(bytes) = read_profile_file_with_control(path, token, budget)? else {
        if let (Some(token), Some(budget)) = (token, budget) {
            budget.check(token).map_err(OperationError::message)?;
        }
        return Ok(ProfileStoreDocument {
            store: ProfileStore::empty(),
            raw: None,
        });
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| PROFILE_READ_ERROR.to_string())?;
    let store = ProfileStore::load(text).map_err(|_| PROFILE_READ_ERROR.to_string())?;
    if let (Some(token), Some(budget)) = (token, budget) {
        budget.check(token).map_err(OperationError::message)?;
    }
    Ok(ProfileStoreDocument {
        store,
        raw: Some(bytes),
    })
}

pub(crate) async fn load_store_document_async(
    app: &AppHandle,
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<ProfileStoreDocument, String> {
    let path = profile_path(app)?;
    let worker_guard = claim.worker_guard().map_err(str::to_string)?;
    let worker_token = token.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _worker_guard = worker_guard;
        load_store_document_at_path(&path, Some(&worker_token), Some(budget))
    });
    tokio::pin!(worker);
    let result = tokio::select! {
        result = &mut worker => result.map_err(|_| "프로필 저장소 작업을 완료하지 못했습니다".to_string())?,
        control = wait_for_change(token.clone(), budget) => {
            token.cancel();
            let _ = worker.await;
            Err(control.message().to_string())
        }
    }?;
    budget.check(&token).map_err(OperationError::message)?;
    Ok(result)
}

pub(crate) fn load_store(app: &AppHandle) -> Result<ProfileStore, String> {
    load_store_document(app).map(|document| document.store)
}

/// Write a validated next store only if the bytes observed before editing are
/// still present. The process-local mutex in CRUD commands serializes app
/// writers; this byte comparison also rejects ordinary concurrent external
/// edits instead of silently losing them.
pub(crate) fn save_store_document(
    app: &AppHandle,
    expected: &ProfileStoreDocument,
    store: &ProfileStore,
) -> Result<(), String> {
    let json = store.to_json_checked()?;
    let path = profile_path(app)?;
    let current = read_profile_file(&path)?;
    if !document_is_current(expected, current.as_deref()) {
        return Err(PROFILE_CONFLICT_ERROR.into());
    }
    ensure_profile_directory(&path)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if is_link_metadata(&metadata) => return Err(PROFILE_PATH_ERROR.into()),
        Ok(metadata) if !metadata.file_type().is_file() => return Err(PROFILE_PATH_ERROR.into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(PROFILE_PATH_ERROR.into()),
    }
    devbox_filesystem::atomic_write(path, json.as_bytes())
        .map_err(|_| PROFILE_WRITE_ERROR.to_string())
}

fn document_is_current(expected: &ProfileStoreDocument, current: Option<&[u8]>) -> bool {
    expected.raw.as_deref() == current
}

/// Process-local writer gate. Reads remain lock-free because writes replace a
/// complete file atomically, while every mutation holds this gate through its
/// load/validate/CAS/write sequence.
pub struct ProfileStoreState {
    pub(crate) lock: Mutex<()>,
}

pub fn profile_store_state() -> Arc<ProfileStoreState> {
    Arc::new(ProfileStoreState {
        lock: Mutex::new(()),
    })
}

#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Result<Vec<ProjectProfile>, String> {
    Ok(load_store(&app)?.profiles)
}

#[tauri::command]
pub fn create_profile(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    mut profile: ProjectProfile,
) -> Result<ProjectProfile, String> {
    let _store_lock = store_state
        .lock
        .lock()
        .map_err(|_| PROFILE_WRITE_ERROR.to_string())?;
    let document = load_store_document(&app)?;
    let mut store = document.store.clone();
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    let dup = store.upsert(profile)?;
    if let Some(existing) = dup {
        return Ok(existing);
    }
    let created = store
        .profiles
        .last()
        .cloned()
        .ok_or_else(|| PROFILE_WRITE_ERROR.to_string())?;
    save_store_document(&app, &document, &store)?;
    Ok(created)
}

#[tauri::command]
pub fn update_profile(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    profile: ProjectProfile,
) -> Result<(), String> {
    let _store_lock = store_state
        .lock
        .lock()
        .map_err(|_| PROFILE_WRITE_ERROR.to_string())?;
    let document = load_store_document(&app)?;
    let mut store = document.store.clone();
    store.replace(profile)?;
    save_store_document(&app, &document, &store)
}

#[tauri::command]
pub fn delete_profile(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    id: String,
) -> Result<(), String> {
    validate_profile_id(&id)?;
    let _transition_claim =
        claim_workspace_transition(&registry.starting_profile, &id).map_err(str::to_string)?;
    let runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?;
    let has_active_run = has_active_profile_run(&runs, &id);
    drop(runs);
    if has_active_run {
        return Err("실행 중인 프로필은 먼저 Workbench가 시작한 리소스를 중지하세요".to_string());
    }
    let _store_lock = store_state
        .lock
        .lock()
        .map_err(|_| PROFILE_WRITE_ERROR.to_string())?;
    let document = load_store_document(&app)?;
    let mut store = document.store.clone();
    if !store.remove(&id) {
        return Err("프로필을 찾을 수 없습니다".to_string());
    }
    save_store_document(&app, &document, &store)
}

/// wsl-desktop의 gitStatus 이관 (§3.1, §15.2). 프로젝트 경로들의 git 상태.
#[tauri::command]
pub async fn git_status(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    projects: Vec<String>,
) -> Result<Vec<crate::core::health::GitStatus>, String> {
    let operation = &registry.health_operation;
    let budget = OperationBudget::from_now(PROJECT_HEALTH_TIMEOUT);
    // Supersede only an older Git-status request. Other read-only surfaces and
    // a mutating Workspace transition keep their own lane ownership.
    operation
        .cancel_kind("git-status")
        .map_err(str::to_string)?;
    let pending = operation.prepare("git-status").map_err(str::to_string)?;
    let token = pending.token();
    operation.wait_until_idle(token.clone(), budget).await?;
    budget.check(&token).map_err(OperationError::message)?;
    let claim = pending.claim().map_err(str::to_string)?;
    let token = claim.token();
    git_status_with_control(projects, token, budget, &claim).await
}

async fn git_status_with_control(
    projects: Vec<String>,
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<Vec<crate::core::health::GitStatus>, String> {
    let mut out = Vec::with_capacity(projects.len());
    for path in projects {
        budget.check(&token).map_err(OperationError::message)?;
        let path_for_worker = path.clone();
        let cancellation = token.cancellation_flag();
        let worker_cancellation = Arc::clone(&cancellation);
        let child_timeout = budget.remaining();
        let worker_guard = claim.worker_guard().map_err(str::to_string)?;
        let worker = tokio::task::spawn_blocking(move || {
            let _worker_guard = worker_guard;
            devbox_git::run_bounded_with_cancel(
                &["status", "--porcelain", "--branch"],
                &path_for_worker,
                child_timeout,
                64 * 1024,
                &worker_cancellation,
            )
        });
        tokio::pin!(worker);
        let result = tokio::select! {
            output = &mut worker => output.map_err(|_| "git 작업을 완료하지 못했습니다".to_string())?,
            control = wait_for_change(token.clone(), budget) => {
                cancellation.store(true, std::sync::atomic::Ordering::Release);
                let _ = worker.await;
                return Err(control.message().to_string());
            }
        };
        budget.check(&token).map_err(OperationError::message)?;
        match result {
            Ok(text) => out.push(parse_git_status(&path, &text)),
            Err(error) if error == "git_cancelled" => {
                return Err(OperationError::Cancelled.message().into())
            }
            Err(error) if error == "git_timeout" => {
                return Err(OperationError::TimedOut.message().into())
            }
            Err(_) => out.push(crate::core::health::GitStatus {
                path,
                branch: "n/a".into(),
                changes: 0,
                clean: false,
            }),
        }
    }
    Ok(out)
}

// ── health ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHealth {
    pub profile_id: String,
    pub items: Vec<HealthItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunManagerSnapshotData {
    active_services: Vec<RunManagerActiveService>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunManagerActiveService {
    id: String,
    uptime_ms: i64,
}

/// Decode the v1 producer payload as one bounded unit. A malformed entry must
/// not turn the entire producer state into the misleading "nothing running"
/// result; callers surface it as unavailable instead.
pub(crate) fn active_service_ids(data: &serde_json::Value) -> Result<HashSet<String>, ()> {
    let snapshot: RunManagerSnapshotData = serde_json::from_value(data.clone()).map_err(|_| ())?;
    if snapshot.active_services.len() > MAX_SERVICES {
        return Err(());
    }

    let mut ids = HashSet::with_capacity(snapshot.active_services.len());
    for service in snapshot.active_services {
        validate_service_id(&service.id).map_err(|_| ())?;
        if service.uptime_ms < 0 || !ids.insert(service.id) {
            return Err(());
        }
    }
    Ok(ids)
}

fn service_health_item(configured: &[String], running: Result<HashSet<String>, ()>) -> HealthItem {
    let Ok(running) = running else {
        return HealthItem {
            name: "services".into(),
            ok: false,
            detail: "서비스 상태를 확인할 수 없습니다".into(),
        };
    };
    let missing: Vec<&str> = configured
        .iter()
        .filter(|id| !running.contains(*id))
        .map(String::as_str)
        .collect();
    HealthItem {
        name: "services".into(),
        ok: missing.is_empty(),
        detail: if missing.is_empty() {
            "서비스 전부 실행 중".into()
        } else {
            format!("미실행 서비스 {}개", missing.len())
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeCommandError {
    Cancelled,
    TimedOut,
    Io,
    OutputTooLarge,
}

struct NativeCommandOutput {
    stdout: Vec<u8>,
    success: bool,
}

/// Run one fixed native command with bounded output and the caller's
/// cancellation/deadline. The child is killed and reaped on every error path;
/// dropping a Tauri invocation therefore cannot strand `wsl.exe`.
async fn run_fixed_native_command(
    args: &[&str],
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<NativeCommandOutput, NativeCommandError> {
    // Keep the single-flight lease in a detached task as well as in the
    // command future. If Tauri drops the invocation, `OperationClaim::drop`
    // cancels this same token; the task then kills/reaps the child before its
    // lease is released, so a newer health request cannot overlap a still
    // unwinding `wsl.exe` process.
    let worker_guard = claim.worker_guard().map_err(|_| NativeCommandError::Io)?;
    let worker_args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let worker_token = token.clone();
    let worker = tokio::spawn(async move {
        let _worker_guard = worker_guard;
        run_fixed_native_command_inner(worker_args, worker_token, budget).await
    });
    worker.await.map_err(|_| NativeCommandError::Io)?
}

async fn run_fixed_native_command_inner(
    args: Vec<String>,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<NativeCommandOutput, NativeCommandError> {
    budget.check(&token).map_err(|error| match error {
        OperationError::Cancelled => NativeCommandError::Cancelled,
        OperationError::TimedOut => NativeCommandError::TimedOut,
    })?;
    let mut command = Command::new("wsl.exe");
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000);
    let mut child = command.spawn().map_err(|_| NativeCommandError::Io)?;
    let mut process_tree = match ProcessTree::assign(&child) {
        Ok(tree) => tree,
        Err(()) => {
            ProcessTree::terminate_unassigned(&mut child).await;
            return Err(NativeCommandError::Io);
        }
    };
    let Some(stdout) = child.stdout.take() else {
        terminate_native_tree(&mut process_tree, &mut child).await;
        return Err(NativeCommandError::Io);
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_native_tree(&mut process_tree, &mut child).await;
        return Err(NativeCommandError::Io);
    };

    // Keep the stream reader future independent from `child`: this allows
    // cancellation/timeout to kill and reap the child after the reader is
    // dropped without holding a second mutable borrow across the await.
    let read_result = {
        let readers = async {
            tokio::try_join!(
                read_native_bounded(stdout, WSL_COMMAND_STDOUT_BYTES),
                drain_native_bounded(stderr, WSL_COMMAND_STDERR_BYTES),
            )
        };
        tokio::pin!(readers);
        tokio::select! {
            output = &mut readers => output,
            control = wait_for_change(token.clone(), budget) => Err(match control {
                OperationError::Cancelled => NativeCommandError::Cancelled,
                OperationError::TimedOut => NativeCommandError::TimedOut,
            }),
        }
    };
    let mut result = match read_result {
        Ok((stdout, _)) => {
            tokio::select! {
                status = child.wait() => status
                    .map(|status| NativeCommandOutput {
                        stdout,
                        success: status.success(),
                    })
                    .map_err(|_| NativeCommandError::Io),
                control = wait_for_change(token.clone(), budget) => Err(match control {
                    OperationError::Cancelled => NativeCommandError::Cancelled,
                    OperationError::TimedOut => NativeCommandError::TimedOut,
                }),
            }
        }
        Err(error) => Err(error),
    };
    if result.is_ok() {
        if let Err(control) = budget.check(&token) {
            result = Err(match control {
                OperationError::Cancelled => NativeCommandError::Cancelled,
                OperationError::TimedOut => NativeCommandError::TimedOut,
            });
        }
    }
    if result.is_err() {
        terminate_native_tree(&mut process_tree, &mut child).await;
    } else {
        // A successful root exit is not proof that a helper it spawned has
        // gone away. Close the complete probe tree before releasing the
        // health single-flight worker guard.
        process_tree.terminate_descendants();
    }
    result
}

async fn terminate_native_tree(tree: &mut ProcessTree, child: &mut Child) {
    tree.terminate(child).await;
}

async fn read_native_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<Vec<u8>, NativeCommandError> {
    let mut output = Vec::with_capacity(max_bytes.min(16 * 1024));
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|_| NativeCommandError::Io)?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > max_bytes {
            return Err(NativeCommandError::OutputTooLarge);
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

async fn drain_native_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(), NativeCommandError> {
    let mut total = 0usize;
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(|_| NativeCommandError::Io)?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count);
        if total > max_bytes {
            return Err(NativeCommandError::OutputTooLarge);
        }
    }
}

async fn wsl_list_output(
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<Option<String>, NativeCommandError> {
    let output = run_fixed_native_command(&["-l", "-v"], token, budget, claim).await?;
    if !output.success {
        return Ok(None);
    }
    // `wsl.exe -l -v`는 UTF-16LE로 출력된다 (공용 crates/wsl 디코더, PR #183과 동일 근거).
    Ok(Some(devbox_wsl::output::decode_output(&output.stdout)))
}

async fn delay_with_control(
    delay: Duration,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<(), OperationError> {
    let end = std::time::Instant::now()
        .checked_add(delay)
        .unwrap_or_else(std::time::Instant::now);
    loop {
        budget.check(&token)?;
        let remaining = end.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        tokio::time::sleep(remaining.min(poll_interval())).await;
    }
}

async fn read_run_manager_snapshot_with_control(
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<Option<devbox_integration::Envelope>, String> {
    budget.check(&token).map_err(OperationError::message)?;
    let worker_guard = claim.worker_guard().map_err(str::to_string)?;
    let worker_token = token.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let _worker_guard = worker_guard;
        budget
            .check(&worker_token)
            .map_err(OperationError::message)?;
        devbox_integration::read_snapshot("run-manager", 1)
            .map_err(|_| "서비스 snapshot을 읽을 수 없습니다".to_string())
    });
    tokio::pin!(worker);
    let result = tokio::select! {
        result = &mut worker => result.map_err(|_| "서비스 snapshot 작업을 완료하지 못했습니다".to_string())?,
        control = wait_for_change(token.clone(), budget) => {
            token.cancel();
            let _ = worker.await;
            Err(control.message().to_string())
        }
    }?;
    budget.check(&token).map_err(OperationError::message)?;
    Ok(result)
}

fn port_open_with_control(
    port: u16,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<bool, OperationError> {
    use std::net::TcpStream;
    use std::time::Duration;
    budget.check(token)?;
    let addr = format!("127.0.0.1:{port}")
        .parse()
        .map_err(|_| OperationError::TimedOut)?;
    // A synchronous connect cannot be interrupted in the middle of the OS
    // call, so cap it by the shared budget. The checks on both sides turn a
    // cancellation racing with the probe into a deterministic operation
    // result instead of letting a long expected-port list run unbounded.
    let timeout = Duration::from_millis(800).min(budget.remaining());
    let open = TcpStream::connect_timeout(&addr, timeout).is_ok();
    budget.check(token)?;
    Ok(open)
}

/// read-only project health. run-manager 서비스는 integration snapshot(§10.1)으로 읽는다.
#[tauri::command]
pub async fn project_health(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
    request_id: Option<String>,
) -> Result<ProjectHealth, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(request_id.as_deref())?;
    let operation_key = health_operation_key(&profile_id, request_id.as_deref());
    let operation = &registry.health_operation;
    let budget = OperationBudget::from_now(PROJECT_HEALTH_TIMEOUT);
    // Supersede only an older project-health request. Dependency health and
    // Start Workspace use the same lane without cancelling this request.
    operation
        .cancel_kind("project-health")
        .map_err(str::to_string)?;
    let pending = operation.prepare(operation_key).map_err(str::to_string)?;
    let token = pending.token();
    operation.wait_until_idle(token.clone(), budget).await?;
    budget.check(&token).map_err(OperationError::message)?;
    let claim = pending.claim().map_err(str::to_string)?;
    let token = claim.token();
    project_health_with_control(app, profile_id, token, budget, &claim).await
}

/// Cancel a health request only when the caller still owns its exact profile
/// key. This covers selection clearing/window teardown, where no newer health
/// request would otherwise claim the single-flight slot.
#[tauri::command]
pub fn cancel_project_health(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
    request_id: String,
) -> Result<bool, String> {
    validate_profile_id(&profile_id)?;
    validate_operation_request_id(Some(&request_id))?;
    registry
        .health_operation
        .cancel(&health_operation_key(&profile_id, Some(&request_id)))
        .map_err(str::to_string)
}

pub(crate) fn validate_operation_request_id(request_id: Option<&str>) -> Result<(), String> {
    if request_id.is_some_and(|value| {
        value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
    }) {
        return Err("Workspace 작업 ID가 올바르지 않습니다".to_string());
    }
    Ok(())
}

fn health_operation_key(profile_id: &str, request_id: Option<&str>) -> String {
    match request_id {
        // Profile IDs and opaque request IDs are independently validated but
        // may contain a printable separator such as `:`. A NUL separator is
        // unambiguous because both IPC inputs reject control characters, so
        // an old request can never cancel a different profile/request pair
        // through string-key concatenation.
        Some(request_id) => format!("project-health\0{profile_id}\0{request_id}"),
        None => format!("project-health\0{profile_id}"),
    }
}

async fn project_health_with_control(
    app: AppHandle,
    profile_id: String,
    token: OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<ProjectHealth, String> {
    budget.check(&token).map_err(OperationError::message)?;
    let store = load_store_document_async(&app, token.clone(), budget, claim)
        .await?
        .store;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;

    let mut items = Vec::new();

    // git
    if let Some(root) = profile
        .git_root
        .clone()
        .or_else(|| profile.windows_path.clone())
    {
        let status = git_status_with_control(vec![root], token.clone(), budget, claim).await?;
        if let Some(s) = status.first() {
            items.push(HealthItem {
                name: "git".into(),
                ok: s.clean,
                detail: if s.clean {
                    "Git 작업 트리가 깨끗합니다".into()
                } else {
                    format!("Git 변경 사항 {}개", s.changes)
                },
            });
        }
    } else {
        items.push(HealthItem {
            name: "git".into(),
            ok: false,
            detail: "gitRoot 미설정".into(),
        });
    }

    // wsl distro
    if let Some(wsl) = profile.wsl.clone() {
        match wsl_list_output(token.clone(), budget, claim).await {
            Ok(Some(output)) => {
                let ok = has_distro(&wsl.distro, &output);
                items.push(HealthItem {
                    name: "wsl".into(),
                    ok,
                    detail: if ok {
                        "WSL distro와 working directory를 사용할 수 있습니다".into()
                    } else {
                        "설정한 WSL distro를 찾을 수 없습니다".into()
                    },
                });
            }
            Ok(None) => items.push(HealthItem {
                name: "wsl".into(),
                ok: false,
                detail: "wsl.exe 조회 불가".into(),
            }),
            Err(NativeCommandError::Cancelled) => {
                return Err(OperationError::Cancelled.message().into())
            }
            Err(NativeCommandError::TimedOut) => {
                return Err(OperationError::TimedOut.message().into())
            }
            Err(NativeCommandError::Io | NativeCommandError::OutputTooLarge) => {
                items.push(HealthItem {
                    name: "wsl".into(),
                    ok: false,
                    detail: "wsl.exe 조회 불가".into(),
                });
            }
        }
    }

    // expected ports
    let mut closed = Vec::new();
    for port in &profile.expected_ports {
        budget.check(&token).map_err(OperationError::message)?;
        if !port_open_with_control(*port, &token, budget).map_err(OperationError::message)? {
            closed.push(*port);
        }
    }
    if profile.expected_ports.is_empty() {
        items.push(HealthItem {
            name: "ports".into(),
            ok: true,
            detail: "예상 포트 없음".into(),
        });
    } else {
        items.push(HealthItem {
            name: "ports".into(),
            ok: closed.is_empty(),
            detail: if closed.is_empty() {
                format!(
                    "전부 open: {}",
                    profile
                        .expected_ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!(
                    "닫힘: {}",
                    closed
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        });
    }

    // run-manager services (integration snapshot)
    budget.check(&token).map_err(OperationError::message)?;
    if profile.run_manager_service_ids.is_empty() {
        items.push(HealthItem {
            name: "services".into(),
            ok: true,
            detail: "서비스 미지정".into(),
        });
    } else {
        let running =
            match read_run_manager_snapshot_with_control(token.clone(), budget, claim).await {
                Err(error) if error == OperationError::Cancelled.message() => return Err(error),
                Err(error) if error == OperationError::TimedOut.message() => return Err(error),
                Err(_) => Err(()),
                // A genuinely missing snapshot means no service is known to be
                // running. Corrupt producer data remains a distinct unavailable
                // state, per the v1 consumer contract.
                Ok(None) => Ok(HashSet::new()),
                Ok(Some(snapshot)) => active_service_ids(&snapshot.data),
            };
        items.push(service_health_item(
            &profile.run_manager_service_ids,
            running,
        ));
    }

    Ok(ProjectHealth { profile_id, items })
}

// ── Start/Stop Workspace ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStep {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub status: PreflightStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRun {
    pub run_id: String,
    pub profile_id: String,
    pub steps: Vec<RunStep>,
    /// Workbench가 시작한 프로세스 PID (Stop What I Started 대상).
    /// This remains backend-only ownership state and is never serialized to
    /// the webview.
    #[serde(skip_serializing)]
    started_pids: Vec<StartedProcess>,
    /// Stable resource ownership observed by the preflight and start steps.
    pub resource_provenance: Vec<ResourceProvenance>,
    /// Number of explicit retries performed for this run.
    pub retry_count: u32,
    /// A retry is offered only when a bounded, known step failed.
    pub can_retry: bool,
    /// Stable step key for the first failure; never a path or native error.
    pub failed_step: Option<String>,
}

/// A PID is only a location in the process table. Keep a creation identity
/// beside it so a later Stop cannot accidentally target an unrelated process
/// after the operating system reuses the PID. This remains backend-only and
/// is never serialized to the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartedProcess {
    /// Stable app identity associated with this PID. It is backend-only and
    /// lets an interrupted transition publish the exact resource ownership
    /// needed for a later Stop without exposing process details.
    app_id: &'static str,
    pid: u32,
    identity: ProcessIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessIdentity {
    #[cfg(windows)]
    Windows(u64),
    #[cfg(unix)]
    Unix(u64),
    Gone,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessObservation {
    Match,
    Missing,
    Mismatch,
    Unavailable,
}

impl StartedProcess {
    fn new(app_id: &'static str, pid: u32) -> Self {
        Self {
            app_id,
            pid,
            identity: capture_process_identity(pid),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRunOwnership {
    pub run_id: String,
    pub profile_id: String,
    pub retry_count: u32,
    pub can_retry: bool,
    pub failed_step: Option<String>,
}

/// Owns children during Start Workspace until the run is committed. Any early
/// return or dropped Tauri invocation therefore rolls back only the PIDs this
/// transition recorded. If the OS refuses cleanup, the remaining ownership is
/// published into the registry so Stop What I Started can recover it instead
/// of allowing an untracked child to outlive the transition.
struct StartedPidGuard {
    pids: Vec<StartedProcess>,
    recorded: Vec<StartedProcess>,
    published_run_id: Option<String>,
    registry: Arc<RunRegistry>,
    profile_id: String,
    existing_run_id: Option<String>,
    committed: bool,
}

impl StartedPidGuard {
    fn new(
        registry: &Arc<RunRegistry>,
        profile_id: impl Into<String>,
        existing_run_id: Option<String>,
    ) -> Self {
        Self {
            pids: Vec::new(),
            recorded: Vec::new(),
            published_run_id: None,
            registry: Arc::clone(registry),
            profile_id: profile_id.into(),
            existing_run_id,
            committed: false,
        }
    }

    fn push(&mut self, app_id: &'static str, pid: u32) {
        let process = StartedProcess::new(app_id, pid);
        self.pids.push(process);
        self.recorded.push(process);
    }

    fn rollback(&mut self) {
        self.cleanup();
    }

    fn commit(mut self) -> Vec<StartedProcess> {
        self.committed = true;
        std::mem::take(&mut self.pids)
    }

    fn cleanup(&mut self) {
        let mut remaining = Vec::with_capacity(self.pids.len());
        for process in &self.pids {
            if !terminate_started_process(process) {
                remaining.push(*process);
            }
        }
        self.pids = remaining;
        self.sync_registry();
    }

    /// Synchronize failed cleanup with the authoritative run. Existing retry
    /// runs keep their prior metadata; only this guard's process identities are
    /// added/removed. A Start failure without an existing run gets a minimal
    /// stop-only run (no failed retry step), making the ownership visible to
    /// the UI while avoiding a malformed retry plan.
    fn sync_registry(&mut self) {
        if self.recorded.is_empty() {
            return;
        }
        let Ok(mut runs) = self.registry.runs.lock() else {
            // Do not include paths, PIDs, or OS errors in the diagnostic. A
            // poisoned registry is unrecoverable for this process lifetime,
            // but the normal path still retains ownership in this guard.
            eprintln!("workbench: failed cleanup ownership registry unavailable");
            return;
        };

        if let Some(run_id) = self.existing_run_id.as_deref() {
            if let Some(run) = runs
                .get_mut(run_id)
                .filter(|run| run.profile_id == self.profile_id)
            {
                sync_guard_processes(run, &self.recorded, &self.pids);
            }
            return;
        }

        let run_id = self
            .published_run_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone();
        if self.pids.is_empty() {
            // A synthetic stop-only run is no longer needed once every child
            // recorded by this guard has been safely cleaned up.
            runs.remove(&run_id);
            self.published_run_id = None;
            return;
        }

        if let Some(run) = runs
            .get_mut(&run_id)
            .filter(|run| run.profile_id == self.profile_id)
        {
            sync_guard_processes(run, &self.recorded, &self.pids);
        } else if runs.is_empty() {
            let resource_provenance = self
                .pids
                .iter()
                .map(|process| ResourceProvenance {
                    kind: "process".into(),
                    id: process.app_id.into(),
                    state: ResourceState::WorkbenchStarted,
                })
                .collect();
            runs.insert(
                run_id,
                WorkspaceRun {
                    run_id: self
                        .published_run_id
                        .as_deref()
                        .unwrap_or_default()
                        .to_string(),
                    profile_id: self.profile_id.clone(),
                    steps: Vec::new(),
                    started_pids: self.pids.clone(),
                    resource_provenance,
                    retry_count: 0,
                    can_retry: false,
                    failed_step: None,
                },
            );
        } else {
            // A concurrent run should be impossible under the transition
            // claim. Keep the vector in this guard if that invariant is ever
            // violated rather than publishing ambiguous ownership.
            self.published_run_id = None;
        }
    }
}

impl Drop for StartedPidGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.cleanup();
        }
    }
}

fn sync_guard_processes(
    run: &mut WorkspaceRun,
    recorded: &[StartedProcess],
    remaining: &[StartedProcess],
) {
    run.started_pids
        .retain(|process| !recorded.contains(process) || remaining.contains(process));
    for process in remaining {
        if !run.started_pids.contains(process) {
            run.started_pids.push(*process);
        }
    }
    let app_ids = recorded
        .iter()
        .map(|process| process.app_id)
        .collect::<HashSet<_>>();
    for app_id in app_ids {
        let owned = run
            .started_pids
            .iter()
            .any(|process| process.app_id == app_id);
        if owned {
            append_process_resource(&mut run.resource_provenance, app_id);
        } else {
            run.resource_provenance.retain(|resource| {
                !(resource.kind == "process"
                    && resource.id == app_id
                    && resource.state == ResourceState::WorkbenchStarted)
            });
        }
    }
}

impl From<&WorkspaceRun> for WorkspaceRunOwnership {
    fn from(run: &WorkspaceRun) -> Self {
        Self {
            run_id: run.run_id.clone(),
            profile_id: run.profile_id.clone(),
            retry_count: run.retry_count,
            can_retry: run.can_retry,
            failed_step: run.failed_step.clone(),
        }
    }
}

/// 실행 기록 (인메모리). 앱 수명 동안 유지한다.
pub struct RunRegistry {
    pub runs: Mutex<HashMap<String, WorkspaceRun>>,
    starting_profile: Mutex<Option<String>>,
    starting_operation: Arc<SingleFlight>,
    pub(crate) health_operation: Arc<SingleFlight>,
    pub(crate) preview_operation: Arc<SingleFlight>,
}

struct WorkspaceTransitionClaim<'a> {
    slot: &'a Mutex<Option<String>>,
}

impl Drop for WorkspaceTransitionClaim<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = None;
        }
    }
}

fn claim_workspace_transition<'a>(
    slot: &'a Mutex<Option<String>>,
    profile_id: &str,
) -> Result<WorkspaceTransitionClaim<'a>, &'static str> {
    let mut current = slot
        .lock()
        .map_err(|_| "Workspace 작업 상태를 확인할 수 없습니다")?;
    if current.is_some() {
        return Err("다른 Workspace 작업이 이미 진행 중입니다");
    }
    *current = Some(profile_id.to_string());
    drop(current);
    Ok(WorkspaceTransitionClaim { slot })
}

fn has_active_profile_run(runs: &HashMap<String, WorkspaceRun>, profile_id: &str) -> bool {
    runs.values().any(|run| run.profile_id == profile_id)
}

#[cfg(test)]
fn take_profile_run(
    runs: &mut HashMap<String, WorkspaceRun>,
    run_id: &str,
    profile_id: &str,
) -> Result<Option<WorkspaceRun>, &'static str> {
    if runs
        .get(run_id)
        .is_some_and(|run| run.profile_id != profile_id)
    {
        return Err("선택한 프로필의 실행 기록이 아닙니다");
    }
    Ok(runs.remove(run_id))
}

fn single_workspace_run(
    runs: &HashMap<String, WorkspaceRun>,
) -> Result<Option<WorkspaceRunOwnership>, &'static str> {
    if runs.len() > 1 {
        return Err("여러 Workspace 실행 상태를 안전하게 복원할 수 없습니다");
    }
    Ok(runs.values().next().map(WorkspaceRunOwnership::from))
}

fn open_request(target: devbox_applink::OpenTarget) -> devbox_applink::OpenRequest {
    devbox_applink::OpenRequest {
        target,
        from: Some("workbench".to_string()),
    }
}

fn wsl_desktop_open_request(
    profile: &ProjectProfile,
) -> Result<devbox_applink::OpenRequest, String> {
    let path = profile
        .wsl
        .as_ref()
        .map(|wsl| wsl.path.as_str())
        .filter(|path| !path.trim().is_empty())
        .or_else(|| {
            profile
                .windows_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
        })
        .ok_or_else(|| "WSL Desktop에서 열 프로젝트 경로가 없습니다".to_string())?;

    Ok(open_request(devbox_applink::OpenTarget::Path {
        path: path.to_string(),
        line: None,
        column: None,
    }))
}

fn code_pad_open_request(profile: &ProjectProfile) -> Result<devbox_applink::OpenRequest, String> {
    let path = profile
        .windows_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| "Code Pad에서 열 Windows 프로젝트 경로가 없습니다".to_string())?;

    Ok(open_request(devbox_applink::OpenTarget::Workspace {
        path: path.to_string(),
    }))
}

fn launch_open_with_profile_environment(
    app_id: &str,
    request: &devbox_applink::OpenRequest,
    environment: Option<&EnvironmentInjection>,
) -> Result<u32, String> {
    // The boundary is applied even when the profile has no enabled `.env` so
    // callers use one launch path. As with an ordinary desktop launch, the
    // child inherits unrelated host variables; this slice only adds the
    // reviewed project overlay and never serializes it.
    let pairs = environment
        .map(EnvironmentInjection::pairs)
        .unwrap_or_default();
    devbox_launch::launch_open_with_environment(app_id, request, &pairs)
}

const PROFILE_CHANGED_ERROR: &str =
    "프로필이 변경되어 Workspace 시작을 중단했습니다. 다시 시도하세요";

fn operation_message(error: OperationError) -> String {
    error.message().to_string()
}

fn retry_step_snapshots(steps: &[RunStep]) -> Vec<RetryStep<'_>> {
    steps
        .iter()
        .map(|step| RetryStep {
            name: step.name.as_str(),
            ok: step.ok,
        })
        .collect()
}

fn retry_metadata(steps: &[RunStep], resources: &[ResourceProvenance]) -> (bool, Option<String>) {
    let snapshots = retry_step_snapshots(steps);
    let Ok(plan) = plan_retry(&snapshots, resources) else {
        return (false, None);
    };
    (
        can_retry(&snapshots, resources) && !plan.pending_steps.is_empty(),
        failed_step(&snapshots).map(str::to_owned),
    )
}

fn set_run_step(steps: &mut Vec<RunStep>, next: RunStep) {
    if let Some(existing) = steps.iter_mut().find(|step| step.name == next.name) {
        *existing = next;
    } else {
        steps.push(next);
    }
}

fn has_successful_step(steps: &[RunStep], name: &str) -> bool {
    steps.iter().any(|step| step.name == name && step.ok)
}

fn has_owned_process(resources: &[ResourceProvenance], app_id: &str) -> bool {
    resources.iter().any(|resource| {
        resource.kind == "process"
            && resource.id == app_id
            && resource.state == ResourceState::WorkbenchStarted
    })
}

fn has_process_resource(resources: &[ResourceProvenance], app_id: &str) -> bool {
    resources
        .iter()
        .any(|resource| resource.kind == "process" && resource.id == app_id)
}

fn append_process_resource(resources: &mut Vec<ResourceProvenance>, app_id: &str) {
    if !has_process_resource(resources, app_id) {
        resources.push(ResourceProvenance {
            kind: "process".into(),
            id: app_id.into(),
            state: ResourceState::WorkbenchStarted,
        });
    }
}

fn merge_resource_provenance(
    observed: impl IntoIterator<Item = ResourceProvenance>,
    existing: &[ResourceProvenance],
) -> Vec<ResourceProvenance> {
    let mut merged = Vec::new();
    let mut keys = HashSet::new();
    for resource in observed {
        let key = format!("{}\0{}", resource.kind, resource.id);
        if keys.insert(key) {
            merged.push(resource);
        }
    }
    for resource in existing {
        let key = format!("{}\0{}", resource.kind, resource.id);
        if keys.insert(key) {
            merged.push(resource.clone());
        }
    }
    merged
}

async fn revalidate_start_profile(
    app: &AppHandle,
    expected: &ProjectProfile,
    token: &OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<ProjectProfile, String> {
    let document = load_store_document_async(app, token.clone(), budget, claim).await?;
    let current = document
        .store
        .profiles
        .into_iter()
        .find(|profile| profile.id == expected.id)
        .ok_or_else(|| PROFILE_CHANGED_ERROR.to_string())?;
    if &current != expected {
        return Err(PROFILE_CHANGED_ERROR.to_string());
    }
    Ok(current)
}

/// Explicit cancellation for the long-running Start Workspace transition.
/// The command sets the same sticky bit observed by Git, WSL and port waits;
/// it does not merely dismiss a frontend spinner.
#[tauri::command]
pub fn cancel_start_workspace(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
) -> Result<bool, String> {
    validate_profile_id(&profile_id)?;
    registry
        .starting_operation
        .cancel(&profile_id)
        .map_err(str::to_string)
}

async fn claim_workspace_health_operation(
    registry: &RunRegistry,
    token: OperationToken,
    budget: OperationBudget,
) -> Result<OperationClaim, String> {
    // A profile navigation health request must not continue consuming native
    // Git/WSL capacity while a Workspace transition owns the lane.
    registry
        .health_operation
        .cancel_active_except("workspace-start")
        .map_err(str::to_string)?;
    loop {
        budget.check(&token).map_err(operation_message)?;
        match registry
            .health_operation
            .claim_reject_with_token("workspace-start", token.clone())
        {
            Ok(claim) => return Ok(claim),
            Err("다른 작업이 이미 진행 중입니다") => {
                // A health request can race the initial cancel/wait window.
                // Cancel any newly active read-only request and retry the
                // claim rather than returning a false collision to Start.
                // `workspace-start` is protected and therefore remains a
                // bounded wait/error instead of being cancelled by a second
                // transition.
                registry
                    .health_operation
                    .cancel_active_except("workspace-start")
                    .map_err(str::to_string)?;
                registry
                    .health_operation
                    .wait_until_idle(token.clone(), budget)
                    .await?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

async fn wait_for_expected_ports(
    profile: &ProjectProfile,
    token: &OperationToken,
    budget: OperationBudget,
) -> Result<RunStep, String> {
    let port_deadline = Instant::now()
        .checked_add(PORT_WAIT_TIMEOUT)
        .unwrap_or_else(Instant::now);
    let mut closed_ports = Vec::new();
    for port in &profile.expected_ports {
        budget.check(token).map_err(operation_message)?;
        let mut ready = Instant::now() < port_deadline
            && port_open_with_control(*port, token, budget).map_err(operation_message)?;
        while !ready {
            budget.check(token).map_err(operation_message)?;
            let remaining = port_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            delay_with_control(remaining.min(PORT_RETRY_INTERVAL), token.clone(), budget)
                .await
                .map_err(operation_message)?;
            ready = Instant::now() < port_deadline
                && port_open_with_control(*port, token, budget).map_err(operation_message)?;
        }
        budget.check(token).map_err(operation_message)?;
        if !ready {
            closed_ports.push(*port);
        }
    }
    Ok(RunStep {
        name: WAIT_PORT_STEP.into(),
        ok: closed_ports.is_empty(),
        detail: if closed_ports.is_empty() {
            if profile.expected_ports.is_empty() {
                "확인할 예상 port가 없습니다".into()
            } else {
                "예상 TCP port를 사용할 수 있습니다".into()
            }
        } else {
            format!("예상 TCP port {}개가 닫혀 있습니다", closed_ports.len())
        },
        status: if closed_ports.is_empty() {
            PreflightStatus::Pass
        } else {
            PreflightStatus::Failure
        },
    })
}

#[tauri::command]
pub async fn start_workspace(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
) -> Result<WorkspaceRun, String> {
    validate_profile_id(&profile_id)?;
    let _start_claim = claim_workspace_transition(&registry.starting_profile, &profile_id)
        .map_err(str::to_string)?;
    let _operation_claim = registry
        .starting_operation
        .claim_reject(profile_id.clone())
        .map_err(str::to_string)?;
    let token = _operation_claim.token();
    let budget = OperationBudget::from_now(WORKSPACE_START_TIMEOUT);
    let _health_claim = claim_workspace_health_operation(&registry, token.clone(), budget).await?;
    budget.check(&token).map_err(operation_message)?;
    if !registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?
        .is_empty()
    {
        return Err("현재 Workspace 실행을 먼저 중지하세요".to_string());
    }
    let document = load_store_document_async(&app, token.clone(), budget, &_health_claim).await?;
    let store = document.store;
    let profile = store
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;
    // The review shown by the UI is read-only and not a reservation. Repeat
    // the exact probes here before resolving `.env` or spawning either child;
    // a changed app/distro/path/port/service state must fail closed without a
    // partial Workspace launch.
    let preflight: WorkspacePreflight = crate::commands::preflight::preflight_profile(
        &profile,
        token.clone(),
        budget,
        &_health_claim,
    )
    .await?;
    if !preflight.ready {
        return Err("Workspace 사전 점검을 통과하지 못했습니다".to_string());
    }
    let mut steps = preflight
        .items
        .iter()
        .map(|item| RunStep {
            name: item.key.clone(),
            ok: item.status.is_non_blocking(),
            detail: item.detail.clone(),
            status: item.status,
        })
        .collect::<Vec<_>>();

    // 예상 포트 대기. 여러 포트가 있어도 하나의 bounded deadline만 쓴다.
    let port_step = wait_for_expected_ports(&profile, &token, budget).await?;
    set_run_step(&mut steps, port_step);

    // Resolve and revalidate as close as possible to the child boundary. The
    // health/port waits above can take seconds, so resolving before them
    // would allow a changed `.env` to become stale while we wait.
    //
    // A changed file or unavailable secret therefore fails before either
    // child is launched and cannot result in a partially injected workspace.
    let mut started_pids = StartedPidGuard::new(&registry, profile_id.clone(), None);
    let mut current_profile =
        revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await?;
    let mut environment =
        match crate::commands::environment::resolve_profile_environment_async_with_control(
            current_profile.clone(),
            token.clone(),
            budget,
            &_health_claim,
        )
        .await
        {
            Ok(environment) => environment,
            Err(error) => {
                started_pids.rollback();
                return Err(error);
            }
        };
    // The resolver is a blocking boundary and may give an external profile
    // writer time to finish. Revalidate again after it returns so the first
    // child is not launched from a profile that changed while its overlay was
    // being prepared.
    current_profile =
        match revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await {
            Ok(profile) => profile,
            Err(error) => {
                started_pids.rollback();
                return Err(error);
            }
        };

    // 앱 열기 (best-effort). Workbench가 시작한 것만 기록한다. Preflight
    // provenance is copied as stable metadata; executable paths/PIDs never
    // cross the UI boundary.
    let mut resource_provenance = preflight.resources().cloned().collect::<Vec<_>>();
    budget.check(&token).map_err(operation_message)?;
    match wsl_desktop_open_request(&current_profile) {
        Ok(request) => match launch_open_with_profile_environment(
            "wsl-desktop",
            &request,
            environment.as_ref(),
        ) {
            Ok(pid) => {
                started_pids.push("wsl-desktop", pid);
                append_process_resource(&mut resource_provenance, "wsl-desktop");
                set_run_step(
                    &mut steps,
                    RunStep {
                        name: OPEN_WSL_STEP.into(),
                        ok: true,
                        detail: "wsl-desktop을 시작했습니다".into(),
                        status: PreflightStatus::Pass,
                    },
                );
                if let Err(error) = budget.check(&token) {
                    started_pids.rollback();
                    return Err(error.message().to_string());
                }
            }
            Err(_) => set_run_step(
                &mut steps,
                RunStep {
                    name: OPEN_WSL_STEP.into(),
                    ok: false,
                    detail: "wsl-desktop을 시작할 수 없습니다".into(),
                    status: PreflightStatus::Failure,
                },
            ),
        },
        Err(_) => set_run_step(
            &mut steps,
            RunStep {
                name: OPEN_WSL_STEP.into(),
                ok: false,
                detail: "wsl-desktop 경로를 준비할 수 없습니다".into(),
                status: PreflightStatus::Failure,
            },
        ),
    }
    let mut current_profile =
        match revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await {
            Ok(profile) => profile,
            Err(error) => {
                started_pids.rollback();
                return Err(error);
            }
        };
    // The first child may have taken long enough for `.env` to change. Re-read
    // the source and compare its immutable revision immediately before the
    // second spawn; a preview or an earlier injection must never authorize a
    // later child after its source has changed.
    environment =
        match crate::commands::environment::resolve_profile_environment_async_with_control(
            current_profile.clone(),
            token.clone(),
            budget,
            &_health_claim,
        )
        .await
        {
            Ok(environment) => environment,
            Err(error) => {
                started_pids.rollback();
                return Err(error);
            }
        };
    current_profile =
        match revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await {
            Ok(profile) => profile,
            Err(error) => {
                started_pids.rollback();
                return Err(error);
            }
        };
    budget.check(&token).map_err(|error| {
        started_pids.rollback();
        error.message().to_string()
    })?;
    match code_pad_open_request(&current_profile) {
        Ok(request) => {
            match launch_open_with_profile_environment("code-pad", &request, environment.as_ref()) {
                Ok(pid) => {
                    started_pids.push("code-pad", pid);
                    append_process_resource(&mut resource_provenance, "code-pad");
                    set_run_step(
                        &mut steps,
                        RunStep {
                            name: OPEN_CODE_PAD_STEP.into(),
                            ok: true,
                            detail: "code-pad를 시작했습니다".into(),
                            status: PreflightStatus::Pass,
                        },
                    );
                    if let Err(error) = budget.check(&token) {
                        started_pids.rollback();
                        return Err(error.message().to_string());
                    }
                }
                Err(_) => set_run_step(
                    &mut steps,
                    RunStep {
                        name: OPEN_CODE_PAD_STEP.into(),
                        ok: false,
                        detail: "code-pad를 시작할 수 없습니다".into(),
                        status: PreflightStatus::Failure,
                    },
                ),
            }
        }
        Err(_) => set_run_step(
            &mut steps,
            RunStep {
                name: OPEN_CODE_PAD_STEP.into(),
                ok: false,
                detail: "code-pad 경로를 준비할 수 없습니다".into(),
                status: PreflightStatus::Failure,
            },
        ),
    }
    // No later transition step needs the resolved values. Drop the holder as
    // soon as the second spawn boundary has returned so zeroizing values do
    // not remain live through registry bookkeeping or the final revalidation.
    drop(environment);

    if let Err(error) =
        revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await
    {
        started_pids.rollback();
        return Err(error);
    }
    if let Err(error) = budget.check(&token) {
        started_pids.rollback();
        return Err(error.message().to_string());
    }
    let mut runs = match registry.runs.lock() {
        Ok(runs) => runs,
        Err(_) => {
            started_pids.rollback();
            return Err("실행 상태를 저장할 수 없습니다".to_string());
        }
    };
    if let Err(error) = budget.check(&token) {
        drop(runs);
        started_pids.rollback();
        return Err(error.message().to_string());
    }
    if !runs.is_empty() {
        drop(runs);
        started_pids.rollback();
        return Err("Workspace 실행 상태가 변경되어 결과를 저장할 수 없습니다".to_string());
    }
    let run = WorkspaceRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        profile_id,
        steps,
        resource_provenance,
        retry_count: 0,
        can_retry: false,
        failed_step: None,
        started_pids: Vec::new(),
    };
    let mut run = run;
    let (can_retry, failed_step) = retry_metadata(&run.steps, &run.resource_provenance);
    run.can_retry = can_retry;
    run.failed_step = failed_step;
    run.started_pids = started_pids.commit();
    runs.insert(run.run_id.clone(), run.clone());
    Ok(run)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildLaunchOutcome {
    Started(u32),
    Failed,
}

/// Revalidate the profile and environment immediately before one child
/// boundary.  Launch failures become a stable step result; cancellation,
/// timeout, stale profile, and provider failures remain transition errors so
/// the caller can roll back only the PIDs created by this attempt.
async fn launch_workspace_child(
    app: &AppHandle,
    expected: &ProjectProfile,
    app_id: &str,
    token: &OperationToken,
    budget: OperationBudget,
    claim: &OperationClaim,
) -> Result<ChildLaunchOutcome, String> {
    let current_profile = revalidate_start_profile(app, expected, token, budget, claim).await?;
    let environment = crate::commands::environment::resolve_profile_environment_async_with_control(
        current_profile.clone(),
        token.clone(),
        budget,
        claim,
    )
    .await?;
    let current_profile = revalidate_start_profile(app, expected, token, budget, claim).await?;
    budget.check(token).map_err(operation_message)?;
    let request = match app_id {
        "wsl-desktop" => wsl_desktop_open_request(&current_profile),
        "code-pad" => code_pad_open_request(&current_profile),
        _ => return Ok(ChildLaunchOutcome::Failed),
    };
    let Ok(request) = request else {
        return Ok(ChildLaunchOutcome::Failed);
    };
    let outcome = match launch_open_with_profile_environment(app_id, &request, environment.as_ref())
    {
        Ok(pid) => ChildLaunchOutcome::Started(pid),
        Err(_) => ChildLaunchOutcome::Failed,
    };
    drop(environment);
    // Do not check the budget after spawning before returning the PID: the
    // caller must first put a successful child into its StartedPidGuard so an
    // immediately-expired budget can still roll that child back.
    Ok(outcome)
}

fn process_step_name(app_id: &str) -> Option<&'static str> {
    match app_id {
        "wsl-desktop" => Some(OPEN_WSL_STEP),
        "code-pad" => Some(OPEN_CODE_PAD_STEP),
        _ => None,
    }
}

fn process_step_detail(app_id: &str, outcome: ChildLaunchOutcome) -> &'static str {
    match (app_id, outcome) {
        ("wsl-desktop", ChildLaunchOutcome::Started(_)) => "wsl-desktop을 시작했습니다",
        ("wsl-desktop", ChildLaunchOutcome::Failed) => "wsl-desktop을 시작할 수 없습니다",
        ("code-pad", ChildLaunchOutcome::Started(_)) => "code-pad를 시작했습니다",
        ("code-pad", ChildLaunchOutcome::Failed) => "code-pad를 시작할 수 없습니다",
        _ => "Workspace 앱을 시작할 수 없습니다",
    }
}

fn process_step(app_id: &str, outcome: ChildLaunchOutcome) -> Option<RunStep> {
    let name = process_step_name(app_id)?;
    let started = matches!(outcome, ChildLaunchOutcome::Started(_));
    Some(RunStep {
        name: name.into(),
        ok: started,
        detail: process_step_detail(app_id, outcome).into(),
        status: if started {
            PreflightStatus::Pass
        } else {
            PreflightStatus::Failure
        },
    })
}

/// Retry only the failed suffix of a Workspace run.  The existing run stays
/// authoritative until the new attempt has finished; newly spawned PIDs are
/// held by a separate guard and are rolled back if profile/revision/budget
/// validation fails.
#[tauri::command]
pub async fn retry_workspace(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    run_id: String,
    profile_id: String,
) -> Result<WorkspaceRun, String> {
    validate_profile_id(&profile_id)?;
    validate_profile_id(&run_id)?;
    let _start_claim = claim_workspace_transition(&registry.starting_profile, &profile_id)
        .map_err(str::to_string)?;
    let _operation_claim = registry
        .starting_operation
        .claim_reject(profile_id.clone())
        .map_err(str::to_string)?;
    let token = _operation_claim.token();
    let budget = OperationBudget::from_now(WORKSPACE_START_TIMEOUT);
    let _health_claim = claim_workspace_health_operation(&registry, token.clone(), budget).await?;

    let existing = {
        let runs = registry
            .runs
            .lock()
            .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?;
        let run = runs
            .get(&run_id)
            .ok_or_else(|| "Workspace 실행 기록을 찾을 수 없습니다".to_string())?;
        if run.profile_id != profile_id {
            return Err("선택한 프로필의 실행 기록이 아닙니다".into());
        }
        run.clone()
    };
    let snapshots = retry_step_snapshots(&existing.steps);
    let plan =
        plan_retry(&snapshots, &existing.resource_provenance).map_err(RetryPlanError::message)?;
    if plan.pending_steps.is_empty() {
        return Err("다시 시도할 실패 단계가 없습니다".into());
    }

    let document = load_store_document_async(&app, token.clone(), budget, &_health_claim).await?;
    let profile = document
        .store
        .profiles
        .iter()
        .find(|candidate| candidate.id == profile_id)
        .cloned()
        .ok_or_else(|| PROFILE_CHANGED_ERROR.to_string())?;
    let preflight = crate::commands::preflight::preflight_profile(
        &profile,
        token.clone(),
        budget,
        &_health_claim,
    )
    .await?;
    if !preflight.ready {
        return Err("Workspace 사전 점검을 통과하지 못했습니다".into());
    }

    let mut steps = existing.steps.clone();
    for item in &preflight.items {
        set_run_step(
            &mut steps,
            RunStep {
                name: item.key.clone(),
                ok: item.status.is_non_blocking(),
                detail: item.detail.clone(),
                status: item.status,
            },
        );
    }
    let mut resource_provenance = merge_resource_provenance(
        preflight.resources().cloned(),
        &existing.resource_provenance,
    );
    let mut new_pids = StartedPidGuard::new(&registry, profile_id.clone(), Some(run_id.clone()));

    for step in &plan.pending_steps {
        budget.check(&token).map_err(operation_message)?;
        match step.as_str() {
            WAIT_PORT_STEP => {
                let port_step = wait_for_expected_ports(&profile, &token, budget).await?;
                let ok = port_step.ok;
                set_run_step(&mut steps, port_step);
                if !ok {
                    // Waiting is the failed dependency boundary. Do not
                    // launch later children while the port contract remains
                    // unsatisfied; the same run can be retried again.
                    break;
                }
            }
            OPEN_WSL_STEP | OPEN_CODE_PAD_STEP => {
                let app_id = if step == OPEN_WSL_STEP {
                    "wsl-desktop"
                } else {
                    "code-pad"
                };
                if has_successful_step(&steps, step)
                    || has_owned_process(&resource_provenance, app_id)
                {
                    continue;
                }
                let outcome =
                    launch_workspace_child(&app, &profile, app_id, &token, budget, &_health_claim)
                        .await?;
                if let ChildLaunchOutcome::Started(pid) = outcome {
                    new_pids.push(app_id, pid);
                    append_process_resource(&mut resource_provenance, app_id);
                }
                if let Some(step_result) = process_step(app_id, outcome) {
                    set_run_step(&mut steps, step_result);
                }
            }
            _ => return Err(RetryPlanError::UnknownStep.message().into()),
        }
    }

    revalidate_start_profile(&app, &profile, &token, budget, &_health_claim).await?;
    budget.check(&token).map_err(operation_message)?;
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 저장할 수 없습니다".to_string())?;
    let current = runs
        .get(&run_id)
        .ok_or_else(|| "Workspace 실행 기록이 변경되어 결과를 저장할 수 없습니다".to_string())?;
    if current.profile_id != profile_id || current.retry_count != existing.retry_count {
        drop(runs);
        new_pids.rollback();
        return Err("Workspace 실행 상태가 변경되어 결과를 저장할 수 없습니다".into());
    }
    let (can_retry, failed_step) = retry_metadata(&steps, &resource_provenance);
    let mut updated = existing;
    updated.steps = steps;
    updated.resource_provenance = resource_provenance;
    updated.retry_count = updated.retry_count.saturating_add(1);
    updated.can_retry = can_retry;
    updated.failed_step = failed_step;
    let mut owned_pids = updated.started_pids;
    owned_pids.extend(new_pids.commit());
    updated.started_pids = owned_pids;
    runs.insert(run_id, updated.clone());
    Ok(updated)
}

fn terminate_started_process(process: &StartedProcess) -> bool {
    #[cfg(target_os = "windows")]
    {
        return terminate_started_process_windows(process);
    }

    #[cfg(not(target_os = "windows"))]
    match process.identity {
        #[cfg(unix)]
        ProcessIdentity::Unix(_) => match observe_process_identity(process) {
            ProcessObservation::Match => terminate_started_pid(process.pid),
            // The root may have exited while a child in its private process
            // group is still alive. The creation identity was captured before
            // this observation, so targeting that group is safe; a reused PID
            // would have produced Mismatch above and is retained instead.
            ProcessObservation::Missing => terminate_started_process_group(process),
            // Never issue a PID-only kill when the creation identity cannot be
            // proven. Retain the ownership record for a later safe attempt.
            ProcessObservation::Mismatch | ProcessObservation::Unavailable => false,
        },
        // If the process exited before its creation identity could be
        // captured, there is no safe authority to signal a later PID/group.
        // Forget the already-gone root, but do not risk killing an unrelated
        // process which reused that PID.
        ProcessIdentity::Gone => true,
        ProcessIdentity::Unavailable => false,
        #[cfg(windows)]
        ProcessIdentity::Windows(_) => false,
    }
}

#[cfg(unix)]
fn terminate_started_process_group(process: &StartedProcess) -> bool {
    // Workbench launches use `process_group(0)` in crates/launch, so the
    // recorded root PID is also the private group ID. `terminate_started_pid`
    // performs bounded TERM/KILL escalation and only reports success after the
    // group is gone.
    terminate_started_pid(process.pid)
}

#[cfg(target_os = "windows")]
struct HeldWindowsProcess(windows::Win32::Foundation::HANDLE);

#[cfg(target_os = "windows")]
impl Drop for HeldWindowsProcess {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Verify and hold the exact Windows process object before invoking the
/// process-tree helper. Holding the query handle prevents the verified PID's
/// process object from being destroyed/reused during the PID-based `/T` call,
/// closing the check-then-kill race as far as the OS helper permits.
#[cfg(target_os = "windows")]
fn terminate_started_process_windows(process: &StartedProcess) -> bool {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let ProcessIdentity::Windows(expected) = process.identity else {
        return matches!(process.identity, ProcessIdentity::Gone);
    };
    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process.pid) }
    {
        Ok(handle) => handle,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => {
            return true
        }
        Err(_) => return false,
    };
    let held = HeldWindowsProcess(handle);
    let mut creation = windows::Win32::Foundation::FILETIME::default();
    let mut exit = windows::Win32::Foundation::FILETIME::default();
    let mut kernel = windows::Win32::Foundation::FILETIME::default();
    let mut user = windows::Win32::Foundation::FILETIME::default();
    let Ok(()) =
        (unsafe { GetProcessTimes(held.0, &mut creation, &mut exit, &mut kernel, &mut user) })
    else {
        return false;
    };
    let observed = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if observed != expected {
        return false;
    }

    // Keep `held` alive until taskkill has returned. The helper still owns the
    // process-tree traversal (`/T`), while the handle prevents root PID reuse
    // between the identity check and that traversal.
    terminate_started_pid(process.pid)
}

fn capture_process_identity(pid: u32) -> ProcessIdentity {
    if pid == 0 {
        return ProcessIdentity::Unavailable;
    }

    #[cfg(target_os = "windows")]
    {
        use windows::core::HRESULT;
        use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER};
        use windows::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(handle) => handle,
            Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => {
                return ProcessIdentity::Gone
            }
            Err(_) => return ProcessIdentity::Unavailable,
        };
        let mut creation = windows::Win32::Foundation::FILETIME::default();
        let mut exit = windows::Win32::Foundation::FILETIME::default();
        let mut kernel = windows::Win32::Foundation::FILETIME::default();
        let mut user = windows::Win32::Foundation::FILETIME::default();
        let result =
            unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        unsafe {
            let _ = CloseHandle(handle);
        }
        return result
            .ok()
            .map(|_| {
                ProcessIdentity::Windows(
                    (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime),
                )
            })
            .unwrap_or(ProcessIdentity::Unavailable);
    }

    #[cfg(unix)]
    return match read_unix_process_start_ticks(pid) {
        Ok(start_ticks) => ProcessIdentity::Unix(start_ticks),
        Err(ProcessObservation::Missing) => ProcessIdentity::Gone,
        Err(_) => ProcessIdentity::Unavailable,
    };

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = pid;
        ProcessIdentity::Unavailable
    }
}

fn observe_process_identity(process: &StartedProcess) -> ProcessObservation {
    match process.identity {
        ProcessIdentity::Gone => ProcessObservation::Missing,
        ProcessIdentity::Unavailable => ProcessObservation::Unavailable,
        #[cfg(target_os = "windows")]
        ProcessIdentity::Windows(expected) => observe_windows_process(process.pid, expected),
        #[cfg(unix)]
        ProcessIdentity::Unix(expected) => match read_unix_process_start_ticks(process.pid) {
            Ok(observed) if observed == expected => ProcessObservation::Match,
            Ok(_) => ProcessObservation::Mismatch,
            Err(ProcessObservation::Missing) => ProcessObservation::Missing,
            Err(_) => ProcessObservation::Unavailable,
        },
    }
}

#[cfg(target_os = "windows")]
fn observe_windows_process(pid: u32, expected: u64) -> ProcessObservation {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => handle,
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => {
            return ProcessObservation::Missing
        }
        Err(_) => return ProcessObservation::Unavailable,
    };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    let Ok(()) = result else {
        return ProcessObservation::Unavailable;
    };
    let observed = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if observed == expected {
        ProcessObservation::Match
    } else {
        ProcessObservation::Mismatch
    }
}

#[cfg(unix)]
fn read_unix_process_start_ticks(pid: u32) -> Result<u64, ProcessObservation> {
    let path = format!("/proc/{pid}/stat");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(ProcessObservation::Missing)
        }
        Err(_) => return Err(ProcessObservation::Unavailable),
    };
    let mut text = String::new();
    let mut bounded = file.take((MAX_PROCESS_STAT_BYTES + 1) as u64);
    bounded
        .read_to_string(&mut text)
        .map_err(|_| ProcessObservation::Unavailable)?;
    if text.len() > MAX_PROCESS_STAT_BYTES {
        return Err(ProcessObservation::Unavailable);
    }
    // The comm field may contain spaces and parentheses, so locate its final
    // closing parenthesis before splitting the remaining fields. Field 22
    // (starttime) is the 20th token after `comm`.
    let Some(comm_end) = text.rfind(") ") else {
        return Err(ProcessObservation::Unavailable);
    };
    text[comm_end + 2..]
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(ProcessObservation::Unavailable)
}

#[cfg(not(unix))]
fn wait_for_termination(mut child: std::process::Child) -> bool {
    let deadline = Instant::now()
        .checked_add(PROCESS_TERMINATION_TIMEOUT)
        .unwrap_or_else(Instant::now);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                // Never leave the helper command itself running after the
                // bounded cleanup window. A timeout is reported as a failed
                // termination so the run keeps ownership for a later retry.
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Err(_) => return false,
        }
    }
}

/// Terminate one process tree and report whether the OS accepted the request.
/// The caller deliberately does not expose the command output: taskkill/kill
/// diagnostics can contain machine paths or account names.  A failed stop is
/// retained in the run registry so a transient access/exit race cannot turn a
/// still-owned process into an untracked orphan.
fn terminate_started_pid(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        // `/T` is required because the devbox app may have spawned a WebView2
        // or helper process; `/F` is restricted to this Workbench-owned PID
        // tree and is never used for an externally discovered process.
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        return command.spawn().is_ok_and(wait_for_termination);
    }

    #[cfg(unix)]
    {
        let Ok(process_group) = i32::try_from(pid) else {
            return false;
        };
        // launch::launch creates a private process group for Workbench-owned
        // apps. Signal the group, not just the root, and wait until every
        // member is gone. An app which ignores TERM receives one bounded KILL
        // escalation; failure to prove disappearance retains ownership.
        let term_result = unsafe { libc::kill(-process_group, libc::SIGTERM) };
        if term_result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                return true;
            }
            return false;
        }
        if wait_for_process_group(process_group, PROCESS_TERMINATION_TIMEOUT) {
            return true;
        }
        if unsafe { libc::kill(-process_group, libc::SIGKILL) } != 0 {
            return std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        }
        wait_for_process_group(process_group, Duration::from_millis(500))
    }

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let mut command = std::process::Command::new("kill");
        command
            .args(["-TERM", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command.spawn().is_ok_and(wait_for_termination)
    }
}

#[cfg(unix)]
fn wait_for_process_group(process_group: i32, timeout: Duration) -> bool {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    loop {
        let result = unsafe { libc::kill(-process_group, 0) };
        if result == 0 {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => true,
            // EPERM means a process in the group still exists but cannot be
            // inspected. Keep ownership rather than claiming cleanup.
            Some(libc::EPERM) => {
                if Instant::now() >= deadline {
                    false
                } else {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
            }
            _ => false,
        };
    }
}

/// Workbench가 시작한 것만 정리한다 (이미 실행 중이던 자원은 건드리지 않는다).
#[tauri::command]
pub fn stop_workspace(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    run_id: String,
    profile_id: String,
) -> Result<usize, String> {
    validate_profile_id(&profile_id)?;
    validate_profile_id(&run_id)?;
    let _transition_claim = claim_workspace_transition(&registry.starting_profile, &profile_id)
        .map_err(str::to_string)?;
    let mut run = {
        let runs = registry
            .runs
            .lock()
            .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?;
        let Some(current) = runs.get(&run_id) else {
            return Ok(0);
        };
        if current.profile_id != profile_id {
            return Err("선택한 프로필의 실행 기록이 아닙니다".into());
        }
        current.clone()
    };
    let mut remaining_pids = Vec::new();
    let mut stopped = 0;
    for process in std::mem::take(&mut run.started_pids) {
        if terminate_started_process(&process) {
            stopped += 1;
        } else {
            remaining_pids.push(process);
        }
    }
    // Keep the authoritative run visible while the OS helper runs. Commit the
    // removal only after all recorded roots have been handled; a concurrent
    // current_workspace_run therefore never observes a transient "no run"
    // state during Stop. Failed/mismatched ownership remains actionable.
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 저장할 수 없습니다".to_string())?;
    let current = runs
        .get(&run_id)
        .ok_or_else(|| "Workspace 실행 상태가 변경되어 중지를 완료할 수 없습니다".to_string())?;
    if current.profile_id != profile_id {
        return Err("선택한 프로필의 실행 기록이 아닙니다".into());
    }
    if remaining_pids.is_empty() {
        runs.remove(&run_id);
    } else if let Some(current) = runs.get_mut(&run_id) {
        // Keep only ownership that still needs a subsequent stop attempt. We
        // intentionally retain the run when any OS termination was rejected;
        // removing it here would make Stop What I Started unable to recover
        // from a temporary process/access race.
        current.started_pids = remaining_pids;
    }
    Ok(stopped)
}

/// frontend reload 뒤에도 backend가 추적 중인 단일 run ownership을 복원한다.
/// start claim이 추가 run을 막으므로 둘 이상이면 손상 상태로 보고 fail-closed한다.
#[tauri::command]
pub fn current_workspace_run(
    registry: tauri::State<'_, Arc<RunRegistry>>,
) -> Result<Option<WorkspaceRunOwnership>, String> {
    let runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?;
    single_workspace_run(&runs).map_err(str::to_string)
}

pub fn run_registry() -> Arc<RunRegistry> {
    Arc::new(RunRegistry {
        runs: Mutex::new(HashMap::new()),
        starting_profile: Mutex::new(None),
        starting_operation: SingleFlight::new(),
        health_operation: SingleFlight::new(),
        preview_operation: SingleFlight::new(),
    })
}

/// Life Log의 versioned projects snapshot을 읽어 Workbench 프로필로 흡수한다.
///
/// snapshot 전체를 먼저 검증하고 임시 store에 반영한 뒤에만 caller의 store를 교체한다.
/// 파일 없음은 정상적인 no-op이고 손상/스키마 불일치는 기존 store를 그대로 둔 채 실패한다.
pub fn absorb_life_log_projects(store: &mut ProfileStore) -> Result<LifeLogAbsorbReport, String> {
    absorb_life_log_projects_in(store, &devbox_integration::integration_root())
}

fn absorb_life_log_projects_in(
    store: &mut ProfileStore,
    integration_root: &std::path::Path,
) -> Result<LifeLogAbsorbReport, String> {
    let Some((entries, freshness_ms)) = read_life_log_projects_in(integration_root)? else {
        return Ok(LifeLogAbsorbReport::default());
    };

    let mut profiles = Vec::new();
    let mut identities = HashSet::new();
    let mut unsupported_paths = 0;
    for entry in entries {
        validate_life_log_project_entry(&entry)?;
        let Some(profile) = profile_from_life_log_entry(entry)? else {
            unsupported_paths += 1;
            continue;
        };
        let identity = profile
            .canonical_key()
            .map_err(|_| "Life Log 프로젝트 identity가 올바르지 않습니다")?;
        if identities.insert(identity) {
            profiles.push(profile);
        }
    }

    let mut next = store.clone();
    let mut added = 0;
    for profile in profiles {
        if next
            .upsert(profile)
            .map_err(|_| "Life Log 프로젝트 프로필을 흡수할 수 없습니다")?
            .is_none()
        {
            added += 1;
        }
    }
    *store = next;
    Ok(LifeLogAbsorbReport {
        added,
        freshness_ms: Some(freshness_ms),
        unsupported_paths,
    })
}

fn read_life_log_projects_in(
    integration_root: &std::path::Path,
) -> Result<Option<(Vec<LifeLogProjectEntry>, u64)>, String> {
    let discovery = devbox_integration::discover_report_in(integration_root);
    if discovery.root_error.is_some() {
        return Err("integration snapshot root를 안전하게 읽을 수 없습니다".into());
    }
    let Some(reference) = discovery.snapshots.iter().find(|snapshot| {
        snapshot.producer == LIFE_LOG_PRODUCER && snapshot.version == LIFE_LOG_SNAPSHOT_VERSION
    }) else {
        if discovery.issues.iter().any(|issue| {
            issue.producer == LIFE_LOG_PRODUCER && issue.version == Some(LIFE_LOG_SNAPSHOT_VERSION)
        }) {
            return Err("Life Log snapshot을 안전하게 읽을 수 없습니다".into());
        }
        return Ok(None);
    };
    let view_reference = reference
        .views
        .iter()
        .find(|view| view.kind == LIFE_LOG_PROJECTS_VIEW)
        .ok_or_else(|| "Life Log projects view가 없습니다".to_string())?;
    if view_reference.schema_version != LIFE_LOG_PROJECTS_VIEW_VERSION {
        return Err("Life Log projects view schema version이 호환되지 않습니다".into());
    }
    if view_reference.entry_count > MAX_LIFE_LOG_PROJECTS {
        return Err("Life Log projects view 항목 수 제한을 초과했습니다".into());
    }

    let envelope = devbox_integration::read_snapshot_in(
        integration_root,
        LIFE_LOG_PRODUCER,
        LIFE_LOG_SNAPSHOT_VERSION,
    )?
    .ok_or_else(|| "Life Log snapshot이 읽는 동안 사라졌습니다".to_string())?;
    let mut views = envelope.views()?;
    let view = views
        .remove(LIFE_LOG_PROJECTS_VIEW)
        .ok_or_else(|| "Life Log projects view가 없습니다".to_string())?;
    if view.schema_version != LIFE_LOG_PROJECTS_VIEW_VERSION
        || view.entries.len() > MAX_LIFE_LOG_PROJECTS
    {
        return Err("Life Log projects view 계약이 올바르지 않습니다".into());
    }
    let entries = view
        .entries
        .into_iter()
        .map(|entry| {
            serde_json::from_value(entry)
                .map_err(|_| "Life Log projects entry 형식이 올바르지 않습니다".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some((entries, view_reference.freshness_ms)))
}

fn validate_life_log_project_entry(entry: &LifeLogProjectEntry) -> Result<(), String> {
    let activity_is_consistent = if entry.recent_session_count == 0 {
        entry.last_activity_at_ms.is_none() && entry.recent_duration_ms == 0
    } else {
        entry.last_activity_at_ms.is_some()
    };
    if entry.activity_window_start_ms < 0
        || entry.recent_duration_ms < 0
        || entry
            .last_activity_at_ms
            .is_some_and(|last| last < entry.activity_window_start_ms)
        || !activity_is_consistent
    {
        return Err("Life Log projects entry 값이 올바르지 않습니다".into());
    }
    Ok(())
}

fn profile_from_life_log_entry(
    entry: LifeLogProjectEntry,
) -> Result<Option<ProjectProfile>, String> {
    let path = parse_safe_project_path(&entry.path)
        .ok_or_else(|| "Life Log projects entry에 안전하지 않은 경로가 있습니다".to_string())?;
    let windows_path = match path.kind() {
        ProjectPathKind::WindowsDrive | ProjectPathKind::WindowsUnc => path.as_str().to_string(),
        ProjectPathKind::Posix => {
            if !path.as_str().starts_with("/mnt/") {
                // distro가 없는 POSIX path에 임의 distro를 붙이지 않는다.
                return Ok(None);
            }
            devbox_wsl::path::wsl_to_windows("", path.as_str())
                .map_err(|_| "Life Log projects entry의 WSL 경로가 올바르지 않습니다")?
        }
    };
    let mut profile = ProjectProfile::new(path.name());
    profile.windows_path = Some(windows_path.clone());
    profile.git_root = Some(windows_path);
    Ok(Some(profile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::WslProfile;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_SNAPSHOT_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn snapshot_root(label: &str) -> PathBuf {
        let sequence = NEXT_SNAPSHOT_ROOT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "workbench-life-log-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    fn project_entry(path: &str) -> serde_json::Value {
        serde_json::json!({
            "path": path,
            "activityWindowStartMs": 1_000,
            "lastActivityAtMs": 2_000,
            "recentSessionCount": 1,
            "recentDurationMs": 500
        })
    }

    fn write_life_log_snapshot(
        root: &Path,
        view_version: u32,
        freshness_ms: u64,
        entries: Vec<serde_json::Value>,
    ) {
        let mut views = devbox_integration::SnapshotViews::new();
        views.insert(
            LIFE_LOG_PROJECTS_VIEW.into(),
            devbox_integration::SnapshotView {
                schema_version: view_version,
                freshness_ms,
                entries,
            },
        );
        let envelope = devbox_integration::Envelope::with_views(LIFE_LOG_PRODUCER, "0.3.1", views);
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(
                root,
                LIFE_LOG_PRODUCER,
                LIFE_LOG_SNAPSHOT_VERSION,
            ),
        )
        .unwrap();
    }

    fn profile(windows_path: Option<&str>, wsl_path: Option<&str>) -> ProjectProfile {
        let mut profile = ProjectProfile::new("devbox");
        profile.windows_path = windows_path.map(str::to_string);
        profile.wsl = wsl_path.map(|path| WslProfile {
            distro: "Ubuntu".into(),
            path: path.into(),
        });
        profile
    }

    #[test]
    fn health_operation_keys_do_not_collide_on_printable_separators() {
        assert!(health_operation_key("project", None).starts_with("project-health\0"));
        assert_ne!(
            health_operation_key("project:one", Some("request")),
            health_operation_key("project", Some("one:request"))
        );
        assert_ne!(
            health_operation_key("project", None),
            health_operation_key("project", Some("request"))
        );
    }

    #[test]
    fn port_probe_honors_cancellation_before_connecting() {
        let token = OperationToken::new();
        token.cancel();
        let budget = OperationBudget::from_now(Duration::from_secs(1));
        assert_eq!(
            port_open_with_control(9, &token, budget),
            Err(OperationError::Cancelled)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn started_process_identity_matches_the_same_linux_process() {
        let pid = std::process::id();
        let process = StartedProcess::new("workbench-test", pid);
        assert!(matches!(process.identity, ProcessIdentity::Unix(_)));
        assert_eq!(
            observe_process_identity(&process),
            ProcessObservation::Match
        );

        let mismatched = StartedProcess {
            app_id: "workbench-test",
            pid,
            identity: ProcessIdentity::Unix(match process.identity {
                ProcessIdentity::Unix(start_ticks) => start_ticks.saturating_add(1),
                _ => unreachable!("the current Linux process must have a /proc identity"),
            }),
        };
        assert_eq!(
            observe_process_identity(&mismatched),
            ProcessObservation::Mismatch
        );
    }

    #[test]
    fn gone_process_identity_is_a_safe_cleanup_outcome() {
        let process = StartedProcess {
            app_id: "workbench-test",
            pid: u32::MAX,
            identity: ProcessIdentity::Gone,
        };
        assert_eq!(
            observe_process_identity(&process),
            ProcessObservation::Missing
        );
    }

    #[test]
    fn failed_transition_cleanup_publishes_stop_only_ownership() {
        let registry = run_registry();
        let mut guard = StartedPidGuard::new(&registry, "profile-1", None);
        let process = StartedProcess {
            app_id: "code-pad",
            pid: u32::MAX,
            identity: ProcessIdentity::Unavailable,
        };
        guard.pids.push(process);
        guard.recorded.push(process);

        guard.rollback();

        let runs = registry.runs.lock().unwrap();
        let run = runs.values().next().expect("failed cleanup is visible");
        assert_eq!(run.profile_id, "profile-1");
        assert!(!run.can_retry);
        assert_eq!(run.started_pids, vec![process]);
        assert!(run.resource_provenance.iter().any(|resource| {
            resource.kind == "process"
                && resource.id == "code-pad"
                && resource.state == ResourceState::WorkbenchStarted
        }));
    }

    #[test]
    fn store_document_rejects_stale_or_missing_bytes() {
        let document = ProfileStoreDocument {
            store: ProfileStore::empty(),
            raw: Some(b"old".to_vec()),
        };
        assert!(document_is_current(&document, Some(b"old")));
        assert!(!document_is_current(&document, Some(b"new")));
        assert!(!document_is_current(&document, None));

        let missing = ProfileStoreDocument {
            store: ProfileStore::empty(),
            raw: None,
        };
        assert!(document_is_current(&missing, None));
        assert!(!document_is_current(&missing, Some(b"new")));
    }

    #[test]
    fn run_manager_snapshot_data_is_complete_bounded_and_safe() {
        let valid = serde_json::json!({
            "activeServices": [
                { "id": "api", "uptimeMs": 1200 },
                { "id": "worker", "uptimeMs": 0 }
            ],
            "runs": { "success": 2, "failed": 0 },
            "lastRunAtMs": null
        });
        assert_eq!(
            active_service_ids(&valid).unwrap(),
            HashSet::from(["api".to_string(), "worker".to_string()])
        );

        for malformed in [
            serde_json::json!({}),
            serde_json::json!({ "activeServices": "TOP_SECRET" }),
            serde_json::json!({ "activeServices": [{ "id": "api" }] }),
            serde_json::json!({ "activeServices": [{ "id": " api", "uptimeMs": 0 }] }),
            serde_json::json!({ "activeServices": [{ "id": "api", "uptimeMs": -1 }] }),
            serde_json::json!({
                "activeServices": [
                    { "id": "api", "uptimeMs": 0 },
                    { "id": "api", "uptimeMs": 1 }
                ]
            }),
        ] {
            assert!(active_service_ids(&malformed).is_err());
        }

        let oversized = serde_json::json!({
            "activeServices": (0..=MAX_SERVICES)
                .map(|index| serde_json::json!({ "id": format!("service-{index}"), "uptimeMs": 0 }))
                .collect::<Vec<_>>()
        });
        assert!(active_service_ids(&oversized).is_err());
    }

    #[test]
    fn missing_and_corrupt_service_snapshots_have_distinct_health_states() {
        let configured = vec!["api".to_string()];
        let missing = service_health_item(&configured, Ok(HashSet::new()));
        assert!(!missing.ok);
        assert_eq!(missing.detail, "미실행 서비스 1개");

        let unavailable = service_health_item(&configured, Err(()));
        assert!(!unavailable.ok);
        assert_eq!(unavailable.detail, "서비스 상태를 확인할 수 없습니다");
    }

    fn workspace_run(run_id: &str, profile_id: &str) -> WorkspaceRun {
        WorkspaceRun {
            run_id: run_id.to_string(),
            profile_id: profile_id.to_string(),
            steps: Vec::new(),
            started_pids: vec![StartedProcess::new("workbench-test", 101)],
            resource_provenance: Vec::new(),
            retry_count: 0,
            can_retry: false,
            failed_step: None,
        }
    }

    #[test]
    fn run_ownership_gate_preserves_mismatched_runs_and_tracks_active_profile() {
        let mut runs = HashMap::from([("run-1".to_string(), workspace_run("run-1", "profile-1"))]);

        assert!(has_active_profile_run(&runs, "profile-1"));
        assert!(!has_active_profile_run(&runs, "profile-2"));
        assert!(matches!(
            take_profile_run(&mut runs, "run-1", "profile-2"),
            Err("선택한 프로필의 실행 기록이 아닙니다")
        ));
        assert!(runs.contains_key("run-1"));

        let taken = take_profile_run(&mut runs, "run-1", "profile-1")
            .unwrap()
            .unwrap();
        assert_eq!(taken.profile_id, "profile-1");
        assert!(runs.is_empty());
        assert!(take_profile_run(&mut runs, "missing", "profile-1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn transition_claim_and_current_run_restore_are_fail_closed() {
        let slot = Mutex::new(None);
        let claim = claim_workspace_transition(&slot, "profile-1").unwrap();
        assert!(matches!(
            claim_workspace_transition(&slot, "profile-2"),
            Err("다른 Workspace 작업이 이미 진행 중입니다")
        ));
        assert_eq!(slot.lock().unwrap().as_deref(), Some("profile-1"));
        drop(claim);
        assert!(slot.lock().unwrap().is_none());

        let mut sensitive_run = workspace_run("run-1", "profile-1");
        sensitive_run.steps.push(RunStep {
            name: "health".to_string(),
            ok: false,
            detail: r"C:\TOP_SECRET\project".to_string(),
            status: PreflightStatus::Failure,
        });
        let one = HashMap::from([("run-1".to_string(), sensitive_run)]);
        let ownership = single_workspace_run(&one).unwrap().unwrap();
        assert_eq!(ownership.run_id, "run-1");
        assert!(ownership.failed_step.is_none());
        let json = serde_json::to_string(&ownership).unwrap();
        assert!(!json.contains("TOP_SECRET"));
        assert!(!json.contains("startedPids"));
        assert!(!json.contains("steps"));

        let full_run_json = serde_json::to_string(&workspace_run("run-1", "profile-a")).unwrap();
        assert!(!full_run_json.contains("startedPids"));
        assert!(!full_run_json.contains("101"));

        let multiple = HashMap::from([
            ("run-1".to_string(), workspace_run("run-1", "profile-1")),
            ("run-2".to_string(), workspace_run("run-2", "profile-2")),
        ]);
        assert!(matches!(
            single_workspace_run(&multiple),
            Err("여러 Workspace 실행 상태를 안전하게 복원할 수 없습니다")
        ));
    }

    #[test]
    fn retry_metadata_exposes_only_known_failed_step_and_owned_processes() {
        let steps = vec![
            RunStep {
                name: "required-apps".into(),
                ok: true,
                detail: "필수 devbox 앱을 사용할 수 있습니다".into(),
                status: PreflightStatus::Pass,
            },
            RunStep {
                name: WAIT_PORT_STEP.into(),
                ok: true,
                detail: "예상 TCP port를 사용할 수 있습니다".into(),
                status: PreflightStatus::Pass,
            },
            RunStep {
                name: OPEN_WSL_STEP.into(),
                ok: true,
                detail: "wsl-desktop을 시작했습니다".into(),
                status: PreflightStatus::Pass,
            },
            RunStep {
                name: OPEN_CODE_PAD_STEP.into(),
                ok: false,
                detail: "code-pad를 시작할 수 없습니다".into(),
                status: PreflightStatus::Failure,
            },
        ];
        let resources = vec![ResourceProvenance {
            kind: "process".into(),
            id: "wsl-desktop".into(),
            state: ResourceState::WorkbenchStarted,
        }];
        let (can_retry, failed_step) = retry_metadata(&steps, &resources);
        assert!(can_retry);
        assert_eq!(failed_step.as_deref(), Some(OPEN_CODE_PAD_STEP));
    }

    #[test]
    fn merge_resource_provenance_keeps_existing_process_ownership_once() {
        let observed = vec![ResourceProvenance {
            kind: "app".into(),
            id: "code-pad:workspace".into(),
            state: ResourceState::Available,
        }];
        let existing = vec![
            ResourceProvenance {
                kind: "app".into(),
                id: "code-pad:workspace".into(),
                state: ResourceState::Available,
            },
            ResourceProvenance {
                kind: "process".into(),
                id: "wsl-desktop".into(),
                state: ResourceState::WorkbenchStarted,
            },
        ];
        let merged = merge_resource_provenance(observed, &existing);
        assert_eq!(merged.len(), 2);
        assert!(has_owned_process(&merged, "wsl-desktop"));

        let mut external = vec![ResourceProvenance {
            kind: "process".into(),
            id: "code-pad".into(),
            state: ResourceState::Existing,
        }];
        append_process_resource(&mut external, "code-pad");
        assert_eq!(external.len(), 1);
        assert!(!has_owned_process(&external, "code-pad"));
    }

    #[cfg(unix)]
    #[test]
    fn profile_store_reader_rejects_symlinked_file_at_handle_boundary() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "workbench-profile-reader-link-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let real = root.join("real.json");
        let link = root.join("project-profiles.json");
        std::fs::write(&real, br#"{"version":1,"profiles":[]}"#).unwrap();
        symlink(&real, &link).unwrap();

        assert!(read_profile_file(&link).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn launch_request_prefers_wsl_path_for_wsl_desktop() {
        let request = wsl_desktop_open_request(&profile(
            Some("E:\\projects\\devbox"),
            Some("/mnt/e/projects/devbox"),
        ))
        .unwrap();
        assert_eq!(
            request.target,
            devbox_applink::OpenTarget::Path {
                path: "/mnt/e/projects/devbox".into(),
                line: None,
                column: None,
            }
        );
    }

    #[test]
    fn launch_request_falls_back_to_windows_path_for_wsl_desktop() {
        let request =
            wsl_desktop_open_request(&profile(Some("E:\\projects\\devbox"), None)).unwrap();
        assert_eq!(
            request.target,
            devbox_applink::OpenTarget::Path {
                path: "E:\\projects\\devbox".into(),
                line: None,
                column: None,
            }
        );
    }

    #[test]
    fn launch_request_rejects_missing_or_empty_code_pad_workspace() {
        assert!(code_pad_open_request(&profile(None, Some("/home/me/project"))).is_err());
        assert!(code_pad_open_request(&profile(Some("   "), None)).is_err());
    }

    #[test]
    fn launch_request_uses_non_empty_windows_path_for_code_pad() {
        let request = code_pad_open_request(&profile(Some("E:\\projects\\devbox"), None)).unwrap();
        assert_eq!(
            request.target,
            devbox_applink::OpenTarget::Workspace {
                path: "E:\\projects\\devbox".into(),
            }
        );
    }

    #[test]
    fn absorbs_valid_versioned_snapshot_without_reading_life_log_database() {
        let root = snapshot_root("valid");
        write_life_log_snapshot(
            &root,
            LIFE_LOG_PROJECTS_VIEW_VERSION,
            250,
            vec![
                project_entry("C:\\work\\devbox"),
                project_entry("c:/work/devbox/"),
                project_entry("\\\\server\\share\\api"),
                project_entry("/mnt/e/projects/toolbox"),
                project_entry("/home/jihoon/distro-required"),
            ],
        );
        let mut store = ProfileStore::empty();

        let report = absorb_life_log_projects_in(&mut store, &root).unwrap();

        assert_eq!(report.added, 3);
        assert_eq!(report.unsupported_paths, 1);
        assert!(report
            .freshness_ms
            .is_some_and(|freshness| freshness >= 250));
        assert_eq!(store.profiles.len(), 3);
        assert!(store
            .profiles
            .iter()
            .any(|profile| profile.windows_path.as_deref() == Some("C:\\work\\devbox")));
        assert!(store
            .profiles
            .iter()
            .any(|profile| { profile.windows_path.as_deref() == Some("\\\\server\\share\\api") }));
        assert!(store.profiles.iter().any(|profile| {
            profile.windows_path.as_deref() == Some("E:\\projects\\toolbox")
                && profile.git_root.as_deref() == Some("E:\\projects\\toolbox")
        }));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_snapshot_is_a_no_op_and_preserves_existing_profiles() {
        let root = snapshot_root("missing");
        let mut store = ProfileStore::empty();
        store.upsert(profile(Some("C:\\existing"), None)).unwrap();
        let before = store.clone();

        let report = absorb_life_log_projects_in(&mut store, &root).unwrap();

        assert_eq!(report, LifeLogAbsorbReport::default());
        assert_eq!(store, before);
    }

    #[test]
    fn corrupt_snapshot_falls_back_without_mutating_existing_profiles_or_echoing_data() {
        let root = snapshot_root("corrupt");
        let directory = devbox_integration::snapshot_dir_in(
            &root,
            LIFE_LOG_PRODUCER,
            LIFE_LOG_SNAPSHOT_VERSION,
        );
        std::fs::create_dir_all(&directory).unwrap();
        let raw = "raw-credential-must-not-be-echoed";
        std::fs::write(
            directory.join("summary.json"),
            format!("{{credential: {raw}}}"),
        )
        .unwrap();
        let mut store = ProfileStore::empty();
        store.upsert(profile(Some("C:\\existing"), None)).unwrap();
        let before = store.clone();

        let error = absorb_life_log_projects_in(&mut store, &root).unwrap_err();

        assert!(!error.contains(raw));
        assert_eq!(store, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sensitive_snapshot_field_is_rejected_without_persistence_or_log_echo() {
        let root = snapshot_root("sensitive");
        let directory = devbox_integration::snapshot_dir_in(
            &root,
            LIFE_LOG_PRODUCER,
            LIFE_LOG_SNAPSHOT_VERSION,
        );
        std::fs::create_dir_all(&directory).unwrap();
        let raw = "raw-secret-must-not-be-echoed";
        let envelope = serde_json::json!({
            "schemaVersion": 1,
            "producer": LIFE_LOG_PRODUCER,
            "producerVersion": "0.3.1",
            "generatedAt": "2026-08-25T12:00:00Z",
            "data": {
                "views": {
                    (LIFE_LOG_PROJECTS_VIEW): {
                        "schemaVersion": LIFE_LOG_PROJECTS_VIEW_VERSION,
                        "freshnessMs": 0,
                        "entries": [{
                            "path": "C:\\work\\devbox",
                            "activityWindowStartMs": 1_000,
                            "lastActivityAtMs": null,
                            "recentSessionCount": 0,
                            "recentDurationMs": 0,
                            "credential": raw
                        }]
                    }
                }
            }
        });
        std::fs::write(
            directory.join("summary.json"),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();
        let mut store = ProfileStore::empty();
        store.upsert(profile(Some("C:\\existing"), None)).unwrap();
        let before = store.clone();

        let error = absorb_life_log_projects_in(&mut store, &root).unwrap_err();

        assert!(!error.contains(raw));
        assert_eq!(store, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn schema_mismatch_fails_closed_without_mutating_store() {
        let root = snapshot_root("schema");
        write_life_log_snapshot(
            &root,
            LIFE_LOG_PROJECTS_VIEW_VERSION + 1,
            0,
            vec![project_entry("C:\\work\\new")],
        );
        let mut store = ProfileStore::empty();
        store.upsert(profile(Some("C:\\existing"), None)).unwrap();
        let before = store.clone();

        let error = absorb_life_log_projects_in(&mut store, &root).unwrap_err();

        assert!(error.contains("schema version"));
        assert_eq!(store, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unsafe_entry_rejects_the_complete_snapshot() {
        let root = snapshot_root("unsafe");
        write_life_log_snapshot(
            &root,
            LIFE_LOG_PROJECTS_VIEW_VERSION,
            0,
            vec![
                project_entry("C:\\work\\would-have-been-added"),
                project_entry("relative/../../escape"),
            ],
        );
        let mut store = ProfileStore::empty();
        store.upsert(profile(Some("C:\\existing"), None)).unwrap();
        let before = store.clone();

        assert!(absorb_life_log_projects_in(&mut store, &root).is_err());
        assert_eq!(store, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn inconsistent_activity_summary_rejects_the_complete_snapshot() {
        let root = snapshot_root("inconsistent");
        let mut inconsistent = project_entry("C:\\work\\invalid");
        inconsistent["recentSessionCount"] = serde_json::json!(0);
        write_life_log_snapshot(
            &root,
            LIFE_LOG_PROJECTS_VIEW_VERSION,
            0,
            vec![project_entry("C:\\work\\valid"), inconsistent],
        );
        let mut store = ProfileStore::empty();
        let before = store.clone();

        let error = absorb_life_log_projects_in(&mut store, &root).unwrap_err();

        assert!(error.contains("entry 값"));
        assert_eq!(store, before);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn entry_limit_is_enforced_before_deserializing_payloads() {
        let root = snapshot_root("limit");
        write_life_log_snapshot(
            &root,
            LIFE_LOG_PROJECTS_VIEW_VERSION,
            0,
            (0..=MAX_LIFE_LOG_PROJECTS)
                .map(|index| project_entry(&format!("C:\\work\\project-{index}")))
                .collect(),
        );
        let mut store = ProfileStore::empty();

        let error = absorb_life_log_projects_in(&mut store, &root).unwrap_err();

        assert!(error.contains("항목 수 제한"));
        assert!(store.profiles.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_safe_entry_metadata_is_forward_compatible() {
        let root = snapshot_root("metadata");
        let mut entry = project_entry("C:\\work\\devbox");
        entry["futureMetadata"] = serde_json::json!({ "label": "safe" });
        write_life_log_snapshot(&root, LIFE_LOG_PROJECTS_VIEW_VERSION, 0, vec![entry]);
        let mut store = ProfileStore::empty();

        let report = absorb_life_log_projects_in(&mut store, &root).unwrap();

        assert_eq!(report.added, 1);
        assert_eq!(store.profiles.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }
}
