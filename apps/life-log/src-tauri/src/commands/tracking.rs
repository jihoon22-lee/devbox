use crate::core::db::insert_session;
use crate::core::idle::DEFAULT_IDLE_THRESHOLD_MS;
use crate::core::models::ClosedSession;
use crate::core::privacy::{apply as apply_privacy, parse_rules, PrivacyRules};
use crate::core::sessionizer::Sessionizer;
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// One native digest at a time. The guard owns the exact cancellation token
/// used by DB/Git work, and cancellation only clears the matching generation
/// when the guard is dropped.
pub struct DigestOperationState {
    active: Mutex<Option<(u64, Arc<AtomicBool>)>>,
    next_id: std::sync::atomic::AtomicU64,
}

pub struct DigestOperationGuard {
    state: Arc<DigestOperationState>,
    id: u64,
    cancellation: Arc<AtomicBool>,
}

impl Default for DigestOperationState {
    fn default() -> Self {
        Self {
            active: Mutex::new(None),
            next_id: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl DigestOperationState {
    pub fn begin(self: &Arc<Self>) -> Result<DigestOperationGuard, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "digest 작업을 잠글 수 없습니다".to_string())?;
        if active.is_some() {
            return Err("digest_busy".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancellation = Arc::new(AtomicBool::new(false));
        *active = Some((id, Arc::clone(&cancellation)));
        Ok(DigestOperationGuard {
            state: Arc::clone(self),
            id,
            cancellation,
        })
    }

    /// Mark the current generation cancelled and return its identity. A caller
    /// waiting for cancellation must retain this id so a newer generation that
    /// starts immediately after the old guard drops is never mistaken for the
    /// operation being cancelled.
    pub fn cancel_generation(&self) -> Option<u64> {
        let Ok(active) = self.active.lock() else {
            return None;
        };
        let (id, cancellation) = active.as_ref()?;
        cancellation.store(true, Ordering::Release);
        Some(*id)
    }

    pub fn is_active(&self) -> bool {
        self.active.lock().map_or(true, |active| active.is_some())
    }

    pub fn is_active_generation(&self, id: u64) -> bool {
        self.active.lock().map_or(true, |active| {
            active.as_ref().is_some_and(|(current, _)| *current == id)
        })
    }
}

impl DigestOperationGuard {
    pub fn cancellation(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancellation)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    /// Linearize a non-interruptible native commit against cancellation. The
    /// active-generation mutex is held while `commit` runs, so cancellation
    /// either wins before the write starts or waits until the write has
    /// completed; it can never report cancellation while a write races the
    /// final pre-write check.
    #[cfg(any(target_os = "windows", test))]
    pub fn commit_if_not_cancelled<T>(
        &self,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let active = self
            .state
            .active
            .lock()
            .map_err(|_| "digest 작업을 잠글 수 없습니다".to_string())?;
        let Some((id, cancellation)) = active.as_ref() else {
            return Err("digest_cancelled".into());
        };
        if *id != self.id || cancellation.load(Ordering::Acquire) {
            return Err("digest_cancelled".into());
        }
        commit()
    }
}

impl Drop for DigestOperationGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            if active.as_ref().is_some_and(|(id, _)| *id == self.id) {
                *active = None;
            }
        }
    }
}

/// 앱 전역 상태: DB 커넥션 + 세션 병합기 + 추적 플래그
pub struct AppState {
    /// Native-owned snapshot namespace; None preserves the standalone legacy contract.
    pub integration_root: Option<std::path::PathBuf>,
    pub db: Mutex<Connection>,
    pub sessionizer: Mutex<Sessionizer>,
    pub tracking: AtomicBool,
    /// Serializes poll observations with explicit start/stop.
    pub tracking_control: Mutex<()>,
    /// Only the new product persists consent; the legacy default is preserved.
    pub persist_tracking_consent: bool,
    pub snapshot_writer: Mutex<()>,
    pub digest_operations: Arc<DigestOperationState>,
    pub digest_handles: crate::core::digest::DigestHandleStore,
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
    start_tracking_inner(&state)
}

