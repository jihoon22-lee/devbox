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

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_timeline(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        day_start: i64,
        day_end: i64,
    }
    let Input { day_start, day_end } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = timeline(component_app.state(), day_start, day_end)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_app_stats(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        start: i64,
        end: i64,
    }
    let Input { start, end } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = app_stats(component_app.state(), start, end)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}
