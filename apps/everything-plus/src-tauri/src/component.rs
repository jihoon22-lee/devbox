//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

/// Read-only projections for the product's bounded, independently cancellable
/// connections. The domain keeps its existing filter and deepest-root rules.
pub mod query {
    pub use crate::core::db::{
        is_indexed_path, list_roots, search_content_with_filter,
        search_content_with_filter_in_scope, search_with_filter, search_with_filter_in_scope,
    };
    pub use crate::core::models::SearchFilter;
}

/// Cached watcher health only; this function performs no filesystem IO.
pub fn product_root_health(app: &tauri::AppHandle) -> Vec<(String, bool, bool)> {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let state = app.state::<std::sync::Arc<crate::commands::indexing::AppState>>();
    let indexing = state.indexing.load(Ordering::Acquire);
    let indexed_once = state.last_indexed_at.load(Ordering::Acquire) > 0;
    app.state::<std::sync::Arc<crate::commands::watcher::WatcherManager>>()
        .statuses()
        .into_iter()
        .map(|status| {
            let offline = status.error.as_deref() == Some("root_unavailable");
            (
                status.root,
                offline,
                status.error.is_some()
                    || status.pending > 0
                    || indexing
                    || (status.last_synced_at.is_none() && !indexed_once),
            )
        })
        .collect()
}

/// Reuse indexing, watcher restoration and saved-query publication with an
/// explicit native data root. Legacy identifier migration remains in run().
pub fn initialize(
    app: &tauri::AppHandle,
    dir: &std::path::Path,
    integration_root: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::commands::indexing::AppState;
    use crate::commands::watcher::WatcherManager;
    use std::sync::atomic::{AtomicBool, AtomicI64};
    use std::sync::{Arc, Mutex};
    use tauri::Manager;
    if app.try_state::<crate::applink::PendingOpen>().is_none() {
        app.manage(crate::applink::PendingOpen::new());
    }
    if app.try_state::<Arc<AppState>>().is_some() {
        return Err("component_state_conflict".into());
    }
    std::fs::create_dir_all(dir)?;
    let (conn, index_cleared) = crate::core::db::init(&dir.join("data.db"))?;
    let product_hosted = integration_root.is_some();
    let state = Arc::new(AppState {
        integration_root,
        db: Mutex::new(conn),
        lifecycle: Mutex::new(()),
        indexing: AtomicBool::new(false),
        cancel_requested: AtomicBool::new(false),
        restart_requested: AtomicBool::new(false),
        indexed: AtomicI64::new(0),
        total: AtomicI64::new(0),
        content_indexed: AtomicI64::new(0),
        content_truncated: AtomicI64::new(0),
        content_failed: AtomicI64::new(0),
        last_indexed_at: AtomicI64::new(0),
        last_error: Mutex::new(None),
    });
    // watcher는 DB 초기화 뒤, 상태 관리 전에 생성한다 (restore_all이 db를 읽는다)
    let watcher = WatcherManager::new(app.clone(), state.clone());
    // 스키마 버전이 올라가 migrate()가 인덱스를 비웠다면, 등록된
    // 루트가 있는 한 사용자가 빈 검색 결과만 보지 않도록 전체
    // 재인덱싱을 자동으로 걸어준다.
    if index_cleared {
        let roots = crate::core::db::list_roots(&state.db.lock().unwrap()).unwrap_or_default();
        if !roots.is_empty() {
            crate::commands::indexing::spawn_index(state.clone(), Vec::new());
        } else {
            crate::core::db::record_pdf_extractor_version(&state.db.lock().unwrap())?;
            crate::core::db::record_docx_extractor_version(&state.db.lock().unwrap())?;
            crate::core::db::record_xls_extractor_version(&state.db.lock().unwrap())?;
            crate::core::db::record_xlsx_extractor_version(&state.db.lock().unwrap())?;
            crate::core::db::record_ods_extractor_version(&state.db.lock().unwrap())?;
        }
    } else {
        let conn = state.db.lock().unwrap();
        let stale_pdf = crate::core::db::pdf_reindex_required(&conn).unwrap_or(true);
        let stale_docx = crate::core::db::docx_reindex_required(&conn).unwrap_or(true);
        let stale_xls = crate::core::db::xls_reindex_required(&conn).unwrap_or(true);
        let stale_xlsx = crate::core::db::xlsx_reindex_required(&conn).unwrap_or(true);
        let stale_ods = crate::core::db::ods_reindex_required(&conn).unwrap_or(true);
        drop(conn);
        let mut formats = crate::commands::indexing::FormatSet::empty();
        if stale_pdf {
            formats = formats.with(crate::commands::indexing::FormatSet::PDF);
        }
        if stale_docx {
            formats = formats.with(crate::commands::indexing::FormatSet::DOCX);
        }
        if stale_xls {
            formats = formats.with(crate::commands::indexing::FormatSet::XLS);
        }
        if stale_xlsx {
            formats = formats.with(crate::commands::indexing::FormatSet::XLSX);
        }
        if stale_ods {
            formats = formats.with(crate::commands::indexing::FormatSet::ODS);
        }
        crate::commands::indexing::spawn_format_reindex(state.clone(), formats);
    }
    app.manage(state.clone());
    app.manage(watcher.clone());
    if let Err(error) = crate::commands::saved_queries::publish_snapshot(&state) {
        // Search remains available when the optional cross-app
        // snapshot directory is unavailable; Launcher reports the
        // source as missing/stale instead of receiving partial data.
        eprintln!("everything-plus: saved query snapshot unavailable: {error}");
    }
    // 앱 재시작 시 등록된 루트의 watcher를 복원한다
    if product_hosted {
        tauri::async_runtime::spawn_blocking(move || watcher.restore_all());
    } else {
        watcher.restore_all();
    }
    Ok(())
}

