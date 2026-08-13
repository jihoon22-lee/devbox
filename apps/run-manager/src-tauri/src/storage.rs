use crate::core::models::{
    ClaimResult, EnvironmentCiphertextUpdate, EnvironmentUpdate, Job, JobInput, JobKind,
    NewNotification, NotificationOutboxItem, OverlapPolicy, RestartPolicy, Run,
    RunExecutionMetadata, RunStatus, ServiceInput, ServiceInstance, ServiceInstanceState,
    TargetKind, DEFAULT_SERVICE_START_GRACE_MS,
};
use crate::core::policies::{
    decide_overlap, OverlapAction, OverlapPolicyInput, RunPolicySnapshot, StopAction,
};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
use std::fmt;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const SCHEMA_VERSION: i64 = 2;
pub const BUSY_TIMEOUT_MS: u64 = 5_000;

const MIGRATION_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL DEFAULT 'job' CHECK (kind IN ('job', 'service')),
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    cwd TEXT,
    target_kind TEXT NOT NULL DEFAULT 'windows'
        CHECK (target_kind IN ('windows', 'wsl')),
    target_distro TEXT,
    env_ciphertext BLOB,
    cron_expr TEXT CHECK (kind = 'service' OR cron_expr IS NOT NULL),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    overlap_policy TEXT NOT NULL DEFAULT 'skip'
        CHECK (overlap_policy IN ('skip', 'queue', 'kill-previous')),
    catch_up INTEGER NOT NULL DEFAULT 0 CHECK (catch_up IN (0, 1)),
    last_evaluated_at INTEGER,
    next_queue_sequence INTEGER NOT NULL DEFAULT 0 CHECK (next_queue_sequence >= 0),
    restart_policy TEXT CHECK (restart_policy IS NULL OR restart_policy IN ('never', 'on-failure', 'always')),
    auto_start INTEGER CHECK (auto_start IS NULL OR auto_start IN (0, 1)),
    health_tcp_address TEXT,
    health_tcp_port INTEGER CHECK (health_tcp_port IS NULL OR (health_tcp_port BETWEEN 1 AND 65535)),
    health_start_grace_ms INTEGER CHECK (health_start_grace_ms IS NULL OR health_start_grace_ms >= 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (target_kind = 'wsl' OR target_distro IS NULL),
    CHECK (target_kind = 'windows' OR (target_distro IS NOT NULL AND length(target_distro) > 0))
);

CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    scheduled_at INTEGER,
    occurrence_wall_key TEXT,
    queue_sequence INTEGER NOT NULL CHECK (queue_sequence > 0),
    blocked_by_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    started_at INTEGER,
    ended_at INTEGER,
    exit_code INTEGER,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued', 'starting', 'running', 'stopping', 'succeeded', 'failed', 'cancelled', 'skipped')),
    owner_instance_id TEXT,
    attempt_token TEXT,
    error_message TEXT,
    target_pid INTEGER,
    target_process_created_at INTEGER,
    target_pgid INTEGER,
    target_sid INTEGER,
    process_marker TEXT,
    log_dir TEXT,
    logs_deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    CHECK ((scheduled_at IS NULL) = (occurrence_wall_key IS NULL)),
    UNIQUE (job_id, occurrence_wall_key)
);

CREATE TABLE IF NOT EXISTS notification_outbox (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind = 'run-failed'),
    job_id TEXT REFERENCES jobs(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    error_code TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    delivered_at INTEGER
);

