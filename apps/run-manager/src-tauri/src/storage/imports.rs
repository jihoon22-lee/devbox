//! Runtime owner transaction. Original IDs are retained inside the new owner
//! namespace. Any existing ID/source-identity collision rejects the whole import;
//! repeated receipts never overwrite edits made after the first import.
use super::{DatabaseState, StorageError};
use crate::core::runtime_import::{text, ImportSummary, PreparedImport};
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, TransactionBehavior,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn initialize(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE IF NOT EXISTS workspace_runtime_imports (
            origin TEXT PRIMARY KEY NOT NULL, digest TEXT NOT NULL, imported_at INTEGER NOT NULL,
            summary_json TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS workspace_runtime_import_backups (
            origin TEXT PRIMARY KEY NOT NULL, manifest_digest TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS workspace_runtime_import_ids (
            origin TEXT NOT NULL, entity TEXT NOT NULL, source_id TEXT NOT NULL,
            destination_id TEXT NOT NULL, PRIMARY KEY(origin, entity, source_id));
        CREATE TABLE IF NOT EXISTS workspace_runtime_import_jobs (
            job_id TEXT PRIMARY KEY NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
            enabled_intent INTEGER NOT NULL, auto_start_intent INTEGER,
            reconnect_required INTEGER NOT NULL CHECK(reconnect_required IN (0,1)));
        CREATE TRIGGER IF NOT EXISTS workspace_runtime_import_enable_gate BEFORE UPDATE OF enabled, auto_start ON jobs
            WHEN (NEW.enabled=1 OR NEW.auto_start=1) AND EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=NEW.id AND reconnect_required=1)
            BEGIN SELECT RAISE(ABORT,'runtime_import_secret_review_required'); END;
        CREATE TRIGGER IF NOT EXISTS workspace_runtime_import_run_gate BEFORE INSERT ON runs
            WHEN NEW.status IN ('queued','starting','running','stopping') AND EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=NEW.job_id AND reconnect_required=1)
            BEGIN SELECT RAISE(ABORT,'runtime_import_secret_review_required'); END;
        CREATE TRIGGER IF NOT EXISTS workspace_runtime_import_start_gate BEFORE UPDATE OF status ON runs
            WHEN NEW.status IN ('starting','running') AND EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=NEW.job_id AND reconnect_required=1)
            BEGIN SELECT RAISE(ABORT,'runtime_import_secret_review_required'); END;
        CREATE TRIGGER IF NOT EXISTS workspace_runtime_import_service_gate BEFORE UPDATE OF state ON service_instances
            WHEN NEW.state IN ('starting','running','retry_waiting') AND EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=NEW.job_id AND reconnect_required=1)
            BEGIN SELECT RAISE(ABORT,'runtime_import_secret_review_required'); END;
        COMMIT;")
}
pub(super) fn resolve_secret_review(connection: &Connection, id: &str) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE workspace_runtime_import_jobs SET reconnect_required=0 WHERE job_id=?",
        [id],
    )?;
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedJobReview {
    pub job_id: String,
    pub reconnect_required: bool,
    pub enabled_intent: bool,
    pub auto_start_intent: Option<bool>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReceipt {
    pub summary: ImportSummary,
    pub already_imported: bool,
}
impl DatabaseState {
    pub fn requires_secret_review(&self, id: &str) -> Result<bool, StorageError> {
        if self.legacy_publication {
            return Ok(false);
        }
        Ok(self.lock()?.query_row("SELECT EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=? AND reconnect_required=1)",[id],|row|row.get(0))?)
    }
    pub fn imported_run(&self, id: &str) -> Result<bool, StorageError> {
        if self.legacy_publication {
            return Ok(false);
        }
        Ok(self.lock()?.query_row("SELECT EXISTS(SELECT 1 FROM workspace_runtime_import_ids WHERE origin='legacy-run-manager-v1' AND entity='runs' AND destination_id=?)",[id],|row|row.get(0))?)
    }
    pub fn imported_job_reviews(&self) -> Result<Vec<ImportedJobReview>, StorageError> {
        if self.legacy_publication {
            return Err(StorageError::Validation(
                "runtime_import_product_required".into(),
            ));
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare("SELECT job_id,reconnect_required,enabled_intent,auto_start_intent FROM workspace_runtime_import_jobs ORDER BY job_id LIMIT 100000")?;
        let rows = statement.query_map([], |row| {
            Ok(ImportedJobReview {
                job_id: row.get(0)?,
                reconnect_required: row.get(1)?,
                enabled_intent: row.get(2)?,
                auto_start_intent: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn import_legacy_runtime(
        &self,
        prepared: &PreparedImport,
        destination: &Path,
        flag: &AtomicBool,
    ) -> Result<ImportReceipt, String> {
        if self.legacy_publication {
            return Err("runtime_import_product_required".into());
        }
        devbox_filesystem::ensure_no_links(destination)
            .map_err(|_| "runtime_import_destination_conflict")?;
        let (_root_handle, root_identity) =
            devbox_filesystem::open_filesystem_object(destination, true)
                .map_err(|_| "runtime_import_destination_conflict")?;
        let (_database_handle, database_identity) =
            devbox_filesystem::open_filesystem_object(destination.join("data.db"), false)
                .map_err(|_| "runtime_import_destination_conflict")?;
        let _maintenance = self.log_maintenance().map_err(|_| "runtime_import_busy")?;
        prepared.verify_logs(flag)?;
        let mut connection = self.lock_mut().map_err(|_| "runtime_import_busy")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "runtime_import_busy")?;
        let previous: Option<(String,String)> = transaction.query_row("SELECT digest,summary_json FROM workspace_runtime_imports WHERE origin='legacy-run-manager-v1'",[],|row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(|_| "runtime_import_invalid")?;
        if let Some((digest, summary)) = previous {
            if digest != prepared.digest() {
                return Err("runtime_import_destination_conflict".into());
            }
            return Ok(ImportReceipt {
                summary: serde_json::from_str(&summary).map_err(|_| "runtime_import_invalid")?,
                already_imported: true,
            });
        }
        transaction
            .execute_batch("PRAGMA defer_foreign_keys=ON")
            .map_err(|_| "runtime_import_invalid")?;
        let now = super::current_epoch_millis();
        for table in &prepared.tables {
            let id_column = match table.name {
                "service_instances" => "job_id",
                "workspace_task_control_receipts" => "request_id",
                "workspace_task_operation_runs" => "operation_id",
                _ => "id",
            };
            let id_index = table.index(id_column);
            let placeholders = vec!["?"; table.columns.len()].join(",");
            let insert = format!(
                "INSERT INTO {} ({}) VALUES ({placeholders})",
                table.name,
                table.columns.join(",")
            );
            for original in &table.rows {
                if flag.load(Ordering::Acquire) {
                    return Err("runtime_import_cancelled".into());
                }
                let mut row = original.clone();
                let id = text(&original[id_index])?;
                if id.is_empty() || id.len() > 256 {
                    return Err("runtime_import_invalid".into());
                }
                let set = |row: &mut Vec<Value>, column: &str, value: Value| {
                    row[table.index(column)] = value;
                };
                match table.name {
                    "jobs" => {
                        set(&mut row, "enabled", Value::Integer(0));
                        if row[table.index("auto_start")] != Value::Null {
                            set(&mut row, "auto_start", Value::Integer(0));
                        }
                        set(&mut row, "env_ciphertext", Value::Null);
                        set(&mut row, "last_evaluated_at", Value::Integer(now));
                    }
                    "workspace_task_sources" => {
                        set(&mut row, "trusted", Value::Integer(0));
                        set(&mut row, "shell_trusted", Value::Integer(0));
                    }
                    "runs" => {
                        if matches!(
                            text(&row[table.index("status")])?,
                            "queued" | "starting" | "running" | "stopping"
                        ) {
                            set(&mut row, "status", Value::Text("failed".into()));
                            set(&mut row, "ended_at", Value::Integer(now));
                            set(
                                &mut row,
                                "error_message",
                                Value::Text("runtime-import-interrupted".into()),
                            );
                        }
                        for column in [
                            "blocked_by_run_id",
                            "owner_instance_id",
                            "attempt_token",
                            "target_pid",
                            "target_process_created_at",
                            "target_pgid",
                            "target_sid",
                            "process_marker",
                        ] {
                            set(&mut row, column, Value::Null);
                        }
                        if !prepared.has_logs(id) && row[table.index("log_dir")] != Value::Null {
                            set(&mut row, "log_dir", Value::Null);
                            set(&mut row, "logs_deleted_at", Value::Integer(now));
                        }
                    }
                    "service_instances" => {
                        set(&mut row, "state", Value::Text("stopped".into()));
                        for column in [
                            "active_run_id",
                            "owner_instance_id",
                            "attempt_token",
                            "next_retry_at",
                        ] {
                            set(&mut row, column, Value::Null);
                        }
                    }
                    "workspace_task_operations" => {
                        if matches!(
                            text(&row[table.index("status")])?,
                            "queued" | "running" | "stopping"
                        ) {
                            set(&mut row, "status", Value::Text("failed".into()));
                            set(&mut row, "ended_at", Value::Integer(now));
                            set(
                                &mut row,
                                "failure_code",
                                Value::Text("runtime-import-interrupted".into()),
                            );
                        }
                    }
                    "workspace_task_operation_runs" => {
                        if matches!(
                            text(&row[table.index("status")])?,
                            "pending" | "launching" | "running"
                        ) {
                            set(&mut row, "status", Value::Text("failed".into()));
                            set(
                                &mut row,
                                "failure_code",
                                Value::Text("runtime-import-interrupted".into()),
                            );
                        }
                    }
                    "workspace_task_control_receipts" => {
                        if text(&row[table.index("status")])? == "accepted" {
                            set(&mut row, "status", Value::Text("failed".into()));
                            set(
                                &mut row,
                                "failure_code",
                                Value::Text("runtime-import-interrupted".into()),
                            );
                        }
                    }
                    _ => return Err("runtime_import_invalid".into()),
                }
                transaction
                    .execute(&insert, params_from_iter(row))
                    .map_err(|_| "runtime_import_destination_conflict")?;
                if table.name == "jobs" {
                    transaction.execute("INSERT INTO workspace_runtime_import_jobs(job_id,enabled_intent,auto_start_intent,reconnect_required) VALUES(?,?,?,?)",params![id,original[table.index("enabled")],original[table.index("auto_start")],original[table.index("env_ciphertext")] != Value::Null]).map_err(|_| "runtime_import_invalid")?;
                }
                let mapping_id = if table.name == "workspace_task_operation_runs" {
                    format!("{id}:{}", text(&original[table.index("job_id")])?)
                } else {
                    id.into()
                };
                transaction.execute("INSERT INTO workspace_runtime_import_ids(origin,entity,source_id,destination_id) VALUES('legacy-run-manager-v1',?,?,?)",params![table.name,mapping_id,mapping_id]).map_err(|_| "runtime_import_invalid")?;
            }
        }
        let invalid: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "runtime_import_invalid")?;
        if invalid {
            return Err("runtime_import_invalid".into());
        }
        prepared.publish_logs(destination, flag)?;
        if flag.load(Ordering::Acquire) {
            return Err("runtime_import_cancelled".into());
        }
        transaction.execute("INSERT INTO workspace_runtime_imports(origin,digest,imported_at,summary_json) VALUES('legacy-run-manager-v1',?,?,?)",params![prepared.digest(),now,serde_json::to_string(prepared.summary()).map_err(|_| "runtime_import_invalid")?]).map_err(|_| "runtime_import_invalid")?;
        transaction
            .execute(
                "INSERT INTO workspace_runtime_import_backups VALUES('legacy-run-manager-v1',?1)",
                [prepared.backup_digest()?],
            )
            .map_err(|_| "runtime_import_invalid")?;
        devbox_filesystem::ensure_no_links(destination)
            .map_err(|_| "runtime_import_destination_conflict")?;
        if devbox_filesystem::filesystem_identity(destination, true)
            .map_err(|_| "runtime_import_destination_conflict")?
            != root_identity
            || devbox_filesystem::filesystem_identity(destination.join("data.db"), false)
                .map_err(|_| "runtime_import_destination_conflict")?
                != database_identity
        {
            return Err("runtime_import_destination_conflict".into());
        }
        transaction
            .commit()
            .map_err(|_| "runtime_import_write_failed")?;
        Ok(ImportReceipt {
            summary: prepared.summary().clone(),
            already_imported: false,
        })
    }
}

/// Read only the product-owned mapping ledger. No schema initialization or
/// scheduler startup occurs; WAL participates in one bounded read transaction.
pub fn mapping_summary(root: &Path) -> Result<(u64, String), String> {
    use sha2::{Digest, Sha256};
    let path = root.join("data.db");
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_import_mapping_unavailable")?;
    let mut hash = Sha256::new();
    hash.update(b"workspace-runtime-mappings-v1");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                0,
                hash.finalize()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
            ))
        }
        Err(_) => return Err("runtime_import_mapping_unavailable".into()),
        Ok(_) => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_import_mapping_unavailable")?;
    let (handle, identity) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| "runtime_import_mapping_unavailable")?;
    let mut connection =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| "runtime_import_mapping_unavailable")?;
    connection
        .busy_timeout(std::time::Duration::from_millis(100))
        .map_err(|_| "runtime_import_mapping_unavailable")?;
    let transaction = connection
        .transaction()
        .map_err(|_| "runtime_import_mapping_unavailable")?;
    let version: String = transaction
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| "runtime_import_mapping_unavailable")?;
    if version != super::SCHEMA_VERSION.to_string() {
        return Err("runtime_import_mapping_schema".into());
    }
    let mut statement = transaction.prepare("SELECT origin,entity,source_id,destination_id FROM workspace_runtime_import_ids ORDER BY origin,entity,source_id LIMIT 100001").map_err(|_| "runtime_import_mapping_unavailable")?;
    let mut rows = statement
        .query([])
        .map_err(|_| "runtime_import_mapping_unavailable")?;
    let mut count = 0;
    let started = std::time::Instant::now();
    while let Some(row) = rows
        .next()
        .map_err(|_| "runtime_import_mapping_unavailable")?
    {
        count += 1;
        if count > 100_000 || started.elapsed() > std::time::Duration::from_secs(5) {
            return Err("runtime_import_mapping_limit".into());
        }
        let mut values = Vec::new();
        for index in 0..4 {
            let raw = row
                .get_ref(index)
                .map_err(|_| "runtime_import_mapping_unavailable")?;
            let value = raw.as_str().map_err(|_| "runtime_import_mapping_invalid")?;
            if value.is_empty() || value.len() > 1024 {
                return Err("runtime_import_mapping_invalid".into());
            }
            values.push(value);
        }
        hash.update(serde_json::to_vec(&values).map_err(|_| "runtime_import_mapping_invalid")?);
    }
    if devbox_filesystem::filesystem_identity(&path, false)
        .map_err(|_| "runtime_import_mapping_unavailable")?
        != identity
    {
        return Err("runtime_import_mapping_changed".into());
    }
    drop(handle);
    Ok((
        count,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvironmentUpdate, JobInput, OverlapPolicy, TargetKind};
    fn fixture() -> (tempfile::TempDir, DatabaseState, String) {
        let root = tempfile::tempdir().unwrap();
        let source = DatabaseState::open(&root.path().join("data.db")).unwrap();
        let job = source
            .create_job_at(
                JobInput {
                    name: "synthetic import".into(),
                    command: "echo synthetic".into(),
                    cwd: None,
                    target_kind: TargetKind::Windows,
                    target_distro: None,
                    environment: EnvironmentUpdate::Keep,
                    cron_expr: "0 * * * *".into(),
                    enabled: true,
                    overlap_policy: OverlapPolicy::Skip,
                    catch_up: true,
                },
                100,
            )
            .unwrap();
        source
            .lock()
            .unwrap()
            .execute(
                "UPDATE jobs SET env_ciphertext=? WHERE id=?",
                params![vec![1u8, 2, 3], job.id],
            )
            .unwrap();
        let run = source.create_manual_run_at(&job.id, 200).unwrap();
        let relative = format!("logs/runs/{}", run.id);
        std::fs::create_dir_all(root.path().join(&relative)).unwrap();
        std::fs::write(
            root.path().join(&relative).join("stdout.log"),
            b"synthetic log\n",
        )
        .unwrap();
        source.lock().unwrap().execute("UPDATE runs SET log_dir=?,target_pid=42,owner_instance_id='synthetic-owner' WHERE id=?",params![relative,run.id]).unwrap();
        (root, source, job.id)
    }
    #[test]
    fn accepted_backup_binds_original_logs_and_detects_their_tampering() {
        let (source_root, _source, _job) = fixture();
        let target = tempfile::tempdir().unwrap();
        let stages = target.path().join("legacy-imports");
        std::fs::create_dir(&stages).unwrap();
        let stage = stages.join("a".repeat(32));
        let flag = AtomicBool::new(false);
        let prepared = PreparedImport::acquire(source_root.path(), &stage, &flag).unwrap();
        let database = DatabaseState::open_product(&target.path().join("data.db")).unwrap();
        database
            .import_legacy_runtime(&prepared, target.path(), &flag)
            .unwrap();
        let (bytes, schema, digest, logs) =
            crate::core::runtime_backup::verify(target.path(), prepared.digest()).unwrap();
        assert!(logs);
        assert_eq!(schema, 1);
        assert_eq!(bytes, prepared.backup_bytes().unwrap());
        assert_eq!(digest, prepared.backup_digest().unwrap());
        let run = std::fs::read_dir(stage.join("logs/runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(run.join("stdout.log"), b"tampered retained log").unwrap();
        assert!(crate::core::runtime_backup::verify(target.path(), prepared.digest()).is_err());
        assert!(crate::core::runtime_backup::receipt(target.path())
            .unwrap()
            .unwrap()
            .1
            .is_some());
    }
    #[test]
    fn wal_snapshot_import_is_inactive_preserves_ids_and_reopens_without_overwriting_edits() {
        let (source_root, source, job_id) = fixture();
        let stage_root = tempfile::tempdir().unwrap();
        let stage = stage_root.path().join("snapshot");
        let flag = AtomicBool::new(false);
        let prepared = PreparedImport::acquire(source_root.path(), &stage, &flag).unwrap();
        assert_eq!(prepared.summary().runs, 1);
        assert_eq!(prepared.summary().secrets_requiring_review, 1);
        let target = tempfile::tempdir().unwrap();
        let database = DatabaseState::open_product(&target.path().join("data.db")).unwrap();
        let receipt = database
            .import_legacy_runtime(&prepared, target.path(), &flag)
            .unwrap();
        assert!(!receipt.already_imported);
        let imported = database.get_job(&job_id).unwrap().unwrap();
        assert!(!imported.enabled);
        assert!(!imported.env_configured);
        assert!(database.set_job_enabled(&job_id, true).is_err());
        assert!(database.create_manual_run_at(&job_id, 300).is_err());
        let connection = database.lock().unwrap();
        let state: (String, Option<i64>, Option<String>) = connection
            .query_row(
                "SELECT status,target_pid,owner_instance_id FROM runs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(state, ("failed".into(), None, None));
        connection
            .execute(
                "UPDATE jobs SET name='edited locally' WHERE id=?",
                [&job_id],
            )
            .unwrap();
        drop(connection);
        drop(database);
        let reopened = PreparedImport::reopen(&stage, &flag).unwrap();
        let database = DatabaseState::open_product(&target.path().join("data.db")).unwrap();
        assert!(
            database
                .import_legacy_runtime(&reopened, target.path(), &flag)
                .unwrap()
                .already_imported
        );
        assert_eq!(
            database.get_job(&job_id).unwrap().unwrap().name,
            "edited locally"
        );
        assert!(source.get_job(&job_id).unwrap().unwrap().enabled);
        assert!(source.get_job(&job_id).unwrap().unwrap().env_configured);
        assert_eq!(
            source
                .lock()
                .unwrap()
                .query_row("SELECT status FROM runs", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "queued"
        );
    }
    #[test]
    fn conflict_rolls_back_all_definitions_and_cancellation_never_accepts_a_snapshot() {
        let (root, _source, job_id) = fixture();
        let stage = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let prepared =
            PreparedImport::acquire(root.path(), &stage.path().join("first"), &flag).unwrap();
        let target = tempfile::tempdir().unwrap();
        let database = DatabaseState::open_product(&target.path().join("data.db")).unwrap();
        database.lock().unwrap().execute("INSERT INTO jobs(id,name,command,cron_expr,created_at,updated_at) VALUES(?,'existing','echo existing','0 * * * *',1,1)",[&job_id]).unwrap();
        assert_eq!(
            database
                .import_legacy_runtime(&prepared, target.path(), &flag)
                .unwrap_err(),
            "runtime_import_destination_conflict"
        );
        assert_eq!(database.list_jobs().unwrap().len(), 1);
        assert_eq!(database.get_job(&job_id).unwrap().unwrap().name, "existing");
        flag.store(true, Ordering::Release);
        assert!(
            PreparedImport::acquire(root.path(), &stage.path().join("cancelled"), &flag).is_err()
        );
        assert!(!stage.path().join("cancelled/runtime-import.json").exists());
    }
    #[test]
    fn damaged_preserved_logs_and_future_source_schema_are_rejected_without_source_migration() {
        let (root, source, _) = fixture();
        let stage = tempfile::tempdir().unwrap();
        let flag = AtomicBool::new(false);
        let prepared =
            PreparedImport::acquire(root.path(), &stage.path().join("first"), &flag).unwrap();
        let run_id = source
            .lock()
            .unwrap()
            .query_row("SELECT id FROM runs", [], |row| row.get::<_, String>(0))
            .unwrap();
        std::fs::write(
            stage
                .path()
                .join(format!("first/logs/runs/{run_id}/stdout.log")),
            b"changed",
        )
        .unwrap();
        assert!(PreparedImport::reopen(&stage.path().join("first"), &flag).is_err());
        assert_eq!(prepared.summary().runs, 1);
        source
            .lock()
            .unwrap()
            .execute("UPDATE meta SET value='999' WHERE key='schema_version'", [])
            .unwrap();
        assert_eq!(
            PreparedImport::acquire(root.path(), &stage.path().join("future"), &flag)
                .err()
                .unwrap(),
            "runtime_import_schema_unsupported"
        );
        assert_eq!(
            source
                .lock()
                .unwrap()
                .query_row(
                    "SELECT value FROM meta WHERE key='schema_version'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "999"
        );
    }
}

#[cfg(test)]
mod mapping_summary_tests {
    use super::*;
    #[test]
    fn mapping_reader_includes_committed_wal_and_excludes_uncommitted_ids() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data.db");
        let initial = mapping_summary(root.path()).unwrap();
        assert_eq!(initial.0, 0);
        assert!(!path.exists());
        drop(DatabaseState::open_product(&path).unwrap());
        let writer = Connection::open(&path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
            INSERT INTO workspace_runtime_import_ids VALUES('fixture','jobs','source-job','destination-job');
            BEGIN IMMEDIATE;
            INSERT INTO workspace_runtime_import_ids VALUES('fixture','runs','uncommitted','uncommitted');").unwrap();
        let observed = mapping_summary(root.path()).unwrap();
        assert_eq!(observed.0, 1);
        assert_ne!(observed.1, initial.1);
        writer.execute_batch("ROLLBACK; UPDATE workspace_runtime_import_ids SET destination_id='changed-destination';").unwrap();
        assert_ne!(mapping_summary(root.path()).unwrap().1, observed.1);
        writer
            .execute_batch("UPDATE meta SET value='999' WHERE key='schema_version';")
            .unwrap();
        assert!(mapping_summary(root.path()).is_err());
    }
}
