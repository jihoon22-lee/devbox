//! Explicitly saved Everything+ query definitions and their Launcher view.
//!
//! A saved query is a small local definition, not a cached result set.  The
//! only cross-app data is the versioned `query` payload plus bounded filters;
//! file contents, result paths, and filesystem bytes never enter the snapshot.

use crate::commands::indexing::AppState;
use crate::core::db;
use crate::core::models::{SavedQuery, SearchFilter};
use devbox_applink::{contains_sensitive_value as contains_sensitive_value_common, QueryFilter};
use devbox_integration::{Envelope, SnapshotView, SnapshotViews};
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};

const PRODUCER_ID: &str = "everything-plus";
const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const SAVED_QUERIES_KIND: &str = "saved-queries";
const SAVED_QUERIES_SCHEMA_VERSION: u32 = 1;
const MAX_NAME_BYTES: usize = 128;
const MAX_QUERY_BYTES: usize = 512;
const MAX_FILTER_JSON_BYTES: usize = 8 * 1024;
const MAX_SAVED_QUERIES: i64 = 2_048;
const SAVED_QUERY_ERROR: &str = "저장된 검색을 처리할 수 없습니다.";
const SAVED_QUERY_PRIVACY_ERROR: &str = "민감한 검색어는 저장할 수 없습니다.";

