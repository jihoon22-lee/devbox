use crate::core::models::{AppTotal, ClosedSession, Session};
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 데이터베이스를 열고 스키마를 준비한다.
///
/// activity-timeline 병합으로 `sessions`(활동)와 `settings`(life-log 설정)가
/// 하나의 DB에 함께 들어간다.
pub fn init(path: &Path) -> rusqlite::Result<Connection> {
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
        -- activity-timeline DB 흡수 중복 방지용 (INSERT OR IGNORE 기준)
        CREATE UNIQUE INDEX IF NOT EXISTS idx_sessions_dedup
            ON sessions(app, title, start_ts, end_ts, duration_ms);
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )
}

pub fn insert_session(conn: &Connection, s: &ClosedSession) -> rusqlite::Result<()> {
    let duration_ms = s
        .end_ts
        .checked_sub(s.start_ts)
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("session duration overflow".into()))?;
    conn.execute(
        "INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![s.app, s.title, s.start_ts, s.end_ts, duration_ms.max(0),],
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
         ORDER BY start_ts, end_ts, app, title, id",
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

/// Bounded timeline query used by export and other memory-sensitive readers.
/// The caller may request one extra row to distinguish an exact limit from a
/// truncated result without loading an unbounded activity table.
pub fn get_timeline_limited(
    conn: &Connection,
    day_start: i64,
    day_end: i64,
    limit: usize,
) -> rusqlite::Result<Vec<Session>> {
    let mut stmt = conn.prepare(
        "SELECT id, app, title, start_ts, end_ts, duration_ms
         FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2
         ORDER BY start_ts, end_ts, app, title, id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(rusqlite::params![day_start, day_end, limit as i64], |r| {
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

/// Bounded timeline query with a cooperative cancellation hook. The caller
/// owns the token and can interrupt a long SQLite VM before a stale digest
/// proceeds to Git. The hook is removed on every return path so it cannot
/// affect the next query using this connection.
pub fn get_timeline_limited_with_cancel(
    conn: &Connection,
    day_start: i64,
    day_end: i64,
    limit: usize,
    cancellation: Arc<AtomicBool>,
) -> rusqlite::Result<Vec<Session>> {
    conn.progress_handler(1_000, Some(move || cancellation.load(Ordering::Acquire)));
    let result = (|| {
        let mut stmt = conn.prepare(
            "SELECT id, app, title, start_ts, end_ts, duration_ms
             FROM sessions
             WHERE start_ts >= ?1 AND start_ts < ?2
             ORDER BY start_ts, end_ts, app, title, id
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(rusqlite::params![day_start, day_end, limit as i64], |r| {
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
    })();
    conn.progress_handler(0, None::<fn() -> bool>);
    result
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

/// 기간 내 총 PC 사용 시간 (ms).
pub fn pc_usage(conn: &Connection, start: i64, end: i64) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(duration_ms), 0) FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2",
        rusqlite::params![start, end],
        |r| r.get(0),
    )
    .unwrap_or(0)
}

/// 기간 내 일별 사용량을 (하루 시작 ms, 사용량) 목록으로 반환한다.
pub fn get_daily_usage(conn: &Connection, start: i64, end: i64) -> Vec<(i64, i64)> {
    let mut stmt = match conn.prepare(
        "SELECT (start_ts / 86400000) * 86400000 AS day, SUM(duration_ms) AS total
         FROM sessions
         WHERE start_ts >= ?1 AND start_ts < ?2
         GROUP BY day ORDER BY day",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(rusqlite::params![start, end], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })
    .map(|it| it.flatten().collect())
    .unwrap_or_default()
}

pub fn get_setting(conn: &Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

/// Read one setting only when its UTF-8 byte length is within the caller's
/// bound. The SQL CASE keeps an oversized value from being materialized into
/// a Rust `String`; callers can then fail closed before parsing raw settings.
pub fn get_setting_bounded(
    conn: &Connection,
    key: &str,
    default: &str,
    max_bytes: usize,
) -> Result<String, String> {
    match conn.query_row(
        "SELECT CASE WHEN length(CAST(value AS BLOB)) <= ?2 THEN value ELSE NULL END
         FROM settings WHERE key = ?1",
        rusqlite::params![key, max_bytes as i64],
        |row| row.get::<_, Option<String>>(0),
    ) {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err("setting value exceeds byte limit".into()),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(default.to_owned()),
        Err(_) => Err("setting value cannot be read".into()),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) {
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    );
}

/// activity-timeline의 세션 DB를 한 번만 흡수한다.
///
/// - 이미 흡수했으면(settings 마커) 아무것도 하지 않는다
/// - 원본 DB는 삭제하지 않는다 (사용자가 직접 정리)
///
/// TODO(0.5.0): activity-timeline 병합에 따른 1회성 흡수. 두 릴리스 뒤 제거한다.
pub fn absorb_activity_timeline(conn: &Connection, legacy_path: &Path) -> rusqlite::Result<()> {
    if get_setting(conn, "activity_absorbed", "") == "1" {
        return Ok(());
    }
    if !legacy_path.exists() {
        return Ok(());
    }
    conn.execute(
        "ATTACH DATABASE ?1 AS legacy",
        rusqlite::params![legacy_path.to_string_lossy()],
    )?;
    conn.execute_batch(
        "INSERT OR IGNORE INTO sessions (app, title, start_ts, end_ts, duration_ms)
         SELECT app, title, start_ts, end_ts, duration_ms FROM legacy.sessions;",
    )?;
    conn.execute("DETACH DATABASE legacy", [])?;
    set_setting(conn, "activity_absorbed", "1");
    Ok(())
}

/// 이전 activity-timeline 앱의 data.db 기본 경로.
pub fn default_legacy_activity_db() -> String {
    let base = if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    format!("{base}/com.devbox.activitytimeline/data.db")
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

    #[test]
    fn bounded_setting_reader_rejects_oversized_raw_values() {
        let conn = mem();
        set_setting(&conn, "projects", "a\nb\nc");
        assert_eq!(
            get_setting_bounded(&conn, "projects", "", 3).unwrap_err(),
            "setting value exceeds byte limit"
        );
        assert_eq!(
            get_setting_bounded(&conn, "projects", "", 5).unwrap(),
            "a\nb\nc"
        );
        assert_eq!(
            get_setting_bounded(&conn, "missing", "fallback", 1).unwrap(),
            "fallback"
        );
    }

    #[test]
    fn pc_usage_sums_duration() {
        let conn = mem();
        sample(&conn);
        assert_eq!(pc_usage(&conn, 0, 5_000), 300);
        assert_eq!(pc_usage(&conn, 0, 1_150), 200);
    }

    #[test]
    fn daily_usage_groups_by_day() {
        let conn = mem();
        conn.execute_batch(
            "INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES
                ('vs', 'A', 86_400_000, 86_400_100, 100),
                ('vs', 'B', 86_400_200, 86_400_300, 100),
                ('vs', 'C', 172_800_000, 172_800_100, 100);",
        )
        .unwrap();
        let daily = get_daily_usage(&conn, 0, 86_400_000 * 3);
        assert_eq!(daily.len(), 2);
        assert_eq!(daily[0].0, 86_400_000);
        assert_eq!(daily[0].1, 200);
        assert_eq!(daily[1].0, 172_800_000);
        assert_eq!(daily[1].1, 100);
    }

    #[test]
    fn settings_roundtrip() {
        let conn = mem();
        set_setting(&conn, "projects", "a\nb");
        assert_eq!(get_setting(&conn, "projects", ""), "a\nb");
        assert_eq!(get_setting(&conn, "missing", "def"), "def");
        set_setting(&conn, "projects", "c");
        assert_eq!(get_setting(&conn, "projects", ""), "c");
    }

    #[test]
    fn absorb_skips_when_marker_set() {
        let conn = mem();
        set_setting(&conn, "activity_absorbed", "1");
        let legacy = std::env::temp_dir().join("ll-absorb-none.db");
        absorb_activity_timeline(&conn, &legacy).unwrap();
        assert!(get_timeline(&conn, 0, 1_000).unwrap().is_empty());
    }

    #[test]
    fn absorb_ignores_missing_legacy() {
        let conn = mem();
        let legacy = std::env::temp_dir().join("ll-absorb-missing-xxxx.db");
        absorb_activity_timeline(&conn, &legacy).unwrap();
        assert!(get_timeline(&conn, 0, 1_000).unwrap().is_empty());
    }

    #[test]
    fn absorb_inserts_and_sets_marker() {
        let dir = std::env::temp_dir().join(format!("ll-absorb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("data.db");
        {
            let legacy_conn = Connection::open(&legacy).unwrap();
            legacy_conn
                .execute_batch(
                    "CREATE TABLE sessions (
                        id INTEGER PRIMARY KEY, app TEXT, title TEXT,
                        start_ts INTEGER, end_ts INTEGER, duration_ms INTEGER
                    );
                    INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES
                        ('vs', 'A', 100, 200, 100),
                        ('chrome', 'B', 200, 300, 100);",
                )
                .unwrap();
        }

        let conn = mem();
        absorb_activity_timeline(&conn, &legacy).unwrap();
        let rows = get_timeline(&conn, 0, 1_000).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(get_setting(&conn, "activity_absorbed", ""), "1");

        // 두 번째 실행은 마커로 건너뛴다 (중복 없음)
        absorb_activity_timeline(&conn, &legacy).unwrap();
        assert_eq!(get_timeline(&conn, 0, 1_000).unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absorb_dedups_same_rows() {
        let dir = std::env::temp_dir().join(format!("ll-absorb-dup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("data.db");
        {
            let legacy_conn = Connection::open(&legacy).unwrap();
            legacy_conn
                .execute_batch(
                    "CREATE TABLE sessions (
                        id INTEGER PRIMARY KEY, app TEXT, title TEXT,
                        start_ts INTEGER, end_ts INTEGER, duration_ms INTEGER
                    );
                    INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES
                        ('vs', 'A', 100, 200, 100),
                        ('vs', 'A', 100, 200, 100);",
                )
                .unwrap();
        }

        let conn = mem();
        absorb_activity_timeline(&conn, &legacy).unwrap();
        assert_eq!(get_timeline(&conn, 0, 1_000).unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
