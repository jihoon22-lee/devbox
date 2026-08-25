//! Workbench command — 프로필 CRUD, health, Start/Stop Workspace.

use crate::core::health::{has_distro, parse_git_status};
use crate::core::profile::{ProfileStore, ProjectProfile};
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

const PROFILE_FILE: &str = "project-profiles.json";
const LIFE_LOG_PRODUCER: &str = "life-log";
const LIFE_LOG_SNAPSHOT_VERSION: u32 = 1;
const LIFE_LOG_PROJECTS_VIEW: &str = "projects";
const LIFE_LOG_PROJECTS_VIEW_VERSION: u32 = 1;
const MAX_LIFE_LOG_PROJECTS: usize = 512;

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
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(PROFILE_FILE))
}

pub(crate) fn load_store(app: &AppHandle) -> ProfileStore {
    profile_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| ProfileStore::load(&t))
        .unwrap_or_default()
}

pub(crate) fn save_store(app: &AppHandle, store: &ProfileStore) -> Result<(), String> {
    let path = profile_path(app)?;
    let json = store.to_json().map_err(|e| e.to_string())?;
    devbox_filesystem::atomic_write(path, json.as_bytes())
        .map_err(|_| "프로필을 원자적으로 저장할 수 없습니다".to_string())
}

#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Vec<ProjectProfile> {
    load_store(&app).profiles
}

#[tauri::command]
pub fn create_profile(
    app: AppHandle,
    mut profile: ProjectProfile,
) -> Result<ProjectProfile, String> {
    let mut store = load_store(&app);
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    let dup = store.upsert(profile)?;
    save_store(&app, &store)?;
    Ok(dup.unwrap_or_else(|| store.profiles.last().cloned().expect("just pushed")))
}

#[tauri::command]
pub fn update_profile(app: AppHandle, profile: ProjectProfile) -> Result<(), String> {
    let mut store = load_store(&app);
    store.profiles.retain(|p| p.id != profile.id);
    store.upsert(profile)?;
    save_store(&app, &store)
}

#[tauri::command]
pub fn delete_profile(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    id: String,
) -> Result<(), String> {
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
    let mut store = load_store(&app);
    if !store.remove(&id) {
        return Err("프로필을 찾을 수 없습니다".to_string());
    }
    save_store(&app, &store)
}

