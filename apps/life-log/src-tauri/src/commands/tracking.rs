use crate::core::db::insert_session;
use crate::core::idle::DEFAULT_IDLE_THRESHOLD_MS;
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
/// idle/lock/suspend 경계에서는 열린 세션을 idle 시작 시점에서 마감하고,
/// resume 후 새 observation으로 시작한다 (§9.3).
pub fn spawn_poller(app: &tauri::AppHandle) {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        let mut idle_active = false;
        loop {
            interval.tick().await;
            if !state.tracking.load(Ordering::SeqCst) {
                continue;
            }
            let now = now_ms();
            let threshold = {
                let conn = state.db.lock().unwrap();
                crate::core::db::get_setting(
                    &conn,
                    "idle_threshold_ms",
                    &DEFAULT_IDLE_THRESHOLD_MS.to_string(),
                )
            };
            let threshold = crate::core::idle::parse_threshold_ms(&threshold);

            let idle_end = crate::core::idle::session_end_on_idle(
                now,
                last_input_ms().unwrap_or(0),
                threshold,
            );
            if let Some(end_ts) = idle_end {
                // idle 시작: 열린 세션을 idle 직전까지로 마감 (idle 시간 미집계)
                if let Some(closed) = state.sessionizer.lock().unwrap().finish(end_ts) {
                    let _ = insert_session(&state.db.lock().unwrap(), &closed);
                }
                idle_active = true;
                continue;
            }
            if idle_active {
                idle_active = false;
                // resume: 새 observation으로 시작
                if let Some((app, title)) = crate::core::window::foreground_window() {
                    let _ = state.sessionizer.lock().unwrap().observe(app, title, now);
                }
                continue;
            }
            let Some((app, title)) = crate::core::window::foreground_window() else {
                continue;
            };
            let closed = state.sessionizer.lock().unwrap().observe(app, title, now);
            if let Some(c) = closed {
                let _ = insert_session(&state.db.lock().unwrap(), &c);
            }
        }
    });
}

/// idle threshold 설정 (ms).
#[tauri::command]
pub fn set_idle_threshold(
    state: tauri::State<'_, Arc<AppState>>,
    threshold_ms: i64,
) -> Result<(), String> {
    crate::core::db::set_setting(
        &state.db.lock().unwrap(),
        "idle_threshold_ms",
        &threshold_ms.to_string(),
    );
    Ok(())
}

#[tauri::command]
pub fn get_idle_threshold(state: tauri::State<'_, Arc<AppState>>) -> i64 {
    let value = crate::core::db::get_setting(
        &state.db.lock().unwrap(),
        "idle_threshold_ms",
        &DEFAULT_IDLE_THRESHOLD_MS.to_string(),
    );
    crate::core::idle::parse_threshold_ms(&value)
}

/// 마지막 입력 이후 경과 시간(ms). Windows에서만 실제 동작하고, 그 외 OS에서는
/// None을 반환한다 (컴파일·테스트 용도).
#[cfg(target_os = "windows")]
fn last_input_ms() -> Option<i64> {
    use windows::Win32::System::SystemInformation::GetTickCount64;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let last = u64::from(info.dwTime);
            let now_tick = GetTickCount64();
            Some((now_tick.saturating_sub(last)) as i64)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn last_input_ms() -> Option<i64> {
    None
}
