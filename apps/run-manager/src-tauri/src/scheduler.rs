//! Phase 1 job scheduler coordinator.
//!
//! The coordinator owns orchestration, not platform process creation.  A
//! platform implementation supplies [`ExecutionAdapter`] and a mockable
//! [`ExecutionHandle`]; the scheduler commits a durable run/CAS before it
//! calls that boundary.  This keeps the 1-second tick, startup recovery,
//! overlap policy, queue ordering, and orderly shutdown testable on every
//! target without importing Windows or WSL APIs.

use crate::core::cron::CronSchedule;
use crate::core::models::{
    Job, RestartPolicy, Run, RunExecutionMetadata, RunStatus, ServiceInstance, ServiceInstanceState,
};
use crate::core::policies::select_occurrences;
use crate::storage::{
    current_epoch_millis, ClaimedRunAction, DatabaseState, PolicyClaim, StorageError,
};
use chrono::{Local, TimeZone};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{watch, Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

pub const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub const DEFAULT_MAX_CONCURRENT_RUNS: usize = 4;
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DUE_BATCH_SIZE: usize = 1024;

/// Service restart backoff steps, indexed by consecutive failure count and
/// capped at 30 seconds (mirrors the code-pad LSP restart policy).
const SERVICE_RESTART_DELAYS_MS: [i64; 6] = [1_000, 2_000, 4_000, 8_000, 16_000, 30_000];
/// Service health probe cadence and failure threshold.
const SERVICE_HEALTH_INTERVAL: Duration = Duration::from_secs(10);
const SERVICE_HEALTH_FAILURE_LIMIT: i64 = 3;

fn service_restart_delay_ms(consecutive_failures: i64) -> i64 {
    let index = (consecutive_failures.max(0) as usize).min(SERVICE_RESTART_DELAYS_MS.len() - 1);
    SERVICE_RESTART_DELAYS_MS[index]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduledOccurrence {
    timestamp: i64,
    wall_key: String,
}

/// Boxed async result used by adapter implementors without adding an
/// `async-trait` dependency to the app.  Implementations can return
/// `Box::pin(async move { ... })` from each method.
pub type AdapterFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AdapterError>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterError {
    pub message: String,
    /// A failed adapter result may still be a cleanup witness.  For example,
    /// log persistence can fail after the process tree has already been
    /// confirmed gone.  Callers must never infer this from the error code.
    pub cleanup_confirmed: bool,
}

impl AdapterError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cleanup_confirmed: false,
        }
    }

    /// Construct a sanitized failure after the adapter has independently
    /// confirmed that the complete process tree is gone.
    pub fn confirmed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cleanup_confirmed: true,
        }
    }

    pub fn with_cleanup_confirmed(mut self) -> Self {
        self.cleanup_confirmed = true;
        self
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AdapterError {}

/// The input passed after a durable `queued -> starting` CAS succeeds.
/// `owner_instance_id` and `attempt_token` must be echoed by the adapter
/// monitor when it reports a terminal result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    pub job: Job,
    pub run: Run,
    pub owner_instance_id: String,
    pub attempt_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionExit {
    pub exit_code: Option<i32>,
}

/// A platform process/tree handle.  `terminate` is a confirmation boundary:
/// successful completion means the adapter has verified the process and all
/// descendants are gone.  Both methods must be safe for an adapter to observe
/// concurrently because the monitor may already be waiting when
/// kill-previous or shutdown requests termination.
pub trait ExecutionHandle: Send + Sync {
    fn terminate(&self) -> AdapterFuture<'_, ExecutionExit>;
    fn wait(&self) -> AdapterFuture<'_, ExecutionExit>;

    /// Metadata is captured before the adapter returns. The default keeps
    /// existing pure scheduler fixtures source-compatible; production
    /// handles override it with their app-owned log/identity snapshot.
    fn metadata(&self) -> RunExecutionMetadata {
        RunExecutionMetadata::default()
    }
}

/// Platform-neutral spawn boundary.  The scheduler never invokes
/// `std::process::Command`, `wsl.exe`, or a shell itself.
pub trait ExecutionAdapter: Send + Sync {
    fn spawn(&self, request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>>;

    /// Reconcile a non-terminal row left by a previous process. Production
    /// adapters exact-check the persisted identity and clean the old tree
    /// before the scheduler writes stale-recovery. The default fixture path is
    /// intentionally a no-op so pure scheduler tests do not need a platform.
    fn recover_stale(&self, _request: ExecutionRequest) -> AdapterFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// Fixed failure categories emitted after a durable terminal transition. The
/// scheduler never forwards adapter text to the notification/UI boundary;
/// listeners receive one of these stable codes instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFailureCode {
    SpawnFailed,
    NonzeroExit,
    LogWriteFailed,
    EnvironmentUnavailable,
    TerminationTimeout,
    WslUnavailable,
    ProcessCrashed,
    StorageFailed,
}

impl TerminalFailureCode {
    pub const fn as_db_message(self) -> &'static str {
        match self {
            Self::SpawnFailed => "spawn-failed",
            Self::NonzeroExit => "nonzero-exit",
            Self::LogWriteFailed => "log-write-failed",
            Self::EnvironmentUnavailable => "environment-unavailable",
            Self::TerminationTimeout => "termination-timeout",
            Self::WslUnavailable => "wsl-unavailable",
            Self::ProcessCrashed => "process-crashed",
            Self::StorageFailed => "storage-failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRunEvent {
    pub job_id: String,
    pub run_id: String,
    pub status: RunStatus,
    pub failure_code: Option<TerminalFailureCode>,
}

/// Receives sanitized terminal transitions. Delivery is deliberately
/// synchronous and non-fatal: a listener may enqueue a durable outbox row or
/// attempt a native notification, but it must never roll back the run.
pub trait TerminalRunListener: Send + Sync {
    fn on_terminal(&self, event: TerminalRunEvent);
}

#[derive(Debug, Default)]
struct NoopTerminalRunListener;

impl TerminalRunListener for NoopTerminalRunListener {
    fn on_terminal(&self, _event: TerminalRunEvent) {}
}

/// A safe default for runtime wiring before a platform adapter is installed.
/// It fails runs durably rather than silently pretending that a process was
/// started.
#[derive(Debug, Default)]
pub struct UnavailableExecutionAdapter;

impl ExecutionAdapter for UnavailableExecutionAdapter {
    fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
        Box::pin(async {
            Err(AdapterError::new(
                "no platform execution adapter is installed",
            ))
        })
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub tick_interval: Duration,
    pub max_concurrent_runs: usize,
    pub startup_cutoff: i64,
    pub owner_instance_id: String,
    pub shutdown_timeout: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval: SCHEDULER_TICK,
            max_concurrent_runs: DEFAULT_MAX_CONCURRENT_RUNS,
            startup_cutoff: current_epoch_millis(),
            owner_instance_id: Uuid::new_v4().to_string(),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

impl SchedulerConfig {
    pub fn with_tick_interval(mut self, tick_interval: Duration) -> Self {
        self.tick_interval = tick_interval.max(Duration::from_millis(1));
        self
    }

    pub fn with_max_concurrent_runs(mut self, max_concurrent_runs: usize) -> Self {
        self.max_concurrent_runs = max_concurrent_runs.max(1);
        self
    }

    pub fn with_startup_cutoff(mut self, startup_cutoff: i64) -> Self {
        self.startup_cutoff = startup_cutoff;
        self
    }

    pub fn with_owner_instance_id(mut self, owner_instance_id: impl Into<String>) -> Self {
        self.owner_instance_id = owner_instance_id.into();
        self
    }

    pub fn with_shutdown_timeout(mut self, shutdown_timeout: Duration) -> Self {
        self.shutdown_timeout = shutdown_timeout;
        self
    }
}

#[derive(Debug)]
pub enum SchedulerError {
    Storage(StorageError),
    Cron(String),
    Adapter {
        run_id: String,
        source: AdapterError,
    },
    Join(String),
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(formatter),
            Self::Cron(error) => write!(formatter, "scheduler cron evaluation failed: {error}"),
            Self::Adapter { run_id, source } => {
                write!(formatter, "adapter failed for run {run_id}: {source}")
            }
            Self::Join(error) => write!(formatter, "scheduler task failed: {error}"),
        }
    }
}

impl std::error::Error for SchedulerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Adapter { source, .. } => Some(source),
            Self::Cron(_) | Self::Join(_) => None,
        }
    }
}

impl From<StorageError> for SchedulerError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

struct ActiveExecution {
    job_id: String,
    owner_instance_id: String,
    attempt_token: String,
    handle: Arc<dyn ExecutionHandle>,
    /// True only after the adapter has confirmed the complete process tree is
    /// gone. This lets a retry repair a terminal CAS failure without asking a
    /// finished actor to terminate a second time.
    cleanup_confirmed: bool,
    _permit: OwnedSemaphorePermit,
}

#[derive(Clone)]
struct PendingTerminal {
    status: RunStatus,
    exit_code: Option<i32>,
    error_message: Option<String>,
    failure_code: Option<TerminalFailureCode>,
}

/// A synchronous ownership ledger protects active executions while shutdown
/// termination futures are polled in spawned tasks. If a task panics or is
/// cancelled before returning its result, the guard's `Drop` puts the handle
/// back in the ledger. The ledger itself has a synchronous orphan sink owned
/// by the coordinator, so cancelling the enclosing shutdown future cannot
/// drop the last process witness before the next retry.
struct ShutdownExecutionLedger {
    entries: StdMutex<HashMap<String, ActiveExecution>>,
    orphan_sink: Arc<StdMutex<HashMap<String, ActiveExecution>>>,
}

impl ShutdownExecutionLedger {
    fn new(orphan_sink: Arc<StdMutex<HashMap<String, ActiveExecution>>>) -> Arc<Self> {
        Arc::new(Self {
            entries: StdMutex::new(HashMap::new()),
            orphan_sink,
        })
    }

    fn insert(&self, run_id: String, execution: ActiveExecution) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(run_id, execution);
    }

    fn take(&self, run_id: &str) -> Option<ActiveExecution> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.remove(run_id)
    }

    fn drain(&self) -> Vec<(String, ActiveExecution)> {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.drain().collect()
    }
}

impl Drop for ShutdownExecutionLedger {
    fn drop(&mut self) {
        let entries = self
            .entries
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.is_empty() {
            return;
        }
        let mut orphan_sink = self
            .orphan_sink
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        orphan_sink.extend(entries.drain());
    }
}

struct ShutdownExecutionGuard {
    run_id: String,
    ledger: Arc<ShutdownExecutionLedger>,
    execution: Option<ActiveExecution>,
}

type StopResultSender = watch::Sender<Option<Result<Run, String>>>;

/// A manual-stop task is intentionally detached from the command future so a
/// dropped IPC request cannot cancel cleanup. If the task is aborted or
/// panics, this synchronous lease releases the stop slot and publishes a
/// fixed failure; the durable row remains `stopping` and the next stop call
/// retries the retained active handle.
struct StopOperationGuard {
    inner: Arc<SchedulerInner>,
    run_id: String,
    job_id: String,
    result_tx: StopResultSender,
    completed: bool,
}

impl StopOperationGuard {
    fn new(
        inner: Arc<SchedulerInner>,
        run_id: String,
        job_id: String,
        result_tx: StopResultSender,
    ) -> Self {
        Self {
            inner,
            run_id,
            job_id,
            result_tx,
            completed: false,
        }
    }

    fn complete(&mut self, result: Result<Run, String>) {
        self.finish(result, false);
    }

    fn complete_cleanup_confirmed(&mut self, result: Result<Run, String>) {
        self.finish(result, true);
    }

    fn finish(&mut self, result: Result<Run, String>, cleanup_confirmed: bool) {
        let _ = self.result_tx.send(Some(result));
        self.inner
            .stops
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.run_id);
        let mut recovery = self
            .inner
            .stop_recovery_required
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cleanup_confirmed {
            recovery.remove(&self.run_id);
        } else {
            recovery.insert(self.run_id.clone(), self.job_id.clone());
        }
        self.completed = true;
    }
}

impl Drop for StopOperationGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let _ = self
            .result_tx
            .send(Some(Err("run-stop-failed".to_string())));
        self.inner
            .stops
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.run_id);
        self.inner
            .stop_recovery_required
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(self.run_id.clone(), self.job_id.clone());
    }
}

impl Drop for ShutdownExecutionGuard {
    fn drop(&mut self) {
        if let Some(execution) = self.execution.take() {
            self.ledger.insert(self.run_id.clone(), execution);
        }
    }
}

struct SchedulerInner {
    database: Arc<DatabaseState>,
    adapter: Arc<dyn ExecutionAdapter>,
    terminal_listener: Arc<dyn TerminalRunListener>,
    config: SchedulerConfig,
    permits: Arc<Semaphore>,
    job_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    active: Mutex<HashMap<String, ActiveExecution>>,
    stops: StdMutex<HashMap<String, StopResultSender>>,
    stop_recovery_required: StdMutex<HashMap<String, String>>,
    pending_terminal: Mutex<HashMap<String, PendingTerminal>>,
    cleanup_pending: Mutex<std::collections::HashSet<String>>,
    shutdown_orphans: Arc<StdMutex<HashMap<String, ActiveExecution>>>,
    shutdown_requested: AtomicBool,
    shutdown_notify: Notify,
}

