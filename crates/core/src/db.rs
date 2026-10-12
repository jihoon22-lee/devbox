//! SQLite transactions with a dedicated writer and read-only reader pool.
use crate::CoreError;
use rusqlite::config::DbConfig;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use tokio::sync::oneshot;

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub domain: &'static str,
    pub version: u32,
    pub sql: &'static str,
}
type Job = Box<dyn FnOnce(&mut Connection) + Send>;
#[derive(Clone)]
pub struct Db {
    writer: mpsc::Sender<Job>,
    readers: Arc<Mutex<Vec<Connection>>>,
    path: PathBuf,
    rev: Arc<AtomicU64>,
}
const BASE: &str = "CREATE TABLE IF NOT EXISTS schema_versions (domain TEXT PRIMARY KEY, version INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS core_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL); INSERT OR IGNORE INTO core_meta(key,value) VALUES ('rev',0);";
fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version != "."
        && version != ".."
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
}
fn invalid(message: &str) -> CoreError {
    CoreError::DbInput(message.into())
}
fn revision(conn: &Connection) -> rusqlite::Result<u64> {
    conn.query_row("SELECT value FROM core_meta WHERE key='rev'", [], |r| {
        r.get(0)
    })
}
fn validate_schema_versions(
    conn: &Connection,
    supported: &HashMap<&str, u32>,
) -> Result<(), CoreError> {
    let initialized: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='schema_versions')",
        [],
        |row| row.get(0),
    )?;
    if !initialized {
        return Ok(());
    }
    let mut statement = conn.prepare("SELECT domain, version FROM schema_versions")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
    })?;
    for row in rows {
        let (domain, stored) = row?;
        let Some(latest) = supported.get(domain.as_str()) else {
            return Err(invalid("database contains an unsupported migration domain"));
        };
        if stored > *latest {
            return Err(invalid(
                "database schema version is newer than supported migrations",
            ));
        }
    }
    Ok(())
}
pub fn current_rev(tx: &Transaction<'_>) -> rusqlite::Result<u64> {
    revision(tx)
}
// Directory fsync makes publication and retention ordering durable on the WSL host.
#[cfg(unix)]
fn sync_directory(dir: &Path) -> Result<(), CoreError> {
    std::fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(crate::io_err("sync backups directory"))
}
#[cfg(not(unix))]
fn sync_directory(_: &Path) -> Result<(), CoreError> {
    Ok(())
}
fn backup(
    conn: &Connection,
    dir: &Path,
    version: &str,
    migrations: &[Migration],
) -> Result<(), CoreError> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temporary = dir.join(format!(
        ".devbox-backup-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    backup_at(conn, dir, version, migrations, &temporary)
}
fn backup_at(
    conn: &Connection,
    dir: &Path,
    version: &str,
    migrations: &[Migration],
    temporary: &Path,
) -> Result<(), CoreError> {
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(invalid("backups directory must not be a symlink"));
    }
    std::fs::create_dir_all(dir).map_err(crate::io_err("create backups dir"))?;
    let name = format!("devbox-pre-{version}.db");
    let target = dir.join(&name);
    // The ownership receipt prevents deleting unrelated prefix matches and identifies
    // the original snapshot when retrying a partially completed version migration.
    let receipt = dir.join(".devbox-backup");
    if std::fs::symlink_metadata(&receipt).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(invalid("backup receipt must be a regular file"));
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Receipt {
        name: String,
        batch: Vec<(String, u32)>,
    }
    let mut batch: Vec<_> = migrations
        .iter()
        .map(|m| (m.domain.to_owned(), m.version))
        .collect();
    batch.sort();
    let previous = match std::fs::read_to_string(&receipt) {
        Ok(s) => Some(
            serde_json::from_str::<Receipt>(&s)
                .map_err(|_| invalid("invalid backup ownership receipt"))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(crate::io_err("read backup receipt")(e)),
    };
    if let Some(prior) = &previous {
        let old = &prior.name;
        let valid = old
            .strip_prefix("devbox-pre-")
            .and_then(|s| s.strip_suffix(".db"))
            .is_some_and(valid_version);
        if !valid {
            return Err(invalid("invalid backup ownership receipt"));
        }
        if std::fs::symlink_metadata(dir.join(old)).is_ok_and(|m| !m.file_type().is_file()) {
            return Err(invalid("owned backup must be a regular file"));
        }
    }
    let owned_target = previous.as_ref().is_some_and(|r| r.name == name) && target.is_file();
    if owned_target && previous.as_ref().is_some_and(|r| r.batch == batch) {
        return Ok(());
    }
    if !owned_target && std::fs::symlink_metadata(&target).is_ok() {
        return Err(invalid(
            "backup destination already exists without ownership receipt",
        ));
    }
    let reserved = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(crate::io_err("reserve backup"))?;
    drop(reserved);
    let result = (|| {
        conn.execute(
            "VACUUM INTO ?1",
            [temporary
                .to_str()
                .ok_or_else(|| invalid("backup path is not UTF-8"))?],
        )?;
        std::fs::File::open(temporary)
            .and_then(|f| f.sync_all())
            .map_err(crate::io_err("sync backup"))?;
        let receipt_temp = temporary.with_extension("receipt");
        // Only clean up staging files after successful create_new ownership.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&receipt_temp)
            .map_err(crate::io_err("reserve backup receipt"))?;
        let receipt_result = (|| {
            let bytes = serde_json::to_vec(&Receipt {
                name: name.clone(),
                batch,
            })
            .map_err(|_| std::io::Error::other("serialize backup receipt"))?;
            std::io::Write::write_all(&mut file, &bytes)?;
            file.sync_all()
        })();
        drop(file);
        if let Err(e) = receipt_result {
            let _ = std::fs::remove_file(&receipt_temp);
            return Err(crate::io_err("write backup receipt")(e));
        }
        let publish: Result<(), CoreError> = (|| {
            if owned_target {
                std::fs::rename(temporary, &target)
                    .map_err(crate::io_err("replace owned backup"))?;
            } else {
                std::fs::hard_link(temporary, &target).map_err(crate::io_err("publish backup"))?;
            }
            sync_directory(dir)?;
            std::fs::rename(&receipt_temp, &receipt)
                .map_err(crate::io_err("publish backup receipt"))?;
            Ok(())
        })();
        if publish.is_err() {
            let _ = std::fs::remove_file(&receipt_temp);
        }
        publish?;
        sync_directory(dir)?;
        if let Some(prior) = previous {
            let old = prior.name;
            if let Some(v) = old
                .strip_prefix("devbox-pre-")
                .and_then(|s| s.strip_suffix(".db"))
                && valid_version(v)
                && old != name
            {
                match std::fs::remove_file(dir.join(old)) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(crate::io_err("remove previous owned backup")(e)),
                }
            }
        }
        sync_directory(dir)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temporary);
    result
}
impl Db {
    pub fn open(
        path: &Path,
        backups_dir: &Path,
        app_version: &str,
        migrations: &[Migration],
    ) -> Result<Self, CoreError> {
        if !valid_version(app_version) {
            return Err(invalid("app version must be a safe path component"));
        }
        let mut versions = HashMap::new();
        for m in migrations {
            if m.domain.is_empty()
                || m.version == 0
                || versions.get(m.domain).is_some_and(|v| *v >= m.version)
            {
                return Err(invalid(
                    "migration versions must be positive and strictly increasing per domain",
                ));
            }
            versions.insert(m.domain, m.version);
        }
        let existed = path.exists();
        let mut conn = Connection::open(path)?;
        // A rejected connection may be the last WAL reader after a crash. Its
        // close must not checkpoint newer data into the database or remove WAL.
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
        // Check every stored domain before changing journal mode, initializing
        // metadata, creating a backup, or applying any other domain's migration.
        validate_schema_versions(&conn, &versions)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        conn.execute_batch(BASE)?;
        let mut todo = Vec::new();
        for m in migrations {
            let have: Option<u32> = conn
                .query_row(
                    "SELECT version FROM schema_versions WHERE domain=?1",
                    [m.domain],
                    |r| r.get(0),
                )
                .optional()?;
            if have.is_none_or(|v| v < m.version) {
                todo.push(m);
            }
        }
        if existed && !todo.is_empty() {
            backup(&conn, backups_dir, app_version, migrations)?;
        }
        for m in todo {
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute("INSERT INTO schema_versions(domain,version) VALUES (?1,?2) ON CONFLICT(domain) DO UPDATE SET version=excluded.version", rusqlite::params![m.domain,m.version])?;
            tx.commit()?;
        }
        let rev = Arc::new(AtomicU64::new(revision(&conn)?));
        let (writer, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("devbox-db-writer".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    // A failed/panicking job drops its reply and rolls back its transaction.
                    let _ =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)));
                }
            })
            .map_err(crate::io_err("spawn db writer"))?;
        Ok(Self {
            writer,
            readers: Arc::new(Mutex::new(Vec::new())),
            path: path.to_owned(),
            rev,
        })
    }
    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::Acquire)
    }
    pub async fn write<T, F>(&self, f: F) -> Result<(T, u64), CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<T> + Send + 'static,
    {
        match self
            .write_checked::<T, std::convert::Infallible, _>(move |tx| f(tx).map(Ok))
            .await?
        {
            Ok(committed) => Ok(committed),
            Err(never) => match never {},
        }
    }
    /// Roll back typed domain rejections without advancing the database revision.
    /// The outer result reports database errors; the inner result reports rejection.
    pub async fn write_checked<T, E, F>(&self, f: F) -> Result<Result<(T, u64), E>, CoreError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<Result<T, E>> + Send + 'static,
    {
        self.write_checked_with_commit(f, |_, _| {}).await
    }
    /// Invoke `committed` on the writer after commit and revision publication,
    /// before replying, even if the caller has dropped its response future.
    /// Rejections and database failures never invoke the callback.
    pub async fn write_checked_with_commit<T, E, F, C>(
        &self,
        f: F,
        committed: C,
    ) -> Result<Result<(T, u64), E>, CoreError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<Result<T, E>> + Send + 'static,
        C: FnOnce(&T, u64) + Send + 'static,
    {
        let (reply, wait) = oneshot::channel();
        let rev = self.rev.clone();
        self.writer
            .send(Box::new(move |conn| {
                let result = (|| {
                    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                    let out = match f(&tx)? {
                        Ok(out) => out,
                        Err(rejected) => {
                            tx.rollback()?;
                            return Ok(Err(rejected));
                        }
                    };
                    tx.execute("UPDATE core_meta SET value=value+1 WHERE key='rev'", [])?;
                    let next = current_rev(&tx)?;
                    tx.commit()?;
                    rev.store(next, Ordering::Release);
                    committed(&out, next);
                    Ok(Ok((out, next)))
                })();
                let _ = reply.send(result.map_err(CoreError::Db));
            }))
            .map_err(|_| CoreError::DbClosed)?;
        wait.await.map_err(|_| CoreError::DbClosed)?
    }
    pub async fn read<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<T> + Send + 'static,
    {
        let readers = self.readers.clone();
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let pooled = readers.lock().map_err(|_| CoreError::DbClosed)?.pop();
            let mut conn = match pooled {
                Some(c) => c,
                None => {
                    let c = Connection::open_with_flags(
                        path,
                        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                    )?;
                    c.execute_batch(
                        "PRAGMA query_only=ON; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
                    )?;
                    c
                }
            };
            let result = (|| {
                let tx = conn.transaction()?;
                let out = f(&tx)?;
                tx.rollback()?;
                Ok(out)
            })();
            readers.lock().map_err(|_| CoreError::DbClosed)?.push(conn);
            result
        })
        .await
        .map_err(|_| CoreError::DbClosed)?
    }
}
#[cfg(test)]
mod tests;
