use crate::RunDef;
use rusqlite::{Transaction, params};

pub const MIGRATION_1: &str = "
CREATE TABLE runtime_definitions (
    project_id TEXT NOT NULL REFERENCES projects_projects(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY(project_id, id)
);";

pub(crate) fn definitions(tx: &Transaction<'_>, project: &str) -> rusqlite::Result<Vec<RunDef>> {
    let mut query =
        tx.prepare("SELECT body FROM runtime_definitions WHERE project_id=?1 ORDER BY id")?;
    query
        .query_map([project], |row| {
            let body: String = row.get(0)?;
            serde_json::from_str(&body).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })?
        .collect()
}

pub(crate) fn define(tx: &Transaction<'_>, project: &str, def: &RunDef) -> rusqlite::Result<()> {
    let body = serde_json::to_string(def)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    tx.execute(
        "INSERT INTO runtime_definitions(project_id,id,body) VALUES(?1,?2,?3) ON CONFLICT(project_id,id) DO UPDATE SET body=excluded.body",
        params![project, def.id(), body],
    )?;
    Ok(())
}

pub(crate) fn undefine(tx: &Transaction<'_>, project: &str, id: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM runtime_definitions WHERE project_id=?1 AND id=?2",
        params![project, id],
    )?;
    Ok(())
}
