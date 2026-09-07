//! Synthetic migration harness; production source selection and domain importers
//! are deliberately not exposed as a Tauri command. Backup acquisition uses
//! SQLite's online API, including committed WAL pages, not immutable readers.
use rusqlite::{
    backup::{Backup, StepResult},
    Connection, OpenFlags,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: i64,
    pub bytes: u64,
    pub sha256: String,
    pub acquisition: String,
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|_| "migration database operation failed".into())
}

fn digest(path: &Path) -> Result<(u64, String), String> {
    let mut file = fs::File::open(path).map_err(|_| "snapshot is unreadable")?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|_| "snapshot read failed")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_SNAPSHOT_BYTES {
            return Err("snapshot exceeds limit".into());
        }
        hash.update(&buffer[..count]);
    }
    Ok((
        bytes,
        hash.finalize().iter().map(|b| format!("{b:02x}")).collect(),
    ))
}

/// The caller owns this newly created staging directory; no existing source or
/// destination is overwritten. Failed/cancelled artifacts remain unaccepted.
pub fn acquire_snapshot(
    source: &Path,
    stage: &Path,
    accepted_schemas: &[i64],
    cancelled: &AtomicBool,
) -> Result<Snapshot, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("snapshot cancelled".into());
    }
    devbox_filesystem::ensure_no_links(source).map_err(|_| "source links are forbidden")?;
    devbox_filesystem::ensure_no_links(stage.parent().ok_or("stage parent is missing")?)
        .map_err(|_| "stage links are forbidden")?;
    let (_source_handle, source_identity) =
        devbox_filesystem::open_filesystem_object(source, false)
            .map_err(|_| "source identity is unavailable")?;
    let source_path = source.to_owned();
    let metadata = fs::symlink_metadata(source).map_err(|_| "source is missing")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("source must be a regular database".into());
    }
    if metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err("source exceeds limit".into());
    }
    let source = source
        .canonicalize()
        .map_err(|_| "source identity is unavailable")?;
    let parent = stage
        .parent()
        .ok_or("stage parent is missing")?
        .canonicalize()
        .map_err(|_| "stage parent is missing")?;
    let stage = parent.join(stage.file_name().ok_or("stage name is missing")?);
    if stage == source || source.starts_with(&stage) {
        return Err("source and stage overlap".into());
    }
    fs::create_dir(&stage).map_err(|_| "stage must be a new directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
            .map_err(|_| "stage permissions failed")?;
    }
    let source = sql(Connection::open_with_flags(
        &source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    sql(source.busy_timeout(Duration::from_millis(100)))?;
    // Pin the schema and page contents to one read transaction. Concurrent
    // writers can continue in WAL mode; multi-store cutover requires quiesce.
    sql(source.execute_batch("BEGIN"))?;
    let schema: i64 = sql(source.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if !accepted_schemas.contains(&schema) {
        return Err("unsupported source schema".into());
    }
    let path = stage.join("snapshot.db");
    let mut target = sql(Connection::open(&path))?;
    {
        let backup = sql(Backup::new(&source, &mut target))?;
        let started = Instant::now();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("snapshot cancelled".into());
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err("snapshot timed out; quiesce source and retry".into());
            }
            match sql(backup.step(128))? {
                StepResult::Done => break,
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                StepResult::More => {}
                _ => return Err("unsupported backup result".into()),
            }
            if fs::metadata(&path)
                .map_err(|_| "snapshot metadata failed")?
                .len()
                > MAX_SNAPSHOT_BYTES
            {
                return Err("snapshot exceeds limit".into());
            }
        }
    }
    let integrity: String = sql(target.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
    if integrity != "ok" {
        return Err("snapshot integrity failed".into());
    }
    let copied_schema: i64 = sql(target.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if copied_schema != schema {
        return Err("snapshot schema changed".into());
    }
    drop(target);
    sql(source.execute_batch("ROLLBACK"))?;
    let (bytes, sha256) = digest(&path)?;
    if devbox_filesystem::filesystem_identity(&source_path, false)
        .map_err(|_| "source identity is unavailable")?
        != source_identity
    {
        return Err("source identity changed".into());
    }
    let snapshot = Snapshot {
        schema_version: schema,
        bytes,
        sha256,
        acquisition: "sqlite-online-backup/v1".into(),
    };
    devbox_filesystem::atomic_write(
        stage.join("snapshot.json"),
        &serde_json::to_vec(&snapshot).map_err(|_| "snapshot metadata encoding failed")?,
    )
    .map_err(|_| "snapshot metadata publication failed")?;
    Ok(snapshot)
}

/// Record one import mapping in the same destination transaction as its write.
/// This journal does not turn multiple DBs or installer activation into an OS
/// transaction. A later domain importer owns conflicts and reference checks.
pub fn import_once(
    destination: &mut Connection,
    snapshot: &Snapshot,
    source_id: &str,
    destination_id: &str,
    apply: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), String>,
) -> Result<bool, String> {
    if source_id.is_empty()
        || source_id.len() > 256
        || destination_id.is_empty()
        || destination_id.len() > 256
        || snapshot.sha256.len() != 64
        || !snapshot.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid migration identity".into());
    }
    let transaction =
        sql(destination.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    sql(transaction.execute_batch("CREATE TABLE IF NOT EXISTS migration_journal_v1 (snapshot TEXT NOT NULL, source_id TEXT NOT NULL, destination_id TEXT NOT NULL, PRIMARY KEY(snapshot, source_id));"))?;
    let previous: Option<String> = sql(transaction
        .query_row(
            "SELECT destination_id FROM migration_journal_v1 WHERE snapshot=?1 AND source_id=?2",
            (&snapshot.sha256, source_id),
            |row| row.get(0),
        )
        .optional())?;
    if let Some(previous) = previous {
        return if previous == destination_id {
            Ok(false)
        } else {
            Err("migration mapping conflict".into())
        };
    }
    apply(&transaction)?;
    sql(transaction.execute(
        "INSERT INTO migration_journal_v1 VALUES (?1, ?2, ?3)",
        (&snapshot.sha256, source_id, destination_id),
    ))?;
    sql(transaction.commit())?;
    Ok(true)
}

use rusqlite::OptionalExtension;

pub fn verify_snapshot(stage: &Path, expected: &Snapshot) -> Result<PathBuf, String> {
    let path = stage.join("snapshot.db");
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "snapshot links are forbidden")?;
    if fs::symlink_metadata(&path)
        .map_err(|_| "snapshot is missing")?
        .file_type()
        .is_symlink()
    {
        return Err("snapshot link is forbidden".into());
    }
    let (bytes, sha256) = digest(&path)?;
    if bytes != expected.bytes || sha256 != expected.sha256 {
        return Err("snapshot identity changed".into());
    }
    Ok(path)
}