CREATE TABLE IF NOT EXISTS service_instances (
    job_id TEXT PRIMARY KEY NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    active_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'stopped'
        CHECK (state IN ('stopped', 'starting', 'running', 'stopping', 'retry_waiting')),
    owner_instance_id TEXT,
    attempt_token TEXT,
    next_retry_at INTEGER,
    consecutive_failures INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jobs_enabled_kind
    ON jobs (enabled, kind);
CREATE INDEX IF NOT EXISTS idx_runs_job_queue
    ON runs (job_id, queue_sequence);
CREATE INDEX IF NOT EXISTS idx_runs_job_scheduled_at
    ON runs (job_id, scheduled_at);
CREATE INDEX IF NOT EXISTS idx_runs_job_created_at
    ON runs (job_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_status
    ON runs (status);
CREATE INDEX IF NOT EXISTS idx_notification_outbox_pending
    ON notification_outbox (delivered_at, created_at);
CREATE INDEX IF NOT EXISTS idx_notification_outbox_job_created
    ON notification_outbox (job_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_service_instances_state_retry
    ON service_instances (state, next_retry_at);
"#;

/// A process-wide SQLite connection. Every connection is configured with the
/// same foreign-key and busy-timeout policy before migrations run.
pub struct DatabaseState {
    connection: Mutex<Connection>,
}

#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    Validation(String),
    NotFound(String),
    JobDisabled(String),
    ConcurrentChange(String),
    ConnectionPoisoned,
}

/// The durable result of applying the same overlap policy to a scheduled or
/// manual run.  A queued result is only an intent; the scheduler must still
/// win the `queued -> starting` CAS before it asks an adapter to spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimedRunAction {
    Existing,
    Start,
    Queue,
    Skip,
    KillPrevious,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyClaim {
    pub inserted: bool,
    pub run: Run,
    pub action: ClaimedRunAction,
    pub previous_run_id: Option<String>,
    /// Status observed for the previous active row before the claim's
    /// kill-previous transition.  A queued previous row has no process/tree
    /// to terminate; starting/running/stopping rows require adapter
    /// confirmation before the new row can be unblocked.
    pub previous_run_status: Option<RunStatus>,
    pub stop_requested: bool,
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "database error: {error}"),
            Self::Validation(error) => formatter.write_str(error),
            Self::NotFound(entity) => write!(formatter, "{entity} not found"),
            Self::JobDisabled(id) => write!(formatter, "job {id} is disabled"),
            Self::ConcurrentChange(entity) => {
                write!(formatter, "concurrent change while updating {entity}")
            }
            Self::ConnectionPoisoned => formatter.write_str("database connection is poisoned"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl DatabaseState {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut connection = Connection::open(path)?;
        configure(&connection)?;
        migrate_connection(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let mut connection = Connection::open_in_memory()?;
        configure(&connection)?;
        migrate_connection(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn migrate(&self) -> Result<(), StorageError> {
        let mut connection = self.lock_mut()?;
        migrate_connection(&mut connection).map_err(StorageError::from)
    }

    pub fn is_ready(&self) -> bool {
        self.connection.lock().ok().and_then(|connection| {
            connection
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .ok()
        }) == Some(1)
    }

    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let connection = self.lock()?;
        let value: String = connection.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        value
            .parse::<i64>()
            .map_err(|error| StorageError::Validation(format!("invalid schema version: {error}")))
    }

    pub fn create_job(&self, input: JobInput) -> Result<Job, StorageError> {
        self.create_job_at(input, current_epoch_millis())
    }

    pub fn create_job_at(&self, input: JobInput, now: i64) -> Result<Job, StorageError> {
        self.create_job_with_ciphertext_at(input, None, now)
    }

    pub(crate) fn create_job_with_ciphertext_at(
        &self,
        input: JobInput,
        env_ciphertext: Option<Vec<u8>>,
        now: i64,
    ) -> Result<Job, StorageError> {
        ensure_encrypted_input(&input)?;
        input
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = Uuid::new_v4().to_string();
        let checkpoint = input.enabled.then_some(now);
        transaction.execute(
            "INSERT INTO jobs (
                id, kind, name, command, cwd, target_kind, target_distro,
                env_ciphertext, cron_expr, enabled, overlap_policy, catch_up,
                last_evaluated_at, next_queue_sequence, created_at, updated_at
             ) VALUES (?, 'job', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
            params![
                id,
                input.name,
                input.command,
                input.cwd,
                input.target_kind.as_str(),
                input.target_distro,
                env_ciphertext,
                input.cron_expr,
                bool_to_sql(input.enabled),
                input.overlap_policy.as_str(),
                bool_to_sql(input.catch_up),
                checkpoint,
                now,
                now,
            ],
        )?;
        let job = fetch_job(&transaction, &id)?
            .ok_or_else(|| StorageError::NotFound(format!("newly created job {id}")))?;
        transaction.commit()?;
        Ok(job)
    }

    pub fn get_job(&self, id: &str) -> Result<Option<Job>, StorageError> {
        let connection = self.lock()?;
        fetch_phase1_job(&connection, id).map_err(StorageError::from)
    }

    /// Return ciphertext only for the execution layer. The normal read DTO
    /// deliberately exposes only `env_configured`.
    pub fn get_job_environment_ciphertext(
        &self,
        id: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ? AND kind = 'job'",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound(format!("job {id}")))
    }

    pub fn list_jobs(&self) -> Result<Vec<Job>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE kind = 'job' ORDER BY name COLLATE NOCASE, id"
        ))?;
        let rows = statement.query_map([], row_to_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    pub fn create_service(&self, input: ServiceInput) -> Result<Job, StorageError> {
        self.create_service_at(input, current_epoch_millis())
    }

    pub fn create_service_at(&self, input: ServiceInput, now: i64) -> Result<Job, StorageError> {
        self.create_service_with_ciphertext_at(input, EnvironmentCiphertextUpdate::Clear, now)
    }

    pub(crate) fn create_service_with_ciphertext_at(
        &self,
        input: ServiceInput,
        environment: EnvironmentCiphertextUpdate,
        now: i64,
    ) -> Result<Job, StorageError> {
        ensure_encrypted_service_input(&input)?;
        input
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = Uuid::new_v4().to_string();
        let env_ciphertext = match environment {
            EnvironmentCiphertextUpdate::Keep | EnvironmentCiphertextUpdate::Clear => None,
            EnvironmentCiphertextUpdate::Replace(ciphertext) => Some(ciphertext),
        };
        transaction.execute(
            "INSERT INTO jobs (
                id, kind, name, command, cwd, target_kind, target_distro,
                env_ciphertext, cron_expr, enabled, overlap_policy, catch_up,
                last_evaluated_at, next_queue_sequence, restart_policy, auto_start,
                health_tcp_address, health_tcp_port, health_start_grace_ms,
                created_at, updated_at
             ) VALUES (?, 'service', ?, ?, ?, ?, ?, ?, NULL, 0, 'skip', 0,
                       NULL, 0, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                input.name,
                input.command,
                input.cwd,
                input.target_kind.as_str(),
                input.target_distro,
                env_ciphertext,
                input.restart_policy.as_str(),
                bool_to_sql(input.auto_start),
                input.health_tcp_address,
                input.health_tcp_port,
                DEFAULT_SERVICE_START_GRACE_MS,
                now,
                now,
            ],
        )?;
        transaction.execute(
            "INSERT INTO service_instances (job_id, updated_at) VALUES (?, ?)",
            params![id, now],
        )?;
        let service = fetch_service(&transaction, &id)?
            .ok_or_else(|| StorageError::NotFound(format!("newly created service {id}")))?;
        transaction.commit()?;
        Ok(service)
    }

    pub fn get_service(&self, id: &str) -> Result<Option<Job>, StorageError> {
        let connection = self.lock()?;
        fetch_service(&connection, id).map_err(StorageError::from)
    }

    pub fn list_services(&self) -> Result<Vec<Job>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE kind = 'service' ORDER BY name COLLATE NOCASE, id"
        ))?;
        let rows = statement.query_map([], row_to_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    pub fn update_service(&self, id: &str, input: ServiceInput) -> Result<Job, StorageError> {
        self.update_service_at(id, input, current_epoch_millis())
    }

    pub fn update_service_at(
        &self,
        id: &str,
        input: ServiceInput,
        now: i64,
    ) -> Result<Job, StorageError> {
        self.update_service_with_ciphertext_at(id, input, EnvironmentCiphertextUpdate::Keep, now)
    }

    pub(crate) fn update_service_with_ciphertext_at(
        &self,
        id: &str,
        input: ServiceInput,
        environment: EnvironmentCiphertextUpdate,
        now: i64,
    ) -> Result<Job, StorageError> {
        ensure_encrypted_service_input(&input)?;
        input
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_service(&transaction, id)?;
        let (environment_action, environment_ciphertext) = match environment {
            EnvironmentCiphertextUpdate::Keep => ("keep", None),
            EnvironmentCiphertextUpdate::Replace(ciphertext) => ("replace", Some(ciphertext)),
            EnvironmentCiphertextUpdate::Clear => ("clear", None),
        };
        let changed = transaction.execute(
            "UPDATE jobs SET
                name = ?, command = ?, cwd = ?, target_kind = ?, target_distro = ?,
                env_ciphertext = CASE ?
                    WHEN 'keep' THEN env_ciphertext
                    WHEN 'replace' THEN ?
                    ELSE NULL
                END,
                restart_policy = ?, auto_start = ?, health_tcp_address = ?, health_tcp_port = ?,
                health_start_grace_ms = ?, updated_at = ?
             WHERE id = ? AND kind = 'service'",
            params![
                input.name,
                input.command,
                input.cwd,
                input.target_kind.as_str(),
                input.target_distro,
                environment_action,
                environment_ciphertext,
                input.restart_policy.as_str(),
                bool_to_sql(input.auto_start),
                input.health_tcp_address,
                input.health_tcp_port,
                DEFAULT_SERVICE_START_GRACE_MS,
                now,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::NotFound(format!("service {id}")));
        }
        let service = fetch_service(&transaction, id)?
            .ok_or_else(|| StorageError::NotFound(format!("updated service {id}")))?;
        transaction.execute(
            "INSERT INTO service_instances (job_id, updated_at) VALUES (?, ?)
             ON CONFLICT(job_id) DO UPDATE SET updated_at = excluded.updated_at",
            params![id, now],
        )?;
        transaction.commit()?;
        Ok(service)
    }

    pub fn delete_service(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.lock_mut()?;
        let deleted =
            connection.execute("DELETE FROM jobs WHERE id = ? AND kind = 'service'", [id])?;
        Ok(deleted == 1)
    }

    pub fn get_service_instance(
        &self,
        service_id: &str,
    ) -> Result<Option<ServiceInstance>, StorageError> {
        let connection = self.lock()?;
        fetch_service_instance(&connection, service_id).map_err(StorageError::from)
    }

    pub fn update_job(&self, id: &str, input: JobInput) -> Result<Job, StorageError> {
        self.update_job_at(id, input, current_epoch_millis())
    }

    pub fn update_job_at(&self, id: &str, input: JobInput, now: i64) -> Result<Job, StorageError> {
        self.update_job_with_ciphertext_at(id, input, EnvironmentCiphertextUpdate::Keep, now)
    }

    pub(crate) fn update_job_with_ciphertext_at(
        &self,
        id: &str,
        input: JobInput,
        environment: EnvironmentCiphertextUpdate,
        now: i64,
    ) -> Result<Job, StorageError> {
        ensure_encrypted_input(&input)?;
        input
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = ensure_job(&transaction, id)?;
        let checkpoint_reset = current.cron_expr.as_deref() != Some(input.cron_expr.as_str())
            || current.catch_up != input.catch_up
            || current.enabled != input.enabled;
        let checkpoint = if checkpoint_reset {
            Some(now)
        } else {
            current.last_evaluated_at
        };
        let (environment_action, environment_ciphertext) = match environment {
            EnvironmentCiphertextUpdate::Keep => ("keep", None),
            EnvironmentCiphertextUpdate::Replace(ciphertext) => ("replace", Some(ciphertext)),
            EnvironmentCiphertextUpdate::Clear => ("clear", None),
        };
        let changed = transaction.execute(
            "UPDATE jobs SET
                name = ?, command = ?, cwd = ?, target_kind = ?, target_distro = ?,
                env_ciphertext = CASE ?
                    WHEN 'keep' THEN env_ciphertext
                    WHEN 'replace' THEN ?
                    ELSE NULL
                END,
                cron_expr = ?, enabled = ?, overlap_policy = ?,
                catch_up = ?, last_evaluated_at = ?, updated_at = ?
             WHERE id = ? AND kind = 'job'",
            params![
                input.name,
                input.command,
                input.cwd,
                input.target_kind.as_str(),
                input.target_distro,
                environment_action,
                environment_ciphertext,
                input.cron_expr,
                bool_to_sql(input.enabled),
                input.overlap_policy.as_str(),
                bool_to_sql(input.catch_up),
                checkpoint,
                now,
                id,
            ],
        )?;
        if changed != 1 {
            return Err(StorageError::NotFound(format!("job {id}")));
        }
        let job = fetch_job(&transaction, id)?
            .ok_or_else(|| StorageError::NotFound(format!("updated job {id}")))?;
        transaction.commit()?;
        Ok(job)
    }

    pub fn set_job_enabled(&self, id: &str, enabled: bool) -> Result<Job, StorageError> {
        self.set_job_enabled_at(id, enabled, current_epoch_millis())
    }

    pub fn set_job_enabled_at(
        &self,
        id: &str,
        enabled: bool,
        now: i64,
    ) -> Result<Job, StorageError> {
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = ensure_job(&transaction, id)?;
        let checkpoint = if current.enabled == enabled {
            current.last_evaluated_at
        } else {
            Some(now)
        };
        transaction.execute(
            "UPDATE jobs SET enabled = ?, last_evaluated_at = ?, updated_at = ?
             WHERE id = ? AND kind = 'job'",
            params![bool_to_sql(enabled), checkpoint, now, id],
        )?;
        let job = fetch_job(&transaction, id)?
            .ok_or_else(|| StorageError::NotFound(format!("updated job {id}")))?;
        transaction.commit()?;
        Ok(job)
    }

    pub fn delete_job(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.lock_mut()?;
        let deleted = connection.execute("DELETE FROM jobs WHERE id = ? AND kind = 'job'", [id])?;
        Ok(deleted == 1)
    }

    /// Inserts a manual queued run. Manual rows intentionally use NULL for
    /// both occurrence columns, so SQLite permits multiple manual runs for a
    /// single job while the same queue allocator still preserves FIFO order.
    pub fn create_manual_run(&self, job_id: &str) -> Result<Run, StorageError> {
        self.create_manual_run_at(job_id, current_epoch_millis())
    }

    pub fn create_manual_run_at(&self, job_id: &str, now: i64) -> Result<Run, StorageError> {
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_job(&transaction, job_id)?;
        let sequence = allocate_queue_sequence(&transaction, job_id)?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO runs (id, job_id, scheduled_at, occurrence_wall_key,
                queue_sequence, status, created_at)
             VALUES (?, ?, NULL, NULL, ?, 'queued', ?)",
            params![id, job_id, sequence, now],
        )?;
        let run = fetch_run(&transaction, &id)?
            .ok_or_else(|| StorageError::NotFound(format!("newly created run {id}")))?;
        transaction.commit()?;
        Ok(run)
    }

    /// List enabled Phase 1 jobs for one scheduler tick.  Services remain
    /// excluded until the Phase 2 service coordinator owns their lifecycle.
    pub fn list_enabled_jobs(&self) -> Result<Vec<Job>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs
             WHERE kind = 'job' AND enabled = 1
             ORDER BY id"
        ))?;
        let rows = statement.query_map([], row_to_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    /// Return all queued rows in durable FIFO order.  A blocked first row is
    /// intentionally included so the coordinator cannot let a later row
    /// overtake a kill-previous cleanup barrier.
    pub fn list_queued_runs(&self, job_id: &str) -> Result<Vec<Run>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs
             WHERE job_id = ? AND status = 'queued'
             ORDER BY queue_sequence, id"
        ))?;
        let rows = statement.query_map([job_id], row_to_run)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    /// Return the earliest active row that currently blocks a job.  The
    /// durable active set is deliberately the same set used by the pure
    /// overlap policy (`queued`, `starting`, `running`, `stopping`).
    pub fn active_run(&self, job_id: &str) -> Result<Option<Run>, StorageError> {
        let connection = self.lock()?;
        fetch_active_run(&connection, job_id).map_err(StorageError::from)
    }

    /// Return an active process row, excluding durable queued intents.  Queue
    /// workers use this guard before their starting CAS; the policy-facing
    /// `active_run` above intentionally includes queued rows.
    pub fn active_process_run(&self, job_id: &str) -> Result<Option<Run>, StorageError> {
        let connection = self.lock()?;
        fetch_active_process_run(&connection, job_id).map_err(StorageError::from)
    }

    /// Advance a job checkpoint without claiming an occurrence.  This is
    /// used when a startup gap is intentionally skipped (`catch_up = false`)
    /// and when a tick has no due rows.  The update is monotonic and remains a
    /// short transaction, never held across an adapter await.
    pub fn advance_job_checkpoint_at(
        &self,
        job_id: &str,
        checkpoint: i64,
        now: i64,
    ) -> Result<Job, StorageError> {
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_job(&transaction, job_id)?;
        transaction.execute(
            "UPDATE jobs SET last_evaluated_at = CASE
                WHEN last_evaluated_at IS NULL OR last_evaluated_at < ?1 THEN ?1
                ELSE last_evaluated_at END,
                updated_at = ?2
             WHERE id = ?3 AND kind = 'job'",
            params![checkpoint, now, job_id],
        )?;
        let job = fetch_job(&transaction, job_id)?
            .ok_or_else(|| StorageError::NotFound(format!("job {job_id}")))?;
        transaction.commit()?;
        Ok(job)
    }

    /// Claim one automatic occurrence and apply its overlap policy inside the
    /// same `BEGIN IMMEDIATE` transaction.  The unique wall key prevents a
    /// second daemon/tick from creating a duplicate row; the active-run CAS
    /// prevents a kill-previous request from racing a terminal transition.
    pub fn claim_scheduled_run(
        &self,
        job_id: &str,
        scheduled_at: i64,
        occurrence_wall_key: &str,
        now: i64,
    ) -> Result<PolicyClaim, StorageError> {
        if occurrence_wall_key.is_empty() {
            return Err(StorageError::Validation(
                "occurrence_wall_key must not be empty".to_string(),
            ));
        }
        if occurrence_wall_key.contains('\0') {
            return Err(StorageError::Validation(
                "occurrence_wall_key contains a NUL byte".to_string(),
            ));
        }
        self.claim_run_with_policy(
            job_id,
            Some(scheduled_at),
            Some(occurrence_wall_key),
            now,
            false,
        )
    }

    /// Claim one manual run through exactly the same overlap-policy/queue/CAS
    /// path as scheduled runs.  Manual runs keep NULL occurrence columns so
    /// SQLite permits multiple explicit invocations.
    pub fn claim_manual_run(&self, job_id: &str, now: i64) -> Result<PolicyClaim, StorageError> {
        // `enabled` gates automatic scheduling only.  An explicit manual
        // invocation is still allowed for a disabled job, but it must use the
        // exact same overlap/queue decision and CAS path as scheduled work.
        self.claim_run_with_policy(job_id, None, None, now, true)
    }

    /// CAS a durable queued intent into an owned starting attempt.  A false
    /// result is not an error: another coordinator already won the row.
    pub fn claim_run_starting(
        &self,
        run_id: &str,
        owner_instance_id: &str,
        attempt_token: &str,
    ) -> Result<bool, StorageError> {
        validate_token("owner_instance_id", owner_instance_id)?;
        validate_token("attempt_token", attempt_token)?;
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE runs SET status = 'starting', owner_instance_id = ?, attempt_token = ?
             WHERE id = ? AND status = 'queued' AND blocked_by_run_id IS NULL",
            params![owner_instance_id, attempt_token, run_id],
        )?;
        Ok(changed == 1)
    }

    /// CAS the external spawn handshake into `running`.
    pub fn mark_run_running(
        &self,
        run_id: &str,
        owner_instance_id: &str,
        attempt_token: &str,
        started_at: i64,
    ) -> Result<bool, StorageError> {
        self.mark_run_running_with_metadata(
            run_id,
            owner_instance_id,
            attempt_token,
            started_at,
            &RunExecutionMetadata::default(),
        )
    }

    /// Persist the app-owned log directory while a run is still `starting`.
    /// This CAS is deliberately separate from process identity because a
    /// failed spawn still has useful lossless logs (for example, adapter
    /// diagnostics) but must never be marked running.
    pub fn mark_run_log_dir(
        &self,
        run_id: &str,
        owner_instance_id: &str,
        attempt_token: &str,
        log_dir: &str,
    ) -> Result<bool, StorageError> {
        validate_token("owner_instance_id", owner_instance_id)?;
        validate_token("attempt_token", attempt_token)?;
        validate_optional_text("log_dir", Some(log_dir))?;
        if Path::new(log_dir).is_absolute()
            || log_dir.contains("..")
            || !log_dir.starts_with("logs/runs/")
        {
            return Err(StorageError::Validation(
                "log_dir must be an app-owned relative path".to_string(),
            ));
        }
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE runs SET log_dir = ?, logs_deleted_at = NULL
             WHERE id = ? AND status = 'starting'
               AND owner_instance_id = ? AND attempt_token = ?",
            params![log_dir, run_id, owner_instance_id, attempt_token],
        )?;
        Ok(changed == 1)
    }

    /// Complete the owner/attempt CAS after the platform handshake and log
    /// setup have succeeded. All identity columns are written together so a
    /// stale attempt can never leave a partially trusted process identity.
    pub fn mark_run_running_with_metadata(
        &self,
        run_id: &str,
        owner_instance_id: &str,
        attempt_token: &str,
        started_at: i64,
        metadata: &RunExecutionMetadata,
    ) -> Result<bool, StorageError> {
        validate_token("owner_instance_id", owner_instance_id)?;
        validate_token("attempt_token", attempt_token)?;
        validate_optional_text("log_dir", metadata.log_dir.as_deref())?;
        validate_optional_text("process_marker", metadata.process_marker.as_deref())?;
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE runs SET status = 'running', started_at = ?,
                    log_dir = COALESCE(?, log_dir),
                    target_pid = ?, target_process_created_at = ?,
                    target_pgid = ?, target_sid = ?, process_marker = ?
             WHERE id = ? AND status = 'starting'
               AND owner_instance_id = ? AND attempt_token = ?",
            params![
                started_at,
                metadata.log_dir,
                metadata.target_pid,
                metadata.target_process_created_at,
                metadata.target_pgid,
                metadata.target_sid,
                metadata.process_marker,
                run_id,
                owner_instance_id,
                attempt_token,
            ],
        )?;
        Ok(changed == 1)
    }

    /// CAS a normal monitor completion.  `Cancelled` is reserved for an
    /// adapter-confirmed stop; a process exit before that confirmation is
    /// recorded as succeeded/failed by the caller.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_run(
        &self,
        run_id: &str,
        owner_instance_id: &str,
        attempt_token: &str,
        status: RunStatus,
        exit_code: Option<i32>,
        error_message: Option<&str>,
        ended_at: i64,
    ) -> Result<bool, StorageError> {
        if !matches!(
            status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Err(StorageError::Validation(
                "finish_run requires a terminal status".to_string(),
            ));
        }
        validate_optional_text("error_message", error_message)?;
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE runs SET status = ?, ended_at = ?, exit_code = ?, error_message = ?
             WHERE id = ? AND status IN ('starting', 'running', 'stopping')
               AND owner_instance_id = ? AND attempt_token = ?",
            params![
                status.as_str(),
                ended_at,
                exit_code,
                error_message,
                run_id,
                owner_instance_id,
                attempt_token,
            ],
        )?;
        Ok(changed == 1)
    }

    /// Apply the result of an adapter-confirmed kill-previous cleanup.  Both
    /// rows are updated in one transaction; no new spawn is allowed until the
    /// confirmed path has unblocked the queued row.
    pub fn resolve_kill_previous(
        &self,
        old_run_id: &str,
        queued_run_id: &str,
        confirmed: bool,
        error_message: Option<&str>,
        now: i64,
    ) -> Result<bool, StorageError> {
        validate_optional_text("error_message", error_message)?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (old_status, queued_status, blocker): (String, String, Option<String>) = transaction
            .query_row(
                "SELECT old.status, queued.status, queued.blocked_by_run_id
                 FROM runs AS old JOIN runs AS queued
                   ON queued.id = ?2
                 WHERE old.id = ?1",
                params![old_run_id, queued_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound("kill-previous run pair".to_string()))?;
        if old_status != RunStatus::Stopping.as_str()
            || queued_status != RunStatus::Queued.as_str()
            || blocker.as_deref() != Some(old_run_id)
        {
            return Ok(false);
        }

        let (old_status, queued_status, queued_blocker, old_error, queued_error) = if confirmed {
            (RunStatus::Cancelled, RunStatus::Queued, None, None, None)
        } else {
            (
                RunStatus::Failed,
                RunStatus::Failed,
                Some(old_run_id),
                error_message,
                error_message,
            )
        };
        let old_changed = transaction.execute(
            "UPDATE runs SET status = ?, ended_at = ?, error_message = ?
             WHERE id = ? AND status = 'stopping'",
            params![old_status.as_str(), now, old_error, old_run_id],
        )?;
        let queued_changed = transaction.execute(
            "UPDATE runs SET status = ?, ended_at = CASE WHEN ? = 'failed' THEN ? ELSE ended_at END,
                    blocked_by_run_id = ?, error_message = ?
             WHERE id = ? AND status = 'queued' AND blocked_by_run_id = ?",
            params![
                queued_status.as_str(),
                queued_status.as_str(),
                now,
                queued_blocker,
                queued_error,
                queued_run_id,
                old_run_id,
            ],
        )?;
        if old_changed != 1 || queued_changed != 1 {
            return Err(StorageError::ConcurrentChange(
                "kill-previous cleanup pair".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Startup is fail-safe: rows from a previous daemon that were in
    /// `starting`, `running`, or `stopping` are terminalized as failed and
    /// never respawned.  Blocked queued rows linked to stale stopping rows are
    /// failed in the same transaction; unblocked queued rows remain durable
    /// resume work for the normal FIFO worker.
    pub fn recover_stale_runs(&self, now: i64) -> Result<usize, StorageError> {
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = transaction.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs
             WHERE status IN ('starting', 'running', 'stopping')
             ORDER BY queue_sequence, id"
        ))?;
        let stale = statement
            .query_map([], row_to_run)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let mut changed = 0;
        for run in stale {
            let updated = transaction.execute(
                "UPDATE runs SET status = 'failed', ended_at = ?,
                        error_message = 'scheduler-stale-recovery'
                 WHERE id = ? AND status IN ('starting', 'running', 'stopping')",
                params![now, run.id],
            )?;
            if updated == 1 {
                changed += 1;
                transaction.execute(
                    "UPDATE runs SET status = 'failed', ended_at = ?,
                            error_message = 'scheduler-stale-recovery'
                     WHERE status = 'queued' AND blocked_by_run_id = ?",
                    params![now, run.id],
                )?;
            }
        }
        transaction.commit()?;
        Ok(changed)
    }

    fn claim_run_with_policy(
        &self,
        job_id: &str,
        scheduled_at: Option<i64>,
        occurrence_wall_key: Option<&str>,
        now: i64,
        allow_disabled: bool,
    ) -> Result<PolicyClaim, StorageError> {
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = ensure_job(&transaction, job_id)?;
        if !job.enabled && !allow_disabled {
            return Err(StorageError::JobDisabled(job_id.to_string()));
        }
        let overlap_policy = job.overlap_policy;

        if let Some(occurrence_wall_key) = occurrence_wall_key {
            if let Some(run) = fetch_run_by_occurrence(&transaction, job_id, occurrence_wall_key)? {
                advance_checkpoint(&transaction, job_id, scheduled_at.unwrap_or(now))?;
                transaction.commit()?;
                return Ok(PolicyClaim {
                    inserted: false,
                    run,
                    action: ClaimedRunAction::Existing,
                    previous_run_id: None,
                    previous_run_status: None,
                    stop_requested: false,
                });
            }
        }

        let active_run = fetch_active_run(&transaction, job_id)?;
        let sequence = allocate_queue_sequence(&transaction, job_id)?;
        let decision = decide_overlap(&OverlapPolicyInput {
            enabled: job.enabled || allow_disabled,
            overlap_policy,
            scheduled_at,
            queue_sequence: sequence,
            active_run: active_run.as_ref().map(|run| {
                RunPolicySnapshot::new(
                    run.id.clone(),
                    run.status,
                    run.queue_sequence,
                    run.blocked_by_run_id.clone(),
                )
            }),
        });
        let intent = decision.run_intent.ok_or_else(|| {
            StorageError::Validation("enabled overlap decision did not produce an intent".into())
        })?;
        let previous_run_status = decision.stop_intent.as_ref().map(|_| {
            active_run
                .as_ref()
                .expect("stop intent has active run")
                .status
        });
        let mut stop_requested = false;
        if let Some(stop) = &decision.stop_intent {
            if stop.action == StopAction::RequestStop {
                let changed = transaction.execute(
                    "UPDATE runs SET status = 'stopping'
                     WHERE id = ? AND status = ?",
                    params![stop.run_id, stop.from_status.as_str()],
                )?;
                if changed != 1 {
                    return Err(StorageError::ConcurrentChange(
                        "kill-previous active run".to_string(),
                    ));
                }
                stop_requested = true;
            }
        }

        let id = Uuid::new_v4().to_string();
        let (status, ended_at) = if intent.status == RunStatus::Skipped {
            (RunStatus::Skipped, Some(now))
        } else {
            (RunStatus::Queued, None)
        };
        transaction.execute(
            "INSERT INTO runs (
                id, job_id, scheduled_at, occurrence_wall_key, queue_sequence,
                blocked_by_run_id, status, ended_at, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                job_id,
                scheduled_at,
                occurrence_wall_key,
                sequence,
                intent.blocked_by_run_id,
                status.as_str(),
                ended_at,
                now,
            ],
        )?;
        if let Some(scheduled_at) = scheduled_at {
            advance_checkpoint(&transaction, job_id, scheduled_at)?;
        }
        let run = fetch_run(&transaction, &id)?
            .ok_or_else(|| StorageError::NotFound(format!("newly claimed run {id}")))?;
        transaction.commit()?;
        let action = match decision.action {
            OverlapAction::Start => ClaimedRunAction::Start,
            OverlapAction::Skip => ClaimedRunAction::Skip,
            OverlapAction::Queue => ClaimedRunAction::Queue,
            OverlapAction::KillPrevious => ClaimedRunAction::KillPrevious,
            OverlapAction::Disabled => return Err(StorageError::JobDisabled(job_id.to_string())),
        };
        Ok(PolicyClaim {
            inserted: true,
            run,
            action,
            previous_run_id: decision.stop_intent.map(|stop| stop.run_id),
            previous_run_status,
            stop_requested,
        })
    }

    /// Atomically advances the scheduler checkpoint and claims one automatic
    /// occurrence. `BEGIN IMMEDIATE` serializes this operation across SQLite
    /// connections; the unique wall key is the second, durable idempotency
    /// guard. No process is started by this persistence operation.
    pub fn claim_scheduled_occurrence(
        &self,
        job_id: &str,
        scheduled_at: i64,
        occurrence_wall_key: &str,
        now: i64,
    ) -> Result<ClaimResult, StorageError> {
        if occurrence_wall_key.is_empty() {
            return Err(StorageError::Validation(
                "occurrence_wall_key must not be empty".to_string(),
            ));
        }
        if occurrence_wall_key.contains('\0') {
            return Err(StorageError::Validation(
                "occurrence_wall_key contains a NUL byte".to_string(),
            ));
        }
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let job = ensure_job(&transaction, job_id)?;
        if !job.enabled {
            return Err(StorageError::JobDisabled(job_id.to_string()));
        }

        if let Some(run) = fetch_run_by_occurrence(&transaction, job_id, occurrence_wall_key)? {
            advance_checkpoint(&transaction, job_id, scheduled_at)?;
            transaction.commit()?;
            return Ok(ClaimResult {
                inserted: false,
                run,
            });
        }

        let sequence = allocate_queue_sequence(&transaction, job_id)?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO runs (id, job_id, scheduled_at, occurrence_wall_key,
                queue_sequence, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'queued', ?)",
            params![id, job_id, scheduled_at, occurrence_wall_key, sequence, now],
        )?;
        advance_checkpoint(&transaction, job_id, scheduled_at)?;
        let run = fetch_run(&transaction, &id)?
            .ok_or_else(|| StorageError::NotFound(format!("newly claimed run {id}")))?;
        transaction.commit()?;
        Ok(ClaimResult {
            inserted: true,
            run,
        })
    }

    pub fn get_run(&self, id: &str) -> Result<Option<Run>, StorageError> {
        let connection = self.lock()?;
        fetch_run(&connection, id).map_err(StorageError::from)
    }

    pub fn list_runs(
        &self,
        job_id: &str,
        limit: u32,
        start_at: Option<i64>,
        end_at: Option<i64>,
    ) -> Result<Vec<Run>, StorageError> {
        let connection = self.lock()?;
        let limit = i64::from(limit.clamp(1, 500));
        let mut statement = connection.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs
             WHERE job_id = ?1
               AND (?2 IS NULL OR COALESCE(started_at, created_at) >= ?2)
               AND (?3 IS NULL OR COALESCE(started_at, created_at) < ?3)
             ORDER BY COALESCE(started_at, created_at) DESC, queue_sequence DESC, id DESC
             LIMIT ?4"
        ))?;
        let rows = statement.query_map(params![job_id, start_at, end_at, limit], row_to_run)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    /// Return the complete run metadata snapshot used by the bounded
    /// retention planner. Log contents remain on disk and are never loaded by
    /// this query.
    pub fn list_runs_for_retention(&self) -> Result<Vec<Run>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs ORDER BY created_at, id"
        ))?;
        let rows = statement.query_map([], row_to_run)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    /// Clear a terminal run's log reference only after the caller has removed
    /// the app-owned directory or confirmed that it no longer exists.
    pub fn mark_run_logs_deleted(
        &self,
        run_id: &str,
        deleted_at: i64,
    ) -> Result<bool, StorageError> {
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE runs
             SET log_dir = NULL, logs_deleted_at = COALESCE(logs_deleted_at, ?2)
             WHERE id = ?1
               AND status IN ('succeeded', 'failed', 'cancelled', 'skipped')
               AND log_dir IS NOT NULL",
            params![run_id, deleted_at],
        )?;
        Ok(changed == 1)
    }

    /// Delete old terminal metadata only when no log reference remains. This
    /// SQL guard is the final protection against orphaning a directory if a
    /// cleanup plan becomes stale between its filesystem and DB phases.
    pub fn delete_terminal_run_without_logs(&self, run_id: &str) -> Result<bool, StorageError> {
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "DELETE FROM runs
             WHERE id = ?1
               AND status IN ('succeeded', 'failed', 'cancelled', 'skipped')
               AND log_dir IS NULL",
            [run_id],
        )?;
        Ok(changed == 1)
    }

    pub fn enqueue_notification(
        &self,
        notification: NewNotification,
    ) -> Result<NotificationOutboxItem, StorageError> {
        notification
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO notification_outbox (
                id, kind, job_id, run_id, error_code, idempotency_key, created_at
             ) VALUES (?, 'run-failed', ?, ?, ?, ?, ?)
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                id,
                notification.job_id,
                notification.run_id,
                notification.error_code,
                notification.idempotency_key,
                notification.created_at,
            ],
        )?;
        let item = fetch_notification_by_key(&transaction, &notification.idempotency_key)?
            .ok_or_else(|| StorageError::NotFound("notification outbox item".to_string()))?;
        transaction.commit()?;
        Ok(item)
    }

    /// Insert one failure event unless the same idempotency key was already
    /// observed or this job already produced an event inside the exact
    /// deduplication window. The immediate transaction makes the decision
    /// stable across concurrent scheduler callbacks.
    pub fn enqueue_notification_if_not_recent(
        &self,
        notification: NewNotification,
        recent_after: i64,
    ) -> Result<Option<NotificationOutboxItem>, StorageError> {
        notification
            .validate()
            .map_err(|error| StorageError::Validation(error.to_string()))?;
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if fetch_notification_by_key(&transaction, &notification.idempotency_key)?.is_some() {
            transaction.commit()?;
            return Ok(None);
        }
        let recent_exists: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM notification_outbox
                WHERE kind = 'run-failed' AND job_id IS ?1 AND created_at > ?2
             )",
            params![notification.job_id.as_deref(), recent_after],
            |row| row.get(0),
        )?;
        if recent_exists {
            transaction.commit()?;
            return Ok(None);
        }

        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO notification_outbox (
                id, kind, job_id, run_id, error_code, idempotency_key, created_at
             ) VALUES (?, 'run-failed', ?, ?, ?, ?, ?)",
            params![
                id,
                notification.job_id,
                notification.run_id,
                notification.error_code,
                notification.idempotency_key,
                notification.created_at,
            ],
        )?;
        let item = fetch_notification_by_key(&transaction, &notification.idempotency_key)?
            .ok_or_else(|| StorageError::NotFound("notification outbox item".to_string()))?;
        transaction.commit()?;
        Ok(Some(item))
    }

    pub fn list_pending_notifications(
        &self,
        limit: u32,
    ) -> Result<Vec<NotificationOutboxItem>, StorageError> {
        let connection = self.lock()?;
        let limit = i64::from(limit.clamp(1, 500));
        let mut statement = connection.prepare(&format!(
            "SELECT {NOTIFICATION_COLUMNS} FROM notification_outbox
             WHERE delivered_at IS NULL ORDER BY created_at, id LIMIT ?1"
        ))?;
        let rows = statement.query_map([limit], row_to_notification)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    pub fn mark_notification_delivered(
        &self,
        id: &str,
        delivered_at: i64,
    ) -> Result<bool, StorageError> {
        let connection = self.lock_mut()?;
        let changed = connection.execute(
            "UPDATE notification_outbox SET delivered_at = ? WHERE id = ?",
            params![delivered_at, id],
        )?;
        Ok(changed == 1)
    }

    pub fn delete_delivered_notifications_before(
        &self,
        cutoff: i64,
    ) -> Result<usize, StorageError> {
        let connection = self.lock_mut()?;
        connection
            .execute(
                "DELETE FROM notification_outbox
                 WHERE delivered_at IS NOT NULL AND delivered_at < ?1",
                [cutoff],
            )
            .map_err(StorageError::from)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::ConnectionPoisoned)
    }

    fn lock_mut(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::ConnectionPoisoned)
    }
}

