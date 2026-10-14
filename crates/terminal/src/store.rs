use crate::TerminalSession;
use rusqlite::{OptionalExtension, Transaction, params};
pub const MIGRATION_1: &str = "
CREATE TABLE terminal_sessions (
 id TEXT PRIMARY KEY,
 project_id TEXT,
 title TEXT NOT NULL,
 cwd TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('shell', 'agent')),
 command TEXT NOT NULL,
 created_ms INTEGER NOT NULL
);";
fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalSession> {
    Ok(TerminalSession {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        cwd: row.get(3)?,
        kind: row.get(4)?,
        command: row.get(5)?,
        created_ms: row.get(6)?,
        alive: false,
    })
}
pub fn all(tx: &Transaction, project_id: Option<&str>) -> rusqlite::Result<Vec<TerminalSession>> {
    let mut statement = tx.prepare("SELECT id, project_id, title, cwd, kind, substr(command,1,257), created_ms FROM terminal_sessions WHERE (?1 IS NULL OR project_id=?1) ORDER BY created_ms, id")?;
    statement.query_map([project_id], row)?.collect()
}
pub fn page(
    tx: &Transaction,
    project_id: Option<&str>,
    created_ms: i64,
    id: &str,
) -> rusqlite::Result<Vec<TerminalSession>> {
    let mut statement = tx.prepare("SELECT id, project_id, title, cwd, kind, substr(command,1,257), created_ms FROM terminal_sessions WHERE (?1 IS NULL OR project_id=?1) AND (created_ms > ?2 OR (created_ms=?2 AND id>?3)) ORDER BY created_ms, id LIMIT 257")?;
    statement
        .query_map(params![project_id, created_ms, id], row)?
        .collect()
}
pub fn get(tx: &Transaction, id: &str) -> rusqlite::Result<Option<TerminalSession>> {
    tx.query_row("SELECT id, project_id, title, cwd, kind, command, created_ms FROM terminal_sessions WHERE id=?1", [id], row).optional()
}
pub fn insert(tx: &Transaction, session: &TerminalSession) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO terminal_sessions(id, project_id, title, cwd, kind, command, created_ms) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![session.id, session.project_id, session.title, session.cwd, session.kind, session.command, session.created_ms])?;
    Ok(())
}
pub fn rename(tx: &Transaction, id: &str, title: &str) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE terminal_sessions SET title=?2 WHERE id=?1",
        params![id, title],
    )
}
pub fn delete(tx: &Transaction, id: &str) -> rusqlite::Result<usize> {
    tx.execute("DELETE FROM terminal_sessions WHERE id=?1", [id])
}
