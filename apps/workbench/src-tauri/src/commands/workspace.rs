//! Workbench command — 프로필 CRUD, health, Start/Stop Workspace.

use crate::core::health::{has_distro, parse_git_status};
use crate::core::profile::{ProfileStore, ProjectProfile};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use tokio::process::Command;

const PROFILE_FILE: &str = "project-profiles.json";

fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(PROFILE_FILE))
}

fn load_store(app: &AppHandle) -> ProfileStore {
    profile_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| ProfileStore::load(&t))
        .unwrap_or_default()
}

fn save_store(app: &AppHandle, store: &ProfileStore) -> Result<(), String> {
    let path = profile_path(app)?;
    let json = store.to_json().map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
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
pub fn delete_profile(app: AppHandle, id: String) -> Result<(), String> {
    let mut store = load_store(&app);
    store.remove(&id);
    save_store(&app, &store)
}

/// wsl-desktop의 gitStatus 이관 (§3.1, §15.2). 프로젝트 경로들의 git 상태.
#[tauri::command]
pub async fn git_status(
    projects: Vec<String>,
) -> Result<Vec<crate::core::health::GitStatus>, String> {
    let mut out = Vec::new();
    for path in projects {
        let mut cmd = Command::new("git");
        cmd.args(["-C", &path, "status", "--porcelain", "--branch"]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000);
        match cmd.output().await {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout).into_owned();
                out.push(parse_git_status(&path, &text));
            }
            _ => out.push(crate::core::health::GitStatus {
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
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
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

/// 실행 기록 (인메모리). 앱 수명 동안 유지한다.
pub struct RunRegistry {
    pub runs: Mutex<HashMap<String, WorkspaceRun>>,
}

#[tauri::command]
pub async fn start_workspace(
    app: AppHandle,
    registry: tauri::State<'_, Arc<RunRegistry>>,
    profile_id: String,
) -> Result<WorkspaceRun, String> {
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
    for (app_name, args) in [(
        "WSLDesktop",
        vec!["--profile".to_string(), profile.id.clone()],
    )] {
        match std::process::Command::new(format!("{app_name}.exe"))
            .args(&args)
            .spawn()
        {
            Ok(child) => started_pids.push(child.id()),
            Err(_) => steps.push(RunStep {
                name: "open".into(),
                ok: false,
                detail: format!("{app_name} 시작 실패 (설치 확인)"),
            }),
        }
    }
    for (app_name, args) in [(
        "CodePad",
        vec![
            "--workspace".to_string(),
            profile.windows_path.clone().unwrap_or_default(),
        ],
    )] {
        match std::process::Command::new(format!("{app_name}.exe"))
            .args(&args)
            .spawn()
        {
            Ok(child) => started_pids.push(child.id()),
            Err(_) => steps.push(RunStep {
                name: "open".into(),
                ok: false,
                detail: format!("{app_name} 시작 실패 (설치 확인)"),
            }),
        }
    }

    let run = WorkspaceRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        profile_id,
        steps,
        started_pids,
    };
    registry
        .runs
        .lock()
        .unwrap()
        .insert(run.run_id.clone(), run.clone());
    Ok(run)
}

/// Workbench가 시작한 것만 정리한다 (이미 실행 중이던 자원은 건드리지 않는다).
#[tauri::command]
pub fn stop_workspace(
    registry: tauri::State<'_, Arc<RunRegistry>>,
    run_id: String,
) -> Result<usize, String> {
    let mut runs = registry.runs.lock().unwrap();
    let Some(run) = runs.remove(&run_id) else {
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

pub fn run_registry() -> Arc<RunRegistry> {
    Arc::new(RunRegistry {
        runs: Mutex::new(HashMap::new()),
    })
}

/// life-log settings의 프로젝트 경로를 읽어 프로필로 흡수 (read-only, §10.2).
pub fn absorb_life_log_projects(store: &mut ProfileStore) -> Result<usize, String> {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let db_path = PathBuf::from(base)
        .join("com.devbox.lifelog")
        .join("data.db");
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
    let projects: Vec<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'projects'",
            [],
            |r| r.get(0),
        )
        .map(|v: String| {
            v.lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let mut absorbed = 0;
    for path in projects {
        let mut profile =
            ProjectProfile::new(path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string());
        profile.windows_path = Some(path.clone());
        profile.git_root = Some(path);
        if store.upsert(profile).map_err(|e| e.to_string())?.is_none() {
            absorbed += 1;
        }
    }
    Ok(absorbed)
}