pub fn current_epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")
}

fn migrate_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(MIGRATION_SQL)?;
    let current: Option<String> = transaction
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let current = current
        .map(|value| {
            value.parse::<i64>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(format!(
                        "invalid schema version {value:?}: {error}"
                    ))),
                )
            })
        })
        .transpose()?;
    if current.is_some_and(|version| version > SCHEMA_VERSION) {
        return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::other("database schema is newer than this application"),
        )));
    }
    transaction.execute(
        "INSERT INTO meta (key, value) VALUES ('schema_version', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [SCHEMA_VERSION.to_string()],
    )?;
    transaction.commit()
}

fn bool_to_sql(value: bool) -> i64 {
    i64::from(value)
}

fn ensure_encrypted_input(input: &JobInput) -> Result<(), StorageError> {
    if matches!(input.environment, EnvironmentUpdate::Keep) {
        Ok(())
    } else {
        Err(StorageError::Validation(
            "environment must be protected before storage".to_string(),
        ))
    }
}

fn ensure_encrypted_service_input(input: &ServiceInput) -> Result<(), StorageError> {
    if matches!(input.environment, EnvironmentUpdate::Keep) {
        Ok(())
    } else {
        Err(StorageError::Validation(
            "environment must be protected before storage".to_string(),
        ))
    }
}

