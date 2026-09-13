//! Runtime-owned metadata readers do not initialize a scheduler, migrate a store,
//! materialize commands/environment or publish a legacy integration snapshot.
use super::{ProductFile, ProductRoot};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::{
    path::Path,
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
pub enum Source {
    Tasks,
    Services,
    Runs,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub job_id: String,
    pub name: String,
    pub revision: serde_json::Value,
}
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}
pub fn read(data: &Path, source: Source) -> Result<Snapshot, String> {
    let root = ProductRoot::open(data)?;
    let path = root.checked()?.join("data.db");
    if !path
        .try_exists()
        .map_err(|_| "runtime_search_unavailable")?
    {
        return Ok(Snapshot {
            entries: Vec::new(),
            truncated: false,
        });
    }
    let database = ProductFile::open(&path)?;
    let mut sidecars = Vec::new();
    for suffix in ["data.db-wal", "data.db-shm"] {
        let path = data.join(suffix);
        if path
            .try_exists()
            .map_err(|_| "runtime_search_unavailable")?
        {
            sidecars.push(ProductFile::open(&path)?);
        }
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "runtime_search_unavailable")?;
    conn.busy_timeout(Duration::from_millis(50))
        .map_err(|_| "runtime_search_unavailable")?;
    let deadline = Instant::now() + Duration::from_millis(1500);
    conn.progress_handler(1000, Some(move || Instant::now() >= deadline));
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN")
        .map_err(|_| "runtime_search_unavailable")?;
    let schema: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "runtime_search_schema")?;
    if schema.parse::<i64>().ok() != Some(crate::storage::SCHEMA_VERSION) {
        return Err("runtime_search_schema".into());
    }
    let result = read_rows(&conn, source).map_err(|_| "runtime_search_unavailable")?;
    database.check()?;
    root.checked()?;
    for file in sidecars {
        file.check()?;
    }
    Ok(result)
}
fn read_rows(conn: &Connection, source: Source) -> rusqlite::Result<Snapshot> {
    let mut statement=conn.prepare(match source{
  Source::Tasks=>"SELECT id,id,name,updated_at,kind,enabled FROM jobs WHERE kind='job' AND length(CAST(name AS BLOB))<=256 ORDER BY id LIMIT 2049",
  Source::Services=>"SELECT id,id,name,updated_at,kind,enabled FROM jobs WHERE kind='service' AND length(CAST(name AS BLOB))<=256 ORDER BY id LIMIT 2049",
  Source::Runs=>"SELECT runs.id,jobs.id,jobs.name,COALESCE(runs.ended_at,runs.started_at,runs.created_at),runs.status,COALESCE(runs.exit_code,0) FROM runs JOIN jobs ON jobs.id=runs.job_id WHERE length(CAST(jobs.name AS BLOB))<=256 ORDER BY runs.created_at DESC,runs.id LIMIT 257",
 })?;
    let mut entries = statement
        .query_map([], |row| {
            Ok(Entry {
                id: row.get(0)?,
                job_id: row.get(1)?,
                name: row.get(2)?,
                revision: serde_json::json!([
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?
                ]),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let limit = if matches!(source, Source::Runs) {
        256
    } else {
        2048
    };
    let truncated = entries.len() > limit;
    entries.truncate(limit);
    Ok(Snapshot { entries, truncated })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_has_no_execution_material_and_includes_service_runs() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE jobs(id TEXT,kind TEXT,name TEXT,command TEXT,env_ciphertext BLOB,updated_at INTEGER,enabled INTEGER); CREATE TABLE runs(id TEXT,job_id TEXT,status TEXT,created_at INTEGER,started_at INTEGER,ended_at INTEGER,exit_code INTEGER); INSERT INTO jobs VALUES('job','job','same','private-command',x'010203',1,1),('service','service','same','private-service',x'040506',2,1); INSERT INTO runs VALUES('run','service','failed',1,2,3,7);").unwrap();
        let tasks = read_rows(&conn, Source::Tasks).unwrap();
        let services = read_rows(&conn, Source::Services).unwrap();
        let runs = read_rows(&conn, Source::Runs).unwrap();
        assert_ne!(tasks.entries[0].id, services.entries[0].id);
        assert_eq!(runs.entries[0].job_id, "service");
        let data = serde_json::to_string(&runs.entries).unwrap();
        assert!(!data.contains("private"));
        assert!(!data.contains("ciphertext"));
    }
}
