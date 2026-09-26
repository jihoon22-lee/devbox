//! Typed note and daily operations; the product host owns admission and workers.
use serde::Deserialize;
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum NotesCall {
    TakePendingOpen {},
    RenderMarkdown {
        rel: String,
        content: String,
    },
    KnowledgeWatcherStatus {},
    AnalyzeWikilinks {
        content: String,
    },
    WikilinkCandidates {
        query: String,
    },
    Backlinks {
        rel: String,
    },
    GetRoot {},
    ListTree {},
    ReadFile {
        rel: String,
    },
    OpenInboundNote {
        path: String,
    },
    WriteFile {
        rel: String,
        content: String,
        expected_revision: String,
    },
    CreateFile {
        rel: String,
        content: Option<String>,
    },
    CreateDirectory {
        rel: String,
    },
    DeleteFile {
        rel: String,
    },
    EntryPath {
        rel: String,
    },
    RevealEntry {
        rel: String,
    },
    CaptureNote {
        input: crate::core::capture::QuickCaptureInput,
    },
    UndoCreatedNote {
        path: String,
        revision: String,
    },
    CreateNoteFromTemplate {
        input: crate::core::templates::TemplateApplyInput,
    },
    SearchDocs {
        query: String,
    },
    ListTags {},
    ListTemplates {},
    CreateTemplate {
        draft: crate::core::templates::TemplateDraft,
    },
    UpdateTemplate {
        id: i64,
        draft: crate::core::templates::TemplateDraft,
    },
    DeleteTemplate {
        id: i64,
    },
    PreviewTemplate {
        approval: crate::core::templates::TemplateApplyInput,
    },
    SaveTemplate {
        preview_id: String,
    },
    DiscardTemplatePreview {
        preview_id: String,
    },
    PreviewKnowledgeDraft {
        id: String,
        kind: String,
    },
    SaveKnowledgeDraft {
        id: String,
    },
    DiscardKnowledgeDraft {
        id: String,
    },
    RenewKnowledgeDraft {
        id: String,
    },
    PreviewRename {
        from: String,
        to: String,
    },
    ApplyRename {
        plan_id: String,
    },
    DiscardRenamePreview {
        plan_id: String,
    },
    SaveImageAsset {
        request: crate::commands::assets::SaveImageAssetRequest,
    },
    ShortcutStatus {},
    SaveNoteJournal {
        path: String,
        content: String,
        base_revision: String,
    },
    ClearNoteJournal {
        path: String,
    },
    LoadNoteJournal {},
    DiscardOtherVaultJournal {},
}
pub const NOTES_METHODS: &[&str] = &[
    "take_pending_open",
    "render_markdown",
    "knowledge_watcher_status",
    "analyze_wikilinks",
    "wikilink_candidates",
    "backlinks",
    "get_root",
    "list_tree",
    "read_file",
    "open_inbound_note",
    "write_file",
    "create_file",
    "create_directory",
    "delete_file",
    "entry_path",
    "reveal_entry",
    "capture_note",
    "undo_created_note",
    "create_note_from_template",
    "search_docs",
    "list_tags",
    "list_templates",
    "create_template",
    "update_template",
    "delete_template",
    "preview_template",
    "save_template",
    "discard_template_preview",
    "preview_knowledge_draft",
    "save_knowledge_draft",
    "discard_knowledge_draft",
    "renew_knowledge_draft",
    "preview_rename",
    "apply_rename",
    "discard_rename_preview",
    "save_image_asset",
    "shortcut_status",
    "save_note_journal",
    "clear_note_journal",
    "load_note_journal",
    "discard_other_vault_journal",
];
impl NotesCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::RenderMarkdown { .. } => "render_markdown",
            Self::KnowledgeWatcherStatus { .. } => "knowledge_watcher_status",
            Self::AnalyzeWikilinks { .. } => "analyze_wikilinks",
            Self::WikilinkCandidates { .. } => "wikilink_candidates",
            Self::Backlinks { .. } => "backlinks",
            Self::GetRoot { .. } => "get_root",
            Self::ListTree { .. } => "list_tree",
            Self::ReadFile { .. } => "read_file",
            Self::OpenInboundNote { .. } => "open_inbound_note",
            Self::WriteFile { .. } => "write_file",
            Self::CreateFile { .. } => "create_file",
            Self::CreateDirectory { .. } => "create_directory",
            Self::DeleteFile { .. } => "delete_file",
            Self::EntryPath { .. } => "entry_path",
            Self::RevealEntry { .. } => "reveal_entry",
            Self::CaptureNote { .. } => "capture_note",
            Self::UndoCreatedNote { .. } => "undo_created_note",
            Self::CreateNoteFromTemplate { .. } => "create_note_from_template",
            Self::SearchDocs { .. } => "search_docs",
            Self::ListTags { .. } => "list_tags",
            Self::ListTemplates { .. } => "list_templates",
            Self::CreateTemplate { .. } => "create_template",
            Self::UpdateTemplate { .. } => "update_template",
            Self::DeleteTemplate { .. } => "delete_template",
            Self::PreviewTemplate { .. } => "preview_template",
            Self::SaveTemplate { .. } => "save_template",
            Self::DiscardTemplatePreview { .. } => "discard_template_preview",
            Self::PreviewKnowledgeDraft { .. } => "preview_knowledge_draft",
            Self::SaveKnowledgeDraft { .. } => "save_knowledge_draft",
            Self::DiscardKnowledgeDraft { .. } => "discard_knowledge_draft",
            Self::RenewKnowledgeDraft { .. } => "renew_knowledge_draft",
            Self::PreviewRename { .. } => "preview_rename",
            Self::ApplyRename { .. } => "apply_rename",
            Self::DiscardRenamePreview { .. } => "discard_rename_preview",
            Self::SaveImageAsset { .. } => "save_image_asset",
            Self::ShortcutStatus { .. } => "shortcut_status",
            Self::SaveNoteJournal { .. } => "save_note_journal",
            Self::ClearNoteJournal { .. } => "clear_note_journal",
            Self::LoadNoteJournal { .. } => "load_note_journal",
            Self::DiscardOtherVaultJournal { .. } => "discard_other_vault_journal",
        }
    }
}
pub async fn dispatch(
    component_app: &tauri::AppHandle,
    call: NotesCall,
) -> Result<serde_json::Value, String> {
    match call {
        NotesCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::RenderMarkdown { rel, content } => {
            use crate::commands::markdown::*;
            let value = render_markdown(component_app.state(), rel, content)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::KnowledgeWatcherStatus {} => {
            use crate::commands::watcher::*;
            let value = knowledge_watcher_status(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::AnalyzeWikilinks { content } => {
            use crate::commands::wikilinks::*;
            let value = analyze_wikilinks(component_app.state(), content)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::WikilinkCandidates { query } => {
            use crate::commands::wikilinks::*;
            let value = wikilink_candidates(component_app.state(), query)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::Backlinks { rel } => {
            use crate::commands::wikilinks::*;
            let value = backlinks(component_app.state(), rel)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::GetRoot {} => {
            use crate::commands::docs::*;
            let value = get_root(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ListTree {} => {
            use crate::commands::docs::*;
            let app = component_app.clone();
            let value = tauri::async_runtime::spawn_blocking(move || list_tree(app.state()))
                .await
                .map_err(|_| "metadata_incomplete")??;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ReadFile { rel } => {
            use crate::commands::docs::*;
            let value = read_file(component_app.state(), rel)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::OpenInboundNote { path } => {
            use crate::commands::docs::*;
            let value = open_inbound_note(component_app.state(), path)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::WriteFile {
            rel,
            content,
            expected_revision,
        } => {
            use crate::commands::docs::*;
            let app = component_app.clone();
            let saved = tauri::async_runtime::spawn_blocking(move || {
                write_file(app.state(), rel, content, expected_revision)
            })
            .await
            .map_err(|_| "note_commit_unknown".to_string())??;
            serde_json::to_value(saved).map_err(|_| "component_response_invalid".into())
        }
        NotesCall::CreateFile { rel, content } => {
            use crate::commands::docs::*;
            let app = component_app.clone();
            tauri::async_runtime::spawn_blocking(move || create_file(app.state(), rel, content))
                .await
                .map_err(|_| "note_commit_unknown".to_string())??;
            Ok(serde_json::Value::Null)
        }
        NotesCall::CreateDirectory { rel } => {
            use crate::commands::docs::*;
            create_directory(component_app.state(), rel)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::DeleteFile { rel } => {
            use crate::commands::docs::*;
            let app = component_app.clone();
            tauri::async_runtime::spawn_blocking(move || delete_file(app.state(), rel))
                .await
                .map_err(|_| "note_commit_unknown".to_string())??;
            Ok(serde_json::Value::Null)
        }
        NotesCall::EntryPath { rel } => {
            use crate::commands::docs::*;
            let value = entry_path(component_app.state(), rel)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::RevealEntry { rel } => {
            use crate::commands::docs::*;
            reveal_entry(component_app.clone(), component_app.state(), rel)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::CaptureNote { input } => serde_json::to_value(
            crate::commands::docs::capture_note(component_app.state(), input)?,
        )
        .map_err(|_| "component_response_invalid".into()),
        NotesCall::UndoCreatedNote { path, revision } => serde_json::to_value(
            crate::commands::docs::undo_created_note(component_app.state(), path, revision)?,
        )
        .map_err(|_| "component_response_invalid".into()),
        NotesCall::CreateNoteFromTemplate { input } => serde_json::to_value(
            crate::commands::templates::create_note_from_template(component_app.state(), input)?,
        )
        .map_err(|_| "component_response_invalid".into()),
        NotesCall::SearchDocs { query } => {
            use crate::commands::docs::*;
            let value = search_docs(component_app.state(), query)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ListTags {} => {
            use crate::commands::docs::*;
            let value = list_tags(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ListTemplates {} => {
            use crate::commands::templates::*;
            let value = list_templates(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::CreateTemplate { draft } => {
            use crate::commands::templates::*;
            let value = create_template(component_app.state(), draft)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::UpdateTemplate { id, draft } => {
            use crate::commands::templates::*;
            let value = update_template(component_app.state(), id, draft)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::DeleteTemplate { id } => {
            use crate::commands::templates::*;
            delete_template(component_app.state(), id)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::PreviewTemplate { approval } => {
            use crate::commands::templates::*;
            let value = preview_template(component_app.state(), approval)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::SaveTemplate { preview_id } => {
            use crate::commands::templates::*;
            let value = save_template(component_app.state(), preview_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::DiscardTemplatePreview { preview_id } => {
            use crate::commands::templates::*;
            discard_template_preview(component_app.state(), preview_id)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::PreviewKnowledgeDraft { id, kind } => {
            use crate::commands::handoff::*;
            let value =
                preview_knowledge_draft(component_app.state(), component_app.state(), id, kind)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::SaveKnowledgeDraft { id } => {
            use crate::commands::handoff::*;
            let value = save_knowledge_draft(component_app.state(), component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::DiscardKnowledgeDraft { id } => {
            use crate::commands::handoff::*;
            discard_knowledge_draft(component_app.state(), id)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::RenewKnowledgeDraft { id } => {
            use crate::commands::handoff::*;
            let value = renew_knowledge_draft(component_app.state(), id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::PreviewRename { from, to } => {
            use crate::commands::rename::*;
            let value = preview_rename(component_app.state(), from, to)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ApplyRename { plan_id } => {
            use crate::commands::rename::*;
            let value = apply_rename(component_app.state(), plan_id)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::DiscardRenamePreview { plan_id } => {
            use crate::commands::rename::*;
            discard_rename_preview(component_app.state(), plan_id);
            Ok(serde_json::Value::Null)
        }
        NotesCall::SaveImageAsset { request } => {
            use crate::commands::assets::*;
            let value = save_image_asset(component_app.state(), request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::ShortcutStatus {} => {
            use crate::platform::*;
            let value = shortcut_status(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        NotesCall::SaveNoteJournal {
            path,
            content,
            base_revision,
        } => {
            let state = component_app.state::<std::sync::Arc<crate::commands::docs::AppState>>();
            let root = state.journal.active_root(&state.db)?;
            state.journal.save(&root, path, content, base_revision)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::ClearNoteJournal { path } => {
            let state = component_app.state::<std::sync::Arc<crate::commands::docs::AppState>>();
            let root = state.journal.active_root(&state.db)?;
            state.journal.clear(&root, &path)?;
            Ok(serde_json::Value::Null)
        }
        NotesCall::LoadNoteJournal {} => {
            let state = component_app.state::<std::sync::Arc<crate::commands::docs::AppState>>();
            let root = state.journal.active_root(&state.db)?;
            serde_json::to_value(state.journal.load(&root)?)
                .map_err(|_| "component_response_invalid".into())
        }
        NotesCall::DiscardOtherVaultJournal {} => {
            let state = component_app.state::<std::sync::Arc<crate::commands::docs::AppState>>();
            let root = state.journal.active_root(&state.db)?;
            state.journal.discard_other(&root)?;
            Ok(serde_json::Value::Null)
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum DailyCall {
    OpenDaily { date: String },
}
impl DailyCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::OpenDaily { .. } => "open_daily",
        }
    }
}
pub use crate::commands::daily::dispatch as daily_dispatch;
product_ipc::issue_codes! {
    pub enum NotesIssue {
    ClipboardUnavailable = "clipboard_unavailable",
    ComponentWorkerUnavailable = "component_worker_unavailable",
    DraftBusy = "draft_busy",
    DraftExpired = "draft_expired",
    DraftInvalid = "draft_invalid",
    DraftLimit = "draft_limit",
    DraftStale = "draft_stale",
    DraftUnavailable = "draft_unavailable",
    OpenerUnavailable = "opener_unavailable",
    ProviderUnavailable = "provider_unavailable",
    SummaryBusy = "summary_busy",
    SummaryExpired = "summary_expired",
    SummaryInvalid = "summary_invalid",
    SummaryLimit = "summary_limit",
    SummaryPreviewBusy = "summary_preview_busy",
    SummaryStale = "summary_stale",
    SummaryStoreInvalid = "summary_store_invalid",
    SummaryStoreUnavailable = "summary_store_unavailable",
    SummaryUnavailable = "summary_unavailable",
    VaultBindingUnavailable = "vault_binding_unavailable",
    ComponentArgsInvalid = "component_args_invalid",
    ComponentResponseInvalid = "component_response_invalid",
    ComponentStateConflict = "component_state_conflict",
    ComponentStorageUnavailable = "component_storage_unavailable",
    ComponentStoreExists = "component_store_exists",
    DailyPreviewStale = "daily_preview_stale",
    DailyTargetExists = "daily_target_exists",
    DailyVaultUnavailable = "daily_vault_unavailable",
    DailyWriteFailed = "daily_write_failed",
    DraftDeliveryBusy = "draft_delivery_busy",
    DraftDeliveryInvalid = "draft_delivery_invalid",
    DraftDeliveryUnavailable = "draft_delivery_unavailable",
    ImportRowInvalid = "import_row_invalid",
    JournalLimit = "journal_limit",
    JournalUnavailable = "journal_unavailable",
    KnowledgeDraftInvalid = "knowledge_draft_invalid",
    MetadataBusy = "metadata_busy",
    MetadataCancelled = "metadata_cancelled",
    MetadataIncomplete = "metadata_incomplete",
    MetadataLimit = "metadata_limit",
    MetadataStale = "metadata_stale",
    MetadataTimeout = "metadata_timeout",
    NoteAppliedPostprocessingFailed = "note_applied_postprocessing_failed",
    NoteCommitUnknown = "note_commit_unknown",
    NoteConflict = "note_conflict",
    NoteInvalid = "note_invalid",
    NoteUnavailable = "note_unavailable",
    PreviewExpired = "preview_expired",
    PreviewStale = "preview_stale",
    QuickCaptureBodyRequired = "quick_capture_body_required",
    QuickCaptureSaveFailed = "quick_capture_save_failed",
    SearchStale = "search_stale",
    SearchUnavailable = "search_unavailable",
    SessionSummaryInvalid = "session_summary_invalid",
    SessionSummaryUnavailable = "session_summary_unavailable",
    Unavailable = "unavailable",
    VaultChangeStale = "vault_change_stale",
    VaultChangeUnavailable = "vault_change_unavailable",
    QuickCaptureBodyLimit = "quick_capture_body_limit",
    QuickCaptureInvalid = "quick_capture_invalid",
    QuickCaptureSensitive = "quick_capture_sensitive",
    QuickCaptureTagCount = "quick_capture_tag_count",
    QuickCaptureTagInvalid = "quick_capture_tag_invalid",
    QuickCaptureTagLimit = "quick_capture_tag_limit",
    QuickCaptureTagsLimit = "quick_capture_tags_limit",
    QuickCaptureTitleLimit = "quick_capture_title_limit",
    }
}
pub fn classify(error: &str) -> &'static str {
    NotesIssue::from_code(error)
        .unwrap_or(NotesIssue::Unavailable)
        .code()
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
        (
            "render_markdown",
            export.register::<crate::commands::markdown::RenderedDoc>()?,
        ),
        (
            "knowledge_watcher_status",
            export.register::<crate::commands::watcher::KnowledgeWatcherStatus>()?,
        ),
        (
            "analyze_wikilinks",
            export.register::<Vec<crate::commands::wikilinks::WikilinkOccurrence>>()?,
        ),
        (
            "wikilink_candidates",
            export.register::<Vec<crate::commands::wikilinks::WikilinkCandidate>>()?,
        ),
        (
            "backlinks",
            export.register::<Vec<crate::commands::wikilinks::Backlink>>()?,
        ),
        ("get_root", export.register::<String>()?),
        (
            "list_tree",
            export.register::<Vec<crate::commands::docs::TreeEntry>>()?,
        ),
        (
            "read_file",
            export.register::<crate::core::document::Snapshot>()?,
        ),
        (
            "open_inbound_note",
            export.register::<crate::commands::docs::InboundNote>()?,
        ),
        (
            "write_file",
            export.register::<crate::core::document::Snapshot>()?,
        ),
        ("create_file", export.register::<()>()?),
        ("create_directory", export.register::<()>()?),
        ("delete_file", export.register::<()>()?),
        ("entry_path", export.register::<String>()?),
        ("reveal_entry", export.register::<()>()?),
        (
            "capture_note",
            export.register::<crate::commands::docs::CreatedNote>()?,
        ),
        (
            "undo_created_note",
            export.register::<crate::commands::docs::UndoResult>()?,
        ),
        (
            "create_note_from_template",
            export.register::<crate::commands::docs::CreatedNote>()?,
        ),
        ("search_docs", export.register::<Vec<(String, String)>>()?),
        ("list_tags", export.register::<Vec<String>>()?),
        (
            "list_templates",
            export.register::<Vec<crate::core::templates::NoteTemplate>>()?,
        ),
        (
            "create_template",
            export.register::<crate::core::templates::NoteTemplate>()?,
        ),
        (
            "update_template",
            export.register::<crate::core::templates::NoteTemplate>()?,
        ),
        ("delete_template", export.register::<()>()?),
        (
            "preview_template",
            export.register::<crate::core::templates::TemplatePreview>()?,
        ),
        (
            "save_template",
            export.register::<crate::commands::templates::SaveTemplateResult>()?,
        ),
        ("discard_template_preview", export.register::<()>()?),
        (
            "preview_knowledge_draft",
            export.register::<crate::core::handoff::KnowledgeDraftPreview>()?,
        ),
        (
            "save_knowledge_draft",
            export.register::<crate::commands::handoff::SaveKnowledgeDraftResult>()?,
        ),
        ("discard_knowledge_draft", export.register::<()>()?),
        (
            "renew_knowledge_draft",
            export.register::<crate::commands::handoff::RenewKnowledgeDraftResult>()?,
        ),
        (
            "preview_rename",
            export.register::<crate::core::rename::RenamePreview>()?,
        ),
        (
            "apply_rename",
            export.register::<crate::core::rename::RenameApplied>()?,
        ),
        ("discard_rename_preview", export.register::<()>()?),
        (
            "save_image_asset",
            export.register::<crate::commands::assets::SavedImageAsset>()?,
        ),
        (
            "shortcut_status",
            export.register::<crate::platform::ShortcutStatus>()?,
        ),
        ("save_note_journal", export.register::<()>()?),
        ("clear_note_journal", export.register::<()>()?),
        (
            "load_note_journal",
            export.register::<crate::core::journal::JournalView>()?,
        ),
        ("discard_other_vault_journal", export.register::<()>()?),
        (
            "open_daily",
            export.register::<crate::commands::daily::DailyOpened>()?,
        ),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_methods_preserve_the_journal_and_host_boundary() {
        let mut names = NOTES_METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 41);
        for method in ["set_root", "open_targets", "open_in", "daily_note"] {
            assert!(!names.contains(&method));
        }
        let call: NotesCall =
            serde_json::from_str(r#"{"method":"read_file","args":{"rel":"a.md"}}"#).unwrap();
        assert_eq!(call.method(), "read_file");
        let call: NotesCall =
            serde_json::from_str(r#"{"method":"discard_other_vault_journal","args":{}}"#).unwrap();
        assert_eq!(call.method(), "discard_other_vault_journal");
        assert!(serde_json::from_str::<NotesCall>(
            r#"{"method":"clear_note_journal","args":{"path":"a","vaultRoot":"other"}}"#
        )
        .is_err());
        for code in [
            "preview_stale",
            "preview_expired",
            "quick_capture_body_required",
            "note_conflict",
        ] {
            assert!(NotesIssue::from_code(code).is_some());
        }
        assert_eq!(classify("private/path/token"), "unavailable");
    }
    #[test]
    fn immediate_creation_retires_preview_commands_without_accepting_target_overrides() {
        for method in [
            "preview_quick_capture",
            "save_quick_capture",
            "discard_quick_capture_preview",
        ] {
            assert!(serde_json::from_value::<NotesCall>(
                serde_json::json!({"method": method, "args": {}})
            )
            .is_err());
        }
        for method in ["preview_daily", "save_daily", "discard_daily"] {
            assert!(serde_json::from_value::<DailyCall>(
                serde_json::json!({"method": method, "args": {"date": "2024-02-29"}})
            )
            .is_err());
        }
        let call: DailyCall = serde_json::from_value(
            serde_json::json!({"method": "open_daily", "args": {"date": "2024-02-29"}}),
        )
        .unwrap();
        assert_eq!(call.method(), "open_daily");
        assert!(serde_json::from_value::<NotesCall>(serde_json::json!({"method": "capture_note", "args": {"input": {"title": "", "body": "body", "tags": [], "path": "outside.md"}}})).is_err());
    }

    #[test]
    fn journal_result_does_not_export_the_storage_vault_root() {
        let cfg = ts_rs::Config::new();
        let entry = <crate::core::journal::JournalEntryView as ts_rs::TS>::decl(&cfg);
        assert!(!entry.contains("vaultRoot"));
        let view = <crate::core::journal::JournalView as ts_rs::TS>::decl(&cfg);
        assert!(view.contains("otherVaultCount"));
    }
}
