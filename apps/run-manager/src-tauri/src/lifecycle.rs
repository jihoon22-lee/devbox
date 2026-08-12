use serde::Serialize;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

const SCHEDULER_TICK: Duration = Duration::from_secs(1);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub background_launch: bool,
    pub scheduler_running: bool,
    pub shutdown_requested: bool,
    pub database_path: String,
}

/// Process-wide lifecycle state. Later scheduler PRs replace the idle tick body
/// while preserving this shutdown contract.
pub struct RuntimeState {
    database_path: PathBuf,
    background_launch: bool,
    scheduler_running: AtomicBool,
    scheduler_stopped: AtomicBool,
    shutdown_requested: AtomicBool,
    exit_authorized: AtomicBool,
    shutdown_notify: Notify,
}

impl RuntimeState {
    pub fn new(database_path: PathBuf, background_launch: bool) -> Self {
        Self {
            database_path,
            background_launch,
            scheduler_running: AtomicBool::new(false),
            scheduler_stopped: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
            exit_authorized: AtomicBool::new(false),
            shutdown_notify: Notify::new(),
        }
    }

    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            background_launch: self.background_launch,
            scheduler_running: self.scheduler_running.load(Ordering::Acquire),
            shutdown_requested: self.shutdown_requested.load(Ordering::Acquire),
            database_path: self.database_path.display().to_string(),
        }
    }

    pub fn request_shutdown(&self) -> bool {
        let first_request = !self.shutdown_requested.swap(true, Ordering::AcqRel);
        if first_request {
            self.shutdown_notify.notify_one();
        }
        first_request
    }

    pub fn exit_authorized(&self) -> bool {
        self.exit_authorized.load(Ordering::Acquire)
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }
}

pub fn is_background_launch(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == OsStr::new("--background"))
}

pub fn spawn_idle_scheduler(state: Arc<RuntimeState>) {
    tauri::async_runtime::spawn(async move {
        state.scheduler_running.store(true, Ordering::Release);
        let mut tick = tokio::time::interval(SCHEDULER_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while !state.shutdown_requested() {
            tokio::select! {
                biased;
                _ = state.shutdown_notify.notified() => {}
                _ = tick.tick() => {}
            }
        }
        state.scheduler_running.store(false, Ordering::Release);
        state.scheduler_stopped.store(true, Ordering::Release);
    });
}

fn wait_for_shutdown(state: &RuntimeState) {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while !state.scheduler_stopped.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    state.exit_authorized.store(true, Ordering::Release);
}

/// Starts the idempotent orderly-exit path. The scheduler gets one full tick to
/// observe shutdown before the app exits; later PRs extend this coordinator with
/// process-tree termination and log flushing.
pub fn request_orderly_exit(app: &AppHandle, state: Arc<RuntimeState>) {
    if !state.request_shutdown() {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state_for_wait = state.clone();
        let _ = tokio::task::spawn_blocking(move || wait_for_shutdown(&state_for_wait)).await;
        app.exit(0);
    });
}

/// Completes the same shutdown boundary synchronously for WM_ENDSESSION.
/// Windows only lets the event loop proceed after this function returns.
#[cfg(target_os = "windows")]
pub fn complete_system_shutdown(state: &RuntimeState) {
    state.request_shutdown();
    wait_for_shutdown(state);
}

pub fn show_main_window(app: &AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

pub fn hide_main_window(app: &AppHandle) -> Result<(), String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_string())?
        .hide()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_flag_must_be_an_exact_argument() {
        assert!(is_background_launch(&[
            OsString::from("run-manager"),
            OsString::from("--background"),
        ]));
        assert!(!is_background_launch(&[
            OsString::from("run-manager"),
            OsString::from("--background=true"),
        ]));
    }

    #[test]
    fn shutdown_request_is_idempotent() {
        let state = RuntimeState::new(PathBuf::from("data.db"), false);
        assert!(state.request_shutdown());
        assert!(!state.request_shutdown());
        assert!(state.status().shutdown_requested);
    }
}
