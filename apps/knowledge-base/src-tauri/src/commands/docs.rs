use crate::core::capture::{self, QuickCaptureApproval, QuickCaptureInput};
use crate::core::db;
use crate::core::store;
use crate::core::vault::{EntryIdentity, VaultIdentity};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::core::entry_actions::{
    canonical_existing_entry, prepare_open_request, select_open_targets, validated_new_entry,
    KnowledgeOpenTarget,
};

/// 앱 전역 상태
pub struct AppState {
    pub db: Mutex<Connection>,
    pub rename_plans: Mutex<crate::core::rename::RenamePlanStore>,
    pub quick_capture_previews: Mutex<QuickCapturePreviewStore>,
    /// 렌더 프리뷰용 이미지 인라인 캐시: (경로, mtime)이 같으면 base64 재인코딩을
    /// 건너뛴다. 항목 32개를 넘기면 통째로 비운다(LRU까지 갈 필요 없음).
    pub image_cache: Mutex<HashMap<PathBuf, (SystemTime, u64, EntryIdentity, String)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboundNote {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCapturePreview {
    pub preview_id: String,
    pub target: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCaptureSaved {
    /// Root-relative only.  The absolute Knowledge path never crosses IPC.
    pub path: String,
}

struct PendingQuickCapture {
    id: String,
    vault: VaultIdentity,
    capture: capture::NormalizedCapture,
}

/// App-managed one-shot slot for the edit → preview → save approval.
///
/// The normalized body/title/tags never live in a serialized plan or a
/// frontend save request.  Issuing a new preview replaces the previous slot;
/// a save/discard attempt consumes only the matching opaque ID.
#[derive(Default)]
pub struct QuickCapturePreviewStore {
    next_id: u64,
    pending: Option<PendingQuickCapture>,
}

impl QuickCapturePreviewStore {
    fn issue(&mut self, vault: VaultIdentity, capture: capture::NormalizedCapture) -> String {
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = format!("qc-{}", self.next_id);
        self.pending = Some(PendingQuickCapture {
            id: id.clone(),
            vault,
            capture,
        });
        id
    }

    fn take(&mut self, id: &str) -> Option<PendingQuickCapture> {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending.take()
        } else {
            None
        }
    }

    fn discard(&mut self, id: &str) {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.id == id)
        {
            self.pending = None;
        }
    }
}

/// KnowledgeRoot 경로를 반환한다. 미설정이면 Documents/Knowledge로 초기화.
///
/// `commands::markdown`도 이미지·링크 해석을 위해 루트 경로가 필요해 `pub(crate)`로
/// 공개한다.
pub(crate) fn resolve_root(conn: &Connection) -> Result<PathBuf, String> {
    if let Some(root) = db::get_setting(conn, "root").map_err(|e| e.to_string())? {
        return Ok(PathBuf::from(root));
    }
    let default = dirs::document_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Knowledge");
    store::ensure_layout(&default)?;
    db::set_setting(conn, "root", &default.to_string_lossy()).map_err(|e| e.to_string())?;
    Ok(default)
}

/// Read the already-selected root without initializing the default layout.
/// Quick-capture preview, explicit image writes, and external handoff preview
/// must not initialize `Documents/Knowledge` or mutate settings as a side
/// effect of a read-only request.
pub(crate) fn resolve_configured_root(conn: &Connection) -> Result<PathBuf, String> {
    db::get_setting(conn, "root")
        .map_err(|_| "빠른 캡처 미리보기를 만들 수 없습니다".to_string())?
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "빠른 캡처 미리보기를 만들 수 없습니다".to_string())
}