/// Create only a new product-owned database; never initialize a legacy source.
pub fn create_empty_store(path: &std::path::Path) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "component_store_exists")?;
    crate::core::db::init(path).map_err(|_| "component_storage_unavailable")?;
    Ok(())
}

/// Migration reuses the actual source filter validator and preserves root filters
/// by remapping IDs, including a reserved ID for a deleted source root.
pub const IMPORT_SAVED_QUERY_LIMIT: usize = crate::core::db::MAX_SAVED_QUERIES as usize;
pub fn validate_import_saved_query(
    name: &str,
    query: &str,
    created: i64,
    updated: i64,
) -> Result<(), String> {
    crate::core::db::validate_saved_query_definition(name, query, created, updated)
        .map_err(|_| "import_row_invalid".into())
}
pub fn import_filter_root(raw: &str) -> Result<Option<i64>, String> {
    if raw.len() > 8 * 1024 {
        return Err("import_row_invalid".into());
    }
    let filter: crate::core::models::SearchFilter =
        serde_json::from_str(raw).map_err(|_| "import_row_invalid")?;
    Ok(filter
        .normalized()
        .map_err(|_| "import_row_invalid")?
        .source_root_id)
}
pub fn remap_import_filter(raw: &str, root: Option<i64>) -> Result<String, String> {
    import_filter_root(raw)?;
    let mut filter: crate::core::models::SearchFilter =
        serde_json::from_str(raw).map_err(|_| "import_row_invalid")?;
    filter.source_root_id = root;
    serde_json::to_string(&filter.normalized().map_err(|_| "import_row_invalid")?)
        .map_err(|_| "import_row_invalid".into())
}
pub fn normalize_import_root(raw: &str) -> Result<String, String> {
    crate::core::db::normalize_absolute_root(raw).map_err(|_| "import_row_invalid".into())
}

pub const COMMANDS: &[&str] = &[
    "take_pending_open",
    "add_root",
    "remove_root",
    "list_roots",
    "index_now",
    "cancel_index",
    "index_status",
    "search_files",
    "search_content",
    "list_saved_queries",
    "save_saved_query",
    "delete_saved_query",
    "watcher_statuses",
    "open_file",
    "reveal_file",
    "open_targets",
    "open_in",
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "add_root" => crate::commands::indexing::__component_add_root(app, args).await,
        "remove_root" => crate::commands::indexing::__component_remove_root(app, args).await,
        "list_roots" => crate::commands::indexing::__component_list_roots(app, args).await,
        "index_now" => crate::commands::indexing::__component_index_now(app, args).await,
        "cancel_index" => crate::commands::indexing::__component_cancel_index(app, args).await,
        "index_status" => crate::commands::indexing::__component_index_status(app, args).await,
        "search_files" => crate::commands::search::__component_search_files(app, args).await,
        "search_content" => crate::commands::search::__component_search_content(app, args).await,
        "list_saved_queries" => {
            crate::commands::saved_queries::__component_list_saved_queries(app, args).await
        }
        "save_saved_query" => {
            crate::commands::saved_queries::__component_save_saved_query(app, args).await
        }
        "delete_saved_query" => {
            crate::commands::saved_queries::__component_delete_saved_query(app, args).await
        }
        "watcher_statuses" => {
            crate::commands::watcher::__component_watcher_statuses(app, args).await
        }
        "open_file" => crate::commands::actions::__component_open_file(app, args).await,
        "reveal_file" => crate::commands::actions::__component_reveal_file(app, args).await,
        "open_targets" => crate::commands::actions::__component_open_targets(app, args).await,
        "open_in" => crate::commands::actions::__component_open_in(app, args).await,
        _ => Err("component_method_unavailable".into()),
    }
}

/// Native Knowledge query provider; it owns authorization and retained worker
/// limits. This returns validated definitions without executing a saved query.
pub fn saved_query_definitions(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let state = app
        .try_state::<std::sync::Arc<crate::commands::indexing::AppState>>()
        .ok_or("component_state_unavailable")?;
    let rows = crate::commands::saved_queries::list_saved_queries(state)?;
    serde_json::to_value(rows).map_err(|_| "component_response_invalid".into())
}

/// Current index worker counters only; no roots, paths, query text or DB reads.
pub fn product_index_operation(app: &tauri::AppHandle) -> Option<(bool, bool, bool, u64, u64)> {
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    let state = app.try_state::<std::sync::Arc<crate::commands::indexing::AppState>>()?;
    Some((
        state.indexing.load(Ordering::Acquire),
        state.cancel_requested.load(Ordering::Acquire),
        state.last_error.lock().ok()?.is_some(),
        state.indexed.load(Ordering::Acquire).max(0) as u64,
        state.last_indexed_at.load(Ordering::Acquire).max(0) as u64,
    ))
}
