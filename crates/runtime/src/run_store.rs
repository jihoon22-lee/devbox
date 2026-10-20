use crate::{RestartPolicy, Run};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub const MIGRATION_2: &str = "
CREATE TABLE runtime_runs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects_projects(id) ON DELETE CASCADE,
    started_ms INTEGER NOT NULL,
    body TEXT NOT NULL
);
CREATE INDEX runtime_runs_project ON runtime_runs(project_id, started_ms DESC);
";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredRun {
    pub run: Run,
    pub restart: RestartPolicy,
    pub start_limit: Option<crate::StartLimit>,
    pub worktree: Option<String>,
    pub start_limit_notified: bool,
}

fn decode(text: String) -> rusqlite::Result<StoredRun> {
    serde_json::from_str(&text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
pub(crate) fn get(tx: &Transaction<'_>, id: &str) -> rusqlite::Result<Option<StoredRun>> {
    tx.query_row("SELECT body FROM runtime_runs WHERE id=?1", [id], |r| {
        decode(r.get(0)?)
    })
    .optional()
}
pub(crate) fn list(
    tx: &Transaction<'_>,
    project: Option<&str>,
    limit: u32,
) -> rusqlite::Result<Vec<StoredRun>> {
    tx.prepare("SELECT body FROM runtime_runs WHERE (?1 IS NULL OR project_id=?1) ORDER BY started_ms DESC,id LIMIT ?2")?
        .query_map(params![project, limit], |r| decode(r.get(0)?))?
        .collect()
}
pub(crate) fn put(tx: &Transaction<'_>, value: &StoredRun) -> rusqlite::Result<()> {
    let body = serde_json::to_string(value)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    tx.execute(
        "INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)
        ON CONFLICT(id) DO UPDATE SET started_ms=excluded.started_ms,body=excluded.body",
        params![
            value.run.id,
            value.run.project_id,
            value.run.started_ms,
            body
        ],
    )?;
    Ok(())
}
