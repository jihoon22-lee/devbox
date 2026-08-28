//! Persistent, privacy-safe history for Life Log -> Knowledge drafts.
//!
//! Only the validated aggregate summary and source provenance are retained.
//! The rendered body, activity rows, project paths, credentials, claim token,
//! and vault path never enter this table. The handoff store's metadata-only
//! sidecar remains authoritative for cross-process status reconciliation.

use crate::core::handoff::{
    validate_source, validate_summary, KnowledgeDraftSource, KnowledgeDraftSummary,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_HISTORY_ENTRIES: usize = 100;
const KIND: &str = "knowledge-draft/v1";
const MAX_KIND_BYTES: i64 = 64;
const MAX_STATUS_BYTES: i64 = 16;
const MAX_SUMMARY_JSON_BYTES: usize = 4 * 1024;
const MAX_SOURCES_JSON_BYTES: usize = 16 * 1024;
const ENTRY_PROJECTION: &str = "length(CAST(handoff_id AS BLOB)),
     length(CAST(kind AS BLOB)),
     length(CAST(status AS BLOB)),
     length(CAST(summary_json AS BLOB)),
     length(CAST(sources_json AS BLOB)),
     length(CAST(COALESCE(regenerated_from, '') AS BLOB)),
     handoff_id, kind, status, summary_json, sources_json,
     created_ts, updated_ts, expires_ts, regenerated_from";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DraftStatus {
    Pending,
    Sent,
    Consumed,
    Expired,
}

impl DraftStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DraftHistoryEntry {
    pub handoff_id: String,
    pub kind: String,
    pub status: DraftStatus,
    pub summary: KnowledgeDraftSummary,
    pub sources: Vec<KnowledgeDraftSource>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: u64,
    pub regenerated_from: Option<String>,
}

#[cfg(any(target_os = "windows", test))]
pub struct DraftHistoryInsert<'a> {
    pub handoff_id: &'a str,
    pub summary: &'a KnowledgeDraftSummary,
    pub sources: &'a [KnowledgeDraftSource],
    pub status: DraftStatus,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub regenerated_from: Option<&'a str>,
}

#[cfg(any(target_os = "windows", test))]
pub fn insert(conn: &Connection, input: DraftHistoryInsert<'_>) -> Result<(), String> {
    let DraftHistoryInsert {
        handoff_id,
        summary,
        sources,
        status,
        created_at_ms,
        expires_at_ms,
        regenerated_from,
    } = input;
    validate_entry(&DraftHistoryEntry {
        handoff_id: handoff_id.to_owned(),
        kind: KIND.to_owned(),
        status,
        summary: summary.clone(),
        sources: sources.to_vec(),
        created_at_ms,
        updated_at_ms: created_at_ms,
        expires_at_ms,
        regenerated_from: regenerated_from.map(str::to_owned),
    })?;
    let created_ts = i64::try_from(created_at_ms)
        .map_err(|_| "draft 이력 시간이 올바르지 않습니다".to_string())?;
    let expires_ts = i64::try_from(expires_at_ms)
        .map_err(|_| "draft 이력 시간이 올바르지 않습니다".to_string())?;
    let summary_json =
        serde_json::to_string(summary).map_err(|_| "draft 이력을 저장할 수 없습니다")?;
    let sources_json =
        serde_json::to_string(sources).map_err(|_| "draft 이력을 저장할 수 없습니다")?;
    if summary_json.len() > MAX_SUMMARY_JSON_BYTES || sources_json.len() > MAX_SOURCES_JSON_BYTES {
        return Err("draft 이력이 크기 제한을 초과했습니다".into());
    }

    // Serialize the duplicate check and insert under one write transaction.
    // A retry after a crash/launch failure must not create a second history
    // row for the same immutable envelope. Treat an exact duplicate as
    // idempotent, but reject a caller attempting to reuse the ID for another
    // summary or lifecycle state.
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|_| "draft 이력을 저장할 수 없습니다".to_string())?;
    let existing_sql = format!(
        "SELECT {ENTRY_PROJECTION}
         FROM knowledge_draft_history WHERE handoff_id = ?1"
    );
    if let Some(existing) = transaction
        .query_row(&existing_sql, params![handoff_id], row_to_entry)
        .optional()
        .map_err(|_| "draft 이력을 읽을 수 없습니다".to_string())?
    {
        let expected = DraftHistoryEntry {
            handoff_id: handoff_id.to_owned(),
            kind: KIND.to_owned(),
            status,
            summary: summary.clone(),
            sources: sources.to_vec(),
            created_at_ms,
            updated_at_ms: created_at_ms,
            expires_at_ms,
            regenerated_from: regenerated_from.map(str::to_owned),
        };
        if existing == expected {
            return transaction
                .commit()
                .map_err(|_| "draft 이력을 저장할 수 없습니다".to_string());
        }
        return Err("동일한 handoff ID의 draft 이력이 이미 있습니다".into());
    }

    transaction
        .execute(
            "INSERT INTO knowledge_draft_history
             (handoff_id, kind, status, summary_json, sources_json, created_ts, updated_ts, expires_ts, regenerated_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8)",
            params![
                handoff_id,
                KIND,
                status.as_str(),
                summary_json,
                sources_json,
                created_ts,
                expires_ts,
                regenerated_from,
            ],
        )
        .map_err(|_| "draft 이력을 저장할 수 없습니다".to_string())?;
    transaction
        .execute(
            "DELETE FROM knowledge_draft_history
             WHERE handoff_id IN (
               SELECT handoff_id FROM knowledge_draft_history
               ORDER BY updated_ts DESC, id DESC LIMIT -1 OFFSET ?1
             )",
            params![MAX_HISTORY_ENTRIES as i64],
        )
        .map_err(|_| "draft 이력을 정리할 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "draft 이력을 저장할 수 없습니다".to_string())
}

