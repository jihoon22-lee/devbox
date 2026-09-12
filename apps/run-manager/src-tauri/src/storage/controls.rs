//! Product-only durable request receipts. A pending request is never replayed
//! as a new side effect after a renderer reload or native crash. The original
//! Run Manager schema/version and standalone startup stay unchanged.
use super::{DatabaseState, StorageError};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const RESULT_LIMIT: usize = 512 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeControlReceipt {
    pub operation_id: String,
    pub method: String,
    pub target_id: String,
    pub state: String,
    pub result: Option<Value>,
    pub created_at: i64,
    pub reviewed: bool,
}
pub(crate) enum ControlReservation {
    New,
    Existing(RuntimeControlReceipt),
}
pub(super) fn validate_schema(connection: &Connection) -> rusqlite::Result<()> {
    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='workspace_runtime_meta')", [], |row| row.get(0))?;
    if exists {
        let versions = connection
            .prepare("SELECT schema_version FROM workspace_runtime_meta LIMIT 2")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if versions != [1] {
            return Err(rusqlite::Error::InvalidQuery);
        }
    } else {
        let orphaned: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='workspace_runtime_controls')",
            [],
            |row| row.get(0),
        )?;
        if orphaned {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}
pub(super) fn initialize(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch("CREATE TABLE IF NOT EXISTS workspace_runtime_meta (schema_version INTEGER PRIMARY KEY NOT NULL);
        INSERT OR IGNORE INTO workspace_runtime_meta(schema_version) VALUES(1);
        CREATE TABLE IF NOT EXISTS workspace_runtime_controls (
            operation_id TEXT PRIMARY KEY NOT NULL,
            method TEXT NOT NULL,
            target_id TEXT NOT NULL,
            fingerprint TEXT NOT NULL,
            state TEXT NOT NULL CHECK(state IN ('pending','completed','failed','interrupted')),
            result_json TEXT,
            created_at INTEGER NOT NULL,
            reviewed INTEGER NOT NULL DEFAULT 0 CHECK(reviewed IN (0,1))
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_runtime_controls_created ON workspace_runtime_controls(created_at DESC);
        UPDATE workspace_runtime_controls SET state='interrupted' WHERE state='pending';")?;
    transaction.commit()
}
fn valid_operation_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn invalid() -> StorageError {
    StorageError::Validation("runtime-control-invalid".into())
}
fn receipt(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeControlReceipt> {
    let encoded: Option<String> = row.get(4)?;
    let result = encoded
        .map(|encoded| {
            if encoded.len() > RESULT_LIMIT {
                return Err(rusqlite::Error::InvalidQuery);
            }
            serde_json::from_str(&encoded).map_err(|_| rusqlite::Error::InvalidQuery)
        })
        .transpose()?;
    Ok(RuntimeControlReceipt {
        operation_id: row.get(0)?,
        method: row.get(1)?,
        target_id: row.get(2)?,
        state: row.get(3)?,
        result,
        created_at: row.get(5)?,
        reviewed: row.get(6)?,
    })
}
const COLUMNS: &str = "operation_id, method, target_id, state, result_json, created_at, reviewed";
impl DatabaseState {
    pub(crate) fn reserve_runtime_control(
        &self,
        id: &str,
        method: &str,
        target_id: &str,
        fingerprint: &str,
        now: i64,
    ) -> Result<ControlReservation, StorageError> {
        if self.legacy_publication
            || !valid_operation_id(id)
            || !crate::core::runtime_controls::legacy_control_method(method)
            || target_id.is_empty()
            || target_id.len() > 128
            || target_id
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
            || fingerprint.len() != 64
            || fingerprint.bytes().any(|byte| !byte.is_ascii_hexdigit())
            || now <= 0
        {
            return Err(invalid());
        }
        let mut connection = self.lock_mut()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String, String)> = transaction.query_row(
            "SELECT method,target_id,fingerprint FROM workspace_runtime_controls WHERE operation_id=?", [id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
        if let Some(existing) = existing {
            if existing != (method.into(), target_id.into(), fingerprint.into()) {
                return Err(invalid());
            }
            let value = transaction.query_row(
                &format!("SELECT {COLUMNS} FROM workspace_runtime_controls WHERE operation_id=?"),
                [id],
                receipt,
            )?;
            transaction.commit()?;
            return Ok(ControlReservation::Existing(value));
        }
        transaction.execute("INSERT INTO workspace_runtime_controls(operation_id,method,target_id,fingerprint,state,created_at) VALUES(?,?,?,?,'pending',?)", params![id,method,target_id,fingerprint,now])?;
        transaction.commit()?;
        Ok(ControlReservation::New)
    }
    pub(crate) fn finish_runtime_control(
        &self,
        id: &str,
        result: Option<&Value>,
    ) -> Result<(), StorageError> {
        if self.legacy_publication || !valid_operation_id(id) {
            return Err(invalid());
        }
        let encoded = result
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| invalid())?;
        if encoded
            .as_ref()
            .is_some_and(|value| value.len() > RESULT_LIMIT)
        {
            return Err(invalid());
        }
        let changed = self.lock_mut()?.execute("UPDATE workspace_runtime_controls SET state=?,result_json=? WHERE operation_id=? AND state='pending'",
            params![if result.is_some() {"completed"} else {"failed"},encoded,id])?;
        if changed != 1 {
            return Err(StorageError::ConcurrentChange(
                "runtime-control-receipt".into(),
            ));
        }
        Ok(())
    }
    pub fn runtime_control(&self, id: &str) -> Result<Option<RuntimeControlReceipt>, StorageError> {
        if self.legacy_publication || !valid_operation_id(id) {
            return Err(invalid());
        }
        self.lock()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM workspace_runtime_controls WHERE operation_id=?"),
                [id],
                receipt,
            )
            .optional()
            .map_err(StorageError::from)
    }
    pub fn unresolved_runtime_controls(&self) -> Result<Vec<RuntimeControlReceipt>, StorageError> {
        if self.legacy_publication {
            return Err(invalid());
        }
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!("SELECT {COLUMNS} FROM workspace_runtime_controls WHERE state IN ('pending','interrupted') AND reviewed=0 ORDER BY created_at DESC,operation_id LIMIT 64"))?;
        let rows = statement.query_map([], receipt)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(StorageError::from)
    }
    pub fn review_runtime_control(&self, id: &str) -> Result<(), StorageError> {
        let request = self.runtime_control(id)?.ok_or_else(invalid)?;
        if request.state != "interrupted" {
            return Err(invalid());
        }
        if self.active_process_run(&request.target_id)?.is_some() {
            return Err(StorageError::ConcurrentChange(
                "runtime-control-owner-unsettled".into(),
            ));
        }
        let changed = self.lock_mut()?.execute("UPDATE workspace_runtime_controls SET reviewed=1 WHERE operation_id=? AND state='interrupted'", [id])?;
        if changed != 1 {
            return Err(invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvironmentUpdate, JobInput, OverlapPolicy, RunView, TargetKind};
    fn definition(database: &DatabaseState) -> String {
        database
            .create_job_at(
                JobInput {
                    name: "receipt fixture".into(),
                    command: "echo fixture".into(),
                    cwd: None,
                    target_kind: TargetKind::Windows,
                    target_distro: None,
                    environment: EnvironmentUpdate::Keep,
                    cron_expr: "0 * * * *".into(),
                    enabled: false,
                    overlap_policy: OverlapPolicy::Queue,
                    catch_up: false,
                },
                100,
            )
            .unwrap()
            .id
    }
    #[test]
    fn durable_replay_does_not_claim_another_manual_run_after_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.db");
        let database = DatabaseState::open_product(&path).unwrap();
        let target = definition(&database);
        let id = uuid::Uuid::new_v4().to_string();
        assert!(matches!(
            database
                .reserve_runtime_control(&id, "run_job_now", &target, &"a".repeat(64), 200)
                .unwrap(),
            ControlReservation::New
        ));
        let run = database.claim_manual_run(&target, 200).unwrap().run;
        let response = serde_json::to_value(RunView::from_run(&run)).unwrap();
        database
            .finish_runtime_control(&id, Some(&response))
            .unwrap();
        drop(database);
        let reopened = DatabaseState::open_product(&path).unwrap();
        let ControlReservation::Existing(receipt) = reopened
            .reserve_runtime_control(&id, "run_job_now", &target, &"a".repeat(64), 300)
            .unwrap()
        else {
            panic!("must replay");
        };
        assert_eq!(receipt.result, Some(response));
        assert_eq!(receipt.state, "completed");
        assert_eq!(
            reopened.list_runs(&target, 100, None, None).unwrap().len(),
            1
        );
        assert_eq!(
            reopened.schema_version().unwrap(),
            super::super::SCHEMA_VERSION
        );
        assert!(reopened
            .reserve_runtime_control(&id, "run_job_now", &target, &"b".repeat(64), 301)
            .is_err());
    }
    #[test]
    fn crash_before_completion_requires_review_and_never_reexecutes_the_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.db");
        let database = DatabaseState::open_product(&path).unwrap();
        let target = definition(&database);
        let id = uuid::Uuid::new_v4().to_string();
        database
            .reserve_runtime_control(&id, "run_job_now", &target, &"a".repeat(64), 200)
            .unwrap();
        drop(database);
        let database = DatabaseState::open_product(&path).unwrap();
        let ControlReservation::Existing(receipt) = database
            .reserve_runtime_control(&id, "run_job_now", &target, &"a".repeat(64), 300)
            .unwrap()
        else {
            panic!("must not reexecute");
        };
        assert_eq!(receipt.state, "interrupted");
        assert_eq!(database.unresolved_runtime_controls().unwrap().len(), 1);
        database.review_runtime_control(&id).unwrap();
        assert!(database.unresolved_runtime_controls().unwrap().is_empty());
        assert!(matches!(
            database
                .reserve_runtime_control(&id, "run_job_now", &target, &"a".repeat(64), 400)
                .unwrap(),
            ControlReservation::Existing(_)
        ));
        assert!(database
            .list_runs(&target, 100, None, None)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn concurrent_submissions_reserve_once_and_keep_legacy_schema_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let database = std::sync::Arc::new(
            DatabaseState::open_product(&directory.path().join("data.db")).unwrap(),
        );
        let target = definition(&database);
        let id = uuid::Uuid::new_v4().to_string();
        let inserted = std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        matches!(
                            database
                                .reserve_runtime_control(
                                    &id,
                                    "run_job_now",
                                    &target,
                                    &"a".repeat(64),
                                    200
                                )
                                .unwrap(),
                            ControlReservation::New
                        )
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| usize::from(handle.join().unwrap()))
                .sum::<usize>()
        });
        assert_eq!(inserted, 1);
        let legacy = DatabaseState::open_in_memory().unwrap();
        let count: i64 = legacy
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='workspace_runtime_controls'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            legacy.schema_version().unwrap(),
            database.schema_version().unwrap()
        );
    }
    #[test]
    fn future_control_schema_is_preserved_before_any_migration_write() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.db");
        let database = DatabaseState::open_product(&path).unwrap();
        database
            .lock_mut()
            .unwrap()
            .execute("UPDATE workspace_runtime_meta SET schema_version=2", [])
            .unwrap();
        drop(database);
        let before = std::fs::read(&path).unwrap();
        assert!(DatabaseState::open_product(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
