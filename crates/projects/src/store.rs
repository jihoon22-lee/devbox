use crate::model::Project;
use rusqlite::{OptionalExtension, Transaction, params};

pub const MIGRATION_1: &str = "
CREATE TABLE projects_projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  favorite INTEGER NOT NULL DEFAULT 0,
  added_ms INTEGER NOT NULL
);";

pub fn all(tx: &Transaction) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = tx.prepare("SELECT id, name, path, favorite, added_ms FROM projects_projects ORDER BY favorite DESC, name, path, id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            favorite: r.get::<_, i64>(3)? != 0,
            added_ms: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn insert(tx: &Transaction, p: &Project) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO projects_projects(id, name, path, favorite, added_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![p.id, p.name, p.path, p.favorite as i64, p.added_ms],
    )?;
    Ok(())
}

pub fn exists_path(tx: &Transaction, path: &str) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM projects_projects WHERE path = ?1)",
        [path],
        |r| r.get(0),
    )
}

pub fn delete(tx: &Transaction, id: &str) -> rusqlite::Result<usize> {
    tx.execute("DELETE FROM projects_projects WHERE id = ?1", [id])
}

pub fn get(tx: &Transaction, id: &str) -> rusqlite::Result<Option<Project>> {
    tx.query_row(
        "SELECT id, name, path, favorite, added_ms FROM projects_projects WHERE id=?1",
        [id],
        |row| {
            Ok(Project {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                favorite: row.get::<_, i64>(3)? != 0,
                added_ms: row.get(4)?,
            })
        },
    )
    .optional()
}

pub fn set_favorite(tx: &Transaction, id: &str, favorite: bool) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE projects_projects SET favorite=?2 WHERE id=?1",
        params![id, favorite],
    )
}