fn ensure_job(transaction: &Transaction<'_>, id: &str) -> Result<Job, StorageError> {
    let job =
        fetch_job(transaction, id)?.ok_or_else(|| StorageError::NotFound(format!("job {id}")))?;
    if job.kind != JobKind::Job {
        return Err(StorageError::Validation(
            "service rows are not supported by Phase 1 job persistence".to_string(),
        ));
    }
    Ok(job)
}

fn validate_token(field: &str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        return Err(StorageError::Validation(format!(
            "{field} must not be empty"
        )));
    }
    if value.contains('\0') {
        return Err(StorageError::Validation(format!(
            "{field} contains a NUL byte"
        )));
    }
    Ok(())
}

fn validate_optional_text(field: &str, value: Option<&str>) -> Result<(), StorageError> {
    if value.is_some_and(|value| value.contains('\0')) {
        return Err(StorageError::Validation(format!(
            "{field} contains a NUL byte"
        )));
    }
    Ok(())
}

fn ensure_service(transaction: &Transaction<'_>, id: &str) -> Result<Job, StorageError> {
    let service = fetch_service(transaction, id)?
        .ok_or_else(|| StorageError::NotFound(format!("service {id}")))?;
    Ok(service)
}

fn allocate_queue_sequence(
    transaction: &Transaction<'_>,
    job_id: &str,
) -> Result<i64, StorageError> {
    let current: i64 = transaction.query_row(
        "SELECT next_queue_sequence FROM jobs WHERE id = ? AND kind = 'job'",
        [job_id],
        |row| row.get(0),
    )?;
    let sequence = current
        .checked_add(1)
        .ok_or_else(|| StorageError::Validation("queue sequence overflow".to_string()))?;
    transaction.execute(
        "UPDATE jobs SET next_queue_sequence = ? WHERE id = ? AND kind = 'job'",
        params![sequence, job_id],
    )?;
    Ok(sequence)
}

