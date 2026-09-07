//! Small destination-owned plan/transaction boundary for domain fixtures.
//! Paths are native configuration; this is not a renderer-accessible resolver.
use super::migration::{resume_snapshot, verify_snapshot, Snapshot};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mapping {
    pub source_id: String,
    pub destination_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportPlan {
    pub schema_version: u32,
    pub namespace: String,
    pub source_store: String,
    pub destination_revision: i64,
    pub snapshot: Snapshot,
    pub mappings: Vec<Mapping>,
    /// Domain-owned conflicts must be resolved by a new reviewed plan.
    pub conflicts: Vec<String>,
}

pub struct MigrationWorkspace {
    root: PathBuf,
    namespace: String,
}

fn bounded_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}
fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|_| "migration destination operation failed".into())
}

impl MigrationWorkspace {
    /// `trusted_root` is an existing, native-selected synthetic/user data root.
    /// Existing legacy stores are never opened as a writable destination.
    pub fn open(trusted_root: &Path, product: &str, installation: &str) -> Result<Self, String> {
        if !["workspace", "api-studio", "knowledge", "control-center"].contains(&product)
            || !bounded_id(installation)
        {
            return Err("invalid destination namespace".into());
        }
        devbox_filesystem::ensure_no_links(trusted_root)
            .map_err(|_| "destination root contains links")?;
        let mut root = trusted_root
            .canonicalize()
            .map_err(|_| "destination root is missing")?;
        for part in ["v08", product, installation, "migration"] {
            root.push(part);
            if !root.exists() {
                fs::create_dir(&root).map_err(|_| "destination directory creation failed")?;
            }
            devbox_filesystem::ensure_no_links(&root)
                .map_err(|_| "destination namespace contains links")?;
            if !root.is_dir() {
                return Err("destination namespace is not a directory".into());
            }
        }
        let workspace = Self {
            root,
            namespace: format!("{product}/{installation}"),
        };
        let connection = workspace.destination()?;
        sql(connection.execute_batch("CREATE TABLE IF NOT EXISTS migration_meta_v1 (singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL); INSERT OR IGNORE INTO migration_meta_v1 VALUES(1,0); CREATE TABLE IF NOT EXISTS migration_runs_v1 (plan_hash TEXT PRIMARY KEY); CREATE TABLE IF NOT EXISTS migration_maps_v1 (source_store TEXT NOT NULL, snapshot TEXT NOT NULL, source_id TEXT NOT NULL, destination_id TEXT NOT NULL, PRIMARY KEY(source_store,snapshot,source_id));"))?;
        Ok(workspace)
    }

