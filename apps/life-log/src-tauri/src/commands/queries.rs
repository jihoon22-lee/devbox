use crate::commands::tracking::AppState;
use crate::core::db::{get_app_stats, get_timeline};
use crate::core::models::{AppTotal, Session};
use std::sync::Arc;

/// 하루(시작~끝 epoch ms)의 세션 타임라인을 조회한다.
#[tauri::command]
pub fn timeline(
    state: tauri::State<'_, Arc<AppState>>,
    day_start: i64,
    day_end: i64,
) -> Result<Vec<Session>, String> {
    get_timeline(&state.db.lock().unwrap(), day_start, day_end).map_err(|e| e.to_string())
}

/// 기간 내 앱별 사용 합계를 조회한다.
#[tauri::command]
pub fn app_stats(
    state: tauri::State<'_, Arc<AppState>>,
    start: i64,
    end: i64,
) -> Result<Vec<AppTotal>, String> {
    get_app_stats(&state.db.lock().unwrap(), start, end).map_err(|e| e.to_string())
}
