//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

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
    watcher.restore_all();
    Ok(())
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
