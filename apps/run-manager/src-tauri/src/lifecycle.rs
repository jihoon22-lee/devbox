use crate::scheduler::{SchedulerCoordinator, SchedulerError};
use serde::Serialize;
use std::ffi::{OsStr, OsString};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(target_os = "windows")]
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

// The production scheduler uses a five-second per-run/global termination
// budget. Keep the synchronous OS hook bounded with margin, while the normal
// async exit path remains fail-closed and retries until cleanup is confirmed.
#[cfg(target_os = "windows")]
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub background_launch: bool,
    pub scheduler_running: bool,
    pub shutdown_requested: bool,
    pub database_path: String,
}

pub struct RuntimeState {
    database_path: PathBuf,
    background_launch: bool,
    coordinator: SchedulerCoordinator,
    scheduler_task: Mutex<Option<tokio::task::JoinHandle<Result<(), SchedulerError>>>>,
    maintenance_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    scheduler_running: AtomicBool,
    scheduler_stopped: AtomicBool,
    maintenance_stopped: AtomicBool,
    shutdown_requested: AtomicBool,
    exit_authorized: AtomicBool,
    shutdown_notify: Notify,
}

impl RuntimeState {
    pub fn new(
        database_path: PathBuf,
        background_launch: bool,
        coordinator: SchedulerCoordinator,
    ) -> Self {
        Self {
            database_path,
            background_launch,
            coordinator,
            scheduler_task: Mutex::new(None),
            maintenance_task: Mutex::new(None),
            scheduler_running: AtomicBool::new(false),
            scheduler_stopped: AtomicBool::new(true),
            maintenance_stopped: AtomicBool::new(true),
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
        self.coordinator.request_shutdown();
        let first_request = !self.shutdown_requested.swap(true, Ordering::AcqRel);
        if first_request {
            self.shutdown_notify.notify_waiters();
        }
        first_request
    }

    pub fn exit_authorized(&self) -> bool {
        self.exit_authorized.load(Ordering::Acquire)
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }

    fn authorize_exit(&self) {
        self.exit_authorized.store(true, Ordering::Release);
    }

    pub fn coordinator(&self) -> SchedulerCoordinator {
        self.coordinator.clone()
    }

    pub fn install_scheduler_task(
        &self,
        task: tokio::task::JoinHandle<Result<(), SchedulerError>>,
    ) {
        self.scheduler_stopped.store(false, Ordering::Release);
        if let Ok(mut slot) = self.scheduler_task.lock() {
            *slot = Some(task);
        }
    }

    pub fn install_maintenance_task(&self, task: tokio::task::JoinHandle<()>) {
        self.maintenance_stopped.store(false, Ordering::Release);
        if let Ok(mut slot) = self.maintenance_task.lock() {
            *slot = Some(task);
        }
    }

    fn take_scheduler_task(&self) -> Option<tokio::task::JoinHandle<Result<(), SchedulerError>>> {
        self.scheduler_task.lock().ok()?.take()
    }

    fn take_maintenance_task(&self) -> Option<tokio::task::JoinHandle<()>> {
        self.maintenance_task.lock().ok()?.take()
    }

    pub async fn wait_for_shutdown(&self) {
        self.shutdown_notify.notified().await;
    }

    pub async fn cleanup_confirmed(&self) -> bool {
        self.coordinator.cleanup_confirmed().await
    }

    #[cfg(target_os = "windows")]
    fn cleanup_confirmed_sync(&self) -> bool {
        self.coordinator.cleanup_confirmed_sync()
    }
}

pub fn is_background_launch(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == OsStr::new("--background"))
}

/// Tauri invokes setup synchronously, so these startup tasks cannot rely on a
/// Tokio runtime being entered on the current thread. Use Tauri's configured
/// runtime handle while retaining Tokio join handles for orderly shutdown.
fn spawn_runtime_task<F>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tauri::async_runtime::handle().inner().spawn(task)
}

pub fn spawn_scheduler(state: Arc<RuntimeState>) {
    state.scheduler_running.store(true, Ordering::Release);
    let task_state = Arc::clone(&state);
    let task = spawn_runtime_task(async move {
        let result = task_state.coordinator.run_until_shutdown().await;
        task_state.scheduler_running.store(false, Ordering::Release);
        task_state.scheduler_stopped.store(true, Ordering::Release);
        result
    });
    state.install_scheduler_task(task);
}

pub fn spawn_maintenance(
    state: Arc<RuntimeState>,
    database: Arc<crate::storage::DatabaseState>,
    app: AppHandle,
    data_dir: PathBuf,
) {
    let task_state = Arc::clone(&state);
    let task = spawn_runtime_task(async move {
        let cleaner = crate::cleanup::RetentionCleaner::new(&data_dir);
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        while !task_state.shutdown_requested() {
            tokio::select! {
                biased;
                _ = task_state.shutdown_notify.notified() => {}
                _ = tick.tick() => {}
            }
            if task_state.shutdown_requested() {
                break;
            }
            let now = crate::storage::current_epoch_millis();
            let _ = cleaner.run(&database, now);
            let _ = crate::notifications::drain_pending(&app, &database);
        }
        task_state
            .maintenance_stopped
            .store(true, Ordering::Release);
    });
    state.install_maintenance_task(task);
}