#[cfg(target_os = "windows")]
pub(crate) fn validate_regenerated_from(value: Option<&str>) -> Result<(), String> {
    if value.is_some_and(|id| !valid_handoff_id(id)) {
        return Err("draft 재생성 원본 ID가 올바르지 않습니다".into());
    }
    Ok(())
}

pub fn set_status(
    conn: &Connection,
    handoff_id: &str,
    status: DraftStatus,
    updated_at_ms: u64,
) -> Result<(), String> {
    if !valid_handoff_id(handoff_id) || updated_at_ms == 0 {
        return Err("draft 이력 상태가 올바르지 않습니다".into());
    }
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|_| "draft 이력 상태를 갱신할 수 없습니다".to_string())?;
    let (current, current_updated_at_ms) = transaction
        .query_row(
            "SELECT status, updated_ts FROM knowledge_draft_history WHERE handoff_id = ?1",
            params![handoff_id],
            |row| {
                let status: String = row.get(0)?;
                let updated_at_ms = u64::try_from(row.get::<_, i64>(1)?)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok((status, updated_at_ms))
            },
        )
        .optional()
        .map_err(|_| "draft 이력 상태를 갱신할 수 없습니다".to_string())?
        .ok_or_else(|| "draft 이력을 찾을 수 없습니다".to_string())?;
    let current = status_from_str(&current)
        .ok_or_else(|| "draft 이력 상태가 올바르지 않습니다".to_string())?;
    if updated_at_ms < current_updated_at_ms || !valid_status_transition(current, status) {
        return Err("draft 이력 상태 전이가 올바르지 않습니다".into());
    }
    let updated_ts = i64::try_from(updated_at_ms)
        .map_err(|_| "draft 이력 시간이 올바르지 않습니다".to_string())?;
    let current_updated_ts = i64::try_from(current_updated_at_ms)
        .map_err(|_| "draft 이력 시간이 올바르지 않습니다".to_string())?;
    let changed = transaction
        .execute(
            "UPDATE knowledge_draft_history
             SET status = ?1, updated_ts = ?2
             WHERE handoff_id = ?3 AND status = ?4 AND updated_ts = ?5",
            params![
                status.as_str(),
                updated_ts,
                handoff_id,
                current.as_str(),
                current_updated_ts,
            ],
        )
        .map_err(|_| "draft 이력 상태를 갱신할 수 없습니다".to_string())?;
    if changed != 1 {
        return Err("draft 이력 상태가 다른 작업에 의해 변경되었습니다".into());
    }
    transaction
        .commit()
        .map_err(|_| "draft 이력 상태를 갱신할 수 없습니다".to_string())
}

/// Remove a producer-side history row while compensating a handoff that never
/// reached the consumer. Deletion is deliberately explicit and transactional
/// so a failed retry cannot leave a row without its corresponding envelope.
#[cfg(target_os = "windows")]
pub fn remove(conn: &Connection, handoff_id: &str) -> Result<bool, String> {
    if !valid_handoff_id(handoff_id) {
        return Err("draft 이력 ID가 올바르지 않습니다".into());
    }
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|_| "draft 이력을 정리할 수 없습니다".to_string())?;
    let changed = transaction
        .execute(
            "DELETE FROM knowledge_draft_history WHERE handoff_id = ?1",
            params![handoff_id],
        )
        .map_err(|_| "draft 이력을 정리할 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "draft 이력을 정리할 수 없습니다".to_string())?;
    Ok(changed == 1)
}

