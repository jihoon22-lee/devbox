//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

/// Connect the actual note engine to a native-owned DB/vault and snapshot
/// namespace after migration. This never performs legacy identifier migration.
pub fn initialize(
    app: &tauri::AppHandle,
    dir: &std::path::Path,
    integration_root: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::commands::docs::AppState;
    use crate::commands::watcher::KnowledgeWatcher;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tauri::Manager;
    if app.try_state::<crate::applink::PendingOpen>().is_none() {
        app.manage(crate::applink::PendingOpen::new());
    }
    if app.try_state::<Arc<AppState>>().is_some() {
        return Err("component_state_conflict".into());
    }
    let pending = match integration_root.as_ref() {
        Some(root) => crate::commands::handoff::PendingKnowledgeDraft::with_store(
            devbox_applink::HandoffStore::new(devbox_applink::handoff_root_in(root)),
        ),
        None => crate::commands::handoff::PendingKnowledgeDraft::new(),
    };
    app.manage(pending);
    std::fs::create_dir_all(dir)?;
    let conn = crate::core::db::init(&dir.join("data.db"))?;
    // An unconfigured product starts in its own vault. Imported/user-selected
    // bindings are installed only after the product migration ownership gate.
    if integration_root.is_some() && crate::core::db::get_setting(&conn, "root")?.is_none() {
        let vault = dir.join("vault");
        crate::core::store::ensure_layout(&vault)?;
        crate::core::db::set_setting(&conn, "root", &vault.to_string_lossy())?;
    }
    if let Ok(root) = crate::commands::docs::resolve_root(&conn) {
        if crate::commands::docs::rebuild_wikilink_index_if_needed(&conn, &root).is_err() {
            eprintln!("wikilink index rebuild will retry next launch");
        }
    }
    let state = Arc::new(AppState {
        integration_root: integration_root.clone(),
        db: Mutex::new(conn),
        rename_plans: Mutex::new(crate::core::rename::RenamePlanStore::default()),
        quick_capture_previews: Mutex::new(
            crate::commands::docs::QuickCapturePreviewStore::default(),
        ),
        template_previews: Mutex::new(crate::commands::templates::TemplatePreviewStore::default()),
        image_cache: Mutex::new(HashMap::new()),
    });
    // watcher 생성 후 루트에 연결 (앱 재시작 시 외부 편집 계속 반영)
    let watcher = KnowledgeWatcher::new(app.clone(), state.clone());
    if let Ok(root) = crate::commands::docs::resolve_root(&state.db.lock().unwrap()) {
        watcher.restore_root(&root);
    }
    // integration snapshot producer (두 번째, §10.1)
    let _ = crate::integration::write_snapshot(
        &state.db.lock().unwrap(),
        state.integration_root.as_deref(),
    );
    let shortcut_state = Arc::new(crate::platform::QuickCaptureShortcutState::default());
    app.manage(shortcut_state.clone());
    crate::platform::install(app.clone(), shortcut_state);
    app.manage(Mutex::new(crate::commands::daily::DailyPreviews::default()));
    app.manage(state);
    app.manage(watcher);
    Ok(())
}

/// The product passes only its own new-user vault here, never an imported binding.
pub fn create_private_vault(path: &std::path::Path) -> Result<(), String> {
    crate::core::store::ensure_layout(path)
}

/// Create only a new product-owned database; never initialize a legacy source.
pub fn create_empty_store(path: &std::path::Path, vault: &std::path::Path) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "component_store_exists")?;
    let conn = crate::core::db::init(path).map_err(|_| "component_storage_unavailable")?;
    crate::core::db::set_setting(&conn, "root", &vault.to_string_lossy())
        .map_err(|_| "component_storage_unavailable".to_owned())
}

