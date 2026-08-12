use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Owns the process-wide SQLite connection. Schema migrations are added in the
/// next feature PR; this scaffold establishes the path and connection policy.
pub struct DatabaseState {
    connection: Mutex<Connection>,
}

impl DatabaseState {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        configure(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn is_ready(&self) -> bool {
        self.connection.lock().ok().and_then(|connection| {
            connection
                .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
                .ok()
        }) == Some(1)
    }
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_enables_required_pragmas() {
        let connection = Connection::open_in_memory().unwrap();
        configure(&connection).unwrap();
        let foreign_keys: i64 = connection
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        let busy_timeout: i64 = connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 5_000);
    }
}
