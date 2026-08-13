//! Phase 1 job scheduler coordinator.
//!
//! The coordinator owns orchestration, not platform process creation.  A
//! platform implementation supplies [`ExecutionAdapter`] and a mockable
//! [`ExecutionHandle`]; the scheduler commits a durable run/CAS before it
//! calls that boundary.  This keeps the 1-second tick, startup recovery,
//! overlap policy, queue ordering, and orderly shutdown testable on every
//! target without importing Windows or WSL APIs.

use crate::core::cron::CronSchedule;
use crate::core::models::{Job, Run, RunExecutionMetadata, RunStatus};
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
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use uuid::Uuid;

pub const SCHEDULER_TICK: Duration = Duration::from_secs(1);
pub const DEFAULT_MAX_CONCURRENT_RUNS: usize = 4;
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const DUE_BATCH_SIZE: usize = 1024;

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
}

impl AdapterError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
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
    owner_instance_id: String,
    attempt_token: String,
    handle: Arc<dyn ExecutionHandle>,
    _permit: OwnedSemaphorePermit,
}

struct SchedulerInner {
    database: Arc<DatabaseState>,
    adapter: Arc<dyn ExecutionAdapter>,
    config: SchedulerConfig,
    permits: Arc<Semaphore>,
    job_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    active: Mutex<HashMap<String, ActiveExecution>>,
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
        mut config: SchedulerConfig,
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
                config,
                permits,
                job_locks: Mutex::new(HashMap::new()),
                active: Mutex::new(HashMap::new()),
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
    /// Startup recovery happens once before the first interval tick.
    pub async fn run_until_shutdown(&self) -> Result<(), SchedulerError> {
        self.recover_stale_at(current_epoch_millis()).await?;
        let mut interval = tokio::time::interval(self.inner.config.tick_interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _ = self.inner.shutdown_notify.notified() => break,
                _ = interval.tick() => {
                    if self.is_shutdown_requested() {
                        break;
                    }
                    // A malformed/tampered job must not kill the daemon. The
                    // next tick retries it while other jobs continue.
                    let _ = self.tick_at(current_epoch_millis()).await;
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
        let claim = self.inner.database.claim_manual_run(job_id, now)?;
        self.process_claim_locked(&job, claim.clone(), now).await?;
        self.inner
            .database
            .get_run(&claim.run.id)?
            .ok_or_else(|| StorageError::NotFound(format!("run {}", claim.run.id)).into())
    }

    pub async fn recover_stale_at(&self, now: i64) -> Result<usize, SchedulerError> {
        Ok(self.inner.database.recover_stale_runs(now)?)
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
                adapter_error = Some(AdapterError::new(
                    "active execution handle is unavailable for kill-previous",
                ));
                let _ = self.inner.database.resolve_kill_previous(
                    old_run_id,
                    queued_run_id,
                    false,
                    adapter_error.as_ref().map(|error| error.message.as_str()),
                    now,
                )?;
                return Err(SchedulerError::Adapter {
                    run_id: old_run_id.to_string(),
                    source: adapter_error.expect("error is set above"),
                });
            };
            match tokio::time::timeout(self.inner.config.shutdown_timeout, handle.terminate()).await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => adapter_error = Some(error),
                Err(error) => {
                    adapter_error = Some(AdapterError::new(format!(
                        "kill-previous termination timed out: {error}"
                    )))
                }
            }
        }
        if let Some(error) = adapter_error {
            let _ = self.inner.database.resolve_kill_previous(
                old_run_id,
                queued_run_id,
                false,
                Some(&error.message),
                now,
            )?;
            return Err(SchedulerError::Adapter {
                run_id: old_run_id.to_string(),
                source: error,
            });
        }
        if !self
            .inner
            .database
            .resolve_kill_previous(old_run_id, queued_run_id, true, None, now)?
        {
            return Err(SchedulerError::Storage(StorageError::ConcurrentChange(
                "kill-previous cleanup pair".into(),
            )));
        }
        self.remove_active(old_run_id).await;
        self.drain_queue_locked(job).await
    }

    async fn start_claimed_run(&self, job: &Job, run: Run) -> Result<(), SchedulerError> {
        let owner = self.inner.config.owner_instance_id.clone();
        let attempt_token = Uuid::new_v4().to_string();
        if !self
            .inner
            .database
            .claim_run_starting(&run.id, &owner, &attempt_token)?
        {
            return Ok(());
        }
        let claimed = self
            .inner
            .database
            .get_run(&run.id)?
            .ok_or_else(|| StorageError::NotFound(format!("run {}", run.id)))?;
        let Some(handle) = self.start_owned_run(&claimed, &attempt_token).await? else {
            return Ok(());
        };
        self.spawn_monitor(claimed.id, job.id.clone(), owner, attempt_token, handle);
        Ok(())
    }

    async fn start_owned_run(
        &self,
        run: &Run,
        attempt_token: &str,
    ) -> Result<Option<Arc<dyn ExecutionHandle>>, SchedulerError> {
        let job = self
            .inner
            .database
            .get_job(&run.job_id)?
            .ok_or_else(|| StorageError::NotFound(format!("job {}", run.job_id)))?;
        let owner = self.inner.config.owner_instance_id.clone();
        if self.is_shutdown_requested() {
            self.cancel_starting_run(run, &owner, attempt_token);
            return Ok(None);
        }
        // A waiter may already be queued on the semaphore when shutdown is
        // requested.  It is safe to let it acquire a released permit: the
        // immediately-following shutdown check performs the durable cancel
        // CAS before the adapter boundary, so no process can be spawned.
        let permit = self
            .inner
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| SchedulerError::Join(error.to_string()))?;
        if self.is_shutdown_requested() {
            let _ = self.inner.database.finish_run(
                &run.id,
                &owner,
                attempt_token,
                RunStatus::Cancelled,
                None,
                Some("scheduler-shutdown"),
                current_epoch_millis(),
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
                self.inner.database.finish_run(
                    &run.id,
                    &owner,
                    attempt_token,
                    RunStatus::Failed,
                    None,
                    Some(&error.message),
                    current_epoch_millis(),
                )?;
                drop(permit);
                return Err(SchedulerError::Adapter {
                    run_id: run.id.clone(),
                    source: error,
                });
            }
        };
        let metadata = handle.metadata();
        if !self.inner.database.mark_run_running_with_metadata(
            &run.id,
            &owner,
            attempt_token,
            current_epoch_millis(),
            &metadata,
        )? {
            let _ = handle.terminate().await;
            let _ = self.inner.database.finish_run(
                &run.id,
                &owner,
                attempt_token,
                RunStatus::Failed,
                None,
                Some("execution-metadata-cas"),
                current_epoch_millis(),
            );
            drop(permit);
            return Err(SchedulerError::Storage(StorageError::ConcurrentChange(
                "run metadata CAS".into(),
            )));
        }

        self.inner.active.lock().await.insert(
            run.id.clone(),
            ActiveExecution {
                owner_instance_id: owner.clone(),
                attempt_token: attempt_token.to_string(),
                handle: Arc::clone(&handle),
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
        let (status, exit_code, error): (RunStatus, Option<i32>, Option<String>) = match result {
            Ok(exit) if exit.exit_code == Some(0) => (RunStatus::Succeeded, exit.exit_code, None),
            Ok(exit) => (
                RunStatus::Failed,
                exit.exit_code,
                Some("process-exit-nonzero".to_string()),
            ),
            Err(error) => (RunStatus::Failed, None, Some(error.message)),
        };
        let _ = self.inner.database.finish_run(
            &run_id,
            &owner,
            &attempt_token,
            status,
            exit_code,
            error.as_deref(),
            current_epoch_millis(),
        );
        self.remove_active(&run_id).await;
        let lock = self.job_mutex(&job_id).await.lock_owned().await;
        let _guard = lock;
        if !self.is_shutdown_requested() {
            let _ = self.drain_queue_locked_by_id(&job_id).await;
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
        let active = std::mem::take(&mut *self.inner.active.lock().await);
        let mut first_error = None;
        for (run_id, execution) in active {
            let result = tokio::time::timeout(
                self.inner.config.shutdown_timeout,
                execution.handle.terminate(),
            )
            .await;
            match result {
                Ok(Ok(exit)) => {
                    self.inner.database.finish_run(
                        &run_id,
                        &execution.owner_instance_id,
                        &execution.attempt_token,
                        RunStatus::Cancelled,
                        exit.exit_code,
                        Some("scheduler-shutdown"),
                        current_epoch_millis(),
                    )?;
                }
                Ok(Err(error)) => {
                    self.inner.database.finish_run(
                        &run_id,
                        &execution.owner_instance_id,
                        &execution.attempt_token,
                        RunStatus::Failed,
                        None,
                        Some(&error.message),
                        current_epoch_millis(),
                    )?;
                }
                Err(error) => {
                    self.inner.database.finish_run(
                        &run_id,
                        &execution.owner_instance_id,
                        &execution.attempt_token,
                        RunStatus::Failed,
                        None,
                        Some("scheduler-shutdown-timeout"),
                        current_epoch_millis(),
                    )?;
                    let shutdown_error = SchedulerError::Adapter {
                        run_id,
                        source: AdapterError::new(format!("shutdown timeout: {error}")),
                    };
                    if first_error.is_none() {
                        first_error = Some(shutdown_error);
                    }
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn remove_active(&self, run_id: &str) {
        self.inner.active.lock().await.remove(run_id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvironmentUpdate, JobInput, OverlapPolicy, TargetKind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::oneshot;

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

    #[test]
    fn config_defaults_to_one_second_and_four_global_permits() {
        let config = SchedulerConfig::default();
        assert_eq!(config.tick_interval, SCHEDULER_TICK);
        assert_eq!(config.max_concurrent_runs, DEFAULT_MAX_CONCURRENT_RUNS);
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
}
