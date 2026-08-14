use crate::commands::tracking::AppState;
use crate::core::aggregate::collect_git;
use crate::core::db;
use crate::core::models::{DayPoint, DaySummary, RangeSummary};
use std::sync::Arc;

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
