//! Native note-template commands.
//!
//! Template management is local SQLite metadata plus an explicit
//! preview-then-save approval.  Preview validates an existing vault and
//! parent directory but never creates a directory or touches the target. Save
//! publishes a flushed sibling without replacing a competing note, then
//! updates the search index in the same bounded transaction pattern as quick
//! capture.

use crate::commands::docs::{resolve_configured_root, AppState};
use crate::core::db;
use crate::core::templates::{
    self, NoteTemplate, TemplateApplyInput, TemplateDraft, TemplateError,
};
use crate::core::vault::{self, EntryIdentity, VaultIdentity};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

const MAX_STAGE_ATTEMPTS: usize = 32;
pub const TEMPLATE_PREVIEW_TTL_MS: u64 = 2 * 60 * 1_000;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct PendingTemplate {
    id: String,
    template_id: i64,
    template_updated_at_ms: i64,
    template_name: String,
    template_content: String,
    target: String,
    content: String,
    vault: VaultIdentity,
    expires_at_ms: u64,
}

struct PendingTemplateInput {
    template_id: i64,
    template_updated_at_ms: i64,
    template_name: String,
    template_content: String,
    target: String,
    content: String,
    vault: VaultIdentity,
}

/// At most one template save approval may be active in a Knowledge process.
/// The generated ID is opaque and the rendered content never crosses the
/// frontend on a save request.
#[derive(Default)]
pub struct TemplatePreviewStore {
    next_id: u64,
    pending: Option<PendingTemplate>,
}

impl TemplatePreviewStore {
    fn issue(&mut self, input: PendingTemplateInput, now_ms: u64) -> Result<String, String> {
        if now_ms == 0 {
            return Err("템플릿 미리보기를 만들 수 없습니다".into());
        }
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = format!("tpl-{}", self.next_id);
        let expires_at_ms = now_ms.saturating_add(TEMPLATE_PREVIEW_TTL_MS);
        self.pending = Some(PendingTemplate {
            id: id.clone(),
            template_id: input.template_id,
            template_updated_at_ms: input.template_updated_at_ms,
            template_name: input.template_name,
            template_content: input.template_content,
            target: input.target,
            content: input.content,
            vault: input.vault,
            expires_at_ms,
        });
        Ok(id)
    }

    fn take(&mut self, id: &str, now_ms: u64) -> Result<PendingTemplate, String> {
        self.expire(now_ms);
        let Some(current) = self.pending.as_ref() else {
            return Err("템플릿 미리보기가 없습니다".into());
        };
        if current.id != id {
            return Err("다른 템플릿 미리보기가 열려 있습니다".into());
        }
        self.pending
            .take()
            .ok_or_else(|| "템플릿 미리보기가 없습니다".to_string())
    }

    fn discard(&mut self, id: &str, now_ms: u64) -> Result<(), String> {
        self.expire(now_ms);
        let Some(current) = self.pending.as_ref() else {
            return Err("템플릿 미리보기가 없습니다".into());
        };
        if current.id != id {
            return Err("다른 템플릿 미리보기가 열려 있습니다".into());
        }
        self.pending = None;
        Ok(())
    }