fn start_tracking_inner(state: &AppState) -> Result<bool, String> {
    let _control = state
        .tracking_control
        .lock()
        .map_err(|_| "tracking_state_unavailable")?;
    if state.persist_tracking_consent {
        let conn = state.db.lock().map_err(|_| "tracking_state_unavailable")?;
        set_product_consent(&conn, true)?;
    }
    Ok(!state.tracking.swap(true, Ordering::SeqCst))
}

/// Stop collection before returning, even when persistence fails.
#[tauri::command]
pub fn stop_tracking(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    stop_tracking_inner(&state)
}

fn stop_tracking_inner(state: &AppState) -> Result<(), String> {
    stop_tracking_runtime(state, true)
}

pub(crate) fn stop_tracking_runtime(state: &AppState, update_consent: bool) -> Result<(), String> {
    let _control = state
        .tracking_control
        .lock()
        .map_err(|_| "tracking_state_unavailable")?;
    state.tracking.store(false, Ordering::SeqCst);
    let closed = state
        .sessionizer
        .lock()
        .map_err(|_| "tracking_state_unavailable")?
        .finish(now_ms());
    let conn = state.db.lock().map_err(|_| "tracking_state_unavailable")?;
    let persisted = if update_consent && state.persist_tracking_consent {
        set_product_consent(&conn, false)
    } else {
        Ok(())
    };
    if let Some(c) = closed {
        insert_filtered(&conn, &c, &privacy_rules(&conn)).map_err(|e| e.to_string())?;
    }
    persisted
}

const PRODUCT_CONSENT_KEY: &str = "product_activity_collection_v1";

/// Missing, malformed or unreadable consent never starts the product collector.
pub(crate) fn product_consent(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [PRODUCT_CONSENT_KEY],
        |row| row.get::<_, String>(0),
    )
    .is_ok_and(|value| value == "enabled")
}

fn set_product_consent(conn: &Connection, enabled: bool) -> Result<(), String> {
    conn.execute("INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [PRODUCT_CONSENT_KEY, if enabled { "enabled" } else { "disabled" }])
        .map_err(|_| "activity_consent_save_failed".to_owned())?;
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
            let Ok(_control) = state.tracking_control.lock() else {
                continue;
            };
            if !state.tracking.load(Ordering::SeqCst) {
                idle_active = false;
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
            let rules = {
                let conn = state.db.lock().unwrap();
                privacy_rules(&conn)
            };

            let idle_end = crate::core::idle::session_end_on_idle(
                now,
                last_input_ms().unwrap_or(0),
                threshold,
            );
            if let Some(end_ts) = idle_end {
                // idle 시작: 열린 세션을 idle 직전까지로 마감 (idle 시간 미집계)
                if let Some(closed) = state.sessionizer.lock().unwrap().finish(end_ts) {
                    let conn = state.db.lock().unwrap();
                    let _ = insert_filtered(&conn, &closed, &rules);
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
                let conn = state.db.lock().unwrap();
                let _ = insert_filtered(&conn, &c, &rules);
            }
        }
    });
}

/// 설정에서 privacy rules를 읽는다 (없으면 기본값).
fn privacy_rules(conn: &Connection) -> PrivacyRules {
    let json = crate::core::db::get_setting(conn, "privacy_rules", "{}");
    parse_rules(&json)
}

