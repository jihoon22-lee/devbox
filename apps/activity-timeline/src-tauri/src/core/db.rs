use crate::core::models::{AppTotal, ClosedSession, Session};
use rusqlite::Connection;

/// 데이터베이스를 열고 스키마를 준비한다.
pub fn init(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY,
            app TEXT NOT NULL,
            title TEXT NOT NULL,
            start_ts INTEGER NOT NULL,
            end_ts INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_start ON sessions(start_ts);
        ",
    )
}

pub fn insert_session(conn: &Connection, s: &ClosedSession) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            s.app,
            s.title,
            s.start_ts,
            s.end_ts,
            (s.end_ts - s.start_ts).max(0),
        ],
    )?;
    Ok(())
}

/// 하루(시작~끝 epoch ms)의 세션을 시작 시각 순으로 반환한다.
pub fn get_timeline(
    conn: &Connection,
    day_start: i64,
    day_end: i64,
) -> rusqlite::Result<Vec<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, app, title, start_ts, end_ts, duration_ms
         FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2
         ORDER BY start_ts",
    )?;
    let rows = stmt.query_map(rusqlite::params![day_start, day_end], |r| {
        Ok(Session {
            id: r.get(0)?,
            app: r.get(1)?,
            title: r.get(2)?,
            start_ts: r.get(3)?,
            end_ts: r.get(4)?,
            duration_ms: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// 기간 내 앱별 사용 합계를 반환한다.
pub fn get_app_stats(conn: &Connection, start: i64, end: i64) -> rusqlite::Result<Vec<AppTotal>> {
    let mut stmt = conn.prepare(
        "SELECT app, SUM(duration_ms) AS total, COUNT(*) AS cnt
         FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2
         GROUP BY app
         ORDER BY total DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![start, end], |r| {
        Ok(AppTotal {
            app: r.get(0)?,
            duration_ms: r.get(1)?,
            sessions: r.get(2)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn sample(conn: &Connection) {
        insert_session(
            conn,
            &ClosedSession {
                app: "vs".into(),
                title: "FamilyCard".into(),
                start_ts: 1_000,
                end_ts: 1_100,
            },
        )
        .unwrap();
        insert_session(
            conn,
            &ClosedSession {
                app: "chrome".into(),
                title: "GitHub".into(),
                start_ts: 1_100,
                end_ts: 1_200,
            },
        )
        .unwrap();
        insert_session(
            conn,
            &ClosedSession {
                app: "vs".into(),
                title: "Other".into(),
                start_ts: 2_000,
                end_ts: 2_100,
            },
        )
        .unwrap();
    }

    #[test]
    fn inserts_and_reads_timeline_in_order() {
        let conn = mem();
        sample(&conn);
        let rows = get_timeline(&conn, 0, 1_500).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].app, "vs");
        assert_eq!(rows[0].duration_ms, 100);
        assert_eq!(rows[1].app, "chrome");
    }

    #[test]
    fn timeline_respects_day_bounds() {
        let conn = mem();
        sample(&conn);
        let rows = get_timeline(&conn, 1_100, 1_900).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].app, "chrome");
    }

    #[test]
    fn app_stats_aggregates() {
        let conn = mem();
        sample(&conn);
        let stats = get_app_stats(&conn, 0, 5_000).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].app, "vs");
        assert_eq!(stats[0].duration_ms, 200);
        assert_eq!(stats[0].sessions, 2);
        assert_eq!(stats[1].app, "chrome");
    }
}
