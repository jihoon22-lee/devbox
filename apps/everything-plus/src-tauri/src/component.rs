//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

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