/// privacy rule을 **insert 전에** 적용해 저장하거나 건너뛴다.
fn insert_filtered(
    conn: &Connection,
    closed: &ClosedSession,
    rules: &PrivacyRules,
) -> rusqlite::Result<()> {
    let Some((app, title)) = apply_privacy(rules, &closed.app, &closed.title) else {
        // 제외 대상 세션 — 저장하지 않는다
        return Ok(());
    };
    insert_session(
        conn,
        &ClosedSession {
            app,
            title,
            start_ts: closed.start_ts,
            end_ts: closed.end_ts,
        },
    )
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

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_start_tracking(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = start_tracking(component_app.state())?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_stop_tracking(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    stop_tracking(component_app.state())?;
    Ok(serde_json::Value::Null)
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_is_tracking(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = is_tracking(component_app.state());
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_set_idle_threshold(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        threshold_ms: i64,
    }
    let Input { threshold_ms } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    set_idle_threshold(component_app.state(), threshold_ms)?;
    Ok(serde_json::Value::Null)
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_get_idle_threshold(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = get_idle_threshold(component_app.state());
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

#[cfg(test)]
mod tests {
    use super::DigestOperationState;
    use std::sync::Arc;

    fn collector_state(conn: rusqlite::Connection) -> super::AppState {
        super::AppState {
            integration_root: None,
            tracking: std::sync::atomic::AtomicBool::new(super::product_consent(&conn)),
            tracking_control: std::sync::Mutex::new(()),
            persist_tracking_consent: true,
            db: std::sync::Mutex::new(conn),
            sessionizer: std::sync::Mutex::new(crate::core::sessionizer::Sessionizer::new()),
            snapshot_writer: std::sync::Mutex::new(()),
            digest_operations: Arc::new(DigestOperationState::default()),
            digest_handles: crate::core::digest::DigestHandleStore::default(),
        }
    }

    #[test]
    fn product_collection_requires_explicit_consent_and_preserves_stop_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("activity.db");
        let state = collector_state(crate::core::db::init(&path).unwrap());
        assert!(!state.tracking.load(std::sync::atomic::Ordering::SeqCst));
        assert!(super::start_tracking_inner(&state).unwrap());
        drop(state);
        let state = collector_state(crate::core::db::init(&path).unwrap());
        assert!(state.tracking.load(std::sync::atomic::Ordering::SeqCst));
        super::stop_tracking_inner(&state).unwrap();
        drop(state);
        let state = collector_state(crate::core::db::init(&path).unwrap());
        assert!(!state.tracking.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn failed_consent_write_never_enables_collection_and_stop_still_disables() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        let state = collector_state(conn);
        state
            .db
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only = ON")
            .unwrap();
        assert_eq!(
            super::start_tracking_inner(&state).unwrap_err(),
            "activity_consent_save_failed"
        );
        assert!(!state.tracking.load(std::sync::atomic::Ordering::SeqCst));
        state
            .tracking
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            super::stop_tracking_inner(&state).unwrap_err(),
            "activity_consent_save_failed"
        );
        assert!(!state.tracking.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn product_exit_flushes_a_privacy_filtered_session_without_revoking_consent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO settings VALUES ('privacy_rules', ?1)",
            [r#"{"maskAllTitles":true}"#],
        )
        .unwrap();
        let state = collector_state(conn);
        super::start_tracking_inner(&state).unwrap();
        state.sessionizer.lock().unwrap().observe(
            "fixture.exe".into(),
            "private synthetic title".into(),
            super::now_ms() - 10,
        );
        super::stop_tracking_runtime(&state, false).unwrap();
        assert!(!state.tracking.load(std::sync::atomic::Ordering::SeqCst));
        let conn = state.db.lock().unwrap();
        assert!(super::product_consent(&conn));
        let title: String = conn
            .query_row("SELECT title FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert!(!title.contains("private synthetic title"));
    }

    #[test]
    fn malformed_or_legacy_settings_do_not_imply_product_consent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO settings VALUES ('tracking', 'true')", [])
            .unwrap();
        assert!(!super::product_consent(&conn));
        conn.execute(
            "INSERT INTO settings VALUES (?1, 'true')",
            [super::PRODUCT_CONSENT_KEY],
        )
        .unwrap();
        assert!(!super::product_consent(&conn));
    }

    #[test]
    fn digest_operations_are_single_flight_and_generation_scoped() {
        let state = Arc::new(DigestOperationState::default());
        let first = state.begin().unwrap();
        assert!(matches!(state.begin(), Err(error) if error == "digest_busy"));
        let first_generation = state.cancel_generation().unwrap();
        assert!(state.is_active_generation(first_generation));
        assert!(first.is_cancelled());
        drop(first);
        assert!(!state.is_active_generation(first_generation));

        let second = state.begin().unwrap();
        assert!(!second.is_cancelled());
        let second_generation = state.cancel_generation().unwrap();
        assert_ne!(first_generation, second_generation);
        assert!(second.is_cancelled());
        drop(second);
        assert!(state.cancel_generation().is_none());
    }

    #[test]
    fn commit_gate_rejects_a_cancelled_generation_before_writing() {
        let state = Arc::new(DigestOperationState::default());
        let operation = state.begin().unwrap();
        assert!(state.cancel_generation().is_some());
        let error = operation
            .commit_if_not_cancelled(|| Ok::<_, String>(()))
            .unwrap_err();
        assert_eq!(error, "digest_cancelled");
    }
}
