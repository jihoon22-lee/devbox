//! `devbox.db` domain `activity`: stored sessions and the write boundary the
//! batching writer uses (`SessionSink`, `Db::write` in the daemon).
use devbox_core::{CoreError, Db, Migration};
use devbox_protocol::router::BoxFuture;

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "activity",
    version: 1,
    sql: "CREATE TABLE activity_sessions(id TEXT PRIMARY KEY, app TEXT NOT NULL, title TEXT NOT NULL, project_id TEXT, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL); CREATE INDEX activity_sessions_start ON activity_sessions(start_ms);",
}];

/// A closed session after the privacy rules and project attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// `sha256(runId \0 opening seq \0 start_ms)[..32]`, or a random id without a run.
    pub id: String,
    pub app: String,
    pub title: String,
    pub project_id: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
}

/// Why a batch was not stored; the message itself goes to the log only.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("disk full")]
    DiskFull,
    #[error("database: {0}")]
    Database(String),
}

impl From<CoreError> for SinkError {
    fn from(e: CoreError) -> Self {
        match &e {
            CoreError::Db(rusqlite::Error::SqliteFailure(f, _))
                if f.code == rusqlite::ErrorCode::DiskFull =>
            {
                SinkError::DiskFull
            }
            _ => SinkError::Database(e.to_string()),
        }
    }
}

/// Stores a batch and returns the commit rev. Rows already stored (same id) are
/// left alone, so a retry after an unclear failure stores nothing twice.
pub trait SessionSink: Send + Sync + 'static {
    fn insert(&self, batch: Vec<Session>) -> BoxFuture<Result<u64, SinkError>>;
}

pub struct DbSink(pub Db);

impl SessionSink for DbSink {
    fn insert(&self, batch: Vec<Session>) -> BoxFuture<Result<u64, SinkError>> {
        let db = self.0.clone();
        Box::pin(async move {
            let ((), rev) = db
                .write(move |tx| {
                    let mut st = tx.prepare_cached(
                        "INSERT OR IGNORE INTO activity_sessions(id, app, title, project_id, start_ms, end_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )?;
                    for s in &batch {
                        st.execute(rusqlite::params![
                            s.id,
                            s.app,
                            s.title,
                            s.project_id,
                            s.start_ms,
                            s.end_ms
                        ])?;
                    }
                    Ok(())
                })
                .await?;
            Ok(rev)
        })
    }
}

/// Sessions overlapping `[from_ms, to_ms)`, oldest first.
pub async fn overlapping(db: &Db, from_ms: i64, to_ms: i64) -> Result<Vec<Session>, CoreError> {
    db.read(move |tx| {
        let mut st = tx.prepare_cached(
            "SELECT id, app, title, project_id, start_ms, end_ms FROM activity_sessions WHERE start_ms < ?2 AND end_ms > ?1 ORDER BY start_ms, id",
        )?;
        st.query_map([from_ms, to_ms], |r| {
            Ok(Session {
                id: r.get(0)?,
                app: r.get(1)?,
                title: r.get(2)?,
                project_id: r.get(3)?,
                start_ms: r.get(4)?,
                end_ms: r.get(5)?,
            })
        })?
        .collect()
    })
    .await
}