/// Cloneable scheduler coordinator.  `run_until_shutdown` is normally started
/// once from Tauri setup; `tick_at` and `trigger_manual_at` are public pure-
/// clock entry points for integration tests and command wiring.
#[derive(Clone)]
pub struct SchedulerCoordinator {
    inner: Arc<SchedulerInner>,
}

pub struct SchedulerTask {
    coordinator: SchedulerCoordinator,
    join: JoinHandle<Result<(), SchedulerError>>,
}

impl SchedulerTask {
    pub async fn shutdown(self) -> Result<(), SchedulerError> {
        self.coordinator.request_shutdown();
        self.join
            .await
            .map_err(|error| SchedulerError::Join(error.to_string()))?
    }
}

impl SchedulerCoordinator {
    pub fn new(database: Arc<DatabaseState>, adapter: Arc<dyn ExecutionAdapter>) -> Self {
        Self::with_config(database, adapter, SchedulerConfig::default())
    }

    pub fn with_config(
        database: Arc<DatabaseState>,
        adapter: Arc<dyn ExecutionAdapter>,
        config: SchedulerConfig,
    ) -> Self {
        Self::with_config_and_listener(database, adapter, config, Arc::new(NoopTerminalRunListener))
    }

    pub fn with_terminal_listener(
        database: Arc<DatabaseState>,
        adapter: Arc<dyn ExecutionAdapter>,
        listener: Arc<dyn TerminalRunListener>,
    ) -> Self {
        Self::with_config_and_listener(database, adapter, SchedulerConfig::default(), listener)
    }

    pub fn with_config_and_listener(
        database: Arc<DatabaseState>,
        adapter: Arc<dyn ExecutionAdapter>,
        mut config: SchedulerConfig,
        terminal_listener: Arc<dyn TerminalRunListener>,
    ) -> Self {
        if config.owner_instance_id.trim().is_empty() {
            config.owner_instance_id = Uuid::new_v4().to_string();
        }
        config.max_concurrent_runs = config.max_concurrent_runs.max(1);
        config.tick_interval = config.tick_interval.max(Duration::from_millis(1));
        let permits = Arc::new(Semaphore::new(config.max_concurrent_runs));
        Self {
            inner: Arc::new(SchedulerInner {
                database,
                adapter,
                terminal_listener,
                config,
                permits,
                job_locks: Mutex::new(HashMap::new()),
                active: Mutex::new(HashMap::new()),
                stops: StdMutex::new(HashMap::new()),
                stop_recovery_required: StdMutex::new(HashMap::new()),
                pending_terminal: Mutex::new(HashMap::new()),
                cleanup_pending: Mutex::new(std::collections::HashSet::new()),
                shutdown_orphans: Arc::new(StdMutex::new(HashMap::new())),
                shutdown_requested: AtomicBool::new(false),
                shutdown_notify: Notify::new(),
            }),
        }
    }

    pub fn config(&self) -> &SchedulerConfig {
        &self.inner.config
    }

    pub fn request_shutdown(&self) -> bool {
        let first = !self.inner.shutdown_requested.swap(true, Ordering::AcqRel);
        if first {
            self.inner.shutdown_notify.notify_waiters();
        }
        first
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.inner.shutdown_requested.load(Ordering::Acquire)
    }

    pub fn spawn(&self) -> SchedulerTask {
        let coordinator = self.clone();
        let task_coordinator = coordinator.clone();
        let join = tokio::spawn(async move { task_coordinator.run_until_shutdown().await });
        SchedulerTask { coordinator, join }
    }