/// wsl-desktop의 gitStatus 이관 (§3.1, §15.2). 프로젝트 경로들의 git 상태.
#[tauri::command]
pub async fn git_status(
    projects: Vec<String>,
) -> Result<Vec<crate::core::health::GitStatus>, String> {
    let mut out = Vec::new();
    for path in projects {
        match devbox_git::run(&["status", "--porcelain", "--branch"], &path) {
            Ok(text) => out.push(parse_git_status(&path, &text)),
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

async fn wsl_list_output() -> Option<String> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-l", "-v"]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    // `wsl.exe -l -v`는 UTF-16LE로 출력된다 (공용 crates/wsl 디코더, PR #183과 동일 근거).
    Some(devbox_wsl::output::decode_output(&output.stdout))
}

fn port_open(port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    if let Ok(addr) = format!("127.0.0.1:{port}").parse() {
        return TcpStream::connect_timeout(&addr, Duration::from_millis(800)).is_ok();
    }
    false
}

/// read-only project health. run-manager 서비스는 integration snapshot(§10.1)으로 읽는다.
#[tauri::command]
pub async fn project_health(app: AppHandle, profile_id: String) -> Result<ProjectHealth, String> {
    let store = load_store(&app);
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
        let status = git_status(vec![root]).await?;
        if let Some(s) = status.first() {
            items.push(HealthItem {
                name: "git".into(),
                ok: s.clean,
                detail: format!("{} · {} · {} changes", s.path, s.branch, s.changes),
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
        match wsl_list_output().await {
            Some(output) => {
                let ok = has_distro(&wsl.distro, &output);
                items.push(HealthItem {
                    name: "wsl".into(),
                    ok,
                    detail: if ok {
                        format!("{} · {}", wsl.distro, wsl.path)
                    } else {
                        format!("distro 없음: {}", wsl.distro)
                    },
                });
            }
            None => items.push(HealthItem {
                name: "wsl".into(),
                ok: false,
                detail: "wsl.exe 조회 불가".into(),
            }),
        }
    }

    // expected ports
    if profile.expected_ports.is_empty() {
        items.push(HealthItem {
            name: "ports".into(),
            ok: true,
            detail: "예상 포트 없음".into(),
        });
    } else {
        let closed: Vec<u16> = profile
            .expected_ports
            .iter()
            .copied()
            .filter(|p| !port_open(*p))
            .collect();
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
    if profile.run_manager_service_ids.is_empty() {
        items.push(HealthItem {
            name: "services".into(),
            ok: true,
            detail: "서비스 미지정".into(),
        });
    } else {
        let snapshot = devbox_integration::read_snapshot("run-manager", 1).unwrap_or(None);
        let running: Vec<String> = snapshot
            .as_ref()
            .and_then(|e| e.data.get("activeServices").and_then(|v| v.as_array()))
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        s.get("id")
                            .and_then(|id| id.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default();
        let missing: Vec<&String> = profile
            .run_manager_service_ids
            .iter()
            .filter(|id| !running.iter().any(|r| r == *id))
            .collect();
        items.push(HealthItem {
            name: "services".into(),
            ok: missing.is_empty(),
            detail: if missing.is_empty() {
                "서비스 전부 실행 중".into()
            } else {
                format!(
                    "미실행: {}",
                    missing
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        });
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRun {
    pub run_id: String,
    pub profile_id: String,
    pub steps: Vec<RunStep>,
    /// Workbench가 시작한 프로세스 PID (Stop What I Started 대상).
    pub started_pids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRunOwnership {
    pub run_id: String,
    pub profile_id: String,
}

impl From<&WorkspaceRun> for WorkspaceRunOwnership {
    fn from(run: &WorkspaceRun) -> Self {
        Self {
            run_id: run.run_id.clone(),
            profile_id: run.profile_id.clone(),
        }
    }
}

/// 실행 기록 (인메모리). 앱 수명 동안 유지한다.
pub struct RunRegistry {
    pub runs: Mutex<HashMap<String, WorkspaceRun>>,
    starting_profile: Mutex<Option<String>>,
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

#[tauri::command]
pub async fn start_workspace(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
) -> Result<WorkspaceRun, String> {
    let _start_claim = claim_workspace_transition(&registry.starting_profile, &profile_id)
        .map_err(str::to_string)?;
    if !registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?
        .is_empty()
    {
        return Err("현재 Workspace 실행을 먼저 중지하세요".to_string());
    }
    let store = load_store(&app);
    let profile = store
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;

    let mut steps = Vec::new();

    // 사전 점검 (health)
    let health = project_health(app.clone(), profile_id.clone()).await?;
    for item in health.items {
        steps.push(RunStep {
            name: item.name,
            ok: item.ok,
            detail: item.detail,
        });
    }

    // 예상 포트 대기 (닫힌 포트는 최대 5×2초 대기)
    for port in &profile.expected_ports {
        let mut waited = false;
        for _ in 0..5 {
            if port_open(*port) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            waited = true;
        }
        if waited && !port_open(*port) {
            steps.push(RunStep {
                name: "wait-port".into(),
                ok: false,
                detail: format!("{port} 여전히 닫힘"),
            });
        }
    }

    // 앱 열기 (best-effort). Workbench가 시작한 것만 기록한다.
    let mut started_pids = Vec::new();
    match wsl_desktop_open_request(&profile) {
        Ok(request) => match devbox_launch::launch_open("wsl-desktop", &request) {
            Ok(pid) => started_pids.push(pid),
            Err(e) => steps.push(RunStep {
                name: "open".into(),
                ok: false,
                detail: format!("wsl-desktop 시작 실패: {e}"),
            }),
        },
        Err(e) => steps.push(RunStep {
            name: "open".into(),
            ok: false,
            detail: e,
        }),
    }
    match code_pad_open_request(&profile) {
        Ok(request) => match devbox_launch::launch_open("code-pad", &request) {
            Ok(pid) => started_pids.push(pid),
            Err(e) => steps.push(RunStep {
                name: "open".into(),
                ok: false,
                detail: format!("code-pad 시작 실패: {e}"),
            }),
        },
        Err(e) => steps.push(RunStep {
            name: "open".into(),
            ok: false,
            detail: e,
        }),
    }

    let run = WorkspaceRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        profile_id,
        steps,
        started_pids,
    };
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 저장할 수 없습니다".to_string())?;
    if !runs.is_empty() {
        return Err("Workspace 실행 상태가 변경되어 결과를 저장할 수 없습니다".to_string());
    }
    runs.insert(run.run_id.clone(), run.clone());
    Ok(run)
}

/// Workbench가 시작한 것만 정리한다 (이미 실행 중이던 자원은 건드리지 않는다).
#[tauri::command]
pub fn stop_workspace(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    run_id: String,
    profile_id: String,
) -> Result<usize, String> {
    let mut runs = registry
        .runs
        .lock()
        .map_err(|_| "실행 상태를 확인할 수 없습니다".to_string())?;
    let Some(run) = take_profile_run(&mut runs, &run_id, &profile_id).map_err(str::to_string)?
    else {
        return Ok(0);
    };
    let stopped: usize = run
        .started_pids
        .iter()
        .filter(|pid| {
            #[cfg(target_os = "windows")]
            {
                // 생성한 프로세스만 종료 (Workbench가 시작한 것)
                std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .output()
                    .is_ok()
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = pid;
                false
            }
        })
        .count();
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

    fn workspace_run(run_id: &str, profile_id: &str) -> WorkspaceRun {
        WorkspaceRun {
            run_id: run_id.to_string(),
            profile_id: profile_id.to_string(),
            steps: Vec::new(),
            started_pids: vec![101],
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
        });
        let one = HashMap::from([("run-1".to_string(), sensitive_run)]);
        let ownership = single_workspace_run(&one).unwrap().unwrap();
        assert_eq!(ownership.run_id, "run-1");
        let json = serde_json::to_string(&ownership).unwrap();
        assert!(!json.contains("TOP_SECRET"));
        assert!(!json.contains("startedPids"));
        assert!(!json.contains("steps"));

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
