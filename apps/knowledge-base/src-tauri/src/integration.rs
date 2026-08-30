//! devbox 공용 integration root에 Knowledge activity snapshot을 발행한다.
//!
//! consumer에는 오늘 작성·수정된 note 수와 DB row 기반 불투명 식별자만 전달한다.
//! note 경로·제목·본문·tag는 snapshot 경계를 넘지 않는다.

use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

const PRODUCER_ID: &str = "knowledge-base";
const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const ACTIVITY_KIND: &str = "activity";
const ACTIVITY_SCHEMA_VERSION: u32 = 1;
const DAILY_ACTIVITY_KIND: &str = "daily-activity";
const DAILY_ACTIVITY_SCHEMA_VERSION: u32 = 1;
const DAY_MS: i64 = 86_400_000;
const MAX_NOTE_IDS: usize = 512;
const MAX_DAILY_NOTE_COUNT: u64 = 10_000_000;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct KnowledgeActivityEntry {
    notes_modified_today: u64,
    last_modified_at_ms: Option<i64>,
    note_ids: Vec<String>,
    identifiers_truncated: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct KnowledgeDailyActivityEntry {
    #[serde(flatten)]
    day: devbox_integration::LocalCivilDay,
    notes_modified: u64,
    last_modified_at_ms: Option<i64>,
}

/// Knowledge activity snapshot을 쓴다. 실패해도 앱 동작을 막지 않는다.
pub fn write_snapshot(db: &Connection) -> Result<(), String> {
    write_snapshot_in(
        db,
        &devbox_integration::integration_root(),
        current_epoch_ms(),
    )
}

fn write_snapshot_in(db: &Connection, root: &Path, now_ms: i64) -> Result<(), String> {
    let entry = activity_entry(db, now_ms)?;
    let serialized =
        serde_json::to_value(entry).map_err(|_| "Knowledge activity를 직렬화할 수 없습니다")?;
    let views = SnapshotViews::from([(
        ACTIVITY_KIND.to_owned(),
        SnapshotView {
            schema_version: ACTIVITY_SCHEMA_VERSION,
            freshness_ms: 0,
            entries: vec![serialized],
        },
    )]);
    let envelope = Envelope::with_views(PRODUCER_ID, env!("CARGO_PKG_VERSION"), views);
    let dir = devbox_integration::snapshot_dir_in(root, PRODUCER_ID, SNAPSHOT_SCHEMA_VERSION);
    devbox_integration::write_atomic(&envelope, &dir)?;
    write_daily_activity_snapshot_in(db, root, now_ms)
}

fn write_daily_activity_snapshot_in(
    db: &Connection,
    root: &Path,
    now_ms: i64,
) -> Result<(), String> {
    let now = u64::try_from(now_ms).map_err(|_| "Knowledge activity 시각이 올바르지 않습니다")?;
    let days = devbox_integration::recent_local_civil_days(
        now,
        devbox_integration::MAX_DAILY_ACTIVITY_DAYS,
    )?;
    let entries = daily_activity_entries(db, &days, now_ms)?
        .into_iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Knowledge daily activity를 직렬화할 수 없습니다")?;
    let views = SnapshotViews::from([(
        DAILY_ACTIVITY_KIND.to_owned(),
        SnapshotView {
            schema_version: DAILY_ACTIVITY_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    )]);
    let envelope = Envelope::with_views(PRODUCER_ID, env!("CARGO_PKG_VERSION"), views);
    devbox_integration::write_named_view_snapshot_atomic(&envelope, root, DAILY_ACTIVITY_KIND)
}

fn daily_activity_entries(
    db: &Connection,
    days: &[devbox_integration::LocalCivilDay],
    now_ms: i64,
) -> Result<Vec<KnowledgeDailyActivityEntry>, String> {
    devbox_integration::validate_local_civil_days(days)?;
    if now_ms <= 0 {
        return Err("Knowledge activity 시각이 올바르지 않습니다".into());
    }
    let mut statement = db
        .prepare(
            "SELECT COUNT(*), MAX(modified_ts)
             FROM docs
             WHERE modified_ts >= ?1 AND modified_ts < ?2",
        )
        .map_err(|_| "Knowledge daily activity를 준비할 수 없습니다")?;
    days.iter()
        .map(|day| {
            let effective_end = day.end_ms.min(now_ms.saturating_add(1));
            let (notes_modified, last_modified_at_ms): (u64, Option<i64>) =
                if effective_end <= day.start_ms {
                    (0, None)
                } else {
                    statement
                        .query_row(rusqlite::params![day.start_ms, effective_end], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })
                        .map_err(|_| "Knowledge daily activity를 계산할 수 없습니다")?
                };
            if notes_modified > MAX_DAILY_NOTE_COUNT
                || last_modified_at_ms
                    .is_some_and(|timestamp| timestamp < day.start_ms || timestamp >= effective_end)
            {
                return Err("Knowledge daily activity 범위를 초과했습니다".into());
            }
            Ok(KnowledgeDailyActivityEntry {
                day: day.clone(),
                notes_modified,
                last_modified_at_ms,
            })
        })
        .collect()
}

fn activity_entry(db: &Connection, now_ms: i64) -> Result<KnowledgeActivityEntry, String> {
    let today_start = now_ms.div_euclid(DAY_MS) * DAY_MS;
    let notes_modified_today: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM docs WHERE modified_ts >= ?1 AND modified_ts <= ?2",
            rusqlite::params![today_start, now_ms],
            |row| row.get(0),
        )
        .map_err(|_| "Knowledge activity 수를 계산할 수 없습니다")?;
    let last_modified_at_ms: Option<i64> = db
        .query_row("SELECT MAX(modified_ts) FROM docs", [], |row| row.get(0))
        .map_err(|_| "Knowledge 최근 수정 시각을 읽을 수 없습니다")?;

    let mut statement = db
        .prepare(
            "SELECT id FROM docs
             WHERE modified_ts >= ?1 AND modified_ts <= ?2
             ORDER BY modified_ts DESC, id ASC
             LIMIT ?3",
        )
        .map_err(|_| "Knowledge activity 식별자를 준비할 수 없습니다")?;
    let rows = statement
        .query_map(
            rusqlite::params![today_start, now_ms, MAX_NOTE_IDS as i64],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| "Knowledge activity 식별자를 읽을 수 없습니다")?;
    let mut note_ids = Vec::new();
    for row in rows {
        let id = row.map_err(|_| "Knowledge activity 식별자를 읽을 수 없습니다")?;
        if id <= 0 {
            return Err("Knowledge activity 식별자가 올바르지 않습니다".into());
        }
        note_ids.push(format!("note-{id}"));
    }

    let identifiers_truncated = notes_modified_today > note_ids.len() as u64;
    Ok(KnowledgeActivityEntry {
        notes_modified_today,
        last_modified_at_ms,
        note_ids,
        identifiers_truncated,
    })
}

