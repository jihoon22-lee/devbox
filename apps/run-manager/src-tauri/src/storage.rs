use crate::core::models::{
    ClaimResult, Job, JobInput, JobKind, NewNotification, NotificationOutboxItem, OverlapPolicy,
    Run, RunStatus, TargetKind,
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
    ConnectionPoisoned,
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "database error: {error}"),
            Self::Validation(error) => formatter.write_str(error),
            Self::NotFound(entity) => write!(formatter, "{entity} not found"),
            Self::JobDisabled(id) => write!(formatter, "job {id} is disabled"),
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
                input.env_ciphertext,
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

    pub fn list_jobs(&self) -> Result<Vec<Job>, StorageError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE kind = 'job' ORDER BY name COLLATE NOCASE, id"
        ))?;
        let rows = statement.query_map([], row_to_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }

    pub fn update_job(&self, id: &str, input: JobInput) -> Result<Job, StorageError> {
        self.update_job_at(id, input, current_epoch_millis())
    }

    pub fn update_job_at(&self, id: &str, input: JobInput, now: i64) -> Result<Job, StorageError> {
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
        let changed = transaction.execute(
            "UPDATE jobs SET
                name = ?, command = ?, cwd = ?, target_kind = ?, target_distro = ?,
                env_ciphertext = COALESCE(?, env_ciphertext), cron_expr = ?, enabled = ?, overlap_policy = ?,
                catch_up = ?, last_evaluated_at = ?, updated_at = ?
             WHERE id = ? AND kind = 'job'",
            params![
                input.name,
                input.command,
                input.cwd,
                input.target_kind.as_str(),
                input.target_distro,
                input.env_ciphertext,
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
        restart_policy: row.get("restart_policy")?,
        auto_start,
        health_tcp_address: row.get("health_tcp_address")?,
        health_tcp_port,
        health_start_grace_ms: row.get("health_start_grace_ms")?,
        created_at: row.get("created_at")?,
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
    use crate::core::models::{JobInput, NewNotification, TargetKind};
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
            env_ciphertext: Some(vec![0xde, 0xad, 0xbe, 0xef]),
            cron_expr: "0 * * * *".to_string(),
            enabled,
            overlap_policy: OverlapPolicy::Queue,
            catch_up: false,
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
    fn job_crud_masks_ciphertext_and_resets_checkpoint_only_for_schedule_changes() {
        let database = DatabaseState::open_in_memory().unwrap();
        let created = database.create_job_at(input(true), 100).unwrap();
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
        let created = database.create_job_at(input(false), 100).unwrap();
        let mut renamed = input(false);
        renamed.name = "renamed".to_string();
        renamed.env_ciphertext = None;
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
