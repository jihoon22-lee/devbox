//! `index.db`의 `api` 도메인. 기록 파일로 언제든 다시 만들 수 있다.
use crate::{HistoryEntry, HistoryOutcome, HistorySummary, RequestOrigin};
use rusqlite::{OptionalExtension, Transaction, params};

pub(crate) const SQL: &str = "CREATE TABLE api_history(id TEXT PRIMARY KEY, sent_ms INTEGER NOT NULL, day TEXT NOT NULL, method TEXT NOT NULL, url TEXT NOT NULL, status INTEGER, elapsed_ms INTEGER NOT NULL, size INTEGER NOT NULL, outcome TEXT NOT NULL, run_id TEXT, origin TEXT); CREATE INDEX api_history_sent ON api_history(sent_ms, id);";

/// 목록 행의 URL은 2KiB까지만 둔다(전체는 기록 파일).
const SUMMARY_URL: usize = 2048;

fn clip(text: &str) -> String {
    let mut end = text.len().min(SUMMARY_URL);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub(crate) fn origin_key(origin: &Option<RequestOrigin>) -> Option<String> {
    origin.as_ref().and_then(|o| serde_json::to_string(o).ok())
}

pub(crate) fn insert(tx: &Transaction<'_>, e: &HistoryEntry, day: &str) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO api_history(id,sent_ms,day,method,url,status,elapsed_ms,size,outcome,run_id,origin) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            e.id,
            e.sent_at_ms,
            day,
            e.method,
            clip(&e.url),
            e.response.as_ref().map(|r| r.status),
            e.response.as_ref().map(|r| r.elapsed_ms).unwrap_or(0),
            e.response.as_ref().map(|r| r.size_bytes).unwrap_or(0),
            e.outcome.as_str(),
            e.run_id,
            origin_key(&e.origin),
        ],
    )?;
    Ok(())
}

fn outcome(text: &str) -> HistoryOutcome {
    match text {
        "ok" => HistoryOutcome::Ok,
        "cancelled" => HistoryOutcome::Cancelled,
        _ => HistoryOutcome::Failed,
    }
}

/// `before` 커서(`<sentMs>:<id>`)보다 앞선 행을 최근 순으로 `limit + 1`개까지.
pub(crate) fn page(
    tx: &Transaction<'_>,
    before: Option<(i64, String)>,
    origin: Option<String>,
    limit: u32,
) -> rusqlite::Result<Vec<HistorySummary>> {
    let (sent, id) = before.unzip();
    let mut statement = tx.prepare(
        "SELECT id,sent_ms,method,url,status,elapsed_ms,size,outcome,run_id,origin FROM api_history WHERE (?1 IS NULL OR sent_ms < ?1 OR (sent_ms = ?1 AND id < ?2)) AND (?3 IS NULL OR origin = ?3) ORDER BY sent_ms DESC, id DESC LIMIT ?4",
    )?;
    let rows = statement.query_map(params![sent, id, origin, limit + 1], |row| {
        let origin: Option<String> = row.get(9)?;
        Ok(HistorySummary {
            id: row.get(0)?,
            sent_at_ms: row.get(1)?,
            method: row.get(2)?,
            url: row.get(3)?,
            status: row.get(4)?,
            elapsed_ms: row.get(5)?,
            size_bytes: row.get(6)?,
            outcome: outcome(&row.get::<_, String>(7)?),
            run_id: row.get(8)?,
            origin: origin.and_then(|o| serde_json::from_str(&o).ok()),
        })
    })?;
    rows.collect()
}

pub(crate) fn day_of(tx: &Transaction<'_>, id: &str) -> rusqlite::Result<Option<String>> {
    tx.query_row("SELECT day FROM api_history WHERE id=?1", [id], |r| {
        r.get(0)
    })
    .optional()
}

pub(crate) fn prune(tx: &Transaction<'_>, cutoff_ms: i64) -> rusqlite::Result<usize> {
    tx.execute("DELETE FROM api_history WHERE sent_ms < ?1", [cutoff_ms])
}
