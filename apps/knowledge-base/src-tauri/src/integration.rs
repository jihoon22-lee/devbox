//! devbox 공용 루트 integration snapshot producer (두 번째).
//!
//! 두 번째 producer가 같은 envelope을 쓰므로 직렬화·원자 기록·경로 계산은
//! `crates/integration`(devbox_integration) 프리미티브를 쓴다 (§10.1, PR 29).
//! 데이터는 note 작성·수정 수와 시각만 포함한다 — **본문은 넣지 않는다**.

use rusqlite::Connection;

const PRODUCER_ID: &str = "knowledge-base";

/// note 통계 snapshot을 쓴다. 실패해도 앱 동작을 막지 않는다 (호출부에서 로그).
pub fn write_snapshot(db: &Connection) -> Result<(), String> {
    let now = current_epoch_ms();
    let today_start = now.div_euclid(86_400_000) * 86_400_000;

    let modified_today: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM docs WHERE modified_ts >= ?1",
            rusqlite::params![today_start],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let last_modified_at_ms: Option<i64> = db
        .query_row("SELECT MAX(modified_ts) FROM docs", [], |r| r.get(0))
        .unwrap_or(None);

    let data = serde_json::json!({
        "notesModifiedToday": modified_today,
        "lastModifiedAtMs": last_modified_at_ms,
    });
    let envelope = devbox_integration::Envelope::new(PRODUCER_ID, env!("CARGO_PKG_VERSION"), data);
    let dir = devbox_integration::snapshot_dir(PRODUCER_ID, 1);
    devbox_integration::write_atomic(&envelope, &dir)
}

fn current_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_notes_modified_today() {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        let today_start = current_epoch_ms().div_euclid(86_400_000) * 86_400_000;
        crate::core::db::index_doc(&conn, "Notes/a.md", "# A").unwrap();
        // modified_ts는 index_doc이 now()로 넣으므로 오늘 범위에 들어간다
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM docs WHERE modified_ts >= ?1",
                rusqlite::params![today_start],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