fn advance_checkpoint(
    transaction: &Transaction<'_>,
    job_id: &str,
    scheduled_at: i64,
) -> Result<(), StorageError> {
    transaction.execute(
        "UPDATE jobs SET last_evaluated_at = CASE
            WHEN last_evaluated_at IS NULL OR last_evaluated_at < ?1 THEN ?1
            ELSE last_evaluated_at END
         WHERE id = ?2 AND kind = 'job'",
        params![scheduled_at, job_id],
    )?;
    Ok(())
}

const JOB_COLUMNS: &str = "id, kind, name, command, cwd, target_kind, target_distro,
    env_ciphertext, cron_expr, enabled, overlap_policy, catch_up, last_evaluated_at,
    next_queue_sequence, restart_policy, auto_start, health_tcp_address,
    health_tcp_port, health_start_grace_ms, created_at, updated_at";

const RUN_COLUMNS: &str = "id, job_id, scheduled_at, occurrence_wall_key, queue_sequence,
    blocked_by_run_id, started_at, ended_at, exit_code, status, owner_instance_id,
    attempt_token, error_message, target_pid, target_process_created_at, target_pgid,
    target_sid, process_marker, log_dir, logs_deleted_at, created_at";

const NOTIFICATION_COLUMNS: &str = "id, kind, job_id, run_id, error_code,
    idempotency_key, created_at, delivered_at";

fn fetch_job(connection: &Connection, id: &str) -> rusqlite::Result<Option<Job>> {
    connection
        .query_row(
            &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?"),
            [id],
            row_to_job,
        )
        .optional()
}

fn fetch_phase1_job(connection: &Connection, id: &str) -> rusqlite::Result<Option<Job>> {
    connection
        .query_row(
            &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ? AND kind = 'job'"),
            [id],
            row_to_job,
        )
        .optional()
}

fn fetch_service(connection: &Connection, id: &str) -> rusqlite::Result<Option<Job>> {
    connection
        .query_row(
            &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ? AND kind = 'service'"),
            [id],
            row_to_job,
        )
        .optional()
}

fn fetch_service_instance(
    connection: &Connection,
    service_id: &str,
) -> rusqlite::Result<Option<ServiceInstance>> {
    connection
        .query_row(
            "SELECT job_id, generation, active_run_id, state, owner_instance_id,
                    attempt_token, next_retry_at, consecutive_failures, updated_at
             FROM service_instances WHERE job_id = ?",
            [service_id],
            row_to_service_instance,
        )
        .optional()
}

fn fetch_run(connection: &Connection, id: &str) -> rusqlite::Result<Option<Run>> {
    connection
        .query_row(
            &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?"),
            [id],
            row_to_run,
        )
        .optional()
}

fn fetch_run_by_occurrence(
    connection: &Connection,
    job_id: &str,
    occurrence_wall_key: &str,
) -> rusqlite::Result<Option<Run>> {
    connection
        .query_row(
            &format!(
                "SELECT {RUN_COLUMNS} FROM runs
                 WHERE job_id = ? AND occurrence_wall_key = ?"
            ),
            params![job_id, occurrence_wall_key],
            row_to_run,
        )
        .optional()
}

fn fetch_active_run(connection: &Connection, job_id: &str) -> rusqlite::Result<Option<Run>> {
    connection
        .query_row(
            &format!(
                "SELECT {RUN_COLUMNS} FROM runs
                 WHERE job_id = ? AND status IN ('queued', 'starting', 'running', 'stopping')
                 ORDER BY queue_sequence, id LIMIT 1"
            ),
            [job_id],
            row_to_run,
        )
        .optional()
}

fn fetch_active_process_run(
    connection: &Connection,
    job_id: &str,
) -> rusqlite::Result<Option<Run>> {
    connection
        .query_row(
            &format!(
                "SELECT {RUN_COLUMNS} FROM runs
                 WHERE job_id = ? AND status IN ('starting', 'running', 'stopping')
                 ORDER BY queue_sequence, id LIMIT 1"
            ),
            [job_id],
            row_to_run,
        )
        .optional()
}

