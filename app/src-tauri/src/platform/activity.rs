//! Activity collection wiring: the native hook feeds `core::activity::Collector`;
//! the main window's uplink drains it to `activity.ingest` (01 §6.2). The hook
//! runs only with consent and while not paused.
use crate::core::{
    activity::{self, Collector, Hook},
    connection::IpcError,
    settings,
};
use devbox_activity::MAX_EVENTS;
use devbox_activity::model::idle::{
    DEFAULT_IDLE_THRESHOLD_MS, MAX_IDLE_THRESHOLD_MS, MIN_IDLE_THRESHOLD_MS,
};
use devbox_activity::model::privacy::PrivacyRules;
use devbox_win::activity::CollectorHandle;
use devbox_win::activity_buffer::Batch;
use serde::Serialize;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

pub struct Activity {
    collector: Arc<Mutex<Collector>>,
    hook: Mutex<Option<CollectorHandle>>,
    threshold_ms: Arc<AtomicI64>,
    /// New for every app start; numbering restarts with it.
    run_id: String,
}
impl Default for Activity {
    fn default() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let random = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        Self {
            collector: Arc::default(),
            hook: Mutex::new(None),
            threshold_ms: Arc::new(AtomicI64::new(DEFAULT_IDLE_THRESHOLD_MS)),
            run_id: activity::run_id(nanos, std::process::id(), random),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityState {
    consent: bool,
    paused: bool,
    legacy_knowledge: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drained {
    run_id: String,
    #[serde(flatten)]
    batch: Batch,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Never called with the collector locked: the hook thread's sink takes that
/// lock, and stopping joins the thread.
fn execute(state: &Activity, hook: Hook) {
    match hook {
        Hook::Keep => {}
        Hook::Stop => {
            let handle = lock(&state.hook).take();
            if let Some(handle) = handle {
                handle.stop();
            }
        }
        Hook::Start => {
            let previous = lock(&state.hook).take();
            if let Some(handle) = previous {
                handle.stop();
            }
            let collector = state.collector.clone();
            match devbox_win::activity::start(state.threshold_ms.clone(), move |observation| {
                lock(&collector).accept(observation);
            }) {
                Ok(handle) => *lock(&state.hook) = Some(handle),
                Err(error) => eprintln!("Cannot start activity collection: {error}"),
            }
        }
    }
}

fn snapshot(app: &AppHandle, state: &Activity) -> ActivityState {
    let (consent, paused) = {
        let collector = lock(&state.collector);
        (collector.consent(), collector.paused())
    };
    let legacy_knowledge = app
        .path()
        .local_data_dir()
        .is_ok_and(|dir| activity::legacy_knowledge(&dir));
    ActivityState {
        consent,
        paused,
        legacy_knowledge,
    }
}

fn publish(app: &AppHandle, state: &Activity) -> ActivityState {
    let current = snapshot(app, state);
    super::tray::set_activity(app, current.consent, current.paused);
    if let Err(error) = app.emit_to("main", "activity://state", current.clone()) {
        eprintln!("Cannot deliver activity state: {error}");
    }
    current
}

/// Stopping takes effect before saving; starting only after the choice is
/// saved (`core::activity::change`, retried). A failure still publishes the
/// state actually in effect.
fn change(
    app: &AppHandle,
    state: &Activity,
    consent: Option<bool>,
    paused: Option<bool>,
) -> Result<ActivityState, IpcError> {
    // Without a settings path a withdrawal still stops (the save fails).
    let path = super::preferences::settings_path(app);
    let saved = activity::change(
        &state.collector,
        consent,
        paused,
        now_ms(),
        || {
            let path = path
                .as_ref()
                .map_err(|error| std::io::Error::other(error.message.clone()))?;
            settings::update(path, |settings| {
                if let Some(consent) = consent {
                    settings.activity.consent = consent;
                }
                if let Some(paused) = paused {
                    settings.activity.paused = paused;
                }
            })
        },
        |hook| execute(state, hook),
    );
    let current = publish(app, state);
    saved
        .map(|()| current)
        .map_err(|error| IpcError::new(format!("활동 기록 설정을 저장하지 못했습니다: {error}")))
}

/// Restores the saved choice at app start; unreadable settings collect nothing.
pub fn initialize(app: &AppHandle) {
    let state = app.state::<Activity>();
    let saved = super::preferences::settings_path(app)
        .and_then(|path| settings::load(&path).map_err(|error| IpcError::new(error.to_string())));
    let saved = match saved {
        Ok(settings) => settings.activity,
        Err(error) => {
            eprintln!("Cannot read activity settings: {}", error.message);
            settings::ActivitySettings::default()
        }
    };
    let (collector, hook) = Collector::restore(saved.consent, saved.paused);
    *lock(&state.collector) = collector;
    execute(&state, hook);
    super::tray::set_activity(app, saved.consent, saved.paused);
}

/// Tray menu: pause or resume.
pub fn toggle_pause(app: &AppHandle) {
    let state = app.state::<Activity>();
    let (consent, paused) = {
        let collector = lock(&state.collector);
        (collector.consent(), collector.paused())
    };
    if consent && let Err(error) = change(app, &state, None, Some(!paused)) {
        eprintln!("Cannot save activity pause: {}", error.message);
    }
}

#[tauri::command]
pub fn activity_state(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Activity>,
) -> Result<ActivityState, IpcError> {
    super::preferences::main_only(&window)?;
    Ok(snapshot(&app, &state))
}

#[tauri::command]
pub fn activity_set(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Activity>,
    consent: Option<bool>,
    paused: Option<bool>,
) -> Result<ActivityState, IpcError> {
    super::preferences::main_only(&window)?;
    change(&app, &state, consent, paused)
}

/// The daemon's rules as the main window received them; `null` drops titles.
#[tauri::command]
pub fn activity_configure(
    window: WebviewWindow,
    state: State<'_, Activity>,
    rules: Option<PrivacyRules>,
    idle_threshold_ms: Option<i64>,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    lock(&state.collector).configure(rules.as_ref());
    if let Some(ms) =
        idle_threshold_ms.filter(|ms| (MIN_IDLE_THRESHOLD_MS..=MAX_IDLE_THRESHOLD_MS).contains(ms))
    {
        state.threshold_ms.store(ms, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn activity_drain(
    window: WebviewWindow,
    state: State<'_, Activity>,
    max: usize,
) -> Result<Drained, IpcError> {
    super::preferences::main_only(&window)?;
    Ok(Drained {
        run_id: state.run_id.clone(),
        batch: lock(&state.collector).drain(max.min(MAX_EVENTS)),
    })
}

#[tauri::command]
pub fn activity_ack(
    window: WebviewWindow,
    state: State<'_, Activity>,
    upto: u64,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    lock(&state.collector).ack(upto);
    Ok(())
}

#[tauri::command]
pub fn activity_tray(
    app: AppHandle,
    window: WebviewWindow,
    storage_failed: bool,
    pending: u32,
) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    super::tray::set_activity_storage(&app, storage_failed, pending);
    Ok(())
}