/// schema v1 최초 실행에만 원문 Markdown에서 정확한 source line을 재구축한다.
/// 읽을 수 없거나 root 밖으로 canonicalize되는 항목은 index 대상에서 제외한다.
pub(crate) fn rebuild_wikilink_index_if_needed(
    conn: &Connection,
    root: &Path,
) -> Result<(), String> {
    if !db::wikilink_index_needs_rebuild(conn)
        .map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())?
    {
        return Ok(());
    }
    let entries =
        store::tree(root).map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())?;
    let docs = entries
        .into_iter()
        .filter(|(path, is_dir)| {
            !*is_dir
                && Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .filter_map(|(path, _)| crate::core::inbound::read_note(root, &path).ok())
        .collect::<Vec<_>>();
    db::rebuild_wikilink_index(conn, &docs)
        .map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())
}

#[tauri::command]
pub fn get_root(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let conn = state.db.lock().unwrap();
    Ok(resolve_root(&conn)?.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn set_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = Path::new(&path);
    crate::core::vault::validate_root_for_creation(root)
        .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
    store::ensure_layout(root)?;
    VaultIdentity::inspect(root)
        .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
    db::set_setting(&conn, "root", &path).map_err(|e| e.to_string())?;
    drop(conn);
    state.rename_plans.lock().unwrap().clear();
    // watcher를 새 루트로 재시작
    let watcher = app.state::<Arc<crate::commands::watcher::KnowledgeWatcher>>();
    watcher.set_root(Path::new(&path))
}

#[tauri::command]
pub fn list_tree(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<TreeEntry>, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let entries = store::tree(&root)?;
    Ok(entries
        .into_iter()
        .map(|(path, is_dir)| TreeEntry { path, is_dir })
        .collect())
}

#[tauri::command]
pub fn read_file(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<String, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    store::read_file(&path)
}

/// Untrusted applink `Path`를 현재 Knowledge root 안의 실제 Markdown note로
/// 해석하고 같은 canonical target에서 bounded read까지 수행한다. 실패 메시지는
/// 요청 경로나 OS 오류를 반향하지 않는다.
#[tauri::command]
pub fn open_inbound_note(
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<InboundNote, String> {
    let root = {
        let conn = state.db.lock().unwrap();
        resolve_root(&conn).map_err(|_| "요청한 노트를 열 수 없습니다".to_string())?
    };
    let (path, content) = crate::core::inbound::read_note(&root, &path).map_err(str::to_string)?;
    Ok(InboundNote { path, content })
}

#[tauri::command]
pub fn write_file(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: String,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    store::write_file(&path, &content)?;
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
    drop(conn);
    // integration snapshot 갱신 (best-effort — 실패해도 저장은 유지)
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

#[tauri::command]
pub fn create_file(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = validated_new_entry(&root, &rel).map_err(str::to_string)?;
    if path.exists() {
        return Err("파일이 이미 존재합니다".into());
    }
    let content = content.unwrap_or_default();
    store::write_file(&path, &content)?;
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
    drop(conn);
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

#[tauri::command]
pub fn create_directory(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = validated_new_entry(&root, &rel).map_err(str::to_string)?;
    if path.exists() {
        return Err("폴더가 이미 존재합니다".into());
    }
    std::fs::create_dir_all(path).map_err(|_| "폴더를 만들 수 없습니다".to_string())
}

#[tauri::command]
pub fn delete_file(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<(), String> {
    let mut conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = canonical_existing_entry(&root, &rel).map_err(str::to_string)?;
    let transaction = conn
        .transaction()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    db::remove_docs_under(&transaction, &rel)
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    store::delete_file(&path).map_err(|_| "항목을 삭제할 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    drop(conn);
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

fn available_open_targets() -> Vec<KnowledgeOpenTarget> {
    select_open_targets("knowledge-base", devbox_launch::installed_targets("path"))
}

fn resolve_entry_for_action(
    state: &tauri::State<'_, Arc<AppState>>,
    rel: &str,
) -> Result<PathBuf, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn).map_err(|_| "Knowledge 루트를 열 수 없습니다".to_string())?;
    canonical_existing_entry(&root, rel).map_err(str::to_string)
}

/// 사용자가 명시적으로 Copy path를 선택했을 때만 absolute path를 frontend에 반환한다.
#[tauri::command]
pub fn entry_path(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<String, String> {
    let path = resolve_entry_for_action(&state, &rel)?;
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| "Knowledge 항목 경로가 올바르지 않습니다".to_string())
}

#[tauri::command]
pub fn reveal_entry(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
) -> Result<(), String> {
    let path = resolve_entry_for_action(&state, &rel)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| "탐색기에서 항목을 표시할 수 없습니다".to_string())
}

/// Catalog capability와 실제 설치 executable의 교집합만 공개한다. executable
/// 경로는 frontend에 보내지 않는다.
#[tauri::command]
pub fn open_targets() -> Vec<KnowledgeOpenTarget> {
    available_open_targets()
}

#[tauri::command]
pub fn open_in(
    state: tauri::State<'_, Arc<AppState>>,
    app_id: String,
    rel: String,
) -> Result<(), String> {
    let entry = resolve_entry_for_action(&state, &rel)?;
    let targets = available_open_targets();
    let (target_id, request) =
        prepare_open_request(&targets, &app_id, &entry).map_err(str::to_string)?;
    devbox_launch::launch_open(&target_id, &request).map(|_| ())
}

fn validate_capture_inbox(vault: &VaultIdentity) -> Result<PathBuf, String> {
    // Validate the fixed destination even during preview.  This means a
    // misconfigured root or a symlinked Inbox cannot appear selectable in UI.
    vault.revalidate().map_err(|error| error.to_string())?;
    let inbox = vault
        .new_entry(capture::INBOX_DIR)
        .map_err(|error| error.to_string())?;
    match std::fs::symlink_metadata(&inbox) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(inbox),
        Ok(_) => Err("빠른 캡처 저장 위치를 사용할 수 없습니다".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(inbox),
        Err(_) => Err("빠른 캡처 저장 위치를 사용할 수 없습니다".to_string()),
    }
}

/// Create only the fixed one-level capture directory after the same canonical
/// root/ancestor checks used by preview.  A recursive create here would make a
/// future path change accidentally become a generic writer and would also
/// widen the symlink race window.
fn ensure_capture_inbox(vault: &VaultIdentity) -> Result<PathBuf, String> {
    vault.revalidate().map_err(|error| error.to_string())?;
    let inbox = validate_capture_inbox(vault)?;
    if matches!(
        std::fs::symlink_metadata(&inbox),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    ) {
        match std::fs::create_dir(&inbox) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("빠른 캡처를 저장하지 못했습니다".to_string()),
        }
    }
    validate_capture_inbox(vault)
}

#[cfg(test)]
fn capture_preview(
    vault: &VaultIdentity,
    input: QuickCaptureInput,
    preview_id: String,
) -> Result<QuickCapturePreview, String> {
    let normalized = capture::normalize(input).map_err(|error| error.to_string())?;
    validate_capture_inbox(vault)?;
    Ok(QuickCapturePreview {
        preview_id,
        target: capture::INBOX_DIR.to_string(),
        title: normalized.title,
        body: normalized.body,
        tags: normalized.tags,
    })
}

#[tauri::command]
pub fn preview_quick_capture(
    state: tauri::State<'_, Arc<AppState>>,
    input: QuickCaptureInput,
) -> Result<QuickCapturePreview, String> {
    // Do not hold the DB mutex while taking the preview-slot mutex. Save takes
    // the slot first and then the DB, so keeping one lock order avoids a
    // concurrent preview/save deadlock.
    let root = {
        let conn = state
            .db
            .lock()
            .map_err(|_| "빠른 캡처 미리보기를 만들 수 없습니다".to_string())?;
        resolve_configured_root(&conn)?
    };
    let vault = VaultIdentity::inspect(&root).map_err(|error| error.to_string())?;
    let normalized = capture::normalize(input).map_err(|error| error.to_string())?;
    validate_capture_inbox(&vault)?;
    let mut previews = state
        .quick_capture_previews
        .lock()
        .map_err(|_| "빠른 캡처 미리보기를 만들 수 없습니다".to_string())?;
    let preview_id = previews.issue(vault, normalized.clone());
    Ok(QuickCapturePreview {
        preview_id,
        target: capture::INBOX_DIR.to_string(),
        title: normalized.title,
        body: normalized.body,
        tags: normalized.tags,
    })
}

fn stage_capture_file(
    vault: &VaultIdentity,
    inbox: &Path,
    filename: &str,
    content: &[u8],
) -> Result<(PathBuf, EntryIdentity), std::io::Error> {
    const MAX_STAGE_ATTEMPTS: u32 = 32;
    for attempt in 0..MAX_STAGE_ATTEMPTS {
        let temporary_name = format!(".{filename}.{}.{}.tmp", std::process::id(), attempt);
        let temporary_rel = format!("{}/{temporary_name}", capture::INBOX_DIR);
        let temporary = vault
            .new_entry(&temporary_rel)
            .map_err(|_| std::io::Error::other("vault changed"))?;
        if temporary.parent() != Some(inbox) {
            return Err(std::io::Error::other("capture staging boundary"));
        }
        vault
            .revalidate()
            .map_err(|_| std::io::Error::other("vault changed"))?;
        let open = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match open {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let identity = file
            .metadata()
            .map(|metadata| VaultIdentity::entry_identity_from_metadata(&temporary, &metadata))?;
        let opened_inside_vault = vault
            .existing_file_identity(&temporary)
            .is_ok_and(|current| identity.matches(&current));
        if !opened_inside_vault {
            drop(file);
            cleanup_vault_file(vault, &temporary, &identity);
            return Err(std::io::Error::other("vault changed"));
        }
        let result = (|| {
            file.write_all(content)?;
            file.flush()?;
            file.sync_all()
        })();
        if let Err(error) = result {
            drop(file);
            cleanup_vault_file(vault, &temporary, &identity);
            return Err(error);
        }
        let current_identity = match vault.existing_file_identity(&temporary) {
            Ok(current_identity) => current_identity,
            Err(_) => {
                drop(file);
                cleanup_vault_file(vault, &temporary, &identity);
                return Err(std::io::Error::other("vault changed"));
            }
        };
        if !identity.matches(&current_identity) {
            drop(file);
            cleanup_vault_file(vault, &temporary, &identity);
            return Err(std::io::Error::other("capture staging replaced"));
        }
        drop(file);
        return Ok((temporary, identity));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many temporary capture collisions",
    ))
}

/// Publish a fully flushed file without replacing an existing capture.
///
/// Windows uses no-replace MoveFileEx, while Unix uses a same-directory
/// hard-link publication because `rename` would replace a target if a
/// competing writer wins the race. The temporary path is never returned to
/// the frontend.
pub(crate) fn publish_new_vault_file(temporary: &Path, path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        std::fs::hard_link(temporary, path)?;
        std::fs::remove_file(temporary)?;
        sync_vault_directory(path.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, WIN32_ERROR,
        };
        use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

        let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // Deliberately omit MOVEFILE_REPLACE_EXISTING.  A competing writer
        // therefore gets an AlreadyExists-style failure and the caller tries
        // the next bounded collision ordinal.
        let result = unsafe {
            MoveFileExW(
                PCWSTR(temporary.as_ptr()),
                PCWSTR(path.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        match result {
            Ok(()) => Ok(()),
            Err(error)
                if WIN32_ERROR::from_error(&error).is_some_and(|code| {
                    code == ERROR_ACCESS_DENIED
                        || code == ERROR_ALREADY_EXISTS
                        || code == ERROR_FILE_EXISTS
                }) =>
            {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "capture target already exists",
                ))
            }
            Err(error) => Err(std::io::Error::other(error)),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::fs::rename(temporary, path)
    }
}

/// Remove a capture artifact only while the original vault and every existing
/// ancestor still resolve to the same object.  If the root was replaced or an
/// Inbox component became a link/reparse point, leaving an orphan is safer
/// than following a changed path and deleting someone else's file.
pub(crate) fn cleanup_vault_file(vault: &VaultIdentity, path: &Path, expected: &EntryIdentity) {
    if vault.revalidate().is_err() {
        return;
    }
    let Ok(current) = vault.existing_file_identity(path) else {
        return;
    };
    if expected.matches(&current) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
fn sync_vault_directory(path: &Path) -> Result<(), std::io::Error> {
    File::open(path)?.sync_all()
}

#[cfg(test)]
fn save_capture_in_root(
    conn: &Connection,
    root: &Path,
    input: QuickCaptureInput,
) -> Result<QuickCaptureSaved, String> {
    let vault = VaultIdentity::inspect(root).map_err(|error| error.to_string())?;
    save_capture_at(conn, &vault, input, current_epoch_seconds())
}

#[cfg(test)]
fn save_capture_at(
    conn: &Connection,
    vault: &VaultIdentity,
    input: QuickCaptureInput,
    now_seconds: i64,
) -> Result<QuickCaptureSaved, String> {
    let normalized = capture::normalize(input).map_err(|error| error.to_string())?;
    save_normalized_capture_at(conn, vault, normalized, now_seconds)
}

fn save_normalized_capture_at(
    conn: &Connection,
    vault: &VaultIdentity,
    normalized: capture::NormalizedCapture,
    now_seconds: i64,
) -> Result<QuickCaptureSaved, String> {
    let document = capture::render_markdown(&normalized).map_err(|error| error.to_string())?;
    let inbox = ensure_capture_inbox(vault)?;

    let mut selected: Option<(String, PathBuf, EntryIdentity)> = None;
    for ordinal in 1..=capture::MAX_COLLISION_ATTEMPTS {
        vault.revalidate().map_err(|error| error.to_string())?;
        let filename = capture::filename_for_timestamp(now_seconds, ordinal);
        let rel = format!("{}/{}", capture::INBOX_DIR, filename);
        let path = vault.new_entry(&rel).map_err(|error| error.to_string())?;
        debug_assert_eq!(path.parent(), Some(inbox.as_path()));
        let (temporary, temporary_identity) =
            match stage_capture_file(vault, &inbox, &filename, document.as_bytes()) {
                Ok(temporary) => temporary,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("빠른 캡처를 저장하지 못했습니다".to_string()),
            };
        // Re-check the destination parent after opening/writing the sibling.
        // A concurrent Inbox replacement must not turn the publication path
        // into a link/reparse escape between the two operations.
        let current_path = match vault.new_entry(&rel) {
            Ok(current_path) if current_path == path => current_path,
            Ok(_) => {
                cleanup_vault_file(vault, &temporary, &temporary_identity);
                return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
            }
            Err(error) => {
                cleanup_vault_file(vault, &temporary, &temporary_identity);
                return Err(error.to_string());
            }
        };
        let publication = if vault.revalidate().is_ok() {
            publish_new_vault_file(&temporary, &current_path)
        } else {
            return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
        };
        match publication {
            Ok(()) => {
                // A competing rename/reparse replacement can occur between
                // the no-replace publication and index transaction. Reject a
                // non-regular target before exposing it through the index.
                let target_is_regular = match vault.existing_path(&current_path) {
                    Ok(path) if path == current_path => {
                        vault.existing_file_identity(&current_path).ok()
                    }
                    Err(_) => None,
                    Ok(_) => None,
                };
                let Some(target_identity) = target_is_regular else {
                    return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
                };
                if !target_identity.matches(&temporary_identity) {
                    // The target was replaced after publication. Do not index
                    // or remove the competing regular file by path.
                    return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
                }
                selected = Some((rel, path, target_identity));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                cleanup_vault_file(vault, &temporary, &temporary_identity);
                continue;
            }
            Err(_) => {
                // Publication may already have made the target visible before
                // reporting an ambiguous cleanup/durability error (for
                // example, after Unix hard-link succeeds).  Only our
                // temporary has an identity we can safely clean up here;
                // never delete the target by path because a competing writer
                // may have replaced it in the meantime.
                cleanup_vault_file(vault, &temporary, &temporary_identity);
                return Err("빠른 캡처를 저장하지 못했습니다".to_string());
            }
        }
    }
    let Some((rel, path, path_identity)) = selected else {
        return Err("빠른 캡처를 저장하지 못했습니다".to_string());
    };

    if let Err(error) = vault.revalidate() {
        // Do not follow a path after the vault identity changed.  The file is
        // intentionally left for a later bounded cleanup/reconcile pass.
        return Err(error.to_string());
    }

    // Keep the file and SQLite index in one bounded operation.  Any index
    // failure removes only the newly-created file and rolls the transaction
    // back, so a failed capture does not leave a half-visible note.
    let transaction = match conn.unchecked_transaction() {
        Ok(transaction) => transaction,
        Err(_) => {
            cleanup_vault_file(vault, &path, &path_identity);
            return Err("빠른 캡처를 저장하지 못했습니다".to_string());
        }
    };
    if let Err(error) = db::index_doc_in_transaction(&transaction, &rel, &document)
        .map_err(|_| "빠른 캡처를 저장하지 못했습니다".to_string())
    {
        let _ = transaction.rollback();
        cleanup_vault_file(vault, &path, &path_identity);
        return Err(error);
    }
    if transaction.commit().is_err() {
        cleanup_vault_file(vault, &path, &path_identity);
        return Err("빠른 캡처를 저장하지 못했습니다".to_string());
    }
    // The transaction and file publication are separate OS operations. If the
    // vault identity changed while SQLite committed, report a stale approval
    // instead of claiming that the current replacement vault contains the
    // capture. The old artifact is left for bounded reconciliation.
    if let Err(error) = vault.revalidate() {
        return Err(error.to_string());
    }
    Ok(QuickCaptureSaved { path: rel })
}

fn current_epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn save_quick_capture(
    state: tauri::State<'_, Arc<AppState>>,
    approval: QuickCaptureApproval,
) -> Result<QuickCaptureSaved, String> {
    if !capture::is_valid_preview_id(&approval.preview_id) {
        return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
    }

    // Consume before doing filesystem work.  A timeout, duplicate click, or
    // stale caller cannot replay the same approved body.  The UI must create a
    // fresh preview after any failed save attempt.
    let pending = state
        .quick_capture_previews
        .lock()
        .map_err(|_| "빠른 캡처를 저장하지 못했습니다".to_string())?
        .take(&approval.preview_id)
        .ok_or_else(|| "빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string())?;

    let conn = state
        .db
        .lock()
        .map_err(|_| "빠른 캡처를 저장하지 못했습니다".to_string())?;
    let root = resolve_configured_root(&conn)
        .map_err(|_| "빠른 캡처를 저장하지 못했습니다".to_string())?;
    let current_vault = VaultIdentity::inspect(&root).map_err(|error| error.to_string())?;
    if current_vault != pending.vault {
        return Err("빠른 캡처 미리보기가 오래되어 다시 확인하세요".to_string());
    }
    let result = save_normalized_capture_at(
        &conn,
        &current_vault,
        pending.capture,
        current_epoch_seconds(),
    )?;
    drop(conn);
    // The snapshot contains counts and opaque IDs only; never capture content.
    if let Ok(conn) = state.db.lock() {
        let _ = crate::integration::write_snapshot(&conn);
    }
    Ok(result)
}

#[tauri::command]
pub fn discard_quick_capture_preview(
    state: tauri::State<'_, Arc<AppState>>,
    approval: QuickCaptureApproval,
) -> Result<(), String> {
    if !capture::is_valid_preview_id(&approval.preview_id) {
        return Ok(());
    }
    state
        .quick_capture_previews
        .lock()
        .map_err(|_| "빠른 캡처 미리보기를 폐기하지 못했습니다".to_string())?
        .discard(&approval.preview_id);
    Ok(())
}

#[tauri::command]
pub fn search_docs(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<(String, String)>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let conn = state.db.lock().unwrap();
    db::search(&conn, q, 100).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tags(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let conn = state.db.lock().unwrap();
    db::list_tags(&conn).map_err(|e| e.to_string())
}

/// 오늘 날짜 데일리 노트를 생성/열기. (경로, 내용)을 반환한다.
#[tauri::command]
pub fn daily_note(state: tauri::State<'_, Arc<AppState>>) -> Result<(String, String), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let rel = format!("Journal/{}.md", today_str());
    let path = store::safe_join(&root, &rel)?;
    if !path.exists() {
        let content = format!("---\ntags: [daily]\n---\n\n# {}\n\n", today_str());
        store::write_file(&path, &content)?;
        db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
        drop(conn);
        let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
        return Ok((rel, content));
    }
    let content = store::read_file(&path)?;
    Ok((rel, content))
}

fn today_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant의 civil_from_days 알고리즘 (epoch 1970-01-01)
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_input(body: &str) -> QuickCaptureInput {
        QuickCaptureInput {
            title: "Captured idea".to_string(),
            body: body.to_string(),
            tags: vec!["rust".to_string(), "offline".to_string()],
        }
    }

    #[test]
    fn handoff_root_resolution_does_not_create_or_mutate_default_root() {
        let database = tempfile::tempdir().unwrap();
        let connection = db::init(&database.path().join("data.db")).unwrap();
        assert!(resolve_configured_root(&connection).is_err());
        assert_eq!(db::get_setting(&connection, "root").unwrap(), None);

        let configured = tempfile::tempdir().unwrap();
        db::set_setting(&connection, "root", &configured.path().to_string_lossy()).unwrap();
        assert_eq!(
            resolve_configured_root(&connection).unwrap(),
            configured.path()
        );
    }

    #[test]
    fn civil_date_for_epoch() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_date_known() {
        // 2026-08-11 = epoch days
        let days = 20_676;
        assert_eq!(civil_from_days(days), (2026, 8, 11));
    }

    #[test]
    fn quick_capture_preview_root_lookup_is_read_only_when_unconfigured() {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();

        assert_eq!(
            resolve_configured_root(&conn).unwrap_err(),
            "빠른 캡처 미리보기를 만들 수 없습니다"
        );
        assert!(db::get_setting(&conn, "root").unwrap().is_none());

        db::set_setting(&conn, "root", "").unwrap();
        assert_eq!(
            resolve_configured_root(&conn).unwrap_err(),
            "빠른 캡처 미리보기를 만들 수 없습니다"
        );
    }

    #[test]
    fn quick_capture_preview_store_replaces_and_consumes_approvals_once() {
        let root = tempfile::tempdir().unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let mut store = QuickCapturePreviewStore::default();
        let first = store.issue(
            vault.clone(),
            capture::normalize(capture_input("first")).unwrap(),
        );
        let second = store.issue(vault, capture::normalize(capture_input("second")).unwrap());

        assert!(store.take(&first).is_none());
        let pending = store.take(&second).expect("latest preview is pending");
        assert_eq!(pending.capture.body, "second");
        assert!(store.take(&second).is_none());

        let third = store.issue(
            VaultIdentity::inspect(root.path()).unwrap(),
            capture::normalize(capture_input("third")).unwrap(),
        );
        store.discard(&third);
        assert!(store.take(&third).is_none());
    }

    #[test]
    fn quick_capture_preview_has_fixed_inbox_target_and_normalized_values() {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let preview = capture_preview(
            &vault,
            QuickCaptureInput {
                title: "  Captured idea  ".into(),
                body: "first\r\nsecond".into(),
                tags: vec!["rust".into(), "rust".into()],
            },
            "qc-test".into(),
        )
        .unwrap();
        assert_eq!(preview.preview_id, "qc-test");
        assert_eq!(preview.target, "Inbox");
        assert_eq!(preview.title, "Captured idea");
        assert_eq!(preview.body, "first\nsecond");
        assert_eq!(preview.tags, ["rust"]);
        assert!(!root.path().join(capture::INBOX_DIR).exists());
    }

    #[test]
    fn quick_capture_saves_portable_markdown_and_indexes_it() {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();

        let saved =
            save_capture_in_root(&conn, root.path(), capture_input("hello\nworld")).unwrap();
        assert!(root.path().join(capture::INBOX_DIR).is_dir());
        assert!(saved.path.starts_with("Inbox/"));
        assert!(saved.path.ends_with(".md"));
        let content = std::fs::read_to_string(root.path().join(&saved.path)).unwrap();
        assert_eq!(
            content,
            "---\ntitle: \"Captured idea\"\ntags: [\"rust\", \"offline\"]\n---\n\nhello\nworld\n"
        );
        assert_eq!(db::search(&conn, "world", 10).unwrap().len(), 1);
        assert!(std::fs::read_dir(root.path().join("Inbox"))
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[test]
    fn quick_capture_collision_never_overwrites_existing_note() {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        std::fs::create_dir(root.path().join(capture::INBOX_DIR)).unwrap();
        let now = 1_754_923_200;
        let first = root
            .path()
            .join("Inbox")
            .join(capture::filename_for_timestamp(now, 1));
        std::fs::write(&first, "keep me").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();

        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let saved = save_capture_at(&conn, &vault, capture_input("new note"), now).unwrap();
        assert_eq!(
            saved.path,
            format!("Inbox/{}", capture::filename_for_timestamp(now, 2))
        );
        assert_eq!(std::fs::read_to_string(first).unwrap(), "keep me");
    }

    #[test]
    fn quick_capture_secret_is_rejected_before_any_file_is_created() {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        let error = save_capture_in_root(&conn, root.path(), capture_input("token=secret-value"))
            .unwrap_err();
        assert_eq!(error, "민감한 정보가 포함되어 있어 저장하지 않았습니다");
        assert!(!root.path().join(capture::INBOX_DIR).exists());
    }

    #[test]
    fn quick_capture_index_failure_removes_the_new_file() {
        let root = tempfile::tempdir().unwrap();
        crate::core::store::ensure_layout(root.path()).unwrap();
        // Without the schema, indexing fails after the file has been created.
        let conn = Connection::open_in_memory().unwrap();
        let error = save_capture_at(
            &conn,
            &VaultIdentity::inspect(root.path()).unwrap(),
            capture_input("index failure"),
            1_754_923_200,
        )
        .unwrap_err();
        assert_eq!(error, "빠른 캡처를 저장하지 못했습니다");
        assert_eq!(
            std::fs::read_dir(root.path().join("Inbox"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn rollback_does_not_remove_a_target_replaced_by_another_writer() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("artifact.md");
        std::fs::write(&target, "original").unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let identity = vault.existing_file_identity(&target).unwrap();

        let original = root.path().join("original-artifact.md");
        std::fs::rename(&target, &original).unwrap();
        std::fs::write(&target, "replacement").unwrap();
        cleanup_vault_file(&vault, &target, &identity);

        assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
    }

    #[test]
    fn quick_capture_rejects_a_replaced_vault_before_creating_inbox() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("vault");
        std::fs::create_dir(&root).unwrap();
        let vault = VaultIdentity::inspect(&root).unwrap();
        std::fs::remove_dir(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();

        let error =
            save_capture_at(&conn, &vault, capture_input("stale"), 1_754_923_200).unwrap_err();
        assert_eq!(error, "빠른 캡처 미리보기가 오래되어 다시 확인하세요");
        assert!(!root.join("Inbox").exists());
    }

    #[cfg(unix)]
    #[test]
    fn quick_capture_rejects_an_inbox_symlink_before_writing() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("Inbox")).unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let result = capture_preview(&vault, capture_input("would escape"), "qc-test".into());
        assert_eq!(
            result.err().as_deref(),
            Some("Knowledge 항목 경로가 올바르지 않습니다")
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