#[cfg(test)]
pub fn get(conn: &Connection, handoff_id: &str) -> Result<Option<DraftHistoryEntry>, String> {
    if !valid_handoff_id(handoff_id) {
        return Err("draft 이력 ID가 올바르지 않습니다".into());
    }
    let sql = format!(
        "SELECT {ENTRY_PROJECTION}
         FROM knowledge_draft_history WHERE handoff_id = ?1"
    );
    let entry = conn
        .query_row(&sql, params![handoff_id], row_to_entry)
        .optional()
        .map_err(|_| "draft 이력을 읽을 수 없습니다".to_string())?;
    entry
        .map(|entry| {
            validate_entry(&entry)?;
            Ok(entry)
        })
        .transpose()
}

pub fn list(conn: &Connection) -> Result<Vec<DraftHistoryEntry>, String> {
    let sql = format!(
        "SELECT {ENTRY_PROJECTION}
         FROM knowledge_draft_history ORDER BY updated_ts DESC, id DESC LIMIT ?1"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|_| "draft 이력을 읽을 수 없습니다".to_string())?;
    let rows = statement
        .query_map(params![MAX_HISTORY_ENTRIES as i64 + 1], row_to_entry)
        .map_err(|_| "draft 이력을 읽을 수 없습니다".to_string())?;
    let entries = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "draft 이력을 읽을 수 없습니다".to_string())?;
    if entries.len() > MAX_HISTORY_ENTRIES {
        return Err("draft 이력 저장소가 개수 제한을 초과했습니다".into());
    }
    entries
        .into_iter()
        .map(|entry| {
            validate_entry(&entry)?;
            Ok(entry)
        })
        .collect()
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftHistoryEntry> {
    let handoff_id_bytes = row.get::<_, i64>(0)?;
    let kind_bytes = row.get::<_, i64>(1)?;
    let status_bytes = row.get::<_, i64>(2)?;
    let summary_bytes = row.get::<_, i64>(3)?;
    let sources_bytes = row.get::<_, i64>(4)?;
    let regenerated_bytes = row.get::<_, i64>(5)?;
    if handoff_id_bytes != 32
        || !(0..=MAX_KIND_BYTES).contains(&kind_bytes)
        || !(0..=MAX_STATUS_BYTES).contains(&status_bytes)
        || !(0..=MAX_SUMMARY_JSON_BYTES as i64).contains(&summary_bytes)
        || !(0..=MAX_SOURCES_JSON_BYTES as i64).contains(&sources_bytes)
        || !(0..=32).contains(&regenerated_bytes)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let status: String = row.get(8)?;
    let status = status_from_str(&status).ok_or(rusqlite::Error::InvalidQuery)?;
    let summary = serde_json::from_str(&row.get::<_, String>(9)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    let sources = serde_json::from_str(&row.get::<_, String>(10)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(DraftHistoryEntry {
        handoff_id: row.get(6)?,
        kind: row.get(7)?,
        status,
        summary,
        sources,
        created_at_ms: u64::try_from(row.get::<_, i64>(11)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        updated_at_ms: u64::try_from(row.get::<_, i64>(12)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        expires_at_ms: u64::try_from(row.get::<_, i64>(13)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        regenerated_from: row.get(14)?,
    })
}

fn validate_entry(entry: &DraftHistoryEntry) -> Result<(), String> {
    if entry.kind != KIND
        || !valid_handoff_id(&entry.handoff_id)
        || i64::try_from(entry.created_at_ms).is_err()
        || i64::try_from(entry.updated_at_ms).is_err()
        || i64::try_from(entry.expires_at_ms).is_err()
        || entry.created_at_ms == 0
        || entry.updated_at_ms < entry.created_at_ms
        || entry.expires_at_ms <= entry.created_at_ms
        || entry.expires_at_ms - entry.created_at_ms > devbox_applink::DEFAULT_HANDOFF_TTL_MS
        || (entry.status == DraftStatus::Expired && entry.updated_at_ms < entry.expires_at_ms)
        || (entry.status != DraftStatus::Expired && entry.updated_at_ms >= entry.expires_at_ms)
        || entry
            .regenerated_from
            .as_deref()
            .is_some_and(|id| !valid_handoff_id(id) || id == entry.handoff_id.as_str())
        || entry.sources.len() != 4
    {
        return Err("draft 이력 형식이 올바르지 않습니다".into());
    }
    validate_summary(&entry.summary).map_err(|_| "draft 이력 요약이 올바르지 않습니다")?;
    let expected = ["life-log", "git", "run-manager", "knowledge-base"];
    for (source, id) in entry.sources.iter().zip(expected) {
        validate_source(source, id).map_err(|_| "draft 이력 source가 올바르지 않습니다")?;
    }
    Ok(())
}

fn status_from_str(value: &str) -> Option<DraftStatus> {
    match value {
        "pending" => Some(DraftStatus::Pending),
        "sent" => Some(DraftStatus::Sent),
        "consumed" => Some(DraftStatus::Consumed),
        "expired" => Some(DraftStatus::Expired),
        _ => None,
    }
}

fn valid_status_transition(from: DraftStatus, to: DraftStatus) -> bool {
    matches!(
        (from, to),
        (DraftStatus::Pending, DraftStatus::Pending)
            | (DraftStatus::Pending, DraftStatus::Sent)
            | (DraftStatus::Pending, DraftStatus::Consumed)
            | (DraftStatus::Pending, DraftStatus::Expired)
            | (DraftStatus::Sent, DraftStatus::Sent)
            | (DraftStatus::Sent, DraftStatus::Pending)
            | (DraftStatus::Sent, DraftStatus::Consumed)
            | (DraftStatus::Sent, DraftStatus::Expired)
            | (DraftStatus::Consumed, DraftStatus::Consumed)
            | (DraftStatus::Expired, DraftStatus::Expired)
    )
}

fn valid_handoff_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn connection() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&connection).unwrap();
        connection
    }

    fn summary() -> KnowledgeDraftSummary {
        KnowledgeDraftSummary {
            period: "day".into(),
            start_date: "2026-08-28".into(),
            end_date: "2026-08-28".into(),
            timezone: "Asia/Seoul".into(),
            filter: None,
            pc_usage_ms: 60_000,
            session_count: 1,
            active_days: 1,
            total_days: 1,
            average_daily_usage_ms: 60_000,
            git_commits: 0,
            top_app: Some("Code.exe".into()),
        }
    }

    fn sources() -> Vec<KnowledgeDraftSource> {
        [
            ("life-log", true, Some(1), None, None),
            ("git", true, None, None, None),
            ("run-manager", false, None, None, None),
            ("knowledge-base", true, Some(1), Some(1), Some(1)),
        ]
        .into_iter()
        .map(
            |(id, available, schema_version, snapshot_version, freshness)| KnowledgeDraftSource {
                id: id.into(),
                available,
                schema_version,
                snapshot_version,
                producer_version: if id != "git" && (id == "life-log" || available) {
                    Some("0.5.0".into())
                } else {
                    None
                },
                generated_at: if available && !matches!(id, "life-log" | "git") {
                    Some("2026-08-28T00:00:00Z".into())
                } else {
                    None
                },
                freshness_ms: freshness.map(|_| 1_000),
                view: None,
                scope: if matches!(id, "run-manager" | "knowledge-base") {
                    "latest-snapshot-out-of-range".into()
                } else {
                    "requested-range".into()
                },
                error_code: if available {
                    None
                } else {
                    Some("snapshot_unavailable".into())
                },
            },
        )
        .collect()
    }

    fn handoff_id(index: usize) -> String {
        format!("{index:032x}")
    }

    fn insert_input<'a>(
        handoff_id: &'a str,
        summary: &'a KnowledgeDraftSummary,
        sources: &'a [KnowledgeDraftSource],
        status: DraftStatus,
        created_at_ms: u64,
        expires_at_ms: u64,
    ) -> DraftHistoryInsert<'a> {
        DraftHistoryInsert {
            handoff_id,
            summary,
            sources,
            status,
            created_at_ms,
            expires_at_ms,
            regenerated_from: None,
        }
    }

    #[test]
    fn status_names_are_stable() {
        assert_eq!(DraftStatus::Pending.as_str(), "pending");
        assert_eq!(DraftStatus::Consumed.as_str(), "consumed");
    }

    #[test]
    fn opaque_ids_are_bounded() {
        assert!(valid_handoff_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_handoff_id("../secrets"));
    }

    #[test]
    fn terminal_statuses_cannot_be_regressed() {
        assert!(valid_status_transition(
            DraftStatus::Pending,
            DraftStatus::Sent
        ));
        assert!(valid_status_transition(
            DraftStatus::Pending,
            DraftStatus::Consumed
        ));
        assert!(valid_status_transition(
            DraftStatus::Sent,
            DraftStatus::Consumed
        ));
        assert!(valid_status_transition(
            DraftStatus::Sent,
            DraftStatus::Pending
        ));
        assert!(!valid_status_transition(
            DraftStatus::Consumed,
            DraftStatus::Pending
        ));
        assert!(!valid_status_transition(
            DraftStatus::Expired,
            DraftStatus::Sent
        ));
    }

    #[test]
    fn insert_is_idempotent_but_rejects_conflicting_id_reuse() {
        let connection = connection();
        let id = handoff_id(1);
        let summary = summary();
        let sources = sources();
        for (source, expected) in
            sources
                .iter()
                .zip(["life-log", "git", "run-manager", "knowledge-base"])
        {
            assert!(
                validate_source(source, expected).is_ok(),
                "invalid fixture source {expected}: {source:?}"
            );
        }
        insert(
            &connection,
            insert_input(
                &id,
                &summary,
                &sources,
                DraftStatus::Pending,
                1_000,
                601_000,
            ),
        )
        .unwrap();
        insert(
            &connection,
            insert_input(
                &id,
                &summary,
                &sources,
                DraftStatus::Pending,
                1_000,
                601_000,
            ),
        )
        .unwrap();
        assert!(insert(
            &connection,
            insert_input(&id, &summary, &sources, DraftStatus::Sent, 1_000, 601_000,),
        )
        .is_err());
        assert_eq!(list(&connection).unwrap().len(), 1);
    }

    #[test]
    fn history_rejects_timestamp_overflow_and_prunes_in_one_transaction() {
        let connection = connection();
        let summary = summary();
        let sources = sources();
        assert!(insert(
            &connection,
            insert_input(
                &handoff_id(0),
                &summary,
                &sources,
                DraftStatus::Pending,
                u64::MAX,
                u64::MAX,
            ),
        )
        .is_err());

        for index in 1..=MAX_HISTORY_ENTRIES + 1 {
            let created = 1_000 + index as u64;
            insert(
                &connection,
                insert_input(
                    &handoff_id(index),
                    &summary,
                    &sources,
                    DraftStatus::Pending,
                    created,
                    created + devbox_applink::DEFAULT_HANDOFF_TTL_MS,
                ),
            )
            .unwrap();
        }
        let entries = list(&connection).unwrap();
        assert_eq!(entries.len(), MAX_HISTORY_ENTRIES);
        assert!(!entries
            .iter()
            .any(|entry| entry.handoff_id == handoff_id(1)));
        assert!(entries
            .iter()
            .any(|entry| entry.handoff_id == handoff_id(101)));
    }

    #[test]
    fn corrupt_oversized_json_is_rejected_before_deserialization() {
        let connection = connection();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        connection
            .execute(
                "INSERT INTO knowledge_draft_history
                 (handoff_id, kind, status, summary_json, sources_json, created_ts, updated_ts, expires_ts)
                 VALUES (?1, ?2, 'pending', ?3, '[]', 1, 1, 600001)",
                params![
                    handoff_id(1),
                    KIND,
                    "x".repeat(MAX_SUMMARY_JSON_BYTES + 1),
                ],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF;")
            .unwrap();
        assert!(get(&connection, &handoff_id(1)).is_err());
        assert!(list(&connection).is_err());
    }

    #[test]
    fn list_detects_a_corrupt_row_count_over_the_retention_limit() {
        let connection = connection();
        let summary_json = serde_json::to_string(&summary()).unwrap();
        let sources_json = serde_json::to_string(&sources()).unwrap();
        for index in 0..=MAX_HISTORY_ENTRIES {
            let created = 1_000 + index as i64;
            connection
                .execute(
                    "INSERT INTO knowledge_draft_history
                     (handoff_id, kind, status, summary_json, sources_json, created_ts, updated_ts, expires_ts)
                     VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?5, ?6)",
                    params![
                        handoff_id(index),
                        KIND,
                        summary_json,
                        sources_json,
                        created,
                        created + devbox_applink::DEFAULT_HANDOFF_TTL_MS as i64,
                    ],
                )
                .unwrap();
        }
        assert!(list(&connection).is_err());
    }
}