#[cfg(target_os = "windows")]
fn wait_for_shutdown(state: &RuntimeState) -> bool {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while (!state.scheduler_stopped.load(Ordering::Acquire)
        || !state.maintenance_stopped.load(Ordering::Acquire)
        || !state.cleanup_confirmed_sync())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(25));
    }
    let confirmed = state.scheduler_stopped.load(Ordering::Acquire)
        && state.maintenance_stopped.load(Ordering::Acquire)
        && state.cleanup_confirmed_sync();
    if confirmed {
        state.exit_authorized.store(true, Ordering::Release);
    }
    confirmed
}

async fn wait_for_scheduler_cleanup(
    state: &RuntimeState,
    scheduler_task: Option<tokio::task::JoinHandle<Result<(), SchedulerError>>>,
    maintenance_task: Option<tokio::task::JoinHandle<()>>,
) -> bool {
    // Maintenance only observes the shutdown notification and has no process
    // ownership, so it is safe to join it before retrying process cleanup.
    if let Some(task) = maintenance_task {
        let _ = task.await;
    }
    // Never abort the scheduler task: it owns the durable terminal CAS and may
    // still be finishing a bounded adapter termination. The coordinator's
    // semaphore and adapter paths are themselves shutdown-aware.
    if let Some(task) = scheduler_task {
        while !task.is_finished() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let _ = task.await;
    }

    // A timeout/error leaves the handle in the coordinator's fail-closed
    // active set. Retry its bounded termination; only a confirmed empty set
    // allows application exit. This loop intentionally has no unsafe timeout.
    loop {
        if state.cleanup_confirmed().await {
            return true;
        }
        let coordinator = state.coordinator();
        let retry = tokio::time::timeout(
            coordinator.config().shutdown_timeout,
            coordinator.shutdown(),
        )
        .await;
        if retry.is_err() {
            // The adapter call itself exceeded its budget. The next iteration
            // retries without dropping its retained handle.
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Starts the idempotent orderly-exit path. The production coordinator owns
/// process-tree termination and terminal CAS; maintenance is stopped at the
/// same boundary before the app is allowed to exit.
pub fn request_orderly_exit(app: &AppHandle, state: Arc<RuntimeState>) {
    if !state.request_shutdown() {
        return;
    }

    let app = app.clone();
    let scheduler_task = state.take_scheduler_task();
    let maintenance_task = state.take_maintenance_task();
    tauri::async_runtime::spawn(async move {
        if wait_for_scheduler_cleanup(&state, scheduler_task, maintenance_task).await {
            state.authorize_exit();
            app.exit(0);
        }
    });
}

/// Completes the same shutdown boundary synchronously for WM_ENDSESSION.
/// Windows only lets the event loop proceed after this function returns.
#[cfg(target_os = "windows")]
pub fn complete_system_shutdown(state: &RuntimeState) {
    state.request_shutdown();
    let _ = wait_for_shutdown(state);
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
        let database =
            std::sync::Arc::new(crate::storage::DatabaseState::open_in_memory().unwrap());
        let coordinator = SchedulerCoordinator::new(
            database,
            std::sync::Arc::new(crate::scheduler::UnavailableExecutionAdapter),
        );
        let state = RuntimeState::new(PathBuf::from("data.db"), false, coordinator);
        assert!(state.request_shutdown());
        assert!(!state.request_shutdown());
        assert!(state.status().shutdown_requested);
    }

    #[test]
    fn scheduler_starts_from_setup_without_a_current_tokio_runtime() {
        let database =
            std::sync::Arc::new(crate::storage::DatabaseState::open_in_memory().unwrap());
        let coordinator = SchedulerCoordinator::with_config(
            database,
            std::sync::Arc::new(crate::scheduler::UnavailableExecutionAdapter),
            crate::scheduler::SchedulerConfig::default()
                .with_tick_interval(Duration::from_millis(1)),
        );
        let state = std::sync::Arc::new(RuntimeState::new(
            PathBuf::from("data.db"),
            false,
            coordinator,
        ));

        // Tauri invokes setup synchronously, before a Tokio runtime is entered.
        // This must not panic at the startup boundary.
        spawn_scheduler(std::sync::Arc::clone(&state));
        state.request_shutdown();

        let task = state
            .take_scheduler_task()
            .expect("scheduler task should be installed");
        let result = tauri::async_runtime::block_on(task).expect("scheduler task should join");
        assert!(result.is_ok());
        assert!(state.scheduler_stopped.load(Ordering::Acquire));
    }
}