pub const COMMANDS: &[&str] = &[
    "save_image_asset",
    "take_pending_open",
    "get_root",
    "set_root",
    "list_tree",
    "read_file",
    "open_inbound_note",
    "write_file",
    "create_file",
    "create_directory",
    "preview_rename",
    "apply_rename",
    "discard_rename_preview",
    "delete_file",
    "entry_path",
    "reveal_entry",
    "open_targets",
    "open_in",
    "preview_quick_capture",
    "save_quick_capture",
    "discard_quick_capture_preview",
    "list_templates",
    "create_template",
    "update_template",
    "delete_template",
    "preview_template",
    "save_template",
    "discard_template_preview",
    "search_docs",
    "list_tags",
    "daily_note",
    "shortcut_status",
    "preview_knowledge_draft",
    "save_knowledge_draft",
    "discard_knowledge_draft",
    "renew_knowledge_draft",
    "render_markdown",
    "analyze_wikilinks",
    "wikilink_candidates",
    "backlinks",
    "knowledge_watcher_status",
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "save_image_asset" => {
            crate::commands::assets::__component_save_image_asset(app, args).await
        }
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "get_root" => crate::commands::docs::__component_get_root(app, args).await,
        "set_root" => crate::commands::docs::__component_set_root(app, args).await,
        "list_tree" => crate::commands::docs::__component_list_tree(app, args).await,
        "read_file" => crate::commands::docs::__component_read_file(app, args).await,
        "open_inbound_note" => {
            crate::commands::docs::__component_open_inbound_note(app, args).await
        }
        "write_file" => crate::commands::docs::__component_write_file(app, args).await,
        "create_file" => crate::commands::docs::__component_create_file(app, args).await,
        "create_directory" => crate::commands::docs::__component_create_directory(app, args).await,
        "preview_rename" => crate::commands::rename::__component_preview_rename(app, args).await,
        "apply_rename" => crate::commands::rename::__component_apply_rename(app, args).await,
        "discard_rename_preview" => {
            crate::commands::rename::__component_discard_rename_preview(app, args).await
        }
        "delete_file" => crate::commands::docs::__component_delete_file(app, args).await,
        "entry_path" => crate::commands::docs::__component_entry_path(app, args).await,
        "reveal_entry" => crate::commands::docs::__component_reveal_entry(app, args).await,
        "open_targets" => crate::commands::docs::__component_open_targets(app, args).await,
        "open_in" => crate::commands::docs::__component_open_in(app, args).await,
        "preview_quick_capture" => {
            crate::commands::docs::__component_preview_quick_capture(app, args).await
        }
        "save_quick_capture" => {
            crate::commands::docs::__component_save_quick_capture(app, args).await
        }
        "discard_quick_capture_preview" => {
            crate::commands::docs::__component_discard_quick_capture_preview(app, args).await
        }
        "list_templates" => crate::commands::templates::__component_list_templates(app, args).await,
        "create_template" => {
            crate::commands::templates::__component_create_template(app, args).await
        }
        "update_template" => {
            crate::commands::templates::__component_update_template(app, args).await
        }
        "delete_template" => {
            crate::commands::templates::__component_delete_template(app, args).await
        }
        "preview_template" => {
            crate::commands::templates::__component_preview_template(app, args).await
        }
        "save_template" => crate::commands::templates::__component_save_template(app, args).await,
        "discard_template_preview" => {
            crate::commands::templates::__component_discard_template_preview(app, args).await
        }
        "search_docs" => crate::commands::docs::__component_search_docs(app, args).await,
        "list_tags" => crate::commands::docs::__component_list_tags(app, args).await,
        "daily_note" => crate::commands::docs::__component_daily_note(app, args).await,
        "shortcut_status" => crate::platform::__component_shortcut_status(app, args).await,
        "preview_knowledge_draft" => {
            crate::commands::handoff::__component_preview_knowledge_draft(app, args).await
        }
        "save_knowledge_draft" => {
            crate::commands::handoff::__component_save_knowledge_draft(app, args).await
        }
        "discard_knowledge_draft" => {
            crate::commands::handoff::__component_discard_knowledge_draft(app, args).await
        }
        "renew_knowledge_draft" => {
            crate::commands::handoff::__component_renew_knowledge_draft(app, args).await
        }
        "render_markdown" => {
            crate::commands::markdown::__component_render_markdown(app, args).await
        }
        "analyze_wikilinks" => {
            crate::commands::wikilinks::__component_analyze_wikilinks(app, args).await
        }
        "wikilink_candidates" => {
            crate::commands::wikilinks::__component_wikilink_candidates(app, args).await
        }
        "backlinks" => crate::commands::wikilinks::__component_backlinks(app, args).await,
        "knowledge_watcher_status" => {
            crate::commands::watcher::__component_knowledge_watcher_status(app, args).await
        }
        _ => Err("component_method_unavailable".into()),
    }
}

/// Product-only Daily owner; no extra commands are exposed by the legacy entry.
pub use crate::commands::daily::{
    dispatch as daily_dispatch, DailyPreviews, METHODS as DAILY_METHODS,
};
