use crate::core::aggregate::{collect_git, summarize_activity};
use crate::core::models::{DayPoint, DaySummary, RangeSummary};
use crate::core::readers;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// 앱 전역 상태 (설정 저장용 DB)
pub struct AppState {
    pub db: Mutex<Connection>,
}

fn get_setting(conn: &Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

fn set_setting(conn: &Connection, key: &str, value: &str) {
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    );
}

/// 활동 DB 경로 설정.
#[tauri::command]
pub fn set_activity_db(state: tauri::State<'_, Arc<AppState>>, path: String) -> Result<(), String> {
    set_setting(&state.db.lock().unwrap(), "activity_db", &path);
    Ok(())
}

#[tauri::command]
pub fn get_activity_db(state: tauri::State<'_, Arc<AppState>>) -> String {
    get_setting(
        &state.db.lock().unwrap(),
        "activity_db",
        &readers::default_activity_db(),
    )
}

/// 프로젝트 경로 목록 설정 (쉼표 구분).
#[tauri::command]
pub fn set_projects(
    state: tauri::State<'_, Arc<AppState>>,
    paths: Vec<String>,
) -> Result<(), String> {
    set_setting(&state.db.lock().unwrap(), "projects", &paths.join("\n"));
    Ok(())
}

#[tauri::command]
pub fn get_projects(state: tauri::State<'_, Arc<AppState>>) -> Vec<String> {
    get_setting(&state.db.lock().unwrap(), "projects", "")
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 하루 요약. 활동 DB(동기) + git(비동기)을 합친다.
#[tauri::command]
pub async fn get_day(
    state: tauri::State<'_, Arc<AppState>>,
    date: String,
    day_start: i64,
    day_end: i64,
) -> Result<DaySummary, String> {
    // 잠금은 await 전에 해제한다 (MutexGuard는 Send가 아님)
    let (activity_db, projects) = {
        let conn = state.db.lock().unwrap();
        let activity_db = get_setting(&conn, "activity_db", &readers::default_activity_db());
        let projects = get_setting(&conn, "projects", "")
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        (activity_db, projects)
    };

    let mut summary = summarize_activity(&date, day_start, day_end, &activity_db);
    summary.git = collect_git(&projects, day_start, day_end).await;
    Ok(summary)
}

/// 기간(주/월) 요약. 일별 사용량 + 합계 + git을 한 번에 조회한다.
#[tauri::command]
pub async fn get_range(
    state: tauri::State<'_, Arc<AppState>>,
    label: String,
    day_start: i64,
    day_end: i64,
) -> Result<RangeSummary, String> {
    let (activity_db, projects) = {
        let conn = state.db.lock().unwrap();
        let activity_db = get_setting(&conn, "activity_db", &readers::default_activity_db());
        let projects = get_setting(&conn, "projects", "")
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        (activity_db, projects)
    };

    let (pc_usage_ms, app_totals) =
        readers::activity::read_activity(&activity_db, day_start, day_end);
    let daily = readers::activity::read_activity_daily(&activity_db, day_start, day_end)
        .into_iter()
        .map(|(day_ms, pc_usage_ms)| DayPoint {
            day_ms,
            pc_usage_ms,
        })
        .collect();
    let git = collect_git(&projects, day_start, day_end).await;

    Ok(RangeSummary {
        label,
        pc_usage_ms,
        app_totals,
        git,
        daily,
    })
}