    fn destination(&self) -> Result<Connection, String> {
        devbox_filesystem::ensure_no_links(&self.root)
            .map_err(|_| "destination namespace contains links")?;
        let path = self.root.join("destination.db");
        if fs::symlink_metadata(&path).is_ok() {
            devbox_filesystem::ensure_no_links(&path)
                .map_err(|_| "destination database contains links")?;
        }
        let connection = sql(Connection::open(&path))?;
        sql(connection.busy_timeout(std::time::Duration::from_millis(100)))?;
        sql(connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;"))?;
        Ok(connection)
    }

    /// Domain readers see only this destination-owned database. The readonly
    /// connection cannot alter import receipts or bypass revision checks.
    pub fn inspect<T>(
        &self,
        inspect: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        devbox_filesystem::ensure_no_links(self.root.join("destination.db"))
            .map_err(|_| "destination database contains links")?;
        let connection = sql(Connection::open_with_flags(
            self.root.join("destination.db"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ))?;
        inspect(&connection)
    }

    /// Domain activation bookkeeping remains in the destination transaction;
    /// this does not claim atomicity with browser storage or native files.
    pub fn update<T>(
        &self,
        update: impl FnOnce(&Transaction<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut connection = self.destination()?;
        let transaction =
            sql(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let result = update(&transaction)?;
        sql(transaction.execute(
            "UPDATE migration_meta_v1 SET revision=revision+1 WHERE singleton=1",
            [],
        ))?;
        sql(transaction.commit())?;
        Ok(result)
    }

    pub fn prepare(
        &self,
        stage: &Path,
        source_store: &str,
        mappings: Vec<Mapping>,
        conflicts: Vec<String>,
    ) -> Result<ImportPlan, String> {
        let snapshot = resume_snapshot(stage)?;
        let revision = sql(self.destination()?.query_row(
            "SELECT revision FROM migration_meta_v1 WHERE singleton=1",
            [],
            |r| r.get(0),
        ))?;
        let plan = ImportPlan {
            schema_version: 1,
            namespace: self.namespace.clone(),
            source_store: source_store.into(),
            destination_revision: revision,
            snapshot,
            mappings,
            conflicts,
        };
        self.validate_plan(&plan)?;
        devbox_filesystem::atomic_write(
            stage.join("import-plan.json"),
            &serde_json::to_vec(&plan).map_err(|_| "plan encoding failed")?,
        )
        .map_err(|_| "plan publication failed")?;
        Ok(plan)
    }

    fn validate_plan(&self, plan: &ImportPlan) -> Result<(), String> {
        if plan.schema_version != 1
            || plan.namespace != self.namespace
            || !bounded_id(&plan.source_store)
            || plan.destination_revision < 0
            || plan.mappings.len() > 10_000
            || plan.conflicts.len() > 1000
            || plan.conflicts.iter().any(|id| !bounded_id(id))
        {
            return Err("invalid migration plan".into());
        }
        let mut sources = HashSet::new();
        let mut destinations = HashSet::new();
        for mapping in &plan.mappings {
            if !bounded_id(&mapping.source_id)
                || !bounded_id(&mapping.destination_id)
                || !sources.insert(&mapping.source_id)
                || !destinations.insert(&mapping.destination_id)
            {
                return Err("ambiguous migration mapping".into());
            }
        }
        Ok(())
    }

    /// Apply and domain validation share one transaction with the receipt and
    /// ID map. Cancellation before commit rolls back; after commit it is done.
    /// Returns false on a previously committed plan after restart.
    pub fn apply(
        &self,
        stage: &Path,
        plan: &ImportPlan,
        cancelled: &AtomicBool,
        import_and_validate: impl FnOnce(
            &Connection,
            &Transaction<'_>,
            &[Mapping],
        ) -> Result<(), String>,
    ) -> Result<bool, String> {
        self.validate_plan(plan)?;
        if !plan.conflicts.is_empty() {
            return Err("migration plan has unresolved conflicts".into());
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err("migration cancelled".into());
        }
        let snapshot_path = verify_snapshot(stage, &plan.snapshot)?;
        let source = sql(Connection::open_with_flags(
            &snapshot_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ))?;
        let schema: i64 = sql(source.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if schema != plan.snapshot.schema_version
            || plan.snapshot.acquisition != "sqlite-online-backup/v1"
        {
            return Err("snapshot schema mismatch".into());
        }
        let hash: String =
            Sha256::digest(serde_json::to_vec(plan).map_err(|_| "plan encoding failed")?)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
        let mut destination = self.destination()?;
        let transaction =
            sql(destination.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let applied: bool = sql(transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_runs_v1 WHERE plan_hash=?1)",
            [&hash],
            |r| r.get(0),
        ))?;
        if applied {
            return Ok(false);
        }
        let revision: i64 = sql(transaction.query_row(
            "SELECT revision FROM migration_meta_v1 WHERE singleton=1",
            [],
            |r| r.get(0),
        ))?;
        if revision != plan.destination_revision {
            return Err("destination changed; review a new plan".into());
        }
        for mapping in &plan.mappings {
            let previous: Option<String> = sql(transaction.query_row("SELECT destination_id FROM migration_maps_v1 WHERE source_store=?1 AND snapshot=?2 AND source_id=?3", (&plan.source_store, &plan.snapshot.sha256, &mapping.source_id), |r| r.get(0)).optional())?;
            if previous.is_some() {
                return Err("source mapping already imported; review a new plan".into());
            }
        }
        import_and_validate(&source, &transaction, &plan.mappings)?;
        if cancelled.load(Ordering::Relaxed) {
            return Err("migration cancelled".into());
        }
        verify_snapshot(stage, &plan.snapshot)?;
        for mapping in &plan.mappings {
            sql(transaction.execute(
                "INSERT INTO migration_maps_v1 VALUES(?1,?2,?3,?4)",
                (
                    &plan.source_store,
                    &plan.snapshot.sha256,
                    &mapping.source_id,
                    &mapping.destination_id,
                ),
            ))?;
        }
        sql(transaction.execute("INSERT INTO migration_runs_v1 VALUES(?1)", [&hash]))?;
        sql(transaction.execute(
            "UPDATE migration_meta_v1 SET revision=revision+1 WHERE singleton=1",
            [],
        ))?;
        sql(transaction.commit())?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::acquire_snapshot;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Fixture, PathBuf, MigrationWorkspace) {
        let root =
            std::env::temp_dir().join(format!("devbox-plan-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let legacy = root.join("legacy.db");
        Connection::open(&legacy).unwrap().execute_batch("PRAGMA user_version=7; CREATE TABLE records(value TEXT); INSERT INTO records VALUES('synthetic');").unwrap();
        let stage = root.join("snapshot");
        acquire_snapshot(&legacy, &stage, &[7], &AtomicBool::new(false)).unwrap();
        let workspace = MigrationWorkspace::open(&root, "knowledge", "fixture-install").unwrap();
        (Fixture(root), stage, workspace)
    }
    fn mappings() -> Vec<Mapping> {
        vec![Mapping {
            source_id: "old-1".into(),
            destination_id: "new-1".into(),
        }]
    }
    fn import(source: &Connection, target: &Transaction<'_>, _: &[Mapping]) -> Result<(), String> {
        let value: String = sql(source.query_row("SELECT value FROM records", [], |r| r.get(0)))?;
        sql(target.execute_batch("CREATE TABLE records(value TEXT);"))?;
        sql(target.execute("INSERT INTO records VALUES(?1)", [&value]))?;
        let count: i64 = sql(target.query_row("SELECT count(*) FROM records", [], |r| r.get(0)))?;
        if count != 1 {
            return Err("domain validation failed".into());
        }
        Ok(())
    }
    #[test]
    fn dry_run_failure_cancel_restart_and_repeat_preserve_transaction_boundary() {
        let (fixture, stage, workspace) = fixture();
        let plan = workspace
            .prepare(&stage, "knowledge-records", mappings(), vec![])
            .unwrap();
        let bytes = fs::read(stage.join("import-plan.json")).unwrap();
        let resumed: ImportPlan = serde_json::from_slice(&bytes).unwrap();
        assert!(workspace
            .apply(&stage, &plan, &AtomicBool::new(false), |source, tx, ids| {
                import(source, tx, ids)?;
                Err("simulated interrupted validation".into())
            })
            .is_err());
        let cancelled = AtomicBool::new(false);
        assert!(workspace
            .apply(&stage, &plan, &cancelled, |source, tx, ids| {
                import(source, tx, ids)?;
                cancelled.store(true, Ordering::Relaxed);
                Ok(())
            })
            .is_err());
        drop(workspace);
        let restarted =
            MigrationWorkspace::open(&fixture.0, "knowledge", "fixture-install").unwrap();
        assert!(restarted
            .apply(&stage, &resumed, &AtomicBool::new(false), import)
            .unwrap());
        assert!(!restarted
            .apply(&stage, &resumed, &AtomicBool::new(false), |_, _, _| panic!(
                "must not rerun committed plan"
            ))
            .unwrap());
        let source = Connection::open(fixture.0.join("legacy.db")).unwrap();
        assert_eq!(
            source
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            source
                .query_row("SELECT count(*) FROM records", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn foreign_namespace_conflict_stale_plan_and_partial_snapshot_fail_closed() {
        let (fixture, stage, workspace) = fixture();
        let plan = workspace
            .prepare(&stage, "knowledge-records", mappings(), vec![])
            .unwrap();
        let other = MigrationWorkspace::open(&fixture.0, "knowledge", "other-install").unwrap();
        assert!(other
            .apply(&stage, &plan, &AtomicBool::new(false), import)
            .is_err());
        let mut conflict = plan.clone();
        conflict.conflicts.push("record-conflict".into());
        assert!(workspace
            .apply(&stage, &conflict, &AtomicBool::new(false), import)
            .is_err());
        assert!(workspace
            .apply(&stage, &plan, &AtomicBool::new(false), import)
            .unwrap());
        let mut stale = plan.clone();
        stale.mappings[0].source_id = "old-2".into();
        assert!(workspace
            .apply(&stage, &stale, &AtomicBool::new(false), import)
            .is_err());
        fs::remove_file(stage.join("snapshot.json")).unwrap();
        assert!(workspace
            .prepare(&stage, "knowledge-records", mappings(), vec![])
            .is_err());
        assert!(MigrationWorkspace::open(&fixture.0, "knowledge", "../legacy").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_source_parent_and_destination_are_rejected() {
        use std::os::unix::fs::symlink;
        let (fixture, _, _) = fixture();
        let linked = fixture.0.join("linked");
        symlink(&fixture.0, &linked).unwrap();
        assert!(acquire_snapshot(
            &linked.join("legacy.db"),
            &fixture.0.join("new-stage"),
            &[7],
            &AtomicBool::new(false)
        )
        .is_err());
        assert!(MigrationWorkspace::open(&linked, "knowledge", "fixture-install").is_err());
    }
}
