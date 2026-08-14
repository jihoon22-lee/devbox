use crate::core::db::insert_session;
use crate::core::sessionizer::Sessionizer;
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// 앱 전역 상태: DB 커넥션 + 세션 병합기 + 추적 플래그
pub struct AppState {
    pub db: Mutex<Connection>,
    pub sessionizer: Mutex<Sessionizer>,
    pub tracking: AtomicBool,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 추적 시작. 이미 진행 중이면 false를 반환한다.
#[tauri::command]
pub fn start_tracking(state: tauri::State<'_, Arc<AppState>>) -> Result<bool, String> {
    let was_tracking = state.tracking.swap(true, Ordering::SeqCst);
    Ok(!was_tracking)
}

/// 추적 중지. 열려 있던 세션을 마감한다.
#[tauri::command]
pub fn stop_tracking(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    state.tracking.store(false, Ordering::SeqCst);
    let closed = state.sessionizer.lock().unwrap().finish(now_ms());
    if let Some(c) = closed {
        insert_session(&state.db.lock().unwrap(), &c).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn is_tracking(state: tauri::State<'_, Arc<AppState>>) -> bool {
    state.tracking.load(Ordering::SeqCst)
}

/// 추적 폴러를 백그라운드로 실행한다 (setup에서 1회 호출).
/// 2초 간격으로 포그라운드 창을 감지해 세션을 병합·저장한다.
pub fn spawn_poller(app: &tauri::AppHandle) {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            interval.tick().await;
            if !state.tracking.load(Ordering::SeqCst) {
                continue;
            }
            let Some((app, title)) = crate::core::window::foreground_window() else {
                continue;
            };
            let closed = state
                .sessionizer
                .lock()
                .unwrap()
                .observe(app, title, now_ms());
            if let Some(c) = closed {
                let _ = insert_session(&state.db.lock().unwrap(), &c);
            }
        }
    });
}