    /// Run the fixed one-second orchestration loop until shutdown is requested.
    /// Recovery runs before startup and on every later tick so fail-closed
    /// orphan rows get another exact-identity cleanup opportunity.
    pub async fn run_until_shutdown(&self) -> Result<(), SchedulerError> {
        // Recovery is retried at every tick. A platform cleanup failure keeps
        // the row blocked and must not terminate the daemon (or turn the
        // shutdown path into an infinite retry that never invokes recovery).
        let _ = self.recover_stale_at(current_epoch_millis()).await;
        // Bring up auto-start services once recovery has settled the durable
        // state. A failing service must not prevent the scheduler from running.
        let _ = self.auto_start_services(current_epoch_millis()).await;
        let mut interval = tokio::time::interval(self.inner.config.tick_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut last_health_check = current_epoch_millis();
        loop {
            tokio::select! {
                biased;
                _ = self.inner.shutdown_notify.notified() => break,
                _ = interval.tick() => {
                    if self.is_shutdown_requested() {
                        break;
                    }
                    let now = current_epoch_millis();
                    let _ = self.recover_stale_at(now).await;
                    let _ = self.supervise_services(now).await;
                    if now - last_health_check >= SERVICE_HEALTH_INTERVAL.as_millis() as i64 {
                        last_health_check = now;
                        let _ = self.run_service_health_checks(now).await;
                    }
                    // A malformed/tampered job must not kill the daemon. The
                    // next tick retries it while other jobs continue.
                    let _ = self.tick_at(now).await;
                }
            }
        }
        self.shutdown_active_runs().await
    }

    /// Execute one deterministic scheduler tick. Jobs are evaluated in
    /// parallel, but each job's orchestration is serialized by its own mutex
    /// and all actual starts consume the global semaphore.
    pub async fn tick_at(&self, now: i64) -> Result<(), SchedulerError> {
        if self.is_shutdown_requested() {
            return Ok(());
        }
        let jobs = self.inner.database.list_enabled_jobs()?;
        let mut tasks = Vec::with_capacity(jobs.len());
        for job in jobs {
            let coordinator = self.clone();
            tasks.push(tokio::spawn(async move {
                coordinator.process_job_at(job, now).await
            }));
        }

        let mut first_error = None;
        for task in tasks {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
                Ok(Err(_)) => {}
                Err(error) if first_error.is_none() => {
                    first_error = Some(SchedulerError::Join(error.to_string()))
                }
                Err(_) => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Trigger a manual run through the exact same claim/overlap/adapter path
    /// used by scheduled occurrences.  The returned row is refreshed after a
    /// successful immediate start so callers can observe `running`.
    pub async fn trigger_manual_at(&self, job_id: &str, now: i64) -> Result<Run, SchedulerError> {
        let job = self
            .inner
            .database
            .get_job(job_id)?
            .ok_or_else(|| StorageError::NotFound(format!("job {job_id}")))?;
        let lock = self.job_mutex(job_id).await;
        let _guard = lock.lock().await;
        if self.is_cleanup_pending(job_id).await {
            return Err(SchedulerError::Adapter {
                run_id: job_id.to_string(),
                source: AdapterError::new("termination-unverified"),
            });
        }
        let claim = self.inner.database.claim_manual_run(job_id, now)?;
        self.process_claim_locked(&job, claim.clone(), now).await?;
        self.inner
            .database
            .get_run(&claim.run.id)?
            .ok_or_else(|| StorageError::NotFound(format!("run {}", claim.run.id)).into())
    }

    /// Stop the current process run for one job. The database `running` /
    /// `starting` -> `stopping` CAS happens before the adapter boundary, and
    /// the row is marked cancelled only after the adapter confirms the whole
    /// process tree is gone. A missing in-memory handle is fail-closed: the
    /// coordinator never guesses at a PID from a durable row.
    pub async fn stop_active_at(
        &self,
        job_id: &str,
        now: i64,
    ) -> Result<Option<Run>, SchedulerError> {
        let lock = self.job_mutex(job_id).await;
        let (run_id, mut result_rx, start_operation) = {
            let _guard = lock.lock().await;
            self.restore_shutdown_orphans_for_job(job_id).await;
            let pending_cleanup = self.is_cleanup_pending(job_id).await;
            let active_run = match self.inner.database.active_process_run(job_id)? {
                Some(run) => run,
                None if !pending_cleanup => return Ok(None),
                None => {
                    let pending = self
                        .inner
                        .active
                        .lock()
                        .await
                        .iter()
                        .find(|(_, execution)| execution.job_id == job_id)
                        .map(|(run_id, _)| run_id.clone())
                        .ok_or_else(|| SchedulerError::Adapter {
                            run_id: job_id.to_string(),
                            source: AdapterError::new("termination-unverified"),
                        })?;
                    self.inner
                        .database
                        .get_run(&pending)?
                        .ok_or_else(|| SchedulerError::Storage(StorageError::NotFound(pending)))?
                }
            };
            let existing_stop = self
                .inner
                .stops
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .get(&active_run.id)
                .cloned();
            if let Some(sender) = existing_stop {
                (active_run.id.clone(), sender.subscribe(), None)
            } else {
                let active = self
                    .inner
                    .active
                    .lock()
                    .await
                    .get(&active_run.id)
                    .map(|execution| {
                        (
                            execution.owner_instance_id.clone(),
                            execution.attempt_token.clone(),
                            Arc::clone(&execution.handle),
                            execution.cleanup_confirmed,
                        )
                    })
                    .ok_or_else(|| SchedulerError::Adapter {
                        run_id: active_run.id.clone(),
                        source: AdapterError::new("active-handle-unavailable"),
                    })?;
                let (sender, receiver) = watch::channel(None);
                self.inner
                    .stops
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(active_run.id.clone(), sender.clone());
                (
                    active_run.id.clone(),
                    receiver,
                    Some((
                        active_run.id,
                        job_id.to_string(),
                        active.0,
                        active.1,
                        active.2,
                        active.3,
                        sender,
                        now,
                        pending_cleanup,
                    )),
                )
            }
        };

        if let Some((
            run_id,
            job_id,
            owner,
            attempt_token,
            handle,
            cleanup_confirmed,
            result_tx,
            now,
            pending_cleanup,
        )) = start_operation
        {
            let coordinator = self.clone();
            tokio::spawn(async move {
                coordinator
                    .complete_stop_operation(
                        run_id,
                        job_id,
                        owner,
                        attempt_token,
                        handle,
                        cleanup_confirmed,
                        result_tx,
                        now,
                        pending_cleanup,
                    )
                    .await;
            });
        }

        loop {
            if let Some(result) = result_rx.borrow().clone() {
                return result.map(Some).map_err(|_| SchedulerError::Adapter {
                    run_id,
                    source: AdapterError::new("run-stop-failed"),
                });
            }
            result_rx
                .changed()
                .await
                .map_err(|_| SchedulerError::Adapter {
                    run_id: run_id.clone(),
                    source: AdapterError::new("run-stop-failed"),
                })?;
        }
    }

    /// Return the durable active process row for command/UI polling. This is
    /// a read-only snapshot and never exposes environment ciphertext.
    pub fn active_run(&self, job_id: &str) -> Result<Option<Run>, SchedulerError> {
        self.inner.database.active_run(job_id).map_err(Into::into)
    }

    pub fn active_process_run(&self, job_id: &str) -> Result<Option<Run>, SchedulerError> {
        self.inner
            .database
            .active_process_run(job_id)
            .map_err(Into::into)
    }

    pub fn active_process_runs(&self) -> Result<Vec<Run>, SchedulerError> {
        self.inner
            .database
            .list_active_process_runs()
            .map_err(Into::into)
    }

    /// Start a service through its durable single-instance claim. A `stopped`
    /// instance moves to a fresh generation, spawns a manual process run, and
    /// links that run into `active_run_id` once the process handshake succeeds.
    pub async fn start_service_at(
        &self,
        service_id: &str,
        now: i64,
    ) -> Result<ServiceInstance, SchedulerError> {
        let service = self
            .inner
            .database
            .get_service(service_id)?
            .ok_or_else(|| StorageError::NotFound(format!("service {service_id}")))?;
        let owner = self.inner.config.owner_instance_id.clone();
        let attempt_token = Uuid::new_v4().to_string();
        let lock = self.job_mutex(service_id).await;
        let _guard = lock.lock().await;
        if self.is_cleanup_pending(service_id).await {
            return Err(service_adapter_error(service_id, "termination-unverified"));
        }
        let instance = self
            .inner
            .database
            .claim_service_start(service_id, &owner, &attempt_token, now)?
            .ok_or_else(|| service_adapter_error(service_id, "service-already-running"))?;
        let generation = instance.generation;
        let run = self.inner.database.create_service_run_at(service_id, now)?;
        let handle = match self.start_owned_claim(&service, &run).await {
            Ok(Some(handle)) => handle,
            Ok(None) => {
                let _ = self
                    .inner
                    .database
                    .mark_service_stopped(service_id, generation, now);
                return Err(service_adapter_error(service_id, "service-start-cancelled"));
            }
            Err(error) => {
                let _ = self
                    .inner
                    .database
                    .mark_service_stopped(service_id, generation, now);
                return Err(error);
            }
        };
        if !self.inner.database.mark_service_running(
            service_id,
            generation,
            &owner,
            &attempt_token,
            &run.id,
            now,
        )? {
            let _ =
                tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await;
            return Err(service_adapter_error(service_id, "service-start-conflict"));
        }
        self.inner
            .database
            .get_service_instance(service_id)?
            .ok_or_else(|| StorageError::NotFound(format!("service instance {service_id}")).into())
    }

    /// Stop an active service by terminating its linked process run. The
    /// instance returns to `stopped` only after the run reaches a terminal
    /// state; a service with no active run is a no-op.
    pub async fn stop_service_at(
        &self,
        service_id: &str,
        now: i64,
    ) -> Result<Option<ServiceInstance>, SchedulerError> {
        let instance = {
            let lock = self.job_mutex(service_id).await;
            let _guard = lock.lock().await;
            self.inner.database.begin_service_stop(service_id, now)?
        };
        let Some(instance) = instance else {
            return Ok(None);
        };
        let generation = instance.generation;
        let _ = self.stop_active_at(service_id, now).await;
        let _ = self
            .inner
            .database
            .mark_service_stopped(service_id, generation, now);
        Ok(self.inner.database.get_service_instance(service_id)?)
    }

    /// Stop and immediately restart an active service. An already-stopped
    /// service is simply started.
    pub async fn restart_service_at(
        &self,
        service_id: &str,
        now: i64,
    ) -> Result<ServiceInstance, SchedulerError> {
        if self
            .inner
            .database
            .get_service_instance(service_id)?
            .is_some_and(|instance| instance.state != ServiceInstanceState::Stopped)
        {
            let _ = self.stop_service_at(service_id, now).await?;
        }
        self.start_service_at(service_id, now).await
    }

    /// Start every `auto_start` service that is currently stopped. Best-effort:
    /// one failing service must not prevent the others from starting.
    pub async fn auto_start_services(&self, now: i64) -> usize {
        let services = match self.inner.database.list_auto_start_services() {
            Ok(services) => services,
            Err(_) => return 0,
        };
        let mut started = 0;
        for service in services {
            if self.start_service_at(&service.id, now).await.is_ok() {
                started += 1;
            }
        }
        started
    }

    /// Sync terminal hook invoked for every terminal run. For a service run it
    /// decides, from the restart policy, whether to leave the instance stopped
    /// or schedule a backoff retry. Intentional stops (`cancelled`) never
    /// restart.
    fn handle_service_terminal(&self, job_id: &str, run_id: &str, status: RunStatus, now: i64) {
        let Ok(Some(service)) = self.inner.database.get_service(job_id) else {
            return;
        };
        let Ok(Some(instance)) = self.inner.database.get_service_instance(job_id) else {
            return;
        };
        if instance.active_run_id.as_deref() != Some(run_id) {
            return;
        }
        let restart_policy = service.restart_policy.unwrap_or(RestartPolicy::Never);
        let should_restart = match status {
            RunStatus::Cancelled => false,
            RunStatus::Succeeded => restart_policy == RestartPolicy::Always,
            RunStatus::Failed => restart_policy != RestartPolicy::Never,
            _ => false,
        };
        if should_restart {
            let delay = service_restart_delay_ms(instance.consecutive_failures);
            let _ = self.inner.database.mark_service_retry_waiting(
                job_id,
                instance.generation,
                now.saturating_add(delay),
                now,
            );
        } else {
            let _ = self
                .inner
                .database
                .mark_service_stopped(job_id, instance.generation, now);
        }
    }

    /// Restart a `retry_waiting` service whose backoff deadline has elapsed.
    async fn restart_retry_service_at(
        &self,
        service_id: &str,
        now: i64,
    ) -> Result<ServiceInstance, SchedulerError> {
        let service = self
            .inner
            .database
            .get_service(service_id)?
            .ok_or_else(|| StorageError::NotFound(format!("service {service_id}")))?;
        let owner = self.inner.config.owner_instance_id.clone();
        let attempt_token = Uuid::new_v4().to_string();
        let lock = self.job_mutex(service_id).await;
        let _guard = lock.lock().await;
        if self.is_cleanup_pending(service_id).await {
            return Err(service_adapter_error(service_id, "termination-unverified"));
        }
        let instance = self
            .inner
            .database
            .claim_service_retry(service_id, &owner, &attempt_token, now)?
            .ok_or_else(|| service_adapter_error(service_id, "service-retry-unavailable"))?;
        let generation = instance.generation;
        let run = self.inner.database.create_service_run_at(service_id, now)?;
        let handle = match self.start_owned_claim(&service, &run).await {
            Ok(Some(handle)) => handle,
            Ok(None) => {
                let _ = self
                    .inner
                    .database
                    .mark_service_stopped(service_id, generation, now);
                return Err(service_adapter_error(service_id, "service-start-cancelled"));
            }
            Err(error) => {
                let _ = self
                    .inner
                    .database
                    .mark_service_stopped(service_id, generation, now);
                return Err(error);
            }
        };
        if !self.inner.database.mark_service_running(
            service_id,
            generation,
            &owner,
            &attempt_token,
            &run.id,
            now,
        )? {
            let _ =
                tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await;
            return Err(service_adapter_error(service_id, "service-start-conflict"));
        }
        // A successful restart restores the shortest backoff.
        let _ = self
            .inner
            .database
            .reset_service_health(service_id, generation, now);
        self.inner
            .database
            .get_service_instance(service_id)?
            .ok_or_else(|| StorageError::NotFound(format!("service instance {service_id}")).into())
    }

    /// One supervisor pass: restart due retries. Best-effort per service so one
    /// failing service does not block the others.
    pub async fn supervise_services(&self, now: i64) {
        let due = match self.inner.database.list_due_service_retries(now) {
            Ok(due) => due,
            Err(_) => return,
        };
        for instance in due {
            let _ = self.restart_retry_service_at(&instance.job_id, now).await;
        }
    }

    /// One health pass: probe every running service and restart it after the
    /// consecutive-failure limit is reached. Best-effort and non-fatal.
    pub async fn run_service_health_checks(&self, now: i64) {
        let running = match self.inner.database.list_running_services() {
            Ok(running) => running,
            Err(_) => return,
        };
        for (service, instance) in running {
            let healthy = self.service_healthy(&service, &instance).await;
            if healthy {
                let _ = self.inner.database.reset_service_health(
                    &instance.job_id,
                    instance.generation,
                    now,
                );
                continue;
            }
            let Some(failures) = self
                .inner
                .database
                .record_service_health_failure(&instance.job_id, instance.generation, now)
                .ok()
                .flatten()
            else {
                continue;
            };
            if failures >= SERVICE_HEALTH_FAILURE_LIMIT {
                // Unhealthy: schedule a backoff retry and terminate the linked
                // process. Clearing `active_run_id` disconnects the run so the
                // cancellation terminal hook cannot re-enter this instance.
                let delay = service_restart_delay_ms(failures);
                let scheduled = self.inner.database.mark_service_retry_waiting(
                    &instance.job_id,
                    instance.generation,
                    now.saturating_add(delay),
                    now,
                );
                if matches!(scheduled, Ok(true)) {
                    let _ = self.stop_active_at(&instance.job_id, now).await;
                }
            }
        }
    }

    async fn service_healthy(&self, service: &Job, instance: &ServiceInstance) -> bool {
        let Some(active_run_id) = instance.active_run_id.as_deref() else {
            return false;
        };
        let Ok(Some(run)) = self.inner.database.get_run(active_run_id) else {
            return false;
        };
        if !matches!(run.status, RunStatus::Running) {
            return false;
        }
        let (Some(address), Some(port)) = (
            service.health_tcp_address.as_deref(),
            service.health_tcp_port,
        ) else {
            return true;
        };
        let Ok(address) = address.parse::<std::net::IpAddr>() else {
            return false;
        };
        tokio::time::timeout(
            Duration::from_secs(3),
            tokio::net::TcpStream::connect((address, port)),
        )
        .await
        .map(|result| result.is_ok())
        .unwrap_or(false)
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_stop_operation(
        &self,
        run_id: String,
        job_id: String,
        owner: String,
        attempt_token: String,
        handle: Arc<dyn ExecutionHandle>,
        cleanup_confirmed: bool,
        result_tx: watch::Sender<Option<Result<Run, String>>>,
        now: i64,
        pending_cleanup: bool,
    ) {
        let mut stop_guard = StopOperationGuard::new(
            Arc::clone(&self.inner),
            run_id.clone(),
            job_id.clone(),
            result_tx,
        );
        if !pending_cleanup {
            match self
                .inner
                .database
                .request_run_stop(&run_id, &owner, &attempt_token)
            {
                Ok(true) => {}
                Ok(false) => {
                    let current = self.inner.database.get_run(&run_id).ok().flatten();
                    if current.as_ref().is_some_and(|run| {
                        matches!(
                            run.status,
                            RunStatus::Queued
                                | RunStatus::Starting
                                | RunStatus::Running
                                | RunStatus::Stopping
                        )
                    }) {
                        self.mark_cleanup_pending(&job_id).await;
                        stop_guard.complete(Err("run-stop-failed".to_string()));
                    } else {
                        stop_guard.complete_cleanup_confirmed(
                            current.ok_or_else(|| "run-stop-failed".to_string()),
                        );
                    }
                    return;
                }
                Err(_) => {
                    self.mark_cleanup_pending(&job_id).await;
                    stop_guard.complete(Err("run-stop-failed".to_string()));
                    return;
                }
            }
        }
        let result = if cleanup_confirmed {
            Ok(Ok(ExecutionExit { exit_code: None }))
        } else {
            tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await
        };
        let operation = match result {
            Ok(Ok(exit)) => {
                let pending = self
                    .inner
                    .pending_terminal
                    .lock()
                    .await
                    .get(&run_id)
                    .cloned();
                let pending = pending.unwrap_or(PendingTerminal {
                    status: RunStatus::Cancelled,
                    exit_code: exit.exit_code,
                    error_message: Some("manual-stop".to_string()),
                    failure_code: None,
                });
                self.finish_run_and_notify(
                    &run_id,
                    &job_id,
                    &owner,
                    &attempt_token,
                    pending.status,
                    pending.exit_code,
                    pending.error_message.as_deref(),
                    now,
                    pending.failure_code,
                )
                .and_then(|confirmed| {
                    if confirmed {
                        Ok(())
                    } else {
                        Err(SchedulerError::Storage(StorageError::ConcurrentChange(
                            "run terminal CAS".into(),
                        )))
                    }
                })
            }
            Ok(Err(error)) if error.cleanup_confirmed => {
                let pending = self
                    .inner
                    .pending_terminal
                    .lock()
                    .await
                    .get(&run_id)
                    .cloned()
                    .unwrap_or(PendingTerminal {
                        status: RunStatus::Cancelled,
                        exit_code: None,
                        error_message: Some("manual-stop".to_string()),
                        failure_code: None,
                    });
                self.finish_run_and_notify(
                    &run_id,
                    &job_id,
                    &owner,
                    &attempt_token,
                    pending.status,
                    pending.exit_code,
                    pending.error_message.as_deref(),
                    now,
                    pending.failure_code,
                )
                .and_then(|confirmed| {
                    if confirmed {
                        Ok(())
                    } else {
                        Err(SchedulerError::Storage(StorageError::ConcurrentChange(
                            "run terminal CAS".into(),
                        )))
                    }
                })
            }
            Ok(Err(error)) => Err(SchedulerError::Adapter {
                run_id: run_id.clone(),
                source: AdapterError::new(failure_code_from_adapter(&error).as_db_message()),
            }),
            Err(_) => Err(SchedulerError::Adapter {
                run_id: run_id.clone(),
                source: AdapterError::new(TerminalFailureCode::TerminationTimeout.as_db_message()),
            }),
        };
        if operation.is_ok() {
            self.inner.pending_terminal.lock().await.remove(&run_id);
            self.clear_cleanup_pending(&job_id).await;
            let result = self
                .inner
                .database
                .get_run(&run_id)
                .ok()
                .flatten()
                .ok_or_else(|| "run-stop-failed".to_string());
            // Keep all fallible/awaiting work before this removal. Once the
            // active permit is released, the guard publishes/removes the
            // stop lease synchronously in the same poll, so cancellation
            // cannot strand a terminal row without its completion signal.
            self.remove_active_and_complete_stop(&run_id, result, &mut stop_guard)
                .await;
            if !self.is_shutdown_requested() {
                let _ = self.drain_queue_locked_by_id(&job_id).await;
            }
            return;
        } else {
            self.mark_cleanup_pending(&job_id).await;
        }
        stop_guard.complete(Err("run-stop-failed".to_string()));
    }

    pub async fn recover_stale_at(&self, now: i64) -> Result<usize, SchedulerError> {
        let stale = self
            .inner
            .database
            .list_runs_for_retention()?
            .into_iter()
            .filter(|run| {
                // A periodic recovery pass must never treat a run owned by
                // this coordinator as an orphan. Its monitor is still the
                // authoritative in-memory owner; only rows from a previous
                // owner (or rows with no owner at all) cross this boundary.
                run.owner_instance_id.as_deref()
                    != Some(self.inner.config.owner_instance_id.as_str())
                    && matches!(
                        run.status,
                        RunStatus::Starting | RunStatus::Running | RunStatus::Stopping
                    )
            })
            .collect::<Vec<_>>();
        let mut recovered = 0;
        let mut blocked_jobs = std::collections::HashSet::new();
        let stale_job_ids = stale
            .iter()
            .map(|run| run.job_id.clone())
            .collect::<std::collections::HashSet<_>>();
        for run in stale {
            let Some(job) = self.inner.database.get_job(&run.job_id)? else {
                continue;
            };
            let owner = run
                .owner_instance_id
                .clone()
                .unwrap_or_else(|| self.inner.config.owner_instance_id.clone());
            let attempt_token = run
                .attempt_token
                .clone()
                .unwrap_or_else(|| format!("stale-{}", run.id));
            let request = ExecutionRequest {
                job,
                run: run.clone(),
                owner_instance_id: owner.clone(),
                attempt_token: attempt_token.clone(),
            };
            if self.inner.adapter.recover_stale(request).await.is_err() {
                // Fail closed: retain the non-terminal row and its queue
                // barrier until an exact adapter cleanup succeeds.  The
                // pending marker also prevents a same-process tick from
                // respawning while a later recovery attempt retries cleanup.
                self.mark_cleanup_pending(&run.job_id).await;
                blocked_jobs.insert(run.job_id.clone());
                continue;
            }
            let terminal = self.inner.database.finish_run(
                &run.id,
                &owner,
                &attempt_token,
                RunStatus::Failed,
                None,
                Some("scheduler-stale-recovery"),
                now,
            );
            let terminalized = match terminal {
                Ok(true) => true,
                Ok(false) => self.inner.database.get_run(&run.id)?.is_none_or(|current| {
                    matches!(
                        current.status,
                        RunStatus::Succeeded
                            | RunStatus::Failed
                            | RunStatus::Cancelled
                            | RunStatus::Skipped
                    )
                }),
                Err(_) => false,
            };
            if !terminalized {
                self.mark_cleanup_pending(&run.job_id).await;
                blocked_jobs.insert(run.job_id.clone());
                continue;
            }
            if terminalized {
                recovered += 1;
                self.emit_failure(&run.job_id, &run.id, TerminalFailureCode::ProcessCrashed);
            }
        }
        for job_id in stale_job_ids {
            if !blocked_jobs.contains(&job_id) {
                self.clear_cleanup_pending(&job_id).await;
            }
        }
        Ok(recovered)
    }

    /// Request shutdown and confirm termination for every known active handle.
    /// Durable queued intents remain in FIFO storage for the next scheduler
    /// startup; it is safe to call this method repeatedly.
    pub async fn shutdown(&self) -> Result<(), SchedulerError> {
        self.request_shutdown();
        self.shutdown_active_runs().await
    }

    async fn process_job_at(&self, job: Job, now: i64) -> Result<(), SchedulerError> {
        let lock = self.job_mutex(&job.id).await;
        let _guard = lock.lock().await;
        if self.is_shutdown_requested() {
            return Ok(());
        }
        if self.is_cleanup_pending(&job.id).await {
            return Err(SchedulerError::Adapter {
                run_id: job.id.clone(),
                source: AdapterError::new("termination-unverified"),
            });
        }
        let due = self.due_occurrences(&job, now)?;
        for occurrence in due {
            if self.is_shutdown_requested() {
                break;
            }
            let claim = self.inner.database.claim_scheduled_run(
                &job.id,
                occurrence.timestamp,
                &occurrence.wall_key,
                now,
            )?;
            self.process_claim_locked(&job, claim, now).await?;
        }
        self.inner
            .database
            .advance_job_checkpoint_at(&job.id, now, now)?;
        self.drain_queue_locked(&job).await
    }

    fn due_occurrences(
        &self,
        job: &Job,
        now: i64,
    ) -> Result<Vec<ScheduledOccurrence>, SchedulerError> {
        let Some(expression) = job.cron_expr.as_deref() else {
            return Ok(Vec::new());
        };
        let schedule = CronSchedule::parse(expression)
            .map_err(|error| SchedulerError::Cron(error.to_string()))?;
        let Some(checkpoint) = job.last_evaluated_at else {
            return Ok(Vec::new());
        };
        if checkpoint >= now {
            return Ok(Vec::new());
        }
        let after = Local
            .timestamp_millis_opt(checkpoint)
            .single()
            .ok_or_else(|| SchedulerError::Cron("checkpoint is outside local time".into()))?;
        let mut cursor = after;
        let mut timestamps = Vec::new();
        let mut candidates = Vec::new();
        loop {
            let batch = schedule
                .next_local_occurrences(cursor, DUE_BATCH_SIZE)
                .map_err(|error| SchedulerError::Cron(error.to_string()))?;
            if batch.is_empty() {
                break;
            }
            let mut progressed = false;
            for occurrence in &batch {
                let timestamp = occurrence.timestamp_millis();
                if timestamp > now {
                    break;
                }
                timestamps.push(timestamp);
                candidates.push(ScheduledOccurrence {
                    timestamp,
                    wall_key: occurrence.wall_key.clone(),
                });
                cursor = occurrence.datetime;
                progressed = true;
            }
            if batch.len() < DUE_BATCH_SIZE || !progressed {
                break;
            }
        }
        let selected = select_occurrences(
            &timestamps,
            self.inner.config.startup_cutoff,
            job.catch_up,
            job.enabled,
        );
        Ok(selected
            .into_iter()
            .filter_map(|selected| {
                candidates
                    .iter()
                    .find(|candidate| candidate.timestamp == selected.timestamp)
                    .cloned()
            })
            .collect())
    }

    async fn process_claim_locked(
        &self,
        job: &Job,
        claim: PolicyClaim,
        now: i64,
    ) -> Result<(), SchedulerError> {
        if !claim.inserted {
            return Ok(());
        }
        match claim.action {
            ClaimedRunAction::Existing | ClaimedRunAction::Skip | ClaimedRunAction::Queue => Ok(()),
            ClaimedRunAction::Start => self.start_claimed_run(job, claim.run).await,
            ClaimedRunAction::KillPrevious => {
                let old_id = claim.previous_run_id.ok_or_else(|| {
                    SchedulerError::Storage(StorageError::ConcurrentChange(
                        "kill-previous claim without previous run".into(),
                    ))
                })?;
                self.stop_previous_and_resolve(
                    job,
                    &old_id,
                    &claim.run.id,
                    claim.previous_run_status,
                    now,
                )
                .await
            }
        }
    }

    async fn stop_previous_and_resolve(
        &self,
        job: &Job,
        old_run_id: &str,
        queued_run_id: &str,
        previous_run_status: Option<RunStatus>,
        now: i64,
    ) -> Result<(), SchedulerError> {
        let mut adapter_error = None;
        if previous_run_status != Some(RunStatus::Queued) {
            let handle = self
                .inner
                .active
                .lock()
                .await
                .get(old_run_id)
                .map(|active| Arc::clone(&active.handle));
            let Some(handle) = handle else {
                adapter_error = Some(AdapterError::new("active-handle-unavailable"));
                self.mark_cleanup_pending(&job.id).await;
                return Err(SchedulerError::Adapter {
                    run_id: old_run_id.to_string(),
                    source: adapter_error.expect("error is set above"),
                });
            };
            match tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) if error.cleanup_confirmed => {}
                Ok(Err(error)) => adapter_error = Some(error),
                Err(_) => adapter_error = Some(AdapterError::new("termination-timeout")),
            }
        }
        if let Some(error) = adapter_error {
            self.mark_cleanup_pending(&job.id).await;
            return Err(SchedulerError::Adapter {
                run_id: old_run_id.to_string(),
                source: AdapterError::new(failure_code_from_adapter(&error).as_db_message()),
            });
        }
        if !self
            .inner
            .database
            .resolve_kill_previous(old_run_id, queued_run_id, true, None, now)?
        {
            self.mark_cleanup_pending(&job.id).await;
            return Err(SchedulerError::Storage(StorageError::ConcurrentChange(
                "kill-previous cleanup pair".into(),
            )));
        }
        self.clear_cleanup_pending(&job.id).await;
        self.remove_active(old_run_id).await;
        self.drain_queue_locked(job).await
    }

    async fn start_claimed_run(&self, job: &Job, run: Run) -> Result<(), SchedulerError> {
        let _ = self.start_owned_claim(job, &run).await?;
        Ok(())
    }

    /// Claim a queued run into `starting` and drive it through the adapter.
    /// Returns the running handle, or `None` when the claim was lost or the run
    /// was cancelled by shutdown.
    async fn start_owned_claim(
        &self,
        job: &Job,
        run: &Run,
    ) -> Result<Option<Arc<dyn ExecutionHandle>>, SchedulerError> {
        let owner = self.inner.config.owner_instance_id.clone();
        let attempt_token = Uuid::new_v4().to_string();
        if !self
            .inner
            .database
            .claim_run_starting(&run.id, &owner, &attempt_token)?
        {
            return Ok(None);
        }
        let claimed = self
            .inner
            .database
            .get_run(&run.id)?
            .ok_or_else(|| StorageError::NotFound(format!("run {}", run.id)))?;
        let Some(handle) = self.start_owned_run(&claimed, &attempt_token).await? else {
            return Ok(None);
        };
        self.spawn_monitor(
            claimed.id,
            job.id.clone(),
            owner,
            attempt_token,
            Arc::clone(&handle),
        );
        Ok(Some(handle))
    }

    async fn start_owned_run(
        &self,
        run: &Run,
        attempt_token: &str,
    ) -> Result<Option<Arc<dyn ExecutionHandle>>, SchedulerError> {
        let job = self
            .inner
            .database
            .get_run_job(&run.job_id)?
            .ok_or_else(|| StorageError::NotFound(format!("job {}", run.job_id)))?;
        let owner = self.inner.config.owner_instance_id.clone();
        if self.is_shutdown_requested() {
            self.cancel_starting_run(run, &owner, attempt_token);
            return Ok(None);
        }
        // A waiter may already be queued on the semaphore when shutdown is
        // requested.  Wake it directly so it can durably cancel its starting
        // row instead of waiting for a permit that will never be released.
        let shutdown = self.inner.shutdown_notify.notified();
        tokio::pin!(shutdown);
        shutdown.as_mut().enable();
        if self.is_shutdown_requested() {
            self.cancel_starting_run(run, &owner, attempt_token);
            return Ok(None);
        }
        let permit = tokio::select! {
            biased;
            _ = &mut shutdown => {
                self.cancel_starting_run(run, &owner, attempt_token);
                return Ok(None);
            }
            acquired = self.inner.permits.clone().acquire_owned() => {
                acquired.map_err(|error| SchedulerError::Join(error.to_string()))?
            }
        };
        if self.is_shutdown_requested() {
            let _ = self.finish_run_and_notify(
                &run.id,
                &run.job_id,
                &owner,
                attempt_token,
                RunStatus::Cancelled,
                None,
                Some("scheduler-shutdown"),
                current_epoch_millis(),
                None,
            );
            drop(permit);
            return Ok(None);
        }
        let request = ExecutionRequest {
            job,
            run: run.clone(),
            owner_instance_id: owner.clone(),
            attempt_token: attempt_token.to_string(),
        };
        let handle = match self.inner.adapter.spawn(request).await {
            Ok(handle) => handle,
            Err(error) => {
                let code = failure_code_from_adapter(&error);
                self.finish_run_and_notify(
                    &run.id,
                    &run.job_id,
                    &owner,
                    attempt_token,
                    RunStatus::Failed,
                    None,
                    Some(code.as_db_message()),
                    current_epoch_millis(),
                    Some(code),
                )?;
                drop(permit);
                return Err(SchedulerError::Adapter {
                    run_id: run.id.clone(),
                    source: error,
                });
            }
        };
        let metadata = handle.metadata();
        let metadata_cas = self.inner.database.mark_run_running_with_metadata(
            &run.id,
            &owner,
            attempt_token,
            current_epoch_millis(),
            &metadata,
        );
        let metadata_error = match metadata_cas {
            Ok(true) => None,
            Ok(false) => Some(StorageError::ConcurrentChange("run metadata CAS".into())),
            Err(error) => Some(error),
        };
        if let Some(metadata_error) = metadata_error {
            let termination =
                tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await;
            let cleanup_confirmed = match &termination {
                Ok(Ok(_)) => true,
                Ok(Err(error)) => error.cleanup_confirmed,
                Err(_) => false,
            };
            let code = TerminalFailureCode::StorageFailed;
            let terminal = if cleanup_confirmed {
                self.finish_run_and_notify(
                    &run.id,
                    &run.job_id,
                    &owner,
                    attempt_token,
                    RunStatus::Failed,
                    None,
                    Some(code.as_db_message()),
                    current_epoch_millis(),
                    Some(code),
                )
            } else {
                Ok(false)
            };
            if !cleanup_confirmed || !matches!(terminal, Ok(true)) {
                self.inner.pending_terminal.lock().await.insert(
                    run.id.clone(),
                    PendingTerminal {
                        status: RunStatus::Failed,
                        exit_code: None,
                        error_message: Some(code.as_db_message().to_string()),
                        failure_code: Some(code),
                    },
                );
                self.mark_cleanup_pending(&run.job_id).await;
                self.inner.active.lock().await.insert(
                    run.id.clone(),
                    ActiveExecution {
                        job_id: run.job_id.clone(),
                        owner_instance_id: owner.clone(),
                        attempt_token: attempt_token.to_string(),
                        handle: Arc::clone(&handle),
                        cleanup_confirmed,
                        _permit: permit,
                    },
                );
                self.spawn_monitor(
                    run.id.clone(),
                    run.job_id.clone(),
                    owner.clone(),
                    attempt_token.to_string(),
                    Arc::clone(&handle),
                );
            } else {
                drop(permit);
            }
            return Err(SchedulerError::Storage(metadata_error));
        }

        self.inner.active.lock().await.insert(
            run.id.clone(),
            ActiveExecution {
                job_id: run.job_id.clone(),
                owner_instance_id: owner.clone(),
                attempt_token: attempt_token.to_string(),
                handle: Arc::clone(&handle),
                cleanup_confirmed: false,
                _permit: permit,
            },
        );
        Ok(Some(handle))
    }

    fn cancel_starting_run(&self, run: &Run, owner: &str, attempt_token: &str) {
        let _ = self.inner.database.finish_run(
            &run.id,
            owner,
            attempt_token,
            RunStatus::Cancelled,
            None,
            Some("scheduler-shutdown"),
            current_epoch_millis(),
        );
    }

    fn spawn_monitor(
        &self,
        run_id: String,
        job_id: String,
        owner: String,
        attempt_token: String,
        handle: Arc<dyn ExecutionHandle>,
    ) {
        let coordinator = self.clone();
        tokio::spawn(async move {
            coordinator
                .monitor_run(run_id, job_id, owner, attempt_token, handle)
                .await;
        });
    }

    async fn monitor_run(
        &self,
        run_id: String,
        job_id: String,
        owner: String,
        attempt_token: String,
        handle: Arc<dyn ExecutionHandle>,
    ) {
        let result = handle.wait().await;
        if self.is_cleanup_pending(&job_id).await {
            // A failed manual stop/shutdown keeps the handle as a fail-closed
            // witness.  A later successful wait is the adapter's confirmation
            // that the process tree is now gone; only then retry terminal CAS
            // and release the guard.  In particular, a storage error after a
            // successful terminate must not be mistaken for cleanup success.
            if cleanup_confirmed_for_result(&result) {
                let pending = self
                    .inner
                    .pending_terminal
                    .lock()
                    .await
                    .get(&run_id)
                    .cloned()
                    .unwrap_or(PendingTerminal {
                        status: RunStatus::Failed,
                        exit_code: None,
                        error_message: Some(
                            TerminalFailureCode::StorageFailed
                                .as_db_message()
                                .to_string(),
                        ),
                        failure_code: Some(TerminalFailureCode::StorageFailed),
                    });
                let terminal = self.finish_run_and_notify(
                    &run_id,
                    &job_id,
                    &owner,
                    &attempt_token,
                    pending.status,
                    pending.exit_code,
                    pending.error_message.as_deref(),
                    current_epoch_millis(),
                    pending.failure_code,
                );
                if matches!(terminal, Ok(true)) {
                    self.inner.pending_terminal.lock().await.remove(&run_id);
                    self.remove_active(&run_id).await;
                    self.clear_cleanup_pending(&job_id).await;
                    if !self.is_shutdown_requested() {
                        let lock = self.job_mutex(&job_id).await.lock_owned().await;
                        let _guard = lock;
                        let _ = self.drain_queue_locked_by_id(&job_id).await;
                    }
                }
            }
            return;
        }
        let current = self.inner.database.get_run(&run_id).ok().flatten();
        // A stop/shutdown path owns terminalization of a `stopping` row only
        // after adapter-confirmed termination; do not let the passive monitor
        // turn it into succeeded/failed first.
        if current
            .as_ref()
            .is_some_and(|run| run.status == RunStatus::Stopping)
        {
            return;
        }
        let cleanup_witness = cleanup_confirmed_for_result(&result);
        let (status, exit_code, error, failure_code): (
            RunStatus,
            Option<i32>,
            Option<String>,
            Option<TerminalFailureCode>,
        ) = match result {
            Ok(exit) if exit.exit_code == Some(0) => {
                (RunStatus::Succeeded, exit.exit_code, None, None)
            }
            Ok(exit) => (
                RunStatus::Failed,
                exit.exit_code,
                Some("process-exit-nonzero".to_string()),
                Some(TerminalFailureCode::NonzeroExit),
            ),
            Err(error) => (
                RunStatus::Failed,
                None,
                Some(
                    failure_code_from_adapter(&error)
                        .as_db_message()
                        .to_string(),
                ),
                Some(failure_code_from_adapter(&error)),
            ),
        };
        let pending = PendingTerminal {
            status,
            exit_code,
            error_message: error.clone(),
            failure_code,
        };
        if !cleanup_witness {
            // A wait/protocol error is not proof that the tree disappeared.
            // Keep the non-terminal row and the actor as a retry witness; a
            // later stop/shutdown request must still own the destructive
            // identity-checked cleanup boundary.
            self.inner
                .pending_terminal
                .lock()
                .await
                .insert(run_id.clone(), pending);
            self.mark_cleanup_pending(&job_id).await;
            return;
        }
        let terminal = self.finish_run_and_notify(
            &run_id,
            &job_id,
            &owner,
            &attempt_token,
            status,
            exit_code,
            error.as_deref(),
            current_epoch_millis(),
            failure_code,
        );
        if matches!(terminal, Ok(true)) {
            self.inner.pending_terminal.lock().await.remove(&run_id);
            self.remove_active(&run_id).await;
            let lock = self.job_mutex(&job_id).await.lock_owned().await;
            let _guard = lock;
            if !self.is_shutdown_requested() {
                let _ = self.drain_queue_locked_by_id(&job_id).await;
            }
        } else {
            self.inner
                .pending_terminal
                .lock()
                .await
                .insert(run_id.clone(), pending);
            self.mark_cleanup_pending(&job_id).await;
            if let Some(active) = self.inner.active.lock().await.get_mut(&run_id) {
                active.cleanup_confirmed = cleanup_witness;
            }
        }
    }

    async fn drain_queue_locked(&self, job: &Job) -> Result<(), SchedulerError> {
        self.drain_queue_locked_by_id(&job.id).await
    }

    async fn drain_queue_locked_by_id(&self, job_id: &str) -> Result<(), SchedulerError> {
        if self.is_shutdown_requested() || self.inner.database.active_process_run(job_id)?.is_some()
        {
            return Ok(());
        }
        loop {
            if self.is_shutdown_requested()
                || self.inner.database.active_process_run(job_id)?.is_some()
            {
                break;
            }
            let Some(run) = self
                .inner
                .database
                .list_queued_runs(job_id)?
                .into_iter()
                .next()
            else {
                break;
            };
            if run.blocked_by_run_id.is_some() {
                break;
            }
            let owner = self.inner.config.owner_instance_id.clone();
            let token = Uuid::new_v4().to_string();
            if !self
                .inner
                .database
                .claim_run_starting(&run.id, &owner, &token)?
            {
                continue;
            }
            // The starting CAS above must carry the same token into the
            // adapter request. Re-read the row and let `start_queued_run`
            // validate/use its owner/token rather than attempting a second
            // non-atomic claim.
            let claimed = self
                .inner
                .database
                .get_run(&run.id)?
                .ok_or_else(|| StorageError::NotFound(format!("run {}", run.id)))?;
            let token = claimed.attempt_token.clone().ok_or_else(|| {
                StorageError::Validation("starting run has no attempt token".into())
            })?;
            match self.start_owned_run(&claimed, &token).await? {
                Some(handle) => {
                    self.spawn_monitor(claimed.id, job_id.to_string(), owner, token, handle);
                    break;
                }
                None => break,
            }
        }
        Ok(())
    }

    async fn shutdown_active_runs(&self) -> Result<(), SchedulerError> {
        // Create the synchronous sink before taking the async active map. If
        // this enclosing future is cancelled after the take, every witness is
        // already owned by the ledger/sink and can be retried by the next
        // shutdown call.
        let ledger = ShutdownExecutionLedger::new(Arc::clone(&self.inner.shutdown_orphans));
        let active = std::mem::take(&mut *self.inner.active.lock().await);
        let deadline = tokio::time::Instant::now() + self.inner.config.shutdown_timeout;
        for (run_id, execution) in active {
            ledger.insert(run_id, execution);
        }
        let orphan_ids = {
            let orphan_sink = self
                .inner
                .shutdown_orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            orphan_sink.keys().cloned().collect::<Vec<_>>()
        };
        for run_id in orphan_ids {
            if let Some(execution) = self
                .inner
                .shutdown_orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&run_id)
            {
                ledger.insert(run_id, execution);
            }
        }
        // The ledger now owns all local execution values. Requesting `stopping`
        // is synchronous DB work and happens before any task is spawned, so an
        // enclosing cancellation cannot lose this durable non-terminal barrier.
        let entries = ledger.drain();
        for (run_id, execution) in entries {
            if !execution.cleanup_confirmed {
                let _ = self.inner.database.request_run_stop(
                    &run_id,
                    &execution.owner_instance_id,
                    &execution.attempt_token,
                );
            }
            ledger.insert(run_id, execution);
        }
        let mut tasks = tokio::task::JoinSet::new();
        let run_ids = ledger
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for run_id in run_ids {
            let ledger_for_task = Arc::clone(&ledger);
            tasks.spawn(async move {
                let Some(execution) = ledger_for_task.take(&run_id) else {
                    // This is only reachable if the in-memory active map had
                    // duplicate keys, which HashMap cannot represent. Keep
                    // the task non-panicking so the outer ledger remains the
                    // fail-closed owner if a future implementation changes
                    // that invariant.
                    return (run_id, None);
                };
                let guard = ShutdownExecutionGuard {
                    run_id: run_id.clone(),
                    ledger: ledger_for_task,
                    execution: Some(execution),
                };
                let handle = Arc::clone(
                    &guard
                        .execution
                        .as_ref()
                        .expect("execution guard initialized")
                        .handle,
                );
                let cleanup_already_confirmed = guard
                    .execution
                    .as_ref()
                    .expect("execution guard initialized")
                    .cleanup_confirmed;
                let result = if cleanup_already_confirmed {
                    Some(Ok(Ok(ExecutionExit { exit_code: None })))
                } else {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        None
                    } else {
                        Some(tokio::time::timeout(remaining, handle.terminate()).await)
                    }
                };
                // Keep the process witness in the synchronous ledger until
                // the join owner explicitly takes it. A completed JoinSet
                // output must never itself be the only owner: dropping an
                // enclosing shutdown future can drop completed outputs.
                drop(guard);
                (run_id, result)
            });
        }
        let mut first_error = None;
        while let Some(joined) = tasks.join_next().await {
            let Ok((run_id, result)) = joined else {
                if first_error.is_none() {
                    first_error = Some(SchedulerError::Join(
                        "shutdown termination task failed".to_string(),
                    ));
                }
                continue;
            };
            let Some(execution) = ledger.take(&run_id) else {
                if first_error.is_none() {
                    first_error = Some(SchedulerError::Join(
                        "shutdown execution ownership unavailable".to_string(),
                    ));
                }
                continue;
            };
            let mut execution_guard = ShutdownExecutionGuard {
                run_id: run_id.clone(),
                ledger: Arc::clone(&ledger),
                execution: Some(execution),
            };
            let job_id = execution_guard
                .execution
                .as_ref()
                .expect("shutdown execution guard initialized")
                .job_id
                .clone();
            let owner = execution_guard
                .execution
                .as_ref()
                .expect("shutdown execution guard initialized")
                .owner_instance_id
                .clone();
            let attempt_token = execution_guard
                .execution
                .as_ref()
                .expect("shutdown execution guard initialized")
                .attempt_token
                .clone();
            // A secondary adapter error (for example a log-write failure)
            // can arrive after the process tree was confirmed gone. Treat it
            // as the same cleanup-confirmed branch while retaining any
            // pending terminal status captured by the monitor.
            let result = match result {
                Some(Ok(Err(error))) if error.cleanup_confirmed => {
                    Some(Ok(Ok(ExecutionExit { exit_code: None })))
                }
                other => other,
            };
            match result {
                Some(Ok(Ok(exit))) => {
                    execution_guard
                        .execution
                        .as_mut()
                        .expect("shutdown execution guard initialized")
                        .cleanup_confirmed = true;
                    let pending = self
                        .inner
                        .pending_terminal
                        .lock()
                        .await
                        .get(&run_id)
                        .cloned()
                        .unwrap_or(PendingTerminal {
                            status: RunStatus::Cancelled,
                            exit_code: exit.exit_code,
                            error_message: Some("scheduler-shutdown".to_string()),
                            failure_code: None,
                        });
                    let finished = self.finish_run_and_notify(
                        &run_id,
                        &job_id,
                        &owner,
                        &attempt_token,
                        pending.status,
                        pending.exit_code,
                        pending.error_message.as_deref(),
                        current_epoch_millis(),
                        pending.failure_code,
                    );
                    match finished {
                        Ok(true) => {
                            self.inner.pending_terminal.lock().await.remove(&run_id);
                            self.clear_cleanup_pending(&job_id).await;
                            // No await follows this take in the successful
                            // branch, so the guard cannot be cancelled with a
                            // missing process witness.
                            let _ = execution_guard.execution.take();
                        }
                        Ok(false) => {
                            self.mark_cleanup_pending(&job_id).await;
                            self.restore_shutdown_guard(&mut execution_guard).await;
                            if first_error.is_none() {
                                first_error = Some(SchedulerError::Storage(
                                    StorageError::ConcurrentChange("run terminal CAS".into()),
                                ));
                            }
                        }
                        Err(error) => {
                            // The process tree is gone, but durable
                            // terminalization is not confirmed. Keep the
                            // execution witness and cleanup barrier so a
                            // later shutdown/retry can CAS the row without
                            // authorizing exit early.
                            self.mark_cleanup_pending(&job_id).await;
                            self.restore_shutdown_guard(&mut execution_guard).await;
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    }
                }
                Some(Ok(Err(error))) => {
                    let code = failure_code_from_adapter(&error);
                    self.mark_cleanup_pending(&job_id).await;
                    self.restore_shutdown_guard(&mut execution_guard).await;
                    if first_error.is_none() {
                        first_error = Some(SchedulerError::Adapter {
                            run_id: run_id.clone(),
                            source: AdapterError::new(code.as_db_message()),
                        });
                    }
                }
                Some(Err(_)) | None => {
                    let code = TerminalFailureCode::TerminationTimeout;
                    self.mark_cleanup_pending(&job_id).await;
                    self.restore_shutdown_guard(&mut execution_guard).await;
                    let shutdown_error = SchedulerError::Adapter {
                        run_id,
                        source: AdapterError::new(code.as_db_message()),
                    };
                    if first_error.is_none() {
                        first_error = Some(shutdown_error);
                    }
                }
            }
        }
        // Tasks that never started, or that unwound before producing their
        // tuple, remain in the synchronous ledger. Preserve every witness.
        for (run_id, execution) in ledger.drain() {
            let mut execution_guard = ShutdownExecutionGuard {
                run_id: run_id.clone(),
                ledger: Arc::clone(&ledger),
                execution: Some(execution),
            };
            let job_id = execution_guard
                .execution
                .as_ref()
                .expect("shutdown execution guard initialized")
                .job_id
                .clone();
            self.mark_cleanup_pending(&job_id).await;
            self.restore_shutdown_guard(&mut execution_guard).await;
            if first_error.is_none() {
                first_error = Some(SchedulerError::Join(
                    "shutdown execution ownership unavailable".to_string(),
                ));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn remove_active(&self, run_id: &str) {
        self.inner.active.lock().await.remove(run_id);
    }

    async fn remove_active_and_complete_stop(
        &self,
        run_id: &str,
        result: Result<Run, String>,
        stop_guard: &mut StopOperationGuard,
    ) {
        let mut active = self.inner.active.lock().await;
        active.remove(run_id);
        // No await occurs after the active witness is removed. A cancellation
        // point before this lock acquisition leaves it in `active`; a
        // cancellation point after it has been acquired cannot strand the
        // terminal result because both operations are synchronous here.
        stop_guard.complete_cleanup_confirmed(result);
    }

    async fn restore_shutdown_guard(&self, guard: &mut ShutdownExecutionGuard) {
        let Some(execution) = guard.execution.take() else {
            return;
        };
        let duplicate = self.inner.active.lock().await.contains_key(&guard.run_id);
        if duplicate {
            self.inner
                .shutdown_orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(guard.run_id.clone(), execution);
        } else {
            self.inner
                .active
                .lock()
                .await
                .insert(guard.run_id.clone(), execution);
        }
    }

    async fn restore_shutdown_orphans_for_job(&self, job_id: &str) {
        let restored = {
            let mut orphan_sink = self
                .inner
                .shutdown_orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let run_ids = orphan_sink
                .iter()
                .filter(|(_, execution)| execution.job_id == job_id)
                .map(|(run_id, _)| run_id.clone())
                .collect::<Vec<_>>();
            run_ids
                .into_iter()
                .filter_map(|run_id| {
                    orphan_sink
                        .remove(&run_id)
                        .map(|execution| (run_id, execution))
                })
                .collect::<Vec<_>>()
        };
        if !restored.is_empty() {
            self.inner.active.lock().await.extend(restored);
        }
    }

    fn has_shutdown_orphan(&self, job_id: &str) -> bool {
        self.inner
            .shutdown_orphans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .any(|execution| execution.job_id == job_id)
    }

    async fn is_cleanup_pending(&self, job_id: &str) -> bool {
        self.inner.cleanup_pending.lock().await.contains(job_id)
            || self.has_shutdown_orphan(job_id)
            || self
                .inner
                .stop_recovery_required
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .values()
                .any(|recovery_job_id| recovery_job_id == job_id)
    }

    async fn mark_cleanup_pending(&self, job_id: &str) {
        self.inner
            .cleanup_pending
            .lock()
            .await
            .insert(job_id.to_string());
    }

    async fn clear_cleanup_pending(&self, job_id: &str) {
        self.inner.cleanup_pending.lock().await.remove(job_id);
        let run_ids = self
            .inner
            .stop_recovery_required
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .filter_map(|(run_id, recovery_job_id)| {
                (recovery_job_id == job_id).then_some(run_id.clone())
            })
            .collect::<Vec<_>>();
        let mut recovery = self
            .inner
            .stop_recovery_required
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for run_id in run_ids {
            recovery.remove(&run_id);
        }
    }

    /// Shutdown may only authorize application exit after every adapter handle
    /// has confirmed cleanup. Failed termination deliberately leaves the
    /// handle owned by the coordinator and keeps this predicate false.
    pub async fn cleanup_confirmed(&self) -> bool {
        self.inner.active.lock().await.is_empty()
            && self.inner.cleanup_pending.lock().await.is_empty()
            && self.inner.pending_terminal.lock().await.is_empty()
            && self
                .inner
                .stop_recovery_required
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
            && self
                .inner
                .shutdown_orphans
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
    }

    pub fn cleanup_confirmed_sync(&self) -> bool {
        let Ok(active) = self.inner.active.try_lock() else {
            return false;
        };
        if !active.is_empty() {
            return false;
        }
        if !self
            .inner
            .shutdown_orphans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
        {
            return false;
        }
        if !self
            .inner
            .stop_recovery_required
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
        {
            return false;
        }
        let Ok(pending) = self.inner.cleanup_pending.try_lock() else {
            return false;
        };
        pending.is_empty()
            && self
                .inner
                .pending_terminal
                .try_lock()
                .is_ok_and(|terminals| terminals.is_empty())
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_run_and_notify(
        &self,
        run_id: &str,
        job_id: &str,
        owner: &str,
        attempt_token: &str,
        status: RunStatus,
        exit_code: Option<i32>,
        error_message: Option<&str>,
        ended_at: i64,
        failure_code: Option<TerminalFailureCode>,
    ) -> Result<bool, SchedulerError> {
        let changed = self.inner.database.finish_run(
            run_id,
            owner,
            attempt_token,
            status,
            exit_code,
            error_message,
            ended_at,
        )?;
        if changed {
            self.inner.terminal_listener.on_terminal(TerminalRunEvent {
                job_id: job_id.to_string(),
                run_id: run_id.to_string(),
                status,
                failure_code,
            });
            // A service whose linked run terminated either returns to `stopped`
            // or schedules a restart retry according to its restart policy.
            // This is a no-op for ordinary job runs, which have no instance row.
            self.handle_service_terminal(job_id, run_id, status, ended_at);
            return Ok(true);
        }
        let already_terminal = self.inner.database.get_run(run_id)?.is_some_and(|run| {
            matches!(
                run.status,
                RunStatus::Succeeded
                    | RunStatus::Failed
                    | RunStatus::Cancelled
                    | RunStatus::Skipped
            )
        });
        Ok(already_terminal)
    }

    fn emit_failure(&self, job_id: &str, run_id: &str, failure_code: TerminalFailureCode) {
        self.inner.terminal_listener.on_terminal(TerminalRunEvent {
            job_id: job_id.to_string(),
            run_id: run_id.to_string(),
            status: RunStatus::Failed,
            failure_code: Some(failure_code),
        });
    }

    async fn job_mutex(&self, job_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.inner.job_locks.lock().await;
        Arc::clone(
            locks
                .entry(job_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
}

fn failure_code_from_adapter(error: &AdapterError) -> TerminalFailureCode {
    match error.message.as_str() {
        "environment-unavailable" => TerminalFailureCode::EnvironmentUnavailable,
        "log-open-failed" | "log-write-failed" => TerminalFailureCode::LogWriteFailed,
        "handshake-failed" | "target-unavailable" => TerminalFailureCode::WslUnavailable,
        "termination-timeout" => TerminalFailureCode::TerminationTimeout,
        "wait-failed" => TerminalFailureCode::ProcessCrashed,
        "storage-failed" | "metadata-cas-failed" | "execution-metadata-cas" => {
            TerminalFailureCode::StorageFailed
        }
        _ => TerminalFailureCode::SpawnFailed,
    }
}

/// Successful adapter results are cleanup witnesses by contract. An error is
/// a witness only when the platform completed tree cleanup before reporting a
/// secondary failure such as log persistence; wait/identity errors otherwise
/// remain retryable and must keep the active handle owned.
fn cleanup_confirmed_for_result(result: &Result<ExecutionExit, AdapterError>) -> bool {
    match result {
        Ok(_) => true,
        Err(error) => error.cleanup_confirmed,
    }
}

/// A stable service-lifecycle error with a fixed, UI-safe code.
fn service_adapter_error(service_id: &str, code: &'static str) -> SchedulerError {
    SchedulerError::Adapter {
        run_id: service_id.to_string(),
        source: AdapterError::new(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{
        EnvironmentUpdate, JobInput, OverlapPolicy, RestartPolicy, ServiceInput, TargetKind,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;
    use tokio::sync::oneshot;

    #[derive(Default)]
    struct RecordingListener {
        events: StdMutex<Vec<TerminalRunEvent>>,
    }

    impl TerminalRunListener for RecordingListener {
        fn on_terminal(&self, event: TerminalRunEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    struct MockHandle {
        wait_rx: Mutex<Option<oneshot::Receiver<ExecutionExit>>>,
        active: Arc<AtomicUsize>,
        terminated: AtomicBool,
    }

    impl ExecutionHandle for MockHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            let active = Arc::clone(&self.active);
            let terminated = &self.terminated;
            Box::pin(async move {
                if !terminated.swap(true, Ordering::SeqCst) {
                    active.fetch_sub(1, Ordering::SeqCst);
                }
                Ok(ExecutionExit { exit_code: Some(0) })
            })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async {
                let result = self
                    .wait_rx
                    .lock()
                    .await
                    .take()
                    .ok_or_else(|| AdapterError::new("wait called twice"))?
                    .await
                    .map_err(|_| AdapterError::new("mock wait channel closed"))?;
                if !self.terminated.swap(true, Ordering::SeqCst) {
                    self.active.fetch_sub(1, Ordering::SeqCst);
                }
                Ok(result)
            })
        }
    }

    struct MockAdapter {
        starts: AtomicUsize,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        waits: Arc<Mutex<Vec<oneshot::Sender<ExecutionExit>>>>,
    }

    impl MockAdapter {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                starts: AtomicUsize::new(0),
                active: Arc::new(AtomicUsize::new(0)),
                max_active: Arc::new(AtomicUsize::new(0)),
                waits: Arc::new(Mutex::new(Vec::new())),
            })
        }

        async fn wait_for_starts(&self, expected: usize) {
            tokio::time::timeout(Duration::from_secs(1), async {
                while self.starts.load(Ordering::SeqCst) < expected {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("mock adapter did not receive expected starts");
        }

        async fn finish_next(&self, exit_code: i32) {
            loop {
                if let Some(sender) = self.waits.lock().await.pop() {
                    let _ = sender.send(ExecutionExit {
                        exit_code: Some(exit_code),
                    });
                    return;
                }
                tokio::task::yield_now().await;
            }
        }
    }

    impl ExecutionAdapter for MockAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            let mut observed = self.max_active.load(Ordering::SeqCst);
            while active > observed {
                match self.max_active.compare_exchange(
                    observed,
                    active,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => break,
                    Err(current) => observed = current,
                }
            }
            let active_counter = Arc::clone(&self.active);
            let waits = Arc::clone(&self.waits);
            Box::pin(async move {
                let (sender, receiver) = oneshot::channel();
                waits.lock().await.push(sender);
                Ok(Arc::new(MockHandle {
                    wait_rx: Mutex::new(Some(receiver)),
                    active: active_counter,
                    terminated: AtomicBool::new(false),
                }) as Arc<dyn ExecutionHandle>)
            })
        }
    }

    struct CasInvalidatingAdapter {
        database: Arc<DatabaseState>,
        terminated: Arc<AtomicBool>,
    }

    struct CasInvalidatingHandle {
        terminated: Arc<AtomicBool>,
    }

    impl ExecutionHandle for CasInvalidatingHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            self.terminated.store(true, Ordering::SeqCst);
            Box::pin(async { Ok(ExecutionExit { exit_code: None }) })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { Ok(ExecutionExit { exit_code: Some(0) }) })
        }

        fn metadata(&self) -> RunExecutionMetadata {
            RunExecutionMetadata {
                target_pid: Some(4242),
                target_process_created_at: Some(7),
                ..RunExecutionMetadata::default()
            }
        }
    }

    impl ExecutionAdapter for CasInvalidatingAdapter {
        fn spawn(&self, request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            let database = Arc::clone(&self.database);
            let terminated = Arc::clone(&self.terminated);
            Box::pin(async move {
                assert!(database
                    .finish_run(
                        &request.run.id,
                        &request.owner_instance_id,
                        &request.attempt_token,
                        RunStatus::Failed,
                        None,
                        Some("fixture-cas-invalidated"),
                        2,
                    )
                    .unwrap());
                Ok(Arc::new(CasInvalidatingHandle { terminated }) as Arc<dyn ExecutionHandle>)
            })
        }
    }

    struct HangingTerminateHandle;

    impl ExecutionHandle for HangingTerminateHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }
    }

    struct HangingTerminateAdapter;

    impl ExecutionAdapter for HangingTerminateAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            Box::pin(async { Ok(Arc::new(HangingTerminateHandle) as Arc<dyn ExecutionHandle>) })
        }
    }

    struct RetryTerminateHandle {
        attempts: Arc<AtomicUsize>,
    }

    impl ExecutionHandle for RetryTerminateHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    std::future::pending().await
                } else {
                    Ok(ExecutionExit { exit_code: None })
                }
            })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }
    }

    struct RetryTerminateAdapter {
        attempts: Arc<AtomicUsize>,
    }

    impl ExecutionAdapter for RetryTerminateAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            let attempts = Arc::clone(&self.attempts);
            Box::pin(async move {
                Ok(Arc::new(RetryTerminateHandle { attempts }) as Arc<dyn ExecutionHandle>)
            })
        }
    }

    struct MetadataErrorHangingAdapter;

    struct MetadataErrorHangingHandle;

    impl ExecutionHandle for MetadataErrorHangingHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }

        fn metadata(&self) -> RunExecutionMetadata {
            RunExecutionMetadata {
                process_marker: Some("bad\0marker".to_string()),
                ..RunExecutionMetadata::default()
            }
        }
    }

    impl ExecutionAdapter for MetadataErrorHangingAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            Box::pin(async { Ok(Arc::new(MetadataErrorHangingHandle) as Arc<dyn ExecutionHandle>) })
        }
    }

    struct WaitOutcomeAdapter {
        wait_error: AdapterError,
    }

    struct WaitOutcomeHandle {
        wait_error: AdapterError,
    }

    impl ExecutionHandle for WaitOutcomeHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { Ok(ExecutionExit { exit_code: None }) })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            let error = self.wait_error.clone();
            Box::pin(async move { Err(error) })
        }
    }

    impl ExecutionAdapter for WaitOutcomeAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            let wait_error = self.wait_error.clone();
            Box::pin(async move {
                Ok(Arc::new(WaitOutcomeHandle { wait_error }) as Arc<dyn ExecutionHandle>)
            })
        }
    }

    struct TransientStaleRecoveryAdapter {
        attempts: AtomicUsize,
        starts: AtomicUsize,
    }

    impl ExecutionAdapter for TransientStaleRecoveryAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(AdapterError::new("spawn-must-not-run")) })
        }

        fn recover_stale(&self, _request: ExecutionRequest) -> AdapterFuture<'_, ()> {
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    Err(AdapterError::new("transient-cleanup-failed"))
                } else {
                    Ok(())
                }
            })
        }
    }

    struct StopRaceHandle {
        release: Arc<Notify>,
    }

    impl ExecutionHandle for StopRaceHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                release.notified().await;
                Ok(ExecutionExit { exit_code: Some(0) })
            })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            Box::pin(async { std::future::pending().await })
        }
    }

    struct StopRaceAdapter {
        release: Arc<Notify>,
    }

    impl ExecutionAdapter for StopRaceAdapter {
        fn spawn(&self, _request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
            let release = Arc::clone(&self.release);
            Box::pin(
                async move { Ok(Arc::new(StopRaceHandle { release }) as Arc<dyn ExecutionHandle>) },
            )
        }
    }

    fn input(name: &str, enabled: bool, overlap_policy: OverlapPolicy) -> JobInput {
        JobInput {
            name: name.to_string(),
            command: "echo test".to_string(),
            cwd: None,
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            cron_expr: "* * * * * *".to_string(),
            enabled,
            overlap_policy,
            catch_up: true,
        }
    }

    fn service_input(name: &str, auto_start: bool) -> ServiceInput {
        ServiceInput {
            name: name.to_string(),
            command: "node server.js".to_string(),
            cwd: None,
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            restart_policy: RestartPolicy::Never,
            auto_start,
            health_tcp_address: None,
            health_tcp_port: None,
        }
    }

    fn service_with_policy(name: &str, restart_policy: RestartPolicy) -> ServiceInput {
        let mut input = service_input(name, false);
        input.restart_policy = restart_policy;
        input
    }

    async fn await_service_state(
        database: &DatabaseState,
        service_id: &str,
        expected: ServiceInstanceState,
    ) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if database
                    .get_service_instance(service_id)
                    .unwrap()
                    .map(|instance| instance.state)
                    == Some(expected)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("service instance did not reach the expected state");
    }

    #[test]
    fn config_defaults_to_one_second_and_four_global_permits() {
        let config = SchedulerConfig::default();
        assert_eq!(config.tick_interval, SCHEDULER_TICK);
        assert_eq!(config.max_concurrent_runs, DEFAULT_MAX_CONCURRENT_RUNS);
    }

    #[test]
    fn metadata_cas_failure_uses_the_storage_failure_code() {
        assert_eq!(
            failure_code_from_adapter(&AdapterError::new("metadata-cas-failed")),
            TerminalFailureCode::StorageFailed
        );
    }

    #[test]
    fn startup_selection_keeps_latest_gap_and_every_steady_occurrence() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let now = current_epoch_millis() / 1_000 * 1_000;
        let checkpoint = now - 10_000;
        let job = database
            .create_job_at(input("catch-up", true, OverlapPolicy::Queue), checkpoint)
            .unwrap();
        let scheduler = SchedulerCoordinator::with_config(
            database,
            Arc::new(UnavailableExecutionAdapter),
            SchedulerConfig::default().with_startup_cutoff(now - 5_000),
        );

        let due = scheduler.due_occurrences(&job, now).unwrap();
        assert_eq!(due.len(), 6);
        assert_eq!(due.first().map(|run| run.timestamp), Some(now - 5_000));
        assert_eq!(due.last().map(|run| run.timestamp), Some(now));
        assert!(due
            .windows(2)
            .all(|occurrences| occurrences[0].wall_key < occurrences[1].wall_key));
    }

    #[tokio::test]
    async fn stale_recovery_never_respawns_old_nonterminal_rows() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("stale", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let run = database.create_manual_run_at(&job.id, 1_001).unwrap();
        let owner = "owner";
        let token = "token";
        assert!(database.claim_run_starting(&run.id, owner, token).unwrap());
        assert!(database
            .mark_run_running(&run.id, owner, token, 1_002)
            .unwrap());
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());
        assert_eq!(scheduler.recover_stale_at(2_000).await.unwrap(), 1);
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Failed
        );
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stale_recovery_retries_after_transient_cleanup_failure() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("stale-retry", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let run = database.create_manual_run_at(&job.id, 1_001).unwrap();
        assert!(database
            .claim_run_starting(&run.id, "owner", "token")
            .unwrap());
        assert!(database
            .mark_run_running(&run.id, "owner", "token", 1_002)
            .unwrap());
        let adapter = Arc::new(TransientStaleRecoveryAdapter {
            attempts: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
        });
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        assert_eq!(scheduler.recover_stale_at(2_000).await.unwrap(), 0);
        assert!(!scheduler.cleanup_confirmed().await);
        assert!(scheduler.trigger_manual_at(&job.id, 2_001).await.is_err());
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Running
        );

        assert_eq!(scheduler.recover_stale_at(2_002).await.unwrap(), 1);
        assert!(scheduler.cleanup_confirmed().await);
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Failed
        );
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stale_recovery_does_not_terminate_current_owner_runs() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("current-owner", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let run = database.create_manual_run_at(&job.id, 1_001).unwrap();
        assert!(database
            .claim_run_starting(&run.id, "current-owner", "attempt")
            .unwrap());
        assert!(database
            .mark_run_running(&run.id, "current-owner", "attempt", 1_002)
            .unwrap());
        let adapter = Arc::new(TransientStaleRecoveryAdapter {
            attempts: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
        });
        let scheduler = SchedulerCoordinator::with_config(
            database.clone(),
            adapter.clone(),
            SchedulerConfig::default().with_owner_instance_id("current-owner"),
        );

        assert_eq!(scheduler.recover_stale_at(2_000).await.unwrap(), 0);
        assert_eq!(adapter.attempts.load(Ordering::SeqCst), 0);
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Running
        );
    }

    #[tokio::test]
    async fn manual_claim_uses_overlap_policy_and_skip_is_terminal() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("manual", true, OverlapPolicy::Skip), 1_000)
            .unwrap();
        let old = database.create_manual_run_at(&job.id, 1_001).unwrap();
        let owner = "owner";
        let token = "token";
        assert!(database.claim_run_starting(&old.id, owner, token).unwrap());
        assert!(database
            .mark_run_running(&old.id, owner, token, 1_002)
            .unwrap());
        let scheduler =
            SchedulerCoordinator::new(database.clone(), Arc::new(UnavailableExecutionAdapter));
        let skipped = scheduler.trigger_manual_at(&job.id, 1_003).await.unwrap();
        assert_eq!(skipped.status, RunStatus::Skipped);
        assert_eq!(
            database.list_runs(&job.id, 10, None, None).unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn manual_trigger_is_allowed_for_disabled_job() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("manual-disabled", false, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let scheduler = SchedulerCoordinator::new(database.clone(), MockAdapter::new());
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        assert_eq!(run.status, RunStatus::Running);
        scheduler.shutdown().await.unwrap();
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn manual_stop_confirms_terminal_state_and_emits_sanitized_event() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("manual-stop", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let listener = Arc::new(RecordingListener::default());
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::with_terminal_listener(
            database.clone(),
            adapter,
            listener.clone(),
        );

        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        assert_eq!(run.status, RunStatus::Running);
        let stopped = scheduler
            .stop_active_at(&job.id, 1_002)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stopped.status, RunStatus::Cancelled);
        let events = listener.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].job_id, job.id);
        assert_eq!(events[0].run_id, run.id);
        assert_eq!(events[0].status, RunStatus::Cancelled);
        assert_eq!(events[0].failure_code, None);
    }

    #[tokio::test]
    async fn dropping_manual_stop_waiter_does_not_strand_stopping_row() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("stop-drop", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let release = Arc::new(Notify::new());
        let scheduler = SchedulerCoordinator::new(
            database.clone(),
            Arc::new(StopRaceAdapter {
                release: Arc::clone(&release),
            }),
        );
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();

        let stop_task = tokio::spawn({
            let scheduler = scheduler.clone();
            let job_id = job.id.clone();
            async move { scheduler.stop_active_at(&job_id, 1_002).await }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if database.get_run(&run.id).unwrap().unwrap().status == RunStatus::Stopping {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "run after release: {:?}",
                database.get_run(&run.id).unwrap()
            )
        });
        stop_task.abort();
        let _ = stop_task.await;
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if database.get_run(&run.id).unwrap().unwrap().status == RunStatus::Cancelled {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "run after release: {:?}",
                database.get_run(&run.id).unwrap()
            )
        });
    }

    #[tokio::test]
    async fn adapter_failure_emits_fixed_notification_code_without_adapter_text() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("spawn-failure", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let listener = Arc::new(RecordingListener::default());
        let scheduler = SchedulerCoordinator::with_terminal_listener(
            database.clone(),
            Arc::new(UnavailableExecutionAdapter),
            listener.clone(),
        );

        assert!(scheduler.trigger_manual_at(&job.id, 1_001).await.is_err());
        let events = listener.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].failure_code,
            Some(TerminalFailureCode::SpawnFailed)
        );
        let run = database
            .list_runs(&job.id, 1, None, None)
            .unwrap()
            .remove(0);
        assert_eq!(run.status, RunStatus::Failed);
        assert_eq!(run.error_message.as_deref(), Some("spawn-failed"));
    }

    #[tokio::test]
    async fn shutdown_preserves_queued_intents_without_starting_them() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("queued", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let queued = database.create_manual_run_at(&job.id, 1_001).unwrap();
        let scheduler =
            SchedulerCoordinator::new(database.clone(), Arc::new(UnavailableExecutionAdapter));
        scheduler.shutdown().await.unwrap();
        assert_eq!(
            database.get_run(&queued.id).unwrap().unwrap().status,
            RunStatus::Queued
        );
    }

    #[tokio::test]
    async fn orderly_shutdown_terminates_active_adapter_runs_before_returning() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("active-shutdown", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter);
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        assert_eq!(run.status, RunStatus::Running);

        scheduler.shutdown().await.unwrap();
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
    }

    #[tokio::test]
    async fn shutdown_uses_one_global_deadline_and_preserves_timeout_rows_for_recovery() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let first_job = database
            .create_job_at(input("timeout-one", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let second_job = database
            .create_job_at(input("timeout-two", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let scheduler = SchedulerCoordinator::with_config(
            database.clone(),
            Arc::new(HangingTerminateAdapter),
            SchedulerConfig::default().with_shutdown_timeout(Duration::from_millis(50)),
        );
        let first = scheduler
            .trigger_manual_at(&first_job.id, 1_001)
            .await
            .unwrap();
        let second = scheduler
            .trigger_manual_at(&second_job.id, 1_001)
            .await
            .unwrap();

        let started = std::time::Instant::now();
        assert!(scheduler.shutdown().await.is_err());
        assert!(started.elapsed() < Duration::from_millis(250));
        assert_eq!(
            database.get_run(&first.id).unwrap().unwrap().status,
            RunStatus::Stopping
        );
        assert_eq!(
            database.get_run(&second.id).unwrap().unwrap().status,
            RunStatus::Stopping
        );
        assert_eq!(
            database
                .get_run(&first.id)
                .unwrap()
                .unwrap()
                .error_message
                .as_deref(),
            None
        );
    }

    #[tokio::test]
    async fn termination_timeout_keeps_nonterminal_row_until_retry_confirms_cleanup() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("retry-stop", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let scheduler = SchedulerCoordinator::with_config(
            database.clone(),
            Arc::new(RetryTerminateAdapter {
                attempts: Arc::clone(&attempts),
            }),
            SchedulerConfig::default().with_shutdown_timeout(Duration::from_millis(20)),
        );
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        assert!(scheduler.shutdown().await.is_err());
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Stopping
        );
        assert!(!scheduler.cleanup_confirmed().await);

        scheduler.shutdown().await.unwrap();
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
        assert!(scheduler.cleanup_confirmed().await);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn cancelled_enclosing_shutdown_preserves_handle_for_next_retry() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("shutdown-cancel", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let scheduler = SchedulerCoordinator::with_config(
            database.clone(),
            Arc::new(RetryTerminateAdapter {
                attempts: Arc::clone(&attempts),
            }),
            SchedulerConfig::default().with_shutdown_timeout(Duration::from_millis(100)),
        );
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(5), scheduler.shutdown())
                .await
                .is_err()
        );
        tokio::task::yield_now().await;
        assert!(!scheduler.cleanup_confirmed().await);
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Stopping
        );

        scheduler.shutdown().await.unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            database.get_run(&run.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
        assert!(scheduler.cleanup_confirmed().await);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn global_concurrency_cap_is_four_and_shutdown_unblocks_waiters() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let now = current_epoch_millis() / 1_000 * 1_000;
        for index in 0..5 {
            database
                .create_job_at(
                    input(&format!("cap-{index}"), true, OverlapPolicy::Queue),
                    now - 1_000,
                )
                .unwrap();
        }
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::with_config(
            database,
            adapter.clone(),
            SchedulerConfig::default()
                .with_startup_cutoff(now - 1_000)
                .with_max_concurrent_runs(4),
        );
        let tick = tokio::spawn({
            let scheduler = scheduler.clone();
            async move { scheduler.tick_at(now).await }
        });

        adapter.wait_for_starts(4).await;
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 4);
        assert_eq!(adapter.max_active.load(Ordering::SeqCst), 4);
        scheduler.request_shutdown();
        tick.await.unwrap().unwrap();
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn queued_manual_runs_drain_after_the_same_adapter_completion_path() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("queue", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let first = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        assert_eq!(first.status, RunStatus::Running);
        let second = scheduler.trigger_manual_at(&job.id, 1_002).await.unwrap();
        assert_eq!(second.status, RunStatus::Queued);
        adapter.finish_next(0).await;
        adapter.wait_for_starts(2).await;
        assert_eq!(
            database.get_run(&second.id).unwrap().unwrap().status,
            RunStatus::Running
        );
        adapter.finish_next(0).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if database.get_run(&second.id).unwrap().unwrap().status == RunStatus::Succeeded {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn metadata_cas_failure_terminates_spawned_child_fixture() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("metadata-cas", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let terminated = Arc::new(AtomicBool::new(false));
        let adapter = Arc::new(CasInvalidatingAdapter {
            database: Arc::clone(&database),
            terminated: Arc::clone(&terminated),
        });
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter);

        let result = scheduler.trigger_manual_at(&job.id, 1_001).await;
        assert!(matches!(
            result,
            Err(SchedulerError::Storage(StorageError::ConcurrentChange(message)))
                if message == "run metadata CAS"
        ));
        assert!(terminated.load(Ordering::SeqCst));
        assert_eq!(
            database
                .list_runs(&job.id, 10, None, None)
                .unwrap()
                .first()
                .map(|run| run.status),
            Some(RunStatus::Failed)
        );
    }

    #[tokio::test]
    async fn metadata_storage_error_retains_unverified_handle_for_shutdown_retry() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(
                input("metadata-storage-error", true, OverlapPolicy::Queue),
                1_000,
            )
            .unwrap();
        let scheduler = SchedulerCoordinator::with_config(
            database.clone(),
            Arc::new(MetadataErrorHangingAdapter),
            SchedulerConfig::default().with_shutdown_timeout(Duration::from_millis(20)),
        );
        let result = scheduler.trigger_manual_at(&job.id, 1_001).await;
        assert!(matches!(
            result,
            Err(SchedulerError::Storage(StorageError::Validation(_)))
        ));
        assert!(!scheduler.cleanup_confirmed().await);
        assert!(scheduler.trigger_manual_at(&job.id, 1_002).await.is_err());
        assert!(scheduler.shutdown().await.is_err());
        assert!(!scheduler.cleanup_confirmed().await);
    }

    #[tokio::test]
    async fn wait_error_without_cleanup_witness_is_retried_by_stop() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("wait-error", true, OverlapPolicy::Queue), 1_000)
            .unwrap();
        let scheduler = SchedulerCoordinator::new(
            database.clone(),
            Arc::new(WaitOutcomeAdapter {
                wait_error: AdapterError::new("wait-failed"),
            }),
        );
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        let stopped = scheduler.stop_active_at(&job.id, 1_002).await.unwrap();
        assert!(stopped.is_some());
        let terminal = database.get_run(&run.id).unwrap().unwrap();
        assert!(matches!(
            terminal.status,
            RunStatus::Failed | RunStatus::Cancelled
        ));
        assert!(scheduler.cleanup_confirmed().await);
    }

    #[tokio::test]
    async fn confirmed_wait_error_can_terminalize_after_tree_cleanup() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(
                input("confirmed-wait-error", true, OverlapPolicy::Queue),
                1_000,
            )
            .unwrap();
        let scheduler = SchedulerCoordinator::new(
            database.clone(),
            Arc::new(WaitOutcomeAdapter {
                wait_error: AdapterError::confirmed("log-write-failed"),
            }),
        );
        let run = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if matches!(
                    database.get_run(&run.id).unwrap().unwrap().status,
                    RunStatus::Failed | RunStatus::Cancelled
                ) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(scheduler.cleanup_confirmed().await);
    }

    #[tokio::test]
    async fn kill_previous_confirms_adapter_cleanup_before_starting_replacement() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let job = database
            .create_job_at(input("replace", true, OverlapPolicy::KillPrevious), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let first = scheduler.trigger_manual_at(&job.id, 1_001).await.unwrap();
        let replacement = scheduler.trigger_manual_at(&job.id, 1_002).await.unwrap();
        adapter.wait_for_starts(2).await;
        assert_eq!(
            database.get_run(&first.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
        assert_eq!(
            database.get_run(&replacement.id).unwrap().unwrap().status,
            RunStatus::Running
        );
        assert_eq!(replacement.blocked_by_run_id, None);
        adapter.finish_next(0).await;
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_start_links_run_and_transitions_to_running() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let instance = scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        assert_eq!(instance.state, ServiceInstanceState::Running);
        assert!(instance.generation >= 1);
        let run_id = instance.active_run_id.clone().unwrap();
        assert_eq!(
            database.get_run(&run_id).unwrap().unwrap().status,
            RunStatus::Running
        );
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_start_is_rejected_when_already_running() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        let error = scheduler
            .start_service_at(&service.id, 1_002)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            SchedulerError::Adapter { source, .. } if source.message == "service-already-running"
        ));
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_stop_returns_to_stopped_after_termination() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        let instance = scheduler
            .stop_service_at(&service.id, 1_002)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(instance.state, ServiceInstanceState::Stopped);
        assert_eq!(
            database
                .get_service_instance(&service.id)
                .unwrap()
                .unwrap()
                .state,
            ServiceInstanceState::Stopped
        );
        assert_eq!(database.active_process_run(&service.id).unwrap(), None);
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_stop_without_active_run_is_a_noop() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let scheduler =
            SchedulerCoordinator::new(database.clone(), Arc::new(UnavailableExecutionAdapter));
        assert!(scheduler
            .stop_service_at(&service.id, 1_001)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn service_restart_advances_generation_and_restarts() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let first = scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        let first_run = first.active_run_id.clone().unwrap();
        let second = scheduler
            .restart_service_at(&service.id, 1_002)
            .await
            .unwrap();
        assert_eq!(second.state, ServiceInstanceState::Running);
        assert!(second.generation > first.generation);
        let second_run = second.active_run_id.clone().unwrap();
        assert_ne!(first_run, second_run);
        assert_eq!(
            database.get_run(&first_run).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn failed_service_run_schedules_retry_for_on_failure_policy() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::OnFailure), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(1).await;
        await_service_state(&database, &service.id, ServiceInstanceState::RetryWaiting).await;
        let instance = database.get_service_instance(&service.id).unwrap().unwrap();
        assert!(instance.next_retry_at.is_some());
        assert_eq!(instance.consecutive_failures, 1);
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_stop_cancels_retry_waiting_backoff() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::OnFailure), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(1).await;
        await_service_state(&database, &service.id, ServiceInstanceState::RetryWaiting).await;

        let stopped = scheduler
            .stop_service_at(&service.id, current_epoch_millis())
            .await
            .unwrap()
            .expect("retry-waiting service should accept an explicit stop");
        assert_eq!(stopped.state, ServiceInstanceState::Stopped);
        assert_eq!(stopped.next_retry_at, None);

        scheduler
            .supervise_services(current_epoch_millis().saturating_add(60_000))
            .await;
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 1);
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_restart_bypasses_retry_waiting_backoff() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::OnFailure), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let first = scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(1).await;
        await_service_state(&database, &service.id, ServiceInstanceState::RetryWaiting).await;

        let restarted = scheduler
            .restart_service_at(&service.id, current_epoch_millis())
            .await
            .unwrap();
        assert_eq!(restarted.state, ServiceInstanceState::Running);
        assert!(restarted.generation > first.generation);
        assert_eq!(restarted.next_retry_at, None);
        assert_eq!(adapter.starts.load(Ordering::SeqCst), 2);

        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn clean_service_exit_stops_without_retry_for_on_failure_policy() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::OnFailure), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(0).await;
        await_service_state(&database, &service.id, ServiceInstanceState::Stopped).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn never_policy_stops_on_failed_run() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::Never), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(1).await;
        await_service_state(&database, &service.id, ServiceInstanceState::Stopped).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn always_policy_retries_clean_exit() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::Always), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(0).await;
        await_service_state(&database, &service.id, ServiceInstanceState::RetryWaiting).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn due_retry_is_restarted_into_a_new_generation() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_with_policy("web", RestartPolicy::OnFailure), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let first = scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(1).await;
        await_service_state(&database, &service.id, ServiceInstanceState::RetryWaiting).await;

        // Fast-forward past the 1-second backoff and run one supervisor pass.
        scheduler
            .supervise_services(crate::storage::current_epoch_millis() + 2_000)
            .await;
        await_service_state(&database, &service.id, ServiceInstanceState::Running).await;
        let second = database.get_service_instance(&service.id).unwrap().unwrap();
        assert!(second.generation > first.generation);
        assert_eq!(second.consecutive_failures, 0);
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn service_run_terminal_marks_instance_stopped() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let service = database
            .create_service_at(service_input("web", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        scheduler
            .start_service_at(&service.id, 1_001)
            .await
            .unwrap();
        adapter.finish_next(0).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if database
                    .get_service_instance(&service.id)
                    .unwrap()
                    .unwrap()
                    .state
                    == ServiceInstanceState::Stopped
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("service instance should return to stopped after run exit");
        scheduler.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn auto_start_brings_up_only_stopped_auto_start_services() {
        let database = Arc::new(DatabaseState::open_in_memory().unwrap());
        let auto = database
            .create_service_at(service_input("auto", true), 1_000)
            .unwrap();
        let manual = database
            .create_service_at(service_input("manual", false), 1_000)
            .unwrap();
        let adapter = MockAdapter::new();
        let scheduler = SchedulerCoordinator::new(database.clone(), adapter.clone());

        let started = scheduler.auto_start_services(1_001).await;
        assert_eq!(started, 1);
        assert_eq!(
            database
                .get_service_instance(&auto.id)
                .unwrap()
                .unwrap()
                .state,
            ServiceInstanceState::Running
        );
        assert_eq!(
            database
                .get_service_instance(&manual.id)
                .unwrap()
                .unwrap()
                .state,
            ServiceInstanceState::Stopped
        );
        adapter.finish_next(0).await;
        scheduler.shutdown().await.unwrap();
    }
}