    fn expire(&mut self, now_ms: u64) {
        if now_ms != 0
            && self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.expires_at_ms <= now_ms)
        {
            self.pending = None;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTemplateResult {
    pub saved: bool,
    pub path: String,
}

#[tauri::command]
pub fn list_templates(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<NoteTemplate>, String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "템플릿을 읽을 수 없습니다".to_string())?;
    let mut statement = connection
        .prepare(
            "SELECT id, name, content, created_ts, updated_ts,
                    length(CAST(name AS BLOB)), length(CAST(content AS BLOB))
             FROM note_templates
             ORDER BY name COLLATE NOCASE, id
             LIMIT ?1",
        )
        .map_err(|_| "템플릿을 읽을 수 없습니다".to_string())?;
    let rows = statement
        .query_map(
            params![templates::MAX_TEMPLATES as i64 + 1],
            template_from_row,
        )
        .map_err(|_| "템플릿을 읽을 수 없습니다".to_string())?;
    let entries = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "템플릿을 읽을 수 없습니다".to_string())?;
    if entries.len() > templates::MAX_TEMPLATES {
        return Err("템플릿 저장소가 개수 제한을 초과했습니다".into());
    }
    let validated = entries
        .into_iter()
        .map(|template| {
            validate_record(&template)?;
            Ok(template)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut names = std::collections::HashSet::with_capacity(validated.len());
    if validated
        .iter()
        .any(|template| !names.insert(template.name.to_ascii_lowercase()))
    {
        return Err("템플릿 저장소에 중복 이름이 있습니다".into());
    }
    Ok(validated)
}

#[tauri::command]
pub fn create_template(
    state: tauri::State<'_, Arc<AppState>>,
    draft: TemplateDraft,
) -> Result<NoteTemplate, String> {
    let draft = normalize_draft(draft);
    templates::validate_draft(&draft).map_err(template_error_message)?;
    let now = current_epoch_ms();
    if now <= 0 {
        return Err("템플릿을 저장할 수 없습니다".into());
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    let count = transaction
        .query_row("SELECT COUNT(*) FROM note_templates", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    if count >= templates::MAX_TEMPLATES as i64 {
        return Err("템플릿 개수가 제한을 초과했습니다".into());
    }
    reject_duplicate_name(&transaction, &draft.name, None)?;
    transaction
        .execute(
            "INSERT INTO note_templates (name, content, created_ts, updated_ts)
             VALUES (?1, ?2, ?3, ?3)",
            params![draft.name, draft.content, now],
        )
        .map_err(|_| "템플릿 이름이 이미 있거나 저장할 수 없습니다".to_string())?;
    let id = transaction.last_insert_rowid();
    let template =
        get_template(&transaction, id)?.ok_or_else(|| "템플릿을 읽을 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    Ok(template)
}

#[tauri::command]
pub fn update_template(
    state: tauri::State<'_, Arc<AppState>>,
    id: i64,
    draft: TemplateDraft,
) -> Result<NoteTemplate, String> {
    if id <= 0 {
        return Err(TemplateError::InvalidId.to_string());
    }
    let draft = normalize_draft(draft);
    templates::validate_draft(&draft).map_err(template_error_message)?;
    let now = current_epoch_ms();
    if now <= 0 {
        return Err("템플릿을 저장할 수 없습니다".into());
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    reject_duplicate_name(&transaction, &draft.name, Some(id))?;
    let changed = transaction
        .execute(
            "UPDATE note_templates SET name = ?1, content = ?2, updated_ts = ?3 WHERE id = ?4",
            params![draft.name, draft.content, now, id],
        )
        .map_err(|_| "템플릿 이름이 이미 있거나 저장할 수 없습니다".to_string())?;
    if changed != 1 {
        return Err(TemplateError::InvalidId.to_string());
    }
    let template =
        get_template(&transaction, id)?.ok_or_else(|| "템플릿을 읽을 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    Ok(template)
}

#[tauri::command]
pub fn delete_template(state: tauri::State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    if id <= 0 {
        return Err(TemplateError::InvalidId.to_string());
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "템플릿을 삭제할 수 없습니다".to_string())?;
    let changed = connection
        .execute("DELETE FROM note_templates WHERE id = ?1", params![id])
        .map_err(|_| "템플릿을 삭제할 수 없습니다".to_string())?;
    if changed == 0 {
        return Err(TemplateError::InvalidId.to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn preview_template(
    state: tauri::State<'_, Arc<AppState>>,
    approval: TemplateApplyInput,
) -> Result<templates::TemplatePreview, String> {
    if approval.template_id <= 0 {
        return Err(TemplateError::InvalidId.to_string());
    }
    let (template, vault) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "템플릿 미리보기를 만들 수 없습니다".to_string())?;
        let template = get_template(&connection, approval.template_id)?
            .ok_or_else(|| TemplateError::InvalidId.to_string())?;
        let root = resolve_configured_root(&connection)
            .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
        let vault = VaultIdentity::inspect(&root)
            .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
        (template, vault)
    };
    let content = templates::render(template.id, &template.content, &approval)
        .map_err(template_error_message)?;
    let target = vault
        .new_entry(&approval.target)
        .map_err(|error| error.to_string())?;
    let parent = target
        .parent()
        .ok_or_else(|| TemplateError::InvalidTarget.to_string())?;
    vault
        .lease_existing_directory(parent)
        .map_err(|_| "템플릿 저장 폴더를 확인할 수 없습니다".to_string())?;
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err("같은 경로의 파일은 덮어쓸 수 없습니다".into());
    }
    let preview_id = state
        .template_previews
        .lock()
        .map_err(|_| "템플릿 미리보기를 만들 수 없습니다".to_string())?
        .issue(
            PendingTemplateInput {
                template_id: template.id,
                template_updated_at_ms: template.updated_at_ms,
                template_name: template.name.clone(),
                template_content: template.content.clone(),
                target: approval.target.clone(),
                content: content.clone(),
                vault,
            },
            u64::try_from(current_epoch_ms()).unwrap_or(0),
        )?;
    Ok(templates::TemplatePreview {
        preview_id,
        template_id: template.id,
        template_updated_at_ms: template.updated_at_ms,
        target: approval.target,
        byte_length: content.len(),
        content,
    })
}

#[tauri::command]
pub fn discard_template_preview(
    state: tauri::State<'_, Arc<AppState>>,
    preview_id: String,
) -> Result<(), String> {
    let now_ms = u64::try_from(current_epoch_ms()).unwrap_or(0);
    if now_ms == 0 {
        return Err("템플릿 미리보기를 취소할 수 없습니다".into());
    }
    state
        .template_previews
        .lock()
        .map_err(|_| "템플릿 미리보기를 취소할 수 없습니다".to_string())?
        .discard(&preview_id, now_ms)
}

#[tauri::command]
pub fn save_template(
    state: tauri::State<'_, Arc<AppState>>,
    preview_id: String,
) -> Result<SaveTemplateResult, String> {
    let now_ms = u64::try_from(current_epoch_ms()).unwrap_or(0);
    if now_ms == 0 {
        return Err("템플릿 미리보기가 오래되어 다시 확인하세요".into());
    }
    let pending = state
        .template_previews
        .lock()
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?
        .take(&preview_id, now_ms)?;
    let vault = VaultIdentity::inspect(pending.vault.canonical_path())
        .map_err(|_| "템플릿 미리보기가 오래되어 다시 확인하세요".to_string())?;
    if vault != pending.vault {
        return Err("템플릿 미리보기가 오래되어 다시 확인하세요".into());
    }
    let target = vault
        .new_entry(&pending.target)
        .map_err(|_| "템플릿 저장 경로가 올바르지 않습니다".to_string())?;
    let parent = target
        .parent()
        .ok_or_else(|| "템플릿 저장 경로가 올바르지 않습니다".to_string())?;
    let _parent_lease = vault
        .lease_existing_directory(parent)
        .map_err(|_| "템플릿 저장 폴더를 확인할 수 없습니다".to_string())?;
    if std::fs::symlink_metadata(&target).is_ok() {
        return Err("같은 경로의 파일은 덮어쓸 수 없습니다".into());
    }
    let (temporary, identity) = stage_template_file(&vault, &pending.target, &pending.content)
        .map_err(|_| "템플릿을 저장할 수 없습니다".to_string())?;
    // Hold the DB writer lock from the revision check through publication and
    // index commit. A template update can therefore not land between the
    // final revision check and the file/index mutation.
    let connection = match state.db.lock() {
        Ok(connection) => connection,
        Err(_) => {
            vault::cleanup_file_by_identity(&temporary, &identity);
            return Err("템플릿을 저장할 수 없습니다".into());
        }
    };
    if !template_revision_matches(&connection, &pending) {
        drop(connection);
        vault::cleanup_file_by_identity(&temporary, &identity);
        return Err("템플릿 미리보기가 오래되어 다시 확인하세요".into());
    }
    if vault.revalidate().is_err() {
        drop(connection);
        vault::cleanup_file_by_identity(&temporary, &identity);
        return Err("템플릿 미리보기가 오래되어 다시 확인하세요".into());
    }
    if let Err(error) = vault::publish_new_file(&temporary, &target) {
        drop(connection);
        vault::cleanup_file_by_identity(&temporary, &identity);
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err("같은 경로의 파일은 덮어쓸 수 없습니다".into());
        }
        return Err("템플릿을 저장할 수 없습니다".into());
    }
    if vault.revalidate().is_err() {
        drop(connection);
        vault::cleanup_file_by_identity(&target, &identity);
        return Err("템플릿 미리보기가 오래되어 다시 확인하세요".into());
    }
    let transaction = match connection.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(_) => {
            vault::cleanup_file(&vault, &target, &identity);
            return Err("템플릿 검색 인덱스를 갱신하지 못했습니다".into());
        }
    };
    if db::index_doc_in_transaction(&transaction, &pending.target, &pending.content).is_err()
        || transaction.commit().is_err()
    {
        vault::cleanup_file(&vault, &target, &identity);
        return Err("템플릿 검색 인덱스를 갱신하지 못했습니다".into());
    }
    drop(connection);
    if let Ok(connection) = state.db.lock() {
        let _ = crate::integration::write_snapshot(&connection);
    }
    Ok(SaveTemplateResult {
        saved: true,
        path: pending.target,
    })
}

fn template_revision_matches(connection: &Connection, pending: &PendingTemplate) -> bool {
    connection
        .query_row(
            "SELECT name = ?2 AND content = ?3 AND updated_ts = ?4
             FROM note_templates WHERE id = ?1",
            params![
                pending.template_id,
                pending.template_name,
                pending.template_content,
                pending.template_updated_at_ms,
            ],
            |row| row.get::<_, bool>(0),
        )
        .optional()
        .ok()
        .flatten()
        .unwrap_or(false)
}

fn template_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NoteTemplate> {
    let name_bytes = row.get::<_, i64>(5)?;
    let content_bytes = row.get::<_, i64>(6)?;
    if !(0..=templates::MAX_TEMPLATE_NAME_BYTES as i64).contains(&name_bytes)
        || !(0..=templates::MAX_TEMPLATE_CONTENT_BYTES as i64).contains(&content_bytes)
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(NoteTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        content: row.get(2)?,
        created_at_ms: row.get(3)?,
        updated_at_ms: row.get(4)?,
    })
}

fn get_template(connection: &Connection, id: i64) -> Result<Option<NoteTemplate>, String> {
    let template = connection
        .query_row(
            "SELECT id, name, content, created_ts, updated_ts,
                    length(CAST(name AS BLOB)), length(CAST(content AS BLOB))
             FROM note_templates WHERE id = ?1",
            params![id],
            template_from_row,
        )
        .optional()
        .map_err(|_| "템플릿을 읽을 수 없습니다".to_string())?;
    template
        .map(|template| {
            validate_record(&template)?;
            Ok(template)
        })
        .transpose()
}

fn normalize_draft(draft: TemplateDraft) -> TemplateDraft {
    TemplateDraft {
        name: draft.name.trim().to_owned(),
        content: draft.content,
    }
}

fn reject_duplicate_name(
    connection: &Connection,
    name: &str,
    except_id: Option<i64>,
) -> Result<(), String> {
    let duplicate = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM note_templates
               WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id <> ?2)
             )",
            params![name, except_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| "템플릿 이름을 확인할 수 없습니다".to_string())?;
    if duplicate {
        Err("템플릿 이름이 이미 있습니다".into())
    } else {
        Ok(())
    }
}

fn validate_record(template: &NoteTemplate) -> Result<(), String> {
    if template.id <= 0
        || template.created_at_ms <= 0
        || template.updated_at_ms < template.created_at_ms
    {
        return Err("템플릿을 읽을 수 없습니다".into());
    }
    templates::validate_draft(&TemplateDraft {
        name: template.name.clone(),
        content: template.content.clone(),
    })
    .map_err(template_error_message)
}

fn template_error_message(error: TemplateError) -> String {
    error.to_string()
}

fn current_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn stage_template_file(
    vault: &VaultIdentity,
    target: &str,
    content: &str,
) -> Result<(PathBuf, EntryIdentity), std::io::Error> {
    let parent = target
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    let filename = target.rsplit('/').next().unwrap_or("note.md");
    for _ in 0..MAX_STAGE_ATTEMPTS {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary_name = format!(
            ".{filename}.devbox-template-{}-{sequence}.tmp",
            std::process::id()
        );
        let relative = if parent.is_empty() {
            temporary_name.clone()
        } else {
            format!("{parent}/{temporary_name}")
        };
        let temporary = vault
            .new_entry(&relative)
            .map_err(|_| std::io::Error::other("vault changed"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let identity = file
            .metadata()
            .map(|metadata| VaultIdentity::entry_identity_from_metadata(&temporary, &metadata))?;
        let write_result = (|| {
            file.write_all(content.as_bytes())?;
            file.flush()?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            drop(file);
            vault::cleanup_file_by_identity(&temporary, &identity);
            return Err(error);
        }
        drop(file);
        if vault
            .existing_file_identity(&temporary)
            .ok()
            .is_none_or(|current| !identity.matches(&current))
        {
            vault::cleanup_file_by_identity(&temporary, &identity);
            return Err(std::io::Error::other("template staging changed"));
        }
        return Ok((temporary, identity));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many template staging collisions",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn vault() -> (PathBuf, VaultIdentity) {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "devbox-knowledge-template-preview-{}-{sequence}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let identity = VaultIdentity::inspect(&path).unwrap();
        (path, identity)
    }

    fn pending_input(vault: VaultIdentity) -> PendingTemplateInput {
        PendingTemplateInput {
            template_id: 1,
            template_updated_at_ms: 1,
            template_name: "Daily".into(),
            template_content: "body".into(),
            target: "Notes/example.md".into(),
            content: "body".into(),
            vault,
        }
    }

    #[test]
    fn preview_is_one_shot_and_expires_at_the_ttl_boundary() {
        let (path, identity) = vault();
        let mut store = TemplatePreviewStore::default();
        assert!(store.issue(pending_input(identity.clone()), 0).is_err());
        let id = store.issue(pending_input(identity), 100).unwrap();
        assert!(store.take(&id, 100 + TEMPLATE_PREVIEW_TTL_MS).is_err());

        let identity = VaultIdentity::inspect(&path).unwrap();
        let id = store.issue(pending_input(identity), 200).unwrap();
        store.discard(&id, 200).unwrap();
        assert!(store.take(&id, 201).is_err());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn preview_revision_rejects_a_changed_template_definition() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO note_templates (id, name, content, created_ts, updated_ts)
                 VALUES (1, 'Daily', '# {{title}}', 1, 10)",
                [],
            )
            .unwrap();
        let (_path, identity) = vault();
        let pending = PendingTemplate {
            id: "tpl-1".into(),
            template_id: 1,
            template_updated_at_ms: 10,
            template_name: "Daily".into(),
            template_content: "# {{title}}".into(),
            target: "Notes/today.md".into(),
            content: "# Today".into(),
            vault: identity,
            expires_at_ms: 20,
        };
        assert!(template_revision_matches(&connection, &pending));
        connection
            .execute(
                "UPDATE note_templates SET content = '# {{date}}', updated_ts = 11 WHERE id = 1",
                [],
            )
            .unwrap();
        assert!(!template_revision_matches(&connection, &pending));
    }

    #[test]
    fn template_names_are_trimmed_and_case_insensitive_for_duplicates() {
        let mut connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        let normalized = normalize_draft(TemplateDraft {
            name: "  Daily  ".into(),
            content: "body".into(),
        });
        assert_eq!(normalized.name, "Daily");
        connection
            .execute(
                "INSERT INTO note_templates (id, name, content, created_ts, updated_ts)
                 VALUES (1, 'Daily', 'body', 1, 1)",
                [],
            )
            .unwrap();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(reject_duplicate_name(&transaction, "daily", None).is_err());
        assert!(reject_duplicate_name(&transaction, "DAILY", Some(1)).is_ok());
    }

    #[test]
    fn corrupt_oversized_template_is_rejected_before_string_projection() {
        let connection = Connection::open_in_memory().unwrap();
        db::migrate(&connection).unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        connection
            .execute(
                "INSERT INTO note_templates (id, name, content, created_ts, updated_ts)
                 VALUES (1, 'Daily', ?1, 1, 1)",
                params!["x".repeat(templates::MAX_TEMPLATE_CONTENT_BYTES + 1)],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF;")
            .unwrap();
        assert!(get_template(&connection, 1).is_err());
    }
}
