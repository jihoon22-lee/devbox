use crate::commands::tracking::AppState;
use crate::core::aggregate::collect_git;
use crate::core::attribution::{attribute_sessions, Attribution, ProjectMatch};
use crate::core::db;
use crate::core::models::{DayPoint, DaySummary, RangeSummary};
use serde::Serialize;
use std::sync::Arc;

/// integration snapshot source 상태 (UI 표시용).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub producer: String,
    pub available: bool,
    pub schema_version: Option<u32>,
    pub producer_version: Option<String>,
    pub generated_at: Option<String>,
    /// 마지막 갱신 이후 경과 (ms). snapshot이 없으면 None.
    pub freshness_ms: Option<i64>,
    pub error: Option<String>,
}

/// 기간 내 활동을 프로젝트로 귀속한다. 미귀속은 별도로 표시한다.
///
/// 프로젝트 identity는 설정된 git 프로젝트 경로의 basename을 쓴다. Workbench의
/// ProjectProfile(§10.2)이 생기면 canonical key로 대체한다.
#[tauri::command]
pub fn project_attribution(
    state: tauri::State<'_, Arc<AppState>>,
    day_start: i64,
    day_end: i64,
) -> Result<AttributionResult, String> {
    let conn = state.db.lock().unwrap();
    let sessions = db::get_timeline(&conn, day_start, day_end).map_err(|e| e.to_string())?;
    let projects = db::get_setting(&conn, "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    drop(conn);

    let profiles: Vec<ProjectMatch> = projects
        .iter()
        .filter_map(|p| {
            let basename = p
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()
                .filter(|b| !b.is_empty())?;
            Some(ProjectMatch {
                project_id: p.clone(),
                basenames: vec![basename.to_string()],
            })
        })
        .collect();

    let rows: Vec<(String, String, i64)> = sessions
        .into_iter()
        .map(|s| (s.app, s.title, s.duration_ms))
        .collect();
    let (attributed, unattributed) = attribute_sessions(&rows, &profiles);

    Ok(AttributionResult {
        attributed,
        unattributed,
        profile_count: profiles.len(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionResult {
    pub attributed: Vec<Attribution>,
    pub unattributed: Attribution,
    pub profile_count: usize,
}

/// 등록된 integration source의 상태를 반환한다.
#[tauri::command]
pub fn integration_sources() -> Vec<SourceStatus> {
    vec![run_manager_source_status()]
}

fn run_manager_source_status() -> SourceStatus {
    match crate::core::readers::read_snapshot(crate::core::readers::PRODUCER_RUN_MANAGER, 1) {
        Ok(Some(envelope)) => {
            let path =
                crate::core::readers::snapshot_path(crate::core::readers::PRODUCER_RUN_MANAGER, 1);
            let freshness_ms = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|n| n.as_millis() as i64)
                        .unwrap_or(0);
                    (now - d.as_millis() as i64).max(0)
                });
            SourceStatus {
                producer: envelope.producer.clone(),
                available: true,
                schema_version: Some(envelope.schema_version),
                producer_version: Some(envelope.producer_version),
                generated_at: Some(envelope.generated_at),
                freshness_ms,
                error: None,
            }
        }
        Ok(None) => SourceStatus {
            producer: crate::core::readers::PRODUCER_RUN_MANAGER.into(),
            available: false,
            schema_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            error: Some("snapshot이 없다 (run-manager를 실행하면 생성된다)".into()),
        },
        Err(error) => SourceStatus {
            producer: crate::core::readers::PRODUCER_RUN_MANAGER.into(),
            available: false,
            schema_version: None,
            producer_version: None,
            generated_at: None,
            freshness_ms: None,
            error: Some(error),
        },
    }
}

/// 프로젝트 경로 목록 설정 (줄바꿈 구분).
#[tauri::command]
pub fn set_projects(
    state: tauri::State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> Result<(), String> {
    db::set_setting(&state.db.lock().unwrap(), "projects", &paths.join("\n"));
    Ok(())
}

#[tauri::command]
pub fn get_projects(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    db::get_setting(&state.db.lock().unwrap(), "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn saved_projects(state: &tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    db::get_setting(&state.db.lock().unwrap(), "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 하루 요약. 내부 활동 DB(동기) + git(비동기)을 합친다.
#[tauri::command]
pub async fn get_day(
    state: tauri::State<'_, Arc<AppState>>,
    date: String,
    day_start: i64,
    day_end: i64,
) -> Result<DaySummary, String> {
    // 잠금은 await 전에 해제한다 (MutexGuard는 Send가 아님)
    let (pc_usage_ms, app_totals) = {
        let conn = state.db.lock().unwrap();
        (
            db::pc_usage(&conn, day_start, day_end),
            db::get_app_stats(&conn, day_start, day_end).unwrap_or_default(),
        )
    };
    let projects = saved_projects(&state);
    let git = collect_git(&projects, day_start, day_end).await;

    Ok(DaySummary {
        date,
        pc_usage_ms,
        app_totals,
        git,
    })
}

/// 기간(주/월) 요약. 일별 사용량 + 합계 + git을 한 번에 조회한다.
#[tauri::command]
pub async fn get_range(
    state: tauri::State<'_, Arc<AppState>>,
    label: String,
    day_start: i64,
    day_end: i64,
) -> Result<RangeSummary, String> {
    let (pc_usage_ms, app_totals, daily) = {
        let conn = state.db.lock().unwrap();
        let app_totals = db::get_app_stats(&conn, day_start, day_end).unwrap_or_default();
        let daily = db::get_daily_usage(&conn, day_start, day_end)
            .into_iter()
            .map(|(day_ms, pc_usage_ms)| DayPoint {
                day_ms,
                pc_usage_ms,
            })
            .collect();
        (db::pc_usage(&conn, day_start, day_end), app_totals, daily)
    };
    let projects = saved_projects(&state);
    let git = collect_git(&projects, day_start, day_end).await;

    Ok(RangeSummary {
        label,
        pc_usage_ms,
        app_totals,
        git,
        daily,
    })
}
