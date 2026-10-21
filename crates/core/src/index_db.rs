//! Rebuildable, independently versioned domain indexes. No backups or migrations.
use crate::{CoreError, db::DbConnections};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::{path::Path, sync::mpsc};
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct IndexDb {
    connections: DbConnections,
}

impl IndexDb {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS index_schema (domain TEXT PRIMARY KEY, version INTEGER NOT NULL);")?;
        Ok(Self {
            connections: DbConnections::new(path, conn)?,
        })
    }

    /// Rebuild this domain when its version changes, returning whether to reindex.
    /// Domain names are lowercase ASCII letters followed by letters or digits.
    /// Underscores would overlap another domain's ownership; `index` and `sqlite`
    /// are reserved for metadata and SQLite internals.
    ///
    /// `create_sql` is trusted developer schema SQL, without transaction control.
    /// It must create only this domain's objects, without cross-domain foreign keys.
    /// This synchronous operation is intended for service initialization, before
    /// serving index requests.
    pub fn ensure_domain(
        &self,
        domain: &str,
        version: u32,
        create_sql: &str,
    ) -> Result<bool, CoreError> {
        if !domain
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase)
            || !domain
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            || matches!(domain, "index" | "sqlite")
        {
            return Err(CoreError::DbInput("invalid index domain name".into()));
        }
        let domain = domain.to_owned();
        let create_sql = create_sql.to_owned();
        let (reply, wait) = mpsc::sync_channel(1);
        self.connections.dispatch(Box::new(move |conn| {
            let result = (|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let stored: Option<u32> = tx.query_row(
                    "SELECT version FROM index_schema WHERE domain=?1",
                    [&domain],
                    |row| row.get(0),
                ).optional()?;
                if stored == Some(version) {
                    return Ok(false);
                }
                // Parent tables can precede their children in SQLite's table list.
                // Check domain-local references only once the rebuild is complete.
                // SQLite resets this pragma at transaction commit or rollback.
                tx.execute_batch("PRAGMA defer_foreign_keys=ON;")?;
                // Read literal names, not LIKE patterns: `_` is not a wildcard.
                // Virtual tables own their shadow tables and drop them themselves.
                let tables = {
                    let mut statement = tx.prepare("SELECT name FROM pragma_table_list WHERE schema='main' AND type IN ('table','virtual')")?;
                    statement.query_map([], |row| row.get::<_, String>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                };
                let prefix = format!("{domain}_");
                for table in tables.into_iter().filter(|name| name.starts_with(&prefix)) {
                    tx.execute_batch(&format!("DROP TABLE \"{}\";", table.replace('"', "\"\"")))?;
                }
                tx.execute_batch(&create_sql)?;
                tx.execute("INSERT INTO index_schema(domain,version) VALUES (?1,?2) ON CONFLICT(domain) DO UPDATE SET version=excluded.version", rusqlite::params![domain, version])?;
                tx.commit()?;
                Ok(true)
            })();
            let _ = reply.send(result.map_err(CoreError::Db));
        }))?;
        wait.recv().map_err(|_| CoreError::DbClosed)?
    }

    /// Run an atomic index update on the dedicated writer.
    pub async fn write<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<T> + Send + 'static,
    {
        let (reply, wait) = oneshot::channel();
        self.connections.dispatch(Box::new(move |conn| {
            let result = (|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })();
            let _ = reply.send(result.map_err(CoreError::Db));
        }))?;
        wait.await.map_err(|_| CoreError::DbClosed)?
    }

    /// Read one consistent snapshot through the shared read-only connection pool.
    pub async fn read<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<T> + Send + 'static,
    {
        self.connections.read(f).await
    }
}