/// Resume only a fully published snapshot, never a partially copied database.
pub fn resume_snapshot(stage: &Path) -> Result<Snapshot, String> {
    let path = stage.join("snapshot.json");
    devbox_filesystem::ensure_no_links(&path)
        .map_err(|_| "snapshot metadata links are forbidden")?;
    let (file, _) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| "snapshot metadata is missing")?;
    let mut bytes = Vec::new();
    file.take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| "snapshot metadata read failed")?;
    if bytes.len() > 16_384 {
        return Err("snapshot metadata exceeds limit".into());
    }
    let snapshot: Snapshot =
        serde_json::from_slice(&bytes).map_err(|_| "invalid snapshot metadata")?;
    if snapshot.acquisition != "sqlite-online-backup/v1" || snapshot.schema_version < 0 {
        return Err("unsupported snapshot metadata".into());
    }
    verify_snapshot(stage, &snapshot)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir()
                .join(format!("devbox-migration-fixture-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn online_snapshot_sees_committed_wal_preserves_source_and_rejects_tampering() {
        let f = Fixture::new();
        let source = f.0.join("legacy.db");
        let writer = Connection::open(&source).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; PRAGMA user_version=7; CREATE TABLE records(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO records VALUES(1,'committed'); BEGIN IMMEDIATE; INSERT INTO records VALUES(2,'uncommitted');").unwrap();
        assert!(f.0.join("legacy.db-wal").exists());
        let stage = f.0.join("v08-snapshot");
        let metadata = acquire_snapshot(&source, &stage, &[7], &AtomicBool::new(false)).unwrap();
        let path = verify_snapshot(&stage, &metadata).unwrap();
        let backup = Connection::open(&path).unwrap();
        assert_eq!(
            backup
                .query_row("SELECT count(*) FROM records", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        writer.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            writer
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(
            writer
                .query_row("SELECT value FROM records", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "committed"
        );
        backup
            .execute("INSERT INTO records VALUES(3,'modified')", [])
            .unwrap();
        drop(backup);
        assert!(verify_snapshot(&stage, &metadata).is_err());
    }
    #[test]
    fn future_corrupt_cancelled_and_existing_destinations_fail_closed() {
        let f = Fixture::new();
        let source = f.0.join("legacy.db");
        let db = Connection::open(&source).unwrap();
        db.execute_batch("PRAGMA user_version=99").unwrap();
        drop(db);
        assert!(
            acquire_snapshot(&source, &f.0.join("future"), &[7], &AtomicBool::new(false)).is_err()
        );
        assert!(
            acquire_snapshot(&source, &f.0.join("cancel"), &[99], &AtomicBool::new(true)).is_err()
        );
        assert!(!f.0.join("cancel").exists());
        assert!(
            acquire_snapshot(&source, &f.0.join("future"), &[99], &AtomicBool::new(false)).is_err()
        );
        fs::write(&source, "corrupt fixture").unwrap();
        assert!(acquire_snapshot(
            &source,
            &f.0.join("corrupt"),
            &[99],
            &AtomicBool::new(false)
        )
        .is_err());
    }
    #[test]
    fn journal_rolls_back_partial_write_and_resumes_without_duplicate() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE records(id TEXT PRIMARY KEY)")
            .unwrap();
        let snapshot = Snapshot {
            schema_version: 7,
            bytes: 1,
            sha256: "a".repeat(64),
            acquisition: "fixture".into(),
        };
        assert!(import_once(&mut db, &snapshot, "old", "new", |tx| {
            sql(tx.execute("INSERT INTO records VALUES ('new')", []))?;
            Err("crash before commit".into())
        })
        .is_err());
        assert_eq!(
            db.query_row("SELECT count(*) FROM records", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(import_once(&mut db, &snapshot, "old", "new", |tx| {
            sql(tx.execute("INSERT INTO records VALUES ('new')", []))?;
            Ok(())
        })
        .unwrap());
        assert!(!import_once(&mut db, &snapshot, "old", "new", |_| panic!(
            "must not run twice"
        ))
        .unwrap());
        assert!(import_once(&mut db, &snapshot, "old", "conflict", |_| Ok(())).is_err());
    }
}
