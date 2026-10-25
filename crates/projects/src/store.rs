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

pub const MIGRATION_2: &str = "
CREATE TABLE projects_command_trust (
  project_id TEXT PRIMARY KEY REFERENCES projects_projects(id) ON DELETE CASCADE,
  canonical_path TEXT NOT NULL,
  digest TEXT NOT NULL,
  scope_version INTEGER NOT NULL,
  approved_ms INTEGER NOT NULL
);";

pub(crate) struct ApprovalIdentity {
    path: String,
    digest: String,
    scope: u32,
    at: i64,
}
impl ApprovalIdentity {
    pub(crate) fn stamp(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for text in [&self.path, &self.digest] {
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
        hash.update(self.scope.to_le_bytes());
        hash.update(self.at.to_le_bytes());
        format!("{:x}", hash.finalize())
    }
}
pub(crate) fn approval_identity(
    tx: &Transaction,
    id: &str,
) -> rusqlite::Result<Option<ApprovalIdentity>> {
    tx.query_row("SELECT canonical_path,digest,scope_version,approved_ms FROM projects_command_trust WHERE project_id=?1",[id],|r|Ok(ApprovalIdentity{path:r.get(0)?,digest:r.get(1)?,scope:r.get(2)?,at:r.get(3)?})).optional()
}

pub(crate) fn approved(
    tx: &Transaction,
    id: &str,
    path: &str,
    digest: &str,
    scope: u32,
) -> rusqlite::Result<bool> {
    tx.query_row("SELECT EXISTS(SELECT 1 FROM projects_command_trust WHERE project_id=?1 AND canonical_path=?2 AND digest=?3 AND scope_version>=?4)", params![id,path,digest,scope], |row| row.get(0))
}

pub(crate) fn approve(
    tx: &Transaction,
    id: &str,
    path: &str,
    digest: &str,
    scope: u32,
    at: i64,
) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO projects_command_trust(project_id,canonical_path,digest,scope_version,approved_ms) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id) DO UPDATE SET canonical_path=excluded.canonical_path,digest=excluded.digest,scope_version=excluded.scope_version,approved_ms=excluded.approved_ms", params![id,path,digest,scope,at])?;
    Ok(())
}

pub fn all(tx: &Transaction) -> rusqlite::Result<Vec<Project>> {
    let mut stmt = tx.prepare("SELECT id, name, path, favorite, added_ms FROM projects_projects ORDER BY favorite DESC, name, path, id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            favorite: r.get::<_, i64>(3)? != 0,
            added_ms: r.get(4)?,
            git_warnings: None,
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

/// Optional domains share this database, but projects does not depend on their crates.
/// Check reservations in the same writer transaction as deletion. Creation must
/// check the project in its own reservation transaction to fence concurrent removal.
pub fn has_dependents(tx: &Transaction, id: &str) -> rusqlite::Result<bool> {
    let has_table = |name: &str| -> rusqlite::Result<bool> {
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |row| row.get(0),
        )
    };
    if has_table("terminal_sessions")?
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM terminal_sessions WHERE project_id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(true);
    }
    if has_table("runtime_definitions")?
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime_definitions WHERE project_id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(true);
    }
    if has_table("runtime_runs")?
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime_runs WHERE project_id=?1 AND json_extract(body,'$.run.state') IN ('starting','running','restarting'))",
            [id],
            |row| row.get::<_, bool>(0),
        )?
    {
        return Ok(true);
    }
    if has_table("runtime_windows_readers")?
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runtime_windows_readers WHERE project_id=?1 AND state!='closed')",
            [id], |row| row.get::<_, bool>(0),
        )?
    { return Ok(true); }
    if !has_table("agents_tasks")? {
        return Ok(false);
    }
    // Closed inline history has no owned Git resources. Planned means the
    // resource reservation was released after both worktree and branch cleanup.
    let resources = if has_table("agents_runtime")? {
        "NOT EXISTS(SELECT 1 FROM agents_runtime r WHERE r.id=t.id AND json_extract(r.body,'$.resources')='Planned')"
    } else {
        "1"
    };
    tx.query_row(&format!(
        "SELECT EXISTS(SELECT 1 FROM agents_tasks t WHERE project_id=?1 AND (state!='closed' OR (json_extract(body,'$.worktreePath') IS NOT NULL AND ({resources}))))"
    ), [id], |row| row.get(0))
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
                git_warnings: None,
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
