use crate::core::models::AppTotal;
use rusqlite::Connection;

/// ActivityTimeline의 data.db를 읽어 (총 사용시간, 앱별 합계)를 반환한다.
/// DB가 없거나 열 수 없으면 0 값을 반환한다 (읽기 전용, 원본 무수정).
pub fn read_activity(db_path: &str, day_start: i64, day_end: i64) -> (i64, Vec<AppTotal>) {
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => return (0, Vec::new()),
    };

    let pc_usage: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_ms), 0) FROM sessions
             WHERE start_ts >= ?1 AND start_ts < ?2",
            rusqlite::params![day_start, day_end],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut stmt = match conn.prepare(
        "SELECT app, SUM(duration_ms) AS total FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2
         GROUP BY app ORDER BY total DESC",
    ) {
        Ok(s) => s,
        Err(_) => return (pc_usage, Vec::new()),
    };
    let rows = stmt
        .query_map(rusqlite::params![day_start, day_end], |r| {
            Ok(AppTotal {
                app: r.get(0)?,
                duration_ms: r.get(1)?,
            })
        })
        .map(|it| it.flatten().collect())
        .unwrap_or_default();

    (pc_usage, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_missing_db_as_zero() {
        let (usage, totals) = read_activity("/nonexistent/x.db", 0, 1000);
        assert_eq!(usage, 0);
        assert!(totals.is_empty());
    }

    #[test]
    fn reads_existing_db() {
        let dir = std::env::temp_dir().join(format!("ll-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                id INTEGER PRIMARY KEY, app TEXT, title TEXT,
                start_ts INTEGER, end_ts INTEGER, duration_ms INTEGER
            );
            INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES
                ('vs', 'A', 100, 200, 100),
                ('chrome', 'B', 200, 300, 100),
                ('vs', 'C', 500, 700, 200);",
        )
        .unwrap();
        drop(conn);

        let (usage, totals) = read_activity(&path.to_string_lossy(), 0, 1000);
        assert_eq!(usage, 400);
        assert_eq!(totals[0].app, "vs");
        assert_eq!(totals[0].duration_ms, 300);

        let _ = fs::remove_dir_all(&dir);
    }
}
