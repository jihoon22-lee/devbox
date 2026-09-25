//! Retain existing job reconnection gates without discovering or importing sources.
use super::{DatabaseState, StorageError};
use rusqlite::Connection;
fn present(connection: &Connection) -> rusqlite::Result<bool> {
    connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='workspace_runtime_import_jobs')",[],|row|row.get(0))
}
pub(super) fn resolve(connection: &Connection, id: &str) -> rusqlite::Result<()> {
    if present(connection)? {
        connection.execute(
            "UPDATE workspace_runtime_import_jobs SET reconnect_required=0 WHERE job_id=?",
            [id],
        )?;
    }
    Ok(())
}
impl DatabaseState {
    pub fn requires_secret_review(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.lock()?;
        if !present(&connection)? {
            return Ok(false);
        }
        Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM workspace_runtime_import_jobs WHERE job_id=? AND reconnect_required=1)",[id],|row|row.get(0))?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_reconnection_records_remain_until_the_matching_job_is_reviewed() {
        let connection = Connection::open_in_memory().unwrap();
        resolve(&connection, "absent").unwrap();
        assert!(!present(&connection).unwrap());
        connection.execute_batch("CREATE TABLE workspace_runtime_import_jobs(job_id TEXT,reconnect_required INTEGER); INSERT INTO workspace_runtime_import_jobs VALUES('a',1),('b',1);").unwrap();
        resolve(&connection, "a").unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM workspace_runtime_import_jobs WHERE reconnect_required=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            connection
                .query_row(
                    "SELECT reconnect_required FROM workspace_runtime_import_jobs WHERE job_id='b'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            1
        );
    }
}
