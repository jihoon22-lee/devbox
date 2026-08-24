//! Life Log의 공용 integration snapshot producer.
//!
//! `<common-root>/integration/life-log/v1/summary.json` 하나에 `projects/v1` view를
//! 발행한다. 업무 데이터 집계는 `core::project_snapshot`, envelope·atomic replace는
//! `crates/integration`이 담당한다.

use crate::commands::tracking::{now_ms, AppState};
use crate::core::project_snapshot;
use rusqlite::Connection;
use std::sync::Arc;

#[cfg(test)]
use std::path::Path;

const PRODUCER_ID: &str = "life-log";
const PROJECTS_KIND: &str = "projects";
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub fn spawn_snapshot_writer(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        write_snapshot_background(state.clone(), "초기 발행").await;
        loop {
            tokio::time::sleep(SNAPSHOT_INTERVAL).await;
            write_snapshot_background(state.clone(), "주기 발행").await;
        }
    });
}

pub fn request_snapshot_write(state: Arc<AppState>) {
    tauri::async_runtime::spawn(write_snapshot_background(state, "설정 변경 발행"));
}

async fn write_snapshot_background(state: Arc<AppState>, phase: &'static str) {
    match tauri::async_runtime::spawn_blocking(move || write_snapshot(&state)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("life-log integration snapshot {phase} 실패: {error}"),
        Err(_) => eprintln!("life-log integration snapshot {phase} 작업을 완료하지 못했습니다"),
    }
}

fn write_snapshot(state: &AppState) -> Result<(), String> {
    let _writer = state
        .snapshot_writer
        .lock()
        .map_err(|_| "Life Log snapshot writer를 잠글 수 없습니다".to_string())?;
    let envelope = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "Life Log 데이터베이스를 잠글 수 없습니다".to_string())?;
        build_envelope(&connection, now_ms())?
    };
    let directory = devbox_integration::snapshot_dir(PRODUCER_ID, 1);
    devbox_integration::write_atomic(&envelope, &directory)
}

fn build_envelope(
    connection: &Connection,
    generated_at_ms: i64,
) -> Result<devbox_integration::Envelope, String> {
    let entries = project_snapshot::build_entries(connection, generated_at_ms)?
        .into_iter()
        .map(|entry| {
            serde_json::to_value(entry)
                .map_err(|_| "프로젝트 snapshot을 직렬화할 수 없습니다".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut views = devbox_integration::SnapshotViews::new();
    views.insert(
        PROJECTS_KIND.into(),
        devbox_integration::SnapshotView {
            schema_version: 1,
            freshness_ms: 0,
            entries,
        },
    );
    Ok(devbox_integration::Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

#[cfg(test)]
fn write_snapshot_in(
    connection: &Connection,
    integration_root: &Path,
    generated_at_ms: i64,
) -> Result<(), String> {
    let envelope = build_envelope(connection, generated_at_ms)?;
    let directory = devbox_integration::snapshot_dir_in(integration_root, PRODUCER_ID, 1);
    devbox_integration::write_atomic(&envelope, &directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db;
    use crate::core::models::ClosedSession;

    fn test_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "devbox-life-log-snapshot-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn writes_and_atomically_replaces_secret_free_projects_view() {
        let root = test_root("replace");
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::set_setting(&connection, "projects", "C:\\work\\devbox");
        db::insert_session(
            &connection,
            &ClosedSession {
                app: "Code".into(),
                title: "devbox — Bearer raw-secret".into(),
                start_ts: 1_000,
                end_ts: 2_000,
            },
        )
        .unwrap();

        write_snapshot_in(&connection, &root, 3_000).unwrap();
        let first = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        let first_views = first.views().unwrap();
        assert_eq!(first_views.keys().collect::<Vec<_>>(), vec![PROJECTS_KIND]);
        assert_eq!(first_views[PROJECTS_KIND].schema_version, 1);
        assert_eq!(first_views[PROJECTS_KIND].freshness_ms, 0);
        assert_eq!(
            first_views[PROJECTS_KIND].entries[0]["path"],
            "C:\\work\\devbox"
        );
        let serialized = serde_json::to_string(&first).unwrap();
        assert!(!serialized.contains("Bearer"));
        assert!(!serialized.contains("raw-secret"));
        assert!(!serialized.contains("Code"));
        let discovered = devbox_integration::discover_report_in(&root);
        assert!(discovered.issues.is_empty());
        assert_eq!(discovered.snapshots.len(), 1);
        assert_eq!(discovered.snapshots[0].producer, PRODUCER_ID);
        assert_eq!(discovered.snapshots[0].version, 1);
        assert_eq!(discovered.snapshots[0].views.len(), 1);
        assert_eq!(discovered.snapshots[0].views[0].kind, PROJECTS_KIND);
        assert_eq!(discovered.snapshots[0].views[0].schema_version, 1);
        assert_eq!(
            discovered.snapshots[0].views[0].freshness_ms,
            discovered.snapshots[0].freshness_ms
        );

        db::set_setting(&connection, "projects", "/mnt/e/projects/other");
        write_snapshot_in(&connection, &root, 4_000).unwrap();
        let replacement = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        let entries = &replacement.views().unwrap()[PROJECTS_KIND].entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["path"], "/mnt/e/projects/other");
        let files = std::fs::read_dir(devbox_integration::snapshot_dir_in(&root, PRODUCER_ID, 1))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(files, vec!["summary.json"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_paths_are_omitted_without_preventing_a_valid_empty_snapshot() {
        let root = test_root("unsafe-path");
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        db::set_setting(
            &connection,
            "projects",
            "relative/private\nC:\\work\\..\\escape\n\\\\?\\C:\\device",
        );

        write_snapshot_in(&connection, &root, 5_000).unwrap();
        let envelope = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        assert!(envelope.views().unwrap()[PROJECTS_KIND].entries.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writer_rejects_a_mismatched_target_without_creating_output() {
        let root = test_root("identity");
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let envelope = build_envelope(&connection, 5_000).unwrap();
        let wrong = devbox_integration::snapshot_dir_in(&root, "workbench", 1);
        let error = devbox_integration::write_atomic(&envelope, &wrong).unwrap_err();
        assert!(error.contains("identity"));
        assert!(!root.exists());
    }
}