fn current_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn database() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&connection).unwrap();
        connection
    }

    fn insert_doc(connection: &Connection, path: &str, body: &str, modified_ts: i64) {
        connection
            .execute(
                "INSERT INTO docs (path, title, body, tags, modified_ts)
                 VALUES (?1, ?1, ?2, '[]', ?3)",
                params![path, body, modified_ts],
            )
            .unwrap();
    }

    #[test]
    fn builds_path_free_activity_summary_for_today() {
        let connection = database();
        let now_ms = DAY_MS * 10 + 5_000;
        insert_doc(
            &connection,
            "private/credential-sk-do-not-export.md",
            "secret body",
            now_ms - 2_000,
        );
        insert_doc(
            &connection,
            "notes/today.md",
            "another body",
            now_ms - 1_000,
        );
        insert_doc(
            &connection,
            "notes/yesterday.md",
            "old body",
            now_ms - 6_000,
        );

        let entry = activity_entry(&connection, now_ms).unwrap();

        assert_eq!(entry.notes_modified_today, 2);
        assert_eq!(entry.last_modified_at_ms, Some(now_ms - 1_000));
        assert_eq!(entry.note_ids, vec!["note-2", "note-1"]);
        assert!(!entry.identifiers_truncated);
        let serialized = serde_json::to_string(&entry).unwrap();
        assert!(!serialized.contains("credential"));
        assert!(!serialized.contains("secret body"));
        assert!(!serialized.contains("today.md"));
    }

    #[test]
    fn retains_overall_last_modified_when_today_has_no_activity() {
        let connection = database();
        let now_ms = DAY_MS * 5 + 1_000;
        let yesterday = now_ms - DAY_MS;
        insert_doc(&connection, "old.md", "old", yesterday);

        let entry = activity_entry(&connection, now_ms).unwrap();

        assert_eq!(entry.notes_modified_today, 0);
        assert_eq!(entry.last_modified_at_ms, Some(yesterday));
        assert!(entry.note_ids.is_empty());
        assert!(!entry.identifiers_truncated);
    }

    #[test]
    fn bounds_opaque_identifiers_and_marks_truncation() {
        let connection = database();
        let now_ms = DAY_MS * 20 + 10_000;
        for index in 0..=MAX_NOTE_IDS {
            insert_doc(
                &connection,
                &format!("notes/{index}.md"),
                "body",
                now_ms - index as i64,
            );
        }

        let entry = activity_entry(&connection, now_ms).unwrap();

        assert_eq!(entry.notes_modified_today, (MAX_NOTE_IDS + 1) as u64);
        assert_eq!(entry.note_ids.len(), MAX_NOTE_IDS);
        assert!(entry.identifiers_truncated);
    }

    #[test]
    fn writes_discoverable_activity_view_without_note_content() {
        let connection = database();
        let now_ms = 1_788_042_600_000_i64;
        insert_doc(
            &connection,
            "notes/private.md",
            "raw credential must stay local",
            now_ms,
        );
        let temp = tempfile::tempdir().unwrap();

        write_snapshot_in(&connection, temp.path(), now_ms).unwrap();

        let report = devbox_integration::discover_report_in(temp.path());
        assert!(report.issues.is_empty());
        assert_eq!(report.snapshots.len(), 1);
        let reference = &report.snapshots[0];
        assert_eq!(reference.producer, PRODUCER_ID);
        assert_eq!(reference.version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(reference.views.len(), 1);
        assert_eq!(reference.views[0].kind, ACTIVITY_KIND);
        assert_eq!(reference.views[0].schema_version, ACTIVITY_SCHEMA_VERSION);
        assert_eq!(reference.views[0].entry_count, 1);

        let bytes = std::fs::read_to_string(devbox_integration::snapshot_path_in(
            temp.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
        ))
        .unwrap();
        assert!(!bytes.contains("private.md"));
        assert!(!bytes.contains("raw credential"));

        let daily = devbox_integration::read_named_view_snapshot_in(
            temp.path(),
            PRODUCER_ID,
            SNAPSHOT_SCHEMA_VERSION,
            DAILY_ACTIVITY_KIND,
        )
        .unwrap()
        .unwrap();
        let views = daily.views().unwrap();
        let view = views.get(DAILY_ACTIVITY_KIND).unwrap();
        assert_eq!(view.schema_version, DAILY_ACTIVITY_SCHEMA_VERSION);
        assert_eq!(
            view.entries.len(),
            devbox_integration::MAX_DAILY_ACTIVITY_DAYS
        );
        let encoded = serde_json::to_string(&daily).unwrap();
        assert!(!encoded.contains("private.md"));
        assert!(!encoded.contains("raw credential"));
        assert!(!encoded.contains("noteIds"));
    }

    #[test]
    fn daily_activity_uses_exact_half_open_civil_boundaries() {
        let connection = database();
        let start = 1_700_000_000_000_i64;
        let days = vec![
            devbox_integration::LocalCivilDay {
                date: "2026-08-29".into(),
                start_ms: start,
                end_ms: start + DAY_MS,
                timezone: "Asia/Seoul".into(),
            },
            devbox_integration::LocalCivilDay {
                date: "2026-08-30".into(),
                start_ms: start + DAY_MS,
                end_ms: start + 2 * DAY_MS,
                timezone: "Asia/Seoul".into(),
            },
        ];
        insert_doc(&connection, "first.md", "body", start);
        insert_doc(&connection, "last-first.md", "body", start + DAY_MS - 1);
        insert_doc(&connection, "first-second.md", "body", start + DAY_MS);
        insert_doc(
            &connection,
            "future-second.md",
            "body",
            start + DAY_MS + 20_000,
        );

        let entries = daily_activity_entries(&connection, &days, start + DAY_MS + 10_000).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].notes_modified, 2);
        assert_eq!(entries[0].last_modified_at_ms, Some(start + DAY_MS - 1));
        assert_eq!(entries[1].notes_modified, 1);
        assert_eq!(entries[1].last_modified_at_ms, Some(start + DAY_MS));
        assert_eq!(entries[0].day.end_ms, entries[1].day.start_ms);
    }
}