// One producer owns one summary.json.  Serialize CRUD-triggered publication
// so a slow filesystem cannot let an older snapshot replace a newer one.
static SNAPSHOT_WRITER: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveSavedQueryRequest {
    pub id: Option<i64>,
    pub name: String,
    pub query: String,
    /// Missing/null is the legacy text-only saved-query shape.
    #[serde(default)]
    pub filter: Option<SearchFilter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherQueryEntry {
    id: String,
    label: String,
    detail: String,
    target_app: &'static str,
    target_kind: &'static str,
    payload_version: u32,
    payload: LauncherQueryPayload,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LauncherQueryPayload {
    text: String,
    filter: QueryFilter,
}

/// List definitions only; result rows are always evaluated against the current
/// index when a query is opened.
#[tauri::command]
pub fn list_saved_queries(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<SavedQuery>, String> {
    let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
    let saved = db::list_saved_queries(&conn).map_err(|_| SAVED_QUERY_ERROR.to_string())?;
    saved
        .into_iter()
        .map(validate_saved_query)
        .collect::<Result<Vec<_>, _>>()
}

/// Create or update one saved query, then publish the complete producer view.
/// The SQLite write and view construction share the producer lock; the file
/// itself is replaced atomically by `crates/integration`.
#[tauri::command]
pub fn save_saved_query(
    state: tauri::State<'_, Arc<AppState>>,
    request: SaveSavedQueryRequest,
) -> Result<SavedQuery, String> {
    let (name, query, filter) = validate_input(&request)?;
    let _writer = snapshot_writer();
    let previous = {
        let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        db::list_saved_queries(&conn).map_err(|_| SAVED_QUERY_ERROR.to_string())?
    };
    let prepared: Result<(SavedQuery, Envelope), String> = (|| {
        let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        if request.id.is_none()
            && db::count_saved_queries(&conn).map_err(|_| SAVED_QUERY_ERROR.to_string())?
                >= MAX_SAVED_QUERIES
        {
            return Err(SAVED_QUERY_ERROR.to_string());
        }
        let saved = db::upsert_saved_query(&conn, request.id, &name, &query, &filter, now_ms())
            .map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        let envelope = build_snapshot(&conn)?;
        Ok((saved, envelope))
    })();
    let (saved, envelope) = match prepared {
        Ok(value) => value,
        Err(error) => {
            restore_after_snapshot_failure(state.inner(), &previous);
            return Err(error);
        }
    };
    if let Err(error) = publish_snapshot_envelope(&envelope) {
        restore_after_snapshot_failure(state.inner(), &previous);
        return Err(error);
    }
    Ok(saved)
}

#[tauri::command]
pub fn delete_saved_query(state: tauri::State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    if id <= 0 {
        return Err(SAVED_QUERY_ERROR.to_string());
    }
    let _writer = snapshot_writer();
    let previous = {
        let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        db::list_saved_queries(&conn).map_err(|_| SAVED_QUERY_ERROR.to_string())?
    };
    let prepared: Result<Envelope, String> = (|| {
        let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        db::delete_saved_query(&conn, id).map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        build_snapshot(&conn)
    })();
    let envelope = match prepared {
        Ok(value) => value,
        Err(error) => {
            restore_after_snapshot_failure(state.inner(), &previous);
            return Err(error);
        }
    };
    if let Err(error) = publish_snapshot_envelope(&envelope) {
        restore_after_snapshot_failure(state.inner(), &previous);
        return Err(error);
    }
    Ok(())
}

/// Publish the current local definitions during startup.  A snapshot failure
/// must not make the search app unavailable; Launcher can report a missing or
/// stale source and Everything+ remains usable.
pub(crate) fn publish_snapshot(state: &Arc<AppState>) -> Result<(), String> {
    let _writer = snapshot_writer();
    let envelope = {
        let conn = state.db.lock().map_err(|_| SAVED_QUERY_ERROR.to_string())?;
        build_snapshot(&conn)?
    };
    publish_snapshot_envelope(&envelope)
}

fn publish_snapshot_envelope(envelope: &Envelope) -> Result<(), String> {
    let directory = devbox_integration::snapshot_dir(PRODUCER_ID, SNAPSHOT_SCHEMA_VERSION);
    devbox_integration::write_atomic(envelope, &directory)
        .map_err(|_| SAVED_QUERY_ERROR.to_string())
}

fn restore_after_snapshot_failure(state: &Arc<AppState>, previous: &[SavedQuery]) {
    if let Ok(conn) = state.db.lock() {
        let _ = db::replace_saved_queries(&conn, previous);
    }
}

fn build_snapshot(conn: &rusqlite::Connection) -> Result<Envelope, String> {
    let saved = db::list_saved_queries(conn)
        .map_err(|_| SAVED_QUERY_ERROR.to_string())?
        .into_iter()
        .map(validate_saved_query)
        .collect::<Result<Vec<_>, _>>()?;
    let entries = saved
        .into_iter()
        .map(|saved| {
            serde_json::to_value(LauncherQueryEntry {
                id: format!("saved-query-{}", saved.id),
                label: saved.name,
                detail: "Everything+ · saved query".to_string(),
                target_app: PRODUCER_ID,
                target_kind: "query",
                payload_version: 1,
                payload: LauncherQueryPayload {
                    text: saved.query,
                    filter: saved.filter.to_applink(),
                },
            })
            .map_err(|_| SAVED_QUERY_ERROR.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut views = SnapshotViews::new();
    views.insert(
        SAVED_QUERIES_KIND.to_string(),
        SnapshotView {
            schema_version: SAVED_QUERIES_SCHEMA_VERSION,
            freshness_ms: 0,
            entries,
        },
    );
    Ok(Envelope::with_views(
        PRODUCER_ID,
        env!("CARGO_PKG_VERSION"),
        views,
    ))
}

fn snapshot_writer() -> MutexGuard<'static, ()> {
    SNAPSHOT_WRITER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn validate_input(
    request: &SaveSavedQueryRequest,
) -> Result<(String, String, SearchFilter), String> {
    if request.id.is_some_and(|id| id <= 0) {
        return Err(SAVED_QUERY_ERROR.to_string());
    }
    let name = validate_label(&request.name)?;
    let query = validate_query(&request.query)?;
    let filter = request
        .filter
        .clone()
        .unwrap_or_default()
        .normalized()
        .map_err(|_| SAVED_QUERY_ERROR.to_string())?;
    if filter
        .extensions
        .iter()
        .any(|extension| contains_sensitive_value(extension))
    {
        return Err(SAVED_QUERY_PRIVACY_ERROR.to_string());
    }
    let serialized_filter =
        serde_json::to_vec(&filter).map_err(|_| SAVED_QUERY_ERROR.to_string())?;
    if serialized_filter.len() > MAX_FILTER_JSON_BYTES {
        return Err(SAVED_QUERY_ERROR.to_string());
    }
    Ok((name, query, filter))
}

fn validate_saved_query(saved: SavedQuery) -> Result<SavedQuery, String> {
    if saved.created_at <= 0 || saved.updated_at < saved.created_at {
        return Err(SAVED_QUERY_ERROR.to_string());
    }
    let request = SaveSavedQueryRequest {
        id: Some(saved.id),
        name: saved.name,
        query: saved.query,
        filter: Some(saved.filter),
    };
    let (name, query, filter) = validate_input(&request)?;
    Ok(SavedQuery {
        id: saved.id,
        name,
        query,
        filter,
        created_at: saved.created_at,
        updated_at: saved.updated_at,
    })
}

fn validate_label(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let sensitive = contains_sensitive_value(trimmed);
    if trimmed.is_empty()
        || trimmed.len() > MAX_NAME_BYTES
        || trimmed.chars().any(char::is_control)
        || sensitive
    {
        return Err(if sensitive {
            SAVED_QUERY_PRIVACY_ERROR.to_string()
        } else {
            SAVED_QUERY_ERROR.to_string()
        });
    }
    Ok(trimmed.to_string())
}

fn validate_query(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_QUERY_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(SAVED_QUERY_ERROR.to_string());
    }
    if contains_sensitive_value(trimmed) {
        return Err(SAVED_QUERY_PRIVACY_ERROR.to_string());
    }
    Ok(trimmed.to_string())
}

fn contains_sensitive_value(value: &str) -> bool {
    contains_sensitive_value_common(value)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db;

    #[test]
    fn saved_queries_reject_raw_credentials_and_oversized_launcher_payloads() {
        assert!(
            serde_json::from_value::<SaveSavedQueryRequest>(serde_json::json!({
                "name": "safe",
                "query": "cargo",
                "filter": {},
                "futureField": true
            }))
            .is_err()
        );
        for query in [
            "Bearer raw-secret",
            "Authorization : Bearer raw-secret",
            "password = raw-secret",
            "access_token=oauth-secret",
            "x-api-key: provider-secret",
            "https://user:password@example.test/private",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.signature-value",
        ] {
            assert!(
                validate_query(query).is_err(),
                "credential fixture accepted: {query}"
            );
        }
        assert!(validate_input(&SaveSavedQueryRequest {
            id: None,
            name: "private extension fixture".into(),
            query: "cargo".into(),
            filter: Some(SearchFilter {
                extensions: vec!["sk-secret".into()],
                ..SearchFilter::default()
            }),
        })
        .is_err());
        assert!(validate_query(&"x".repeat(MAX_QUERY_BYTES + 1)).is_err());
        assert!(validate_label("safe label").is_ok());
    }

    #[test]
    fn launcher_entry_contains_query_and_filter_but_no_results() {
        let value = serde_json::to_value(LauncherQueryEntry {
            id: "saved-query-1".into(),
            label: "Rust files".into(),
            detail: "Everything+ · saved query".into(),
            target_app: PRODUCER_ID,
            target_kind: "query",
            payload_version: 1,
            payload: LauncherQueryPayload {
                text: "cargo".into(),
                filter: SearchFilter {
                    extensions: vec!["rs".into()],
                    ..SearchFilter::default()
                }
                .to_applink(),
            },
        })
        .unwrap();
        assert_eq!(value["payload"]["text"], serde_json::json!("cargo"));
        assert_eq!(
            value["payload"]["filter"]["extensions"],
            serde_json::json!(["rs"])
        );
        assert!(value.get("results").is_none());
        assert!(value.get("path").is_none());
    }

    #[test]
    fn snapshot_rebuilds_definition_only_and_keeps_result_data_out() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::upsert_saved_query(
            &conn,
            None,
            "Rust sources",
            "cargo",
            &SearchFilter {
                extensions: vec!["rs".into()],
                ..SearchFilter::default()
            },
            1,
        )
        .unwrap();

        let envelope = build_snapshot(&conn).unwrap();
        let entry = &envelope.data["views"][SAVED_QUERIES_KIND]["entries"][0];
        assert_eq!(entry["payload"]["text"], serde_json::json!("cargo"));
        assert_eq!(
            entry["payload"]["filter"]["extensions"],
            serde_json::json!(["rs"])
        );
        assert!(entry.get("results").is_none());
        assert!(entry.get("path").is_none());
        assert!(entry.get("content").is_none());

        let root = std::env::temp_dir().join(format!(
            "everything-saved-query-snapshot-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let directory = devbox_integration::snapshot_dir_in(&root, PRODUCER_ID, 1);
        devbox_integration::write_atomic(&envelope, &directory).unwrap();
        let loaded = devbox_integration::read_snapshot_in(&root, PRODUCER_ID, 1)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.data, envelope.data);
        let _ = std::fs::remove_dir_all(root);
    }
}