fn fetch_notification_by_key(
    connection: &Connection,
    idempotency_key: &str,
) -> rusqlite::Result<Option<NotificationOutboxItem>> {
    connection
        .query_row(
            &format!(
                "SELECT {NOTIFICATION_COLUMNS} FROM notification_outbox
                 WHERE idempotency_key = ?"
            ),
            [idempotency_key],
            row_to_notification,
        )
        .optional()
}

fn row_to_job(row: &Row<'_>) -> rusqlite::Result<Job> {
    let kind = parse_job_kind(&row.get::<_, String>("kind")?)?;
    let target_kind = parse_target_kind(&row.get::<_, String>("target_kind")?)?;
    let overlap_policy = parse_overlap_policy(&row.get::<_, String>("overlap_policy")?)?;
    let auto_start = row
        .get::<_, Option<i64>>("auto_start")?
        .map(|value| value != 0);
    let health_tcp_port = row
        .get::<_, Option<i64>>("health_tcp_port")?
        .map(|value| u16::try_from(value).map_err(|_| conversion_error("health_tcp_port", value)))
        .transpose()?;
    let restart_policy = parse_restart_policy(&row.get("restart_policy")?)?;
    Ok(Job {
        id: row.get("id")?,
        kind,
        name: row.get("name")?,
        command: row.get("command")?,
        cwd: row.get("cwd")?,
        target_kind,
        target_distro: row.get("target_distro")?,
        env_configured: row.get::<_, Option<Vec<u8>>>("env_ciphertext")?.is_some(),
        cron_expr: row.get("cron_expr")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        overlap_policy,
        catch_up: row.get::<_, i64>("catch_up")? != 0,
        last_evaluated_at: row.get("last_evaluated_at")?,
        next_queue_sequence: row.get("next_queue_sequence")?,
        restart_policy,
        auto_start,
        health_tcp_address: row.get("health_tcp_address")?,
        health_tcp_port,
        health_start_grace_ms: row.get("health_start_grace_ms")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_service_instance(row: &Row<'_>) -> rusqlite::Result<ServiceInstance> {
    Ok(ServiceInstance {
        job_id: row.get("job_id")?,
        generation: row.get("generation")?,
        active_run_id: row.get("active_run_id")?,
        state: parse_service_instance_state(&row.get::<_, String>("state")?)?,
        owner_instance_id: row.get("owner_instance_id")?,
        attempt_token: row.get("attempt_token")?,
        next_retry_at: row.get("next_retry_at")?,
        consecutive_failures: row.get("consecutive_failures")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_run(row: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: row.get("id")?,
        job_id: row.get("job_id")?,
        scheduled_at: row.get("scheduled_at")?,
        occurrence_wall_key: row.get("occurrence_wall_key")?,
        queue_sequence: row.get("queue_sequence")?,
        blocked_by_run_id: row.get("blocked_by_run_id")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        exit_code: row.get("exit_code")?,
        status: parse_run_status(&row.get::<_, String>("status")?)?,
        owner_instance_id: row.get("owner_instance_id")?,
        attempt_token: row.get("attempt_token")?,
        error_message: row.get("error_message")?,
        target_pid: row.get("target_pid")?,
        target_process_created_at: row.get("target_process_created_at")?,
        target_pgid: row.get("target_pgid")?,
        target_sid: row.get("target_sid")?,
        process_marker: row.get("process_marker")?,
        log_dir: row.get("log_dir")?,
        logs_deleted_at: row.get("logs_deleted_at")?,
        created_at: row.get("created_at")?,
    })
}

fn row_to_notification(row: &Row<'_>) -> rusqlite::Result<NotificationOutboxItem> {
    Ok(NotificationOutboxItem {
        id: row.get("id")?,
        kind: row.get("kind")?,
        job_id: row.get("job_id")?,
        run_id: row.get("run_id")?,
        error_code: row.get("error_code")?,
        idempotency_key: row.get("idempotency_key")?,
        created_at: row.get("created_at")?,
        delivered_at: row.get("delivered_at")?,
    })
}

fn parse_job_kind(value: &str) -> rusqlite::Result<JobKind> {
    match value {
        "job" => Ok(JobKind::Job),
        "service" => Ok(JobKind::Service),
        _ => Err(conversion_error("kind", value)),
    }
}

fn parse_target_kind(value: &str) -> rusqlite::Result<TargetKind> {
    match value {
        "windows" => Ok(TargetKind::Windows),
        "wsl" => Ok(TargetKind::Wsl),
        _ => Err(conversion_error("target_kind", value)),
    }
}

fn parse_overlap_policy(value: &str) -> rusqlite::Result<OverlapPolicy> {
    match value {
        "skip" => Ok(OverlapPolicy::Skip),
        "queue" => Ok(OverlapPolicy::Queue),
        "kill-previous" => Ok(OverlapPolicy::KillPrevious),
        _ => Err(conversion_error("overlap_policy", value)),
    }
}

fn parse_restart_policy(value: &Option<String>) -> rusqlite::Result<Option<RestartPolicy>> {
    value
        .as_deref()
        .map(|value| match value {
            "never" => Ok(RestartPolicy::Never),
            "on-failure" => Ok(RestartPolicy::OnFailure),
            "always" => Ok(RestartPolicy::Always),
            _ => Err(conversion_error("restart_policy", value)),
        })
        .transpose()
}

fn parse_service_instance_state(value: &str) -> rusqlite::Result<ServiceInstanceState> {
    match value {
        "stopped" => Ok(ServiceInstanceState::Stopped),
        "starting" => Ok(ServiceInstanceState::Starting),
        "running" => Ok(ServiceInstanceState::Running),
        "stopping" => Ok(ServiceInstanceState::Stopping),
        "retry_waiting" => Ok(ServiceInstanceState::RetryWaiting),
        _ => Err(conversion_error("service_instances.state", value)),
    }
}

fn parse_run_status(value: &str) -> rusqlite::Result<RunStatus> {
    match value {
        "queued" => Ok(RunStatus::Queued),
        "starting" => Ok(RunStatus::Starting),
        "running" => Ok(RunStatus::Running),
        "stopping" => Ok(RunStatus::Stopping),
        "succeeded" => Ok(RunStatus::Succeeded),
        "failed" => Ok(RunStatus::Failed),
        "cancelled" => Ok(RunStatus::Cancelled),
        "skipped" => Ok(RunStatus::Skipped),
        _ => Err(conversion_error("status", value)),
    }
}

fn conversion_error<T: fmt::Display>(column: &str, value: T) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(format!(
            "invalid {column} value: {value}"
        ))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{
        EnvironmentUpdate, JobInput, NewNotification, RestartPolicy, ServiceInput,
        ServiceInstanceState, TargetKind,
    };
    use rusqlite::OptionalExtension;
    use std::sync::{Arc, Barrier};
    use tempfile::NamedTempFile;

    fn input(enabled: bool) -> JobInput {
        JobInput {
            name: "backup".to_string(),
            command: "echo backup".to_string(),
            cwd: None,
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            cron_expr: "0 * * * *".to_string(),
            enabled,
            overlap_policy: OverlapPolicy::Queue,
            catch_up: false,
        }
    }

    fn service_input() -> ServiceInput {
        ServiceInput {
            name: "web".to_string(),
            command: "node server.js".to_string(),
            cwd: Some("C:\\work\\web".to_string()),
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment: EnvironmentUpdate::Keep,
            restart_policy: RestartPolicy::Always,
            auto_start: true,
            health_tcp_address: Some("localhost".to_string()),
            health_tcp_port: Some(3000),
        }
    }

    #[test]
    fn migration_is_idempotent_and_keeps_required_pragmas() {
        let database = DatabaseState::open_in_memory().unwrap();
        database.migrate().unwrap();
        assert_eq!(database.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(database.is_ready());
        let connection = database.connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            BUSY_TIMEOUT_MS as i64
        );
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('jobs', 'runs', 'service_instances', 'notification_outbox', 'meta')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 5);
    }

    #[test]
    fn service_instances_has_durable_keys_and_foreign_key_cleanup() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let run = database.create_manual_run_at(&job.id, 101).unwrap();
        let connection = database.connection.lock().unwrap();

        let missing_job = connection.execute(
            "INSERT INTO service_instances (job_id, updated_at) VALUES ('missing', 1)",
            [],
        );
        assert!(matches!(
            missing_job,
            Err(rusqlite::Error::SqliteFailure(_, _))
        ));

        let invalid_state = connection.execute(
            "INSERT INTO service_instances (job_id, state, updated_at) VALUES (?, 'unknown', 1)",
            [&job.id],
        );
        assert!(matches!(
            invalid_state,
            Err(rusqlite::Error::SqliteFailure(_, _))
        ));

        connection
            .execute(
                "INSERT INTO service_instances (
                    job_id, generation, active_run_id, state, consecutive_failures, updated_at
                 ) VALUES (?, 2, ?, 'running', 3, 102)",
                params![job.id, run.id],
            )
            .unwrap();
        connection
            .execute("DELETE FROM runs WHERE id = ?", [&run.id])
            .unwrap();
        let active_run: Option<String> = connection
            .query_row(
                "SELECT active_run_id FROM service_instances WHERE job_id = ?",
                [&job.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_run, None);
        connection
            .execute("DELETE FROM jobs WHERE id = ?", [&job.id])
            .unwrap();
        let service_count: i64 = connection
            .query_row("SELECT count(*) FROM service_instances", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(service_count, 0);
    }

    #[test]
    fn service_crud_persists_policy_health_and_instance_without_leaking_secrets() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let service = database
            .create_service_with_ciphertext_at(
                service_input(),
                EnvironmentCiphertextUpdate::Replace(vec![0xca, 0xfe]),
                200,
            )
            .unwrap();

        assert_eq!(service.kind, JobKind::Service);
        assert_eq!(service.cron_expr, None);
        assert_eq!(service.restart_policy, Some(RestartPolicy::Always));
        assert_eq!(service.auto_start, Some(true));
        assert_eq!(service.health_tcp_address.as_deref(), Some("localhost"));
        assert_eq!(service.health_tcp_port, Some(3000));
        assert_eq!(
            service.health_start_grace_ms,
            Some(DEFAULT_SERVICE_START_GRACE_MS)
        );
        assert!(service.env_configured);
        assert_eq!(database.list_jobs().unwrap().len(), 1);
        assert_eq!(database.list_services().unwrap(), vec![service.clone()]);
        assert!(database.get_job(&service.id).unwrap().is_none());
        assert_eq!(database.get_service(&job.id).unwrap(), None);

        let instance = database.get_service_instance(&service.id).unwrap().unwrap();
        assert_eq!(instance.job_id, service.id);
        assert_eq!(instance.state, ServiceInstanceState::Stopped);
        assert_eq!(instance.generation, 0);

        let mut updated = service_input();
        updated.name = "web-renamed".to_string();
        updated.restart_policy = RestartPolicy::Never;
        updated.auto_start = false;
        updated.health_tcp_address = Some("127.0.0.1".to_string());
        updated.health_tcp_port = Some(8080);
        let service = database
            .update_service_with_ciphertext_at(
                &service.id,
                updated,
                EnvironmentCiphertextUpdate::Keep,
                300,
            )
            .unwrap();
        assert_eq!(service.name, "web-renamed");
        assert_eq!(service.restart_policy, Some(RestartPolicy::Never));
        assert_eq!(service.auto_start, Some(false));
        assert_eq!(service.health_tcp_port, Some(8080));
        assert_eq!(service.updated_at, 300);
        let raw_ciphertext: Vec<u8> = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ?",
                [&service.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw_ciphertext, vec![0xca, 0xfe]);
        let wire = serde_json::to_value(&service).unwrap();
        assert!(wire.get("envCiphertext").is_none());
        assert_eq!(wire.get("restartPolicy"), Some(&serde_json::json!("never")));

        let mut replaced = service_input();
        replaced.name = "web-replaced".to_string();
        database
            .update_service_with_ciphertext_at(
                &service.id,
                replaced,
                EnvironmentCiphertextUpdate::Replace(vec![0xba, 0xbe]),
                350,
            )
            .unwrap();
        let replaced_ciphertext: Vec<u8> = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ?",
                [&service.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(replaced_ciphertext, vec![0xba, 0xbe]);

        let cleared = service_input();
        database
            .update_service_with_ciphertext_at(
                &service.id,
                cleared,
                EnvironmentCiphertextUpdate::Clear,
                400,
            )
            .unwrap();
        let cleared_ciphertext: Option<Vec<u8>> = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ?",
                [&service.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cleared_ciphertext, None);

        assert!(database.delete_service(&service.id).unwrap());
        assert!(database.get_service(&service.id).unwrap().is_none());
        assert!(database
            .list_jobs()
            .unwrap()
            .iter()
            .all(|item| item.id == job.id));
        let connection = database.connection.lock().unwrap();
        let instance_count: i64 = connection
            .query_row("SELECT count(*) FROM service_instances", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(instance_count, 0);
    }

    #[test]
    fn service_updates_do_not_accept_job_rows() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(false), 100).unwrap();
        let error = database
            .update_service_at(&job.id, service_input(), 200)
            .unwrap_err();
        assert!(matches!(error, StorageError::NotFound(_)));
    }

    #[test]
    fn job_crud_masks_ciphertext_and_resets_checkpoint_only_for_schedule_changes() {
        let database = DatabaseState::open_in_memory().unwrap();
        let created = database
            .create_job_with_ciphertext_at(input(true), Some(vec![0xde, 0xad, 0xbe, 0xef]), 100)
            .unwrap();
        assert_eq!(created.last_evaluated_at, Some(100));
        assert!(created.env_configured);
        let raw_ciphertext: Vec<u8> = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ?",
                [&created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(raw_ciphertext, vec![0xde, 0xad, 0xbe, 0xef]);
        let wire = serde_json::to_value(&created).unwrap();
        assert_eq!(
            wire.get("envConfigured"),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(wire.get("envCiphertext").is_none());

        let mut renamed = input(true);
        renamed.name = "renamed".to_string();
        let unchanged_checkpoint = database.update_job_at(&created.id, renamed, 200).unwrap();
        assert_eq!(unchanged_checkpoint.last_evaluated_at, Some(100));

        let mut rescheduled = input(true);
        rescheduled.cron_expr = "30 * * * *".to_string();
        let reset_checkpoint = database
            .update_job_at(&created.id, rescheduled, 300)
            .unwrap();
        assert_eq!(reset_checkpoint.last_evaluated_at, Some(300));
        assert_eq!(database.list_jobs().unwrap().len(), 1);
        assert_eq!(
            database.get_job(&created.id).unwrap().unwrap().id,
            created.id
        );
        assert!(database.delete_job(&created.id).unwrap());
        assert!(database.get_job(&created.id).unwrap().is_none());
    }

    #[test]
    fn metadata_updates_keep_existing_ciphertext_when_no_adapter_payload_is_supplied() {
        let database = DatabaseState::open_in_memory().unwrap();
        let created = database
            .create_job_with_ciphertext_at(input(false), Some(vec![0xde, 0xad, 0xbe, 0xef]), 100)
            .unwrap();
        let mut renamed = input(false);
        renamed.name = "renamed".to_string();
        database.update_job_at(&created.id, renamed, 200).unwrap();

        let ciphertext: Vec<u8> = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT env_ciphertext FROM jobs WHERE id = ?",
                [&created.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ciphertext, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn explicit_environment_replace_and_clear_are_distinct_from_keep() {
        let database = DatabaseState::open_in_memory().unwrap();
        let created = database
            .create_job_with_ciphertext_at(input(false), Some(vec![1, 2, 3]), 100)
            .unwrap();

        let mut replacement = input(false);
        replacement.name = "replaced".to_string();
        database
            .update_job_with_ciphertext_at(
                &created.id,
                replacement,
                EnvironmentCiphertextUpdate::Replace(vec![4, 5, 6]),
                200,
            )
            .unwrap();
        assert_eq!(
            database
                .get_job_environment_ciphertext(&created.id)
                .unwrap(),
            Some(vec![4, 5, 6])
        );

        let cleared = input(false);
        database
            .update_job_with_ciphertext_at(
                &created.id,
                cleared,
                EnvironmentCiphertextUpdate::Clear,
                300,
            )
            .unwrap();
        assert_eq!(
            database
                .get_job_environment_ciphertext(&created.id)
                .unwrap(),
            None
        );
        assert!(
            !database
                .get_job(&created.id)
                .unwrap()
                .unwrap()
                .env_configured
        );
    }

    #[test]
    fn disabled_job_does_not_claim_and_enable_resets_checkpoint() {
        let database = DatabaseState::open_in_memory().unwrap();
        let created = database.create_job_at(input(false), 100).unwrap();
        assert_eq!(created.last_evaluated_at, None);
        let mut changed = input(false);
        changed.cron_expr = "30 * * * *".to_string();
        let changed = database.update_job_at(&created.id, changed, 250).unwrap();
        assert_eq!(changed.last_evaluated_at, Some(250));
        assert!(matches!(
            database.claim_scheduled_occurrence(&created.id, 200, "2026-08-12T00:00:00", 210),
            Err(StorageError::JobDisabled(_))
        ));
        let enabled = database.set_job_enabled_at(&created.id, true, 300).unwrap();
        assert_eq!(enabled.last_evaluated_at, Some(300));
    }

    #[test]
    fn occurrence_claim_is_idempotent_and_checkpoint_is_monotonic() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let first = database
            .claim_scheduled_occurrence(&job.id, 200, "2026-08-12T00:00:00", 201)
            .unwrap();
        assert!(first.inserted);
        assert_eq!(first.run.queue_sequence, 1);
        let duplicate = database
            .claim_scheduled_occurrence(&job.id, 150, "2026-08-12T00:00:00", 202)
            .unwrap();
        assert!(!duplicate.inserted);
        assert_eq!(duplicate.run.id, first.run.id);
        assert_eq!(duplicate.run.queue_sequence, 1);
        let later = database
            .claim_scheduled_occurrence(&job.id, 250, "2026-08-12T00:05:00", 251)
            .unwrap();
        assert!(later.inserted);
        assert_eq!(later.run.queue_sequence, 2);
        let stored = database.get_job(&job.id).unwrap().unwrap();
        assert_eq!(stored.last_evaluated_at, Some(250));
        assert_eq!(stored.next_queue_sequence, 2);
        assert_eq!(
            database.list_runs(&job.id, 10, None, None).unwrap().len(),
            2
        );
    }

    #[test]
    fn manual_null_occurrences_are_not_unique_conflicts_and_share_fifo_allocator() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let first = database.create_manual_run_at(&job.id, 101).unwrap();
        let second = database.create_manual_run_at(&job.id, 102).unwrap();
        assert_eq!(first.scheduled_at, None);
        assert_eq!(first.occurrence_wall_key, None);
        assert_eq!(second.scheduled_at, None);
        assert_eq!(second.occurrence_wall_key, None);
        assert_eq!((first.queue_sequence, second.queue_sequence), (1, 2));
        let count: i64 = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM runs WHERE job_id = ? AND scheduled_at IS NULL AND occurrence_wall_key IS NULL",
                [&job.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn running_metadata_requires_owner_attempt_and_persists_identity_atomically() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let run = database.create_manual_run_at(&job.id, 101).unwrap();
        assert!(database
            .claim_run_starting(&run.id, "owner", "attempt")
            .unwrap());
        assert!(database
            .mark_run_log_dir(&run.id, "owner", "attempt", "logs/runs/run-1")
            .unwrap());
        assert!(!database
            .mark_run_log_dir(&run.id, "other", "attempt", "logs/runs/run-1")
            .unwrap());
        let metadata = RunExecutionMetadata {
            log_dir: Some("logs/runs/run-1".to_owned()),
            target_pid: Some(42),
            target_process_created_at: Some(1234),
            target_pgid: Some(41),
            target_sid: Some(40),
            process_marker: Some("123e4567-e89b-12d3-a456-426614174000".to_owned()),
        };
        assert!(database
            .mark_run_running_with_metadata(&run.id, "owner", "attempt", 102, &metadata)
            .unwrap());
        let stored = database.get_run(&run.id).unwrap().unwrap();
        assert_eq!(stored.status, RunStatus::Running);
        assert_eq!(stored.log_dir, metadata.log_dir);
        assert_eq!(stored.target_pid, Some(42));
        assert_eq!(stored.target_process_created_at, Some(1234));
        assert_eq!(stored.target_pgid, Some(41));
        assert_eq!(stored.target_sid, Some(40));
        assert_eq!(stored.process_marker, metadata.process_marker);
    }

    #[test]
    fn scheduled_and_manual_claims_share_overlap_decision_and_start_cas() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let first = database
            .claim_scheduled_run(&job.id, 200, "2026-08-12T00:00:00", 201)
            .unwrap();
        assert_eq!(first.action, ClaimedRunAction::Start);
        assert!(first.inserted);
        assert_eq!(first.previous_run_status, None);

        let manual = database.claim_manual_run(&job.id, 202).unwrap();
        assert_eq!(manual.action, ClaimedRunAction::Queue);
        assert!(manual.inserted);
        assert_eq!(manual.run.status, RunStatus::Queued);

        assert!(database
            .claim_run_starting(&first.run.id, "owner", "token")
            .unwrap());
        assert!(!database
            .claim_run_starting(&first.run.id, "other", "other-token")
            .unwrap());
        assert!(!database
            .mark_run_running(&first.run.id, "owner", "wrong-token", 203)
            .unwrap());
        assert!(database
            .mark_run_running(&first.run.id, "owner", "token", 203)
            .unwrap());
        assert!(!database
            .finish_run(
                &first.run.id,
                "other",
                "token",
                RunStatus::Succeeded,
                Some(0),
                None,
                204,
            )
            .unwrap());
        assert!(database
            .finish_run(
                &first.run.id,
                "owner",
                "token",
                RunStatus::Succeeded,
                Some(0),
                None,
                204,
            )
            .unwrap());
    }

    #[test]
    fn kill_previous_claim_records_queued_previous_status_without_spawn_permission() {
        let database = DatabaseState::open_in_memory().unwrap();
        let mut job_input = input(true);
        job_input.overlap_policy = OverlapPolicy::KillPrevious;
        let job = database.create_job_at(job_input, 100).unwrap();
        let old = database.create_manual_run_at(&job.id, 101).unwrap();
        let claim = database
            .claim_scheduled_run(&job.id, 200, "2026-08-12T00:00:00", 201)
            .unwrap();
        assert_eq!(claim.action, ClaimedRunAction::KillPrevious);
        assert_eq!(claim.previous_run_id.as_deref(), Some(old.id.as_str()));
        assert_eq!(claim.previous_run_status, Some(RunStatus::Queued));
        assert!(claim.stop_requested);
        assert_eq!(claim.run.blocked_by_run_id, Some(old.id.clone()));

        assert!(database
            .resolve_kill_previous(&old.id, &claim.run.id, true, None, 202)
            .unwrap());
        assert_eq!(
            database.get_run(&old.id).unwrap().unwrap().status,
            RunStatus::Cancelled
        );
        let replacement = database.get_run(&claim.run.id).unwrap().unwrap();
        assert_eq!(replacement.status, RunStatus::Queued);
        assert_eq!(replacement.blocked_by_run_id, None);
    }

    #[test]
    fn same_file_connections_observe_unique_occurrence_claim() {
        let file = NamedTempFile::new().unwrap();
        let first = DatabaseState::open(file.path()).unwrap();
        let job = first.create_job_at(input(true), 100).unwrap();
        let second = DatabaseState::open(file.path()).unwrap();
        let first_claim = first
            .claim_scheduled_occurrence(&job.id, 200, "2026-08-12T00:00:00", 201)
            .unwrap();
        let second_claim = second
            .claim_scheduled_occurrence(&job.id, 200, "2026-08-12T00:00:00", 202)
            .unwrap();
        assert!(first_claim.inserted);
        assert!(!second_claim.inserted);
        assert_eq!(first_claim.run.id, second_claim.run.id);
        assert_eq!(
            second
                .get_job(&job.id)
                .unwrap()
                .unwrap()
                .next_queue_sequence,
            1
        );
    }

    #[test]
    fn concurrent_connections_claim_one_occurrence_and_one_sequence() {
        let file = NamedTempFile::new().unwrap();
        let first = Arc::new(DatabaseState::open(file.path()).unwrap());
        let job = first.create_job_at(input(true), 100).unwrap();
        let second = Arc::new(DatabaseState::open(file.path()).unwrap());
        let barrier = Arc::new(Barrier::new(2));

        let first_barrier = Arc::clone(&barrier);
        let first_database = Arc::clone(&first);
        let job_id = job.id.clone();
        let first_thread = std::thread::spawn(move || {
            first_barrier.wait();
            first_database.claim_scheduled_occurrence(&job_id, 200, "2026-08-12T00:00:00", 201)
        });

        let second_barrier = Arc::clone(&barrier);
        let second_database = Arc::clone(&second);
        let second_job_id = job.id.clone();
        let second_thread = std::thread::spawn(move || {
            second_barrier.wait();
            second_database.claim_scheduled_occurrence(
                &second_job_id,
                200,
                "2026-08-12T00:00:00",
                202,
            )
        });

        let first_result = first_thread.join().unwrap().unwrap();
        let second_result = second_thread.join().unwrap().unwrap();
        assert_eq!(
            [first_result.inserted, second_result.inserted]
                .into_iter()
                .filter(|inserted| *inserted)
                .count(),
            1
        );
        assert_eq!(first_result.run.id, second_result.run.id);
        assert_eq!(
            first.get_job(&job.id).unwrap().unwrap().next_queue_sequence,
            1
        );
    }

    #[test]
    fn notification_outbox_is_sanitized_and_idempotent() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let run = database
            .claim_scheduled_occurrence(&job.id, 200, "2026-08-12T00:00:00", 201)
            .unwrap();
        let notification = NewNotification {
            job_id: Some(job.id.clone()),
            run_id: Some(run.run.id.clone()),
            error_code: "spawn_failed".to_string(),
            idempotency_key: format!("{}:{}", job.id, run.run.id),
            created_at: 202,
        };
        let first = database.enqueue_notification(notification.clone()).unwrap();
        let duplicate = database.enqueue_notification(notification).unwrap();
        assert_eq!(first.id, duplicate.id);
        assert_eq!(database.list_pending_notifications(10).unwrap().len(), 1);
        assert!(database
            .mark_notification_delivered(&first.id, 300)
            .unwrap());
        assert!(database.list_pending_notifications(10).unwrap().is_empty());
    }

    #[test]
    fn notification_deduplication_uses_an_exact_rolling_window() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let first = NewNotification {
            job_id: Some(job.id.clone()),
            run_id: None,
            error_code: "spawn_failed".to_string(),
            idempotency_key: "first".to_string(),
            created_at: 1_000,
        };
        assert!(database
            .enqueue_notification_if_not_recent(first, 700)
            .unwrap()
            .is_some());
        assert!(database
            .enqueue_notification_if_not_recent(
                NewNotification {
                    job_id: Some(job.id.clone()),
                    run_id: None,
                    error_code: "nonzero_exit".to_string(),
                    idempotency_key: "inside-window".to_string(),
                    created_at: 1_299,
                },
                999,
            )
            .unwrap()
            .is_none());
        assert!(database
            .enqueue_notification_if_not_recent(
                NewNotification {
                    job_id: Some(job.id),
                    run_id: None,
                    error_code: "nonzero_exit".to_string(),
                    idempotency_key: "at-boundary".to_string(),
                    created_at: 1_300,
                },
                1_000,
            )
            .unwrap()
            .is_some());
    }

    #[test]
    fn delivered_notification_retention_keeps_pending_rows() {
        let database = DatabaseState::open_in_memory().unwrap();
        let old = database
            .enqueue_notification(NewNotification {
                job_id: None,
                run_id: None,
                error_code: "spawn_failed".to_string(),
                idempotency_key: "old".to_string(),
                created_at: 10,
            })
            .unwrap();
        database.mark_notification_delivered(&old.id, 20).unwrap();
        database
            .enqueue_notification(NewNotification {
                job_id: None,
                run_id: None,
                error_code: "spawn_failed".to_string(),
                idempotency_key: "pending".to_string(),
                created_at: 30,
            })
            .unwrap();
        assert_eq!(
            database.delete_delivered_notifications_before(21).unwrap(),
            1
        );
        assert_eq!(database.list_pending_notifications(10).unwrap().len(), 1);
    }

    #[test]
    fn foreign_keys_cascade_runs_and_null_outbox_references_on_job_delete() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let run = database.create_manual_run_at(&job.id, 101).unwrap();
        database
            .enqueue_notification(NewNotification {
                job_id: Some(job.id.clone()),
                run_id: Some(run.id),
                error_code: "failed".to_string(),
                idempotency_key: "job-delete-test".to_string(),
                created_at: 102,
            })
            .unwrap();
        assert!(database.delete_job(&job.id).unwrap());
        let connection = database.connection.lock().unwrap();
        let runs: i64 = connection
            .query_row("SELECT count(*) FROM runs", [], |row| row.get(0))
            .unwrap();
        let notification_refs: Option<String> = connection
            .query_row("SELECT job_id FROM notification_outbox", [], |row| {
                row.get(0)
            })
            .optional()
            .unwrap()
            .flatten();
        assert_eq!(runs, 0);
        assert_eq!(notification_refs, None);
    }

    #[test]
    fn automatic_occurrence_columns_must_be_both_null_or_both_present() {
        let database = DatabaseState::open_in_memory().unwrap();
        let job = database.create_job_at(input(true), 100).unwrap();
        let connection = database.connection.lock().unwrap();
        let result = connection.execute(
            "INSERT INTO runs (id, job_id, scheduled_at, occurrence_wall_key,
                queue_sequence, status, created_at)
             VALUES ('invalid', ?, 200, NULL, 1, 'queued', 201)",
            [&job.id],
        );
        assert!(matches!(result, Err(rusqlite::Error::SqliteFailure(_, _))));
    }
}
