//! Tauri commands for the Life Log -> Knowledge draft handoff.
//!
//! Claiming is intentionally separate from saving. Preview keeps the
//! one-time claim in process memory; cancel or any pre-commit failure restores
//! it to pending/, while a successful note/index transaction acknowledges and
//! deletes the claim.

use crate::commands::docs::{resolve_configured_root, AppState};
use crate::core::db;
use crate::core::handoff::{self, KnowledgeDraftPayload, KnowledgeDraftPreview};
use crate::core::vault::{self, EntryIdentity, VaultError, VaultIdentity};
use devbox_applink::{
    HandoffClaim, HandoffError, HandoffStatus, HandoffStore, RecordHandoffStatus,
};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const CONSUMER_APP: &str = "knowledge-base";
const EXPECTED_KIND: &str = handoff::KNOWLEDGE_DRAFT_KIND;
const MAX_NOTE_COLLISIONS: usize = 100;
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct ClaimedKnowledgeDraft {
    claim: HandoffClaim,
    payload: KnowledgeDraftPayload,
    vault: VaultIdentity,
}

/// At most one preview can be active in a Knowledge process. The claim token
/// never crosses the frontend boundary.
pub struct PendingKnowledgeDraft(Mutex<Option<ClaimedKnowledgeDraft>>);

impl PendingKnowledgeDraft {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn is_open(&self) -> bool {
        self.slot().is_some()
    }

    fn put_if_empty(&self, draft: ClaimedKnowledgeDraft) -> bool {
        let mut slot = self.slot();
        if slot.is_some() {
            return false;
        }
        *slot = Some(draft);
        true
    }

    fn take(&self, id: &str) -> Result<ClaimedKnowledgeDraft, String> {
        let mut slot = self.slot();
        let Some(current) = slot.as_ref() else {
            return Err("Knowledge draft 미리보기가 없습니다".into());
        };
        if current.claim.envelope.id != id {
            return Err("다른 Knowledge draft 미리보기가 열려 있습니다".into());
        }
        slot.take()
            .ok_or_else(|| "Knowledge draft 미리보기가 없습니다".to_string())
    }

    fn slot(&self) -> MutexGuard<'_, Option<ClaimedKnowledgeDraft>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for PendingKnowledgeDraft {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveKnowledgeDraftResult {
    pub saved: bool,
    pub path: String,
    pub handoff_deleted: bool,
    pub handoff_status_recorded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenewKnowledgeDraftResult {
    pub lease_until_ms: u64,
}

/// Claim and validate one pending Life Log draft. The returned preview has no
/// filesystem path, token, or raw activity record.
#[tauri::command]
pub fn preview_knowledge_draft(
    state: tauri::State<'_, Arc<AppState>>,
    pending: tauri::State<'_, PendingKnowledgeDraft>,
    id: String,
) -> Result<KnowledgeDraftPreview, String> {
    if !valid_handoff_id(&id) {
        return Err("Knowledge draft를 사용할 수 없습니다".into());
    }
    let now_ms = current_epoch_ms();
    if now_ms == 0 {
        return Err("Knowledge draft를 사용할 수 없습니다".into());
    }
    if pending.is_open() {
        return Err("Knowledge draft가 이미 미리보기 중입니다".into());
    }
    let vault = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
        let root = resolve_configured_root(&connection)
            .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
        let vault = VaultIdentity::inspect(&root)
            .map_err(|_| "Knowledge 저장 위치를 확인할 수 없습니다".to_string())?;
        validate_note_parent(&vault)?;
        vault
    };
    let store = handoff_store();
    let claim = store
        .claim(&id, EXPECTED_KIND, CONSUMER_APP, now_ms)
        .map_err(|error| handoff::map_claim_error(&error).to_string())?;
    let payload = match handoff::parse_claim(&claim) {
        Ok(payload) => payload,
        Err(_) => {
            let _ = store.restore(&claim, CONSUMER_APP, now_ms);
            let _ = record_handoff_status(&store, &claim, HandoffStatus::Pending, now_ms);
            return Err("Knowledge draft를 처리할 수 없습니다".into());
        }
    };
    let preview = KnowledgeDraftPreview::from_claim(&claim, &payload);
    let mut slot = pending.slot();
    if slot.is_some() {
        drop(slot);
        let _ = store.restore(&claim, CONSUMER_APP, now_ms);
        let _ = record_handoff_status(&store, &claim, HandoffStatus::Pending, now_ms);
        return Err("Knowledge draft가 이미 미리보기 중입니다".into());
    }
    *slot = Some(ClaimedKnowledgeDraft {
        claim,
        payload,
        vault,
    });
    Ok(preview)
}

/// Save only after the preview's explicit confirmation. File creation is
/// exclusive and the DB index is updated before applink ack; either failure
/// restores the claim for retry.
#[tauri::command]
pub fn save_knowledge_draft(
    state: tauri::State<'_, Arc<AppState>>,
    pending: tauri::State<'_, PendingKnowledgeDraft>,
    id: String,
) -> Result<SaveKnowledgeDraftResult, String> {
    let claimed = pending.take(&id)?;
    let store = handoff_store();
    let now_ms = current_epoch_ms();
    if now_ms == 0 {
        restore_for_retry(&store, pending.inner(), claimed);
        return Err("Knowledge draft를 저장하지 못했습니다".into());
    }
    if now_ms >= claimed.claim.envelope.expires_at_ms {
        // An expired preview must never turn into a note. ack() removes the
        // expired claim without resurrecting it; the user can send a fresh
        // digest from Life Log instead.
        let _ = store.ack(&claimed.claim, CONSUMER_APP, now_ms);
        let _ = record_handoff_status(&store, &claimed.claim, HandoffStatus::Expired, now_ms);
        return Err("Knowledge draft가 만료되었습니다. Life Log에서 새로 생성하세요".into());
    }
    let root = match state
        .db
        .lock()
        .map_err(|_| ())
        .and_then(|connection| resolve_configured_root(&connection).map_err(|_| ()))
    {
        Ok(root) => root,
        Err(_) => {
            restore_for_retry(&store, pending.inner(), claimed);
            return Err(stale_vault_message());
        }
    };
    let current_vault = match VaultIdentity::inspect(&root) {
        Ok(vault) => vault,
        Err(_) => {
            restore_for_retry(&store, pending.inner(), claimed);
            return Err(stale_vault_message());
        }
    };
    if current_vault != claimed.vault {
        restore_for_retry(&store, pending.inner(), claimed);
        return Err(stale_vault_message());
    }
    if validate_note_parent(&current_vault).is_err() {
        restore_for_retry(&store, pending.inner(), claimed);
        return Err(stale_vault_message());
    }
    let content = note_content(&claimed.payload);
    let (rel, path, path_identity) =
        match write_new_note_with_suffix(&current_vault, &claimed.payload, content.as_bytes()) {
            Ok(result) => result,
            Err(NewNoteError::Stale) => {
                restore_for_retry(&store, pending.inner(), claimed);
                return Err(stale_vault_message());
            }
            Err(_) => {
                restore_for_retry(&store, pending.inner(), claimed);
                return Err("Knowledge draft를 저장하지 못했습니다".into());
            }
        };
    let indexed = state.db.lock().ok().is_some_and(|connection| {
        let Ok(transaction) = connection.unchecked_transaction() else {
            return false;
        };
        if db::index_doc_in_transaction(&transaction, &rel, &content).is_err() {
            let _ = transaction.rollback();
            return false;
        }
        transaction.commit().is_ok()
    });
    if !indexed {
        vault::cleanup_file(&current_vault, &path, &path_identity);
        restore_for_retry(&store, pending.inner(), claimed);
        return Err("Knowledge 검색 인덱스를 갱신하지 못했습니다".into());
    }
    if let Ok(connection) = state.db.lock() {
        let _ = crate::integration::write_snapshot(&connection);
    }
    let handoff_deleted = match store.ack(&claimed.claim, CONSUMER_APP, current_epoch_ms()) {
        Ok(()) => true,
        Err(error) => {
            // The note/index commit happened before ack.  A failed ack must
            // not silently report success while leaving a replayable claim;
            // compensate the note and put the exact claim back when possible.
            let rolled_back =
                rollback_saved_note(&state, &current_vault, &path, &path_identity, &rel);
            restore_for_retry(&store, pending.inner(), claimed);
            if !rolled_back {
                return Err("Knowledge draft 저장 결과를 확정하지 못했습니다".into());
            }
            return Err(handoff::map_claim_error(&error).to_string());
        }
    };
    let handoff_status_recorded = record_handoff_status(
        &store,
        &claimed.claim,
        HandoffStatus::Consumed,
        current_epoch_ms(),
    )
    .is_ok();
    Ok(SaveKnowledgeDraftResult {
        saved: true,
        path: rel,
        handoff_deleted,
        handoff_status_recorded,
    })
}

/// Cancel a preview without writing a note. The envelope becomes pending
/// again and can be opened by a later retry until its TTL expires.
#[tauri::command]
pub fn discard_knowledge_draft(
    pending: tauri::State<'_, PendingKnowledgeDraft>,
    id: String,
) -> Result<(), String> {
    let claimed = pending.take(&id)?;
    let store = handoff_store();
    let now_ms = current_epoch_ms();
    match store.restore(&claimed.claim, CONSUMER_APP, now_ms) {
        Ok(()) => record_handoff_status(&store, &claimed.claim, HandoffStatus::Pending, now_ms)
            .map_err(|_| "Knowledge draft 취소 상태를 기록하지 못했습니다".to_string()),
        Err(HandoffError::Expired | HandoffError::LeaseExpired | HandoffError::Missing) => {
            if now_ms >= claimed.claim.envelope.expires_at_ms {
                let _ =
                    record_handoff_status(&store, &claimed.claim, HandoffStatus::Expired, now_ms);
            }
            Err("Knowledge draft가 만료되었습니다. Life Log에서 새로 생성하세요".into())
        }
        Err(_) => {
            let _ = pending.inner().put_if_empty(claimed);
            Err("Knowledge draft를 취소하지 못했습니다".into())
        }
    }
}

/// Extend the short claim lease while a user is reading the preview. The
/// envelope TTL remains authoritative, so renewal can never keep an expired
/// handoff alive.
#[tauri::command]
pub fn renew_knowledge_draft(
    pending: tauri::State<'_, PendingKnowledgeDraft>,
    id: String,
) -> Result<RenewKnowledgeDraftResult, String> {
    let mut slot = pending.slot();
    let Some(current) = slot.as_mut() else {
        return Err("Knowledge draft 미리보기가 없습니다".into());
    };
    if current.claim.envelope.id != id {
        return Err("다른 Knowledge draft 미리보기가 열려 있습니다".into());
    }
    let renewed = match handoff_store().renew(
        &current.claim,
        CONSUMER_APP,
        current_epoch_ms(),
        devbox_applink::DEFAULT_CLAIM_LEASE_MS,
    ) {
        Ok(renewed) => renewed,
        Err(error) => {
            if matches!(
                error,
                HandoffError::Expired
                    | HandoffError::LeaseExpired
                    | HandoffError::Missing
                    | HandoffError::Corrupt
            ) {
                slot.take();
            }
            return Err(handoff::map_claim_error(&error).to_string());
        }
    };
    current.claim = renewed.clone();
    Ok(RenewKnowledgeDraftResult {
        lease_until_ms: renewed.lease_until_ms,
    })
}

fn handoff_store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn valid_handoff_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn restore_for_retry(
    store: &HandoffStore,
    pending: &PendingKnowledgeDraft,
    claimed: ClaimedKnowledgeDraft,
) {
    let now_ms = current_epoch_ms();
    match store.restore(&claimed.claim, CONSUMER_APP, now_ms) {
        Ok(()) => {
            let _ = record_handoff_status(store, &claimed.claim, HandoffStatus::Pending, now_ms);
        }
        Err(HandoffError::Expired | HandoffError::LeaseExpired | HandoffError::Missing) => {
            if now_ms >= claimed.claim.envelope.expires_at_ms {
                let _ =
                    record_handoff_status(store, &claimed.claim, HandoffStatus::Expired, now_ms);
            }
        }
        Err(_) => {
            // Keep the in-memory claim if the store could not restore it, but
            // never overwrite a newer preview opened while this save ran.
            let _ = pending.put_if_empty(claimed);
        }
    }
}

fn rollback_saved_note(
    state: &AppState,
    vault: &VaultIdentity,
    path: &Path,
    identity: &EntryIdentity,
    relative: &str,
) -> bool {
    let indexed = state.db.lock().ok().is_some_and(|connection| {
        let Ok(transaction) = connection.unchecked_transaction() else {
            return false;
        };
        if db::remove_doc(&transaction, relative).is_err() {
            let _ = transaction.rollback();
            return false;
        }
        transaction.commit().is_ok()
    });
    vault::cleanup_file(vault, path, identity);
    let snapshot_restored = indexed
        && state
            .db
            .lock()
            .ok()
            .is_some_and(|connection| crate::integration::write_snapshot(&connection).is_ok());
    indexed && !path.exists() && snapshot_restored
}

fn record_handoff_status(
    store: &HandoffStore,
    claim: &HandoffClaim,
    status: HandoffStatus,
    updated_at_ms: u64,
) -> Result<(), String> {
    if updated_at_ms == 0 {
        return Err("handoff 상태 시간이 올바르지 않습니다".into());
    }
    store
        .record_status(RecordHandoffStatus {
            id: claim.envelope.id.clone(),
            kind: claim.envelope.kind.clone(),
            source_app: claim.envelope.source_app.clone(),
            target_app: claim.envelope.target_app.clone(),
            status,
            updated_at_ms,
            expires_at_ms: claim.envelope.expires_at_ms,
        })
        .map(|_| ())
        .map_err(|_| "handoff 상태를 기록하지 못했습니다".to_string())
}

fn validate_note_parent(vault: &VaultIdentity) -> Result<(), String> {
    let journal = vault
        .new_entry("Journal")
        .map_err(|_| "Knowledge draft 저장 위치가 올바르지 않습니다".to_string())?;
    let journal = vault
        .existing_path(&journal)
        .map_err(|_| "Knowledge draft 저장 위치가 올바르지 않습니다".to_string())?;
    match fs::symlink_metadata(journal) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        _ => Err("Knowledge draft 저장 위치가 올바르지 않습니다".into()),
    }
}

fn stale_vault_message() -> String {
    "Knowledge 저장 위치가 변경되어 다시 확인해야 합니다".to_string()
}

fn note_content(payload: &KnowledgeDraftPayload) -> String {
    format!(
        "---\ntitle: {}\ntags: [{}]\n---\n\n{}",
        payload.title,
        payload.tags.join(", "),
        payload.body
    )
}

fn draft_note_stem(payload: &KnowledgeDraftPayload) -> String {
    format!(
        "Journal/{}-life-log-{}",
        payload.summary.start_date, payload.summary.period
    )
}

fn write_new_note_with_suffix(
    vault: &VaultIdentity,
    payload: &KnowledgeDraftPayload,
    contents: &[u8],
) -> Result<(String, PathBuf, EntryIdentity), NewNoteError> {
    for index in 0..=MAX_NOTE_COLLISIONS {
        let stem = draft_note_stem(payload);
        let rel = if index == 0 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{index}.md")
        };
        let path = vault.new_entry(&rel).map_err(|error| match error {
            VaultError::Stale => NewNoteError::Stale,
            VaultError::InvalidRoot | VaultError::InvalidEntry => NewNoteError::Storage,
        })?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(NewNoteError::Storage)
            }
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(NewNoteError::Storage),
        }
        match write_new_note(vault, &path, contents) {
            Ok(identity) => return Ok((rel, path, identity)),
            Err(NewNoteError::Exists) => continue,
            Err(error @ (NewNoteError::Storage | NewNoteError::Stale)) => return Err(error),
        }
    }
    Err(NewNoteError::Storage)
}

#[derive(Debug)]
enum NewNoteError {
    Exists,
    Storage,
    Stale,
}

/// Write a complete private file to a unique temporary sibling, then create
/// the final name with an exclusive hard link. This preserves no-overwrite
/// semantics even if two handoff saves race for the same date.
fn write_new_note(
    vault: &VaultIdentity,
    path: &Path,
    contents: &[u8],
) -> Result<EntryIdentity, NewNoteError> {
    let parent = vault
        .existing_path(path.parent().ok_or(NewNoteError::Storage)?)
        .map_err(|_| NewNoteError::Stale)?;
    // Keep the validated parent object alive through publication. Without the
    // lease, a very fast delete-and-recreate can recycle a filesystem ID and
    // make a path-only recheck mistake the replacement for the original.
    let parent_lease = vault
        .lease_existing_directory(&parent)
        .map_err(|_| NewNoteError::Stale)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(NewNoteError::Storage)?;
    for _ in 0..16 {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
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
            Err(_) => return Err(NewNoteError::Storage),
        };
        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                drop(file);
                let _ = fs::remove_file(&temporary);
                return Err(NewNoteError::Storage);
            }
        };
        let identity = VaultIdentity::entry_identity_from_metadata(&temporary, &metadata);
        if !entry_identity_matches(vault, &parent, parent_lease.identity()) {
            drop(file);
            vault::cleanup_file_by_identity(&temporary, &identity);
            return Err(NewNoteError::Stale);
        }
        let written = file
            .write_all(contents)
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_all());
        drop(file);
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(NewNoteError::Storage);
        }
        // The parent was checked before the temporary file was created.  A
        // final identity check closes the ordinary root-replacement window
        // before the no-replace publication primitive follows that parent.
        if !entry_identity_matches(vault, &parent, parent_lease.identity()) {
            vault::cleanup_file_by_identity(&temporary, &identity);
            return Err(NewNoteError::Stale);
        }
        let published = vault::publish_new_file(&temporary, path);
        return match published {
            Ok(()) => {
                let current = match vault.existing_file_identity(path) {
                    Ok(current) => current,
                    Err(_) => {
                        vault::cleanup_file(vault, path, &identity);
                        return Err(NewNoteError::Stale);
                    }
                };
                if !identity.matches(&current) {
                    vault::cleanup_file(vault, path, &identity);
                    return Err(NewNoteError::Stale);
                }
                Ok(current)
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                vault::cleanup_file_by_identity(&temporary, &identity);
                Err(NewNoteError::Exists)
            }
            Err(_) => {
                vault::cleanup_file_by_identity(&temporary, &identity);
                Err(NewNoteError::Storage)
            }
        };
    }
    Err(NewNoteError::Storage)
}

fn entry_identity_matches(vault: &VaultIdentity, path: &Path, expected: &EntryIdentity) -> bool {
    let Ok(path) = vault.existing_path(path) else {
        return false;
    };
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    let current = VaultIdentity::entry_identity_from_metadata(&path, &metadata);
    expected.matches(&current)
}

fn current_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::handoff;
    use crate::core::handoff::{KnowledgeDraftSource, KnowledgeDraftSummary};
    use devbox_applink::{CreateHandoff, HandoffError};
    use serde_json::Value;

    fn fixture() -> KnowledgeDraftPayload {
        let summary = KnowledgeDraftSummary {
            period: "day".into(),
            start_date: "2026-08-27".into(),
            end_date: "2026-08-27".into(),
            timezone: "UTC".into(),
            filter: None,
            pc_usage_ms: 0,
            session_count: 0,
            active_days: 0,
            total_days: 1,
            average_daily_usage_ms: 0,
            git_commits: 0,
            top_app: None,
        };
        let sources = (0..4)
            .map(|index| KnowledgeDraftSource {
                id: ["life-log", "git", "run-manager", "knowledge-base"][index].into(),
                available: index == 0,
                schema_version: (index == 0).then_some(1),
                snapshot_version: None,
                producer_version: (index == 0).then_some("0.3.1".into()),
                generated_at: None,
                freshness_ms: None,
                view: None,
                scope: if index < 2 {
                    "requested-range".into()
                } else {
                    "latest-snapshot-out-of-range".into()
                },
                error_code: (index >= 2).then_some("snapshot_unavailable".into()),
            })
            .collect::<Vec<_>>();
        KnowledgeDraftPayload {
            schema_version: 1,
            title: "Life Log digest · 2026-08-27 ~ 2026-08-27".into(),
            body: handoff::render_body(&summary, &sources),
            tags: vec!["life-log".into(), "digest".into(), "day".into()],
            summary,
            sources,
        }
    }

    #[test]
    fn note_save_path_is_bounded_and_non_overwriting() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Journal")).unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let payload = fixture();
        let content = note_content(&payload);
        let first = write_new_note_with_suffix(&vault, &payload, content.as_bytes()).unwrap();
        let second = write_new_note_with_suffix(&vault, &payload, content.as_bytes()).unwrap();
        assert_eq!(first.0, "Journal/2026-08-27-life-log-day.md");
        assert_eq!(second.0, "Journal/2026-08-27-life-log-day-1.md");
        assert_eq!(fs::read(first.1).unwrap(), fs::read(second.1).unwrap());
    }

    #[test]
    fn handoff_parent_validation_never_creates_a_default_journal() {
        let root = tempfile::tempdir().unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        assert!(validate_note_parent(&vault).is_err());
        assert!(!root.path().join("Journal").exists());
    }

    #[test]
    fn journal_lease_rejects_a_different_directory_identity() {
        let root = tempfile::tempdir().unwrap();
        let journal = root.path().join("Journal");
        let other = root.path().join("Other");
        fs::create_dir(&journal).unwrap();
        fs::create_dir(&other).unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let journal_lease = vault.lease_existing_directory(&journal).unwrap();
        let other_lease = vault.lease_existing_directory(&other).unwrap();

        assert!(entry_identity_matches(
            &vault,
            &journal,
            journal_lease.identity()
        ));
        assert!(!entry_identity_matches(
            &vault,
            &journal,
            other_lease.identity()
        ));
    }

    #[test]
    fn malformed_ids_are_rejected_before_store_access() {
        assert!(valid_handoff_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_handoff_id("0123456789ABCDEF0123456789abcdef"));
        assert!(!valid_handoff_id("raw-secret"));
    }

    #[test]
    fn fixture_handoff_roundtrip_supports_restore_ack_and_fresh_expiry_retry() {
        let root = tempfile::tempdir().unwrap();
        let store = HandoffStore::new(root.path().join("handoff/v1"));
        let payload: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/knowledge-draft-v1.json"))
                .unwrap();
        let request = || CreateHandoff {
            kind: EXPECTED_KIND.into(),
            source_app: "life-log".into(),
            target_app: Some(CONSUMER_APP.into()),
            payload: payload.clone(),
        };

        let descriptor = store.create(request(), 1_000).unwrap();
        let claim = store
            .claim(&descriptor.id, EXPECTED_KIND, CONSUMER_APP, 2_000)
            .unwrap();
        let parsed = handoff::parse_claim(&claim).unwrap();
        assert_eq!(parsed.schema_version, 1);
        store.restore(&claim, CONSUMER_APP, 3_000).unwrap();

        let retried = store
            .claim(&descriptor.id, EXPECTED_KIND, CONSUMER_APP, 4_000)
            .unwrap();
        store.ack(&retried, CONSUMER_APP, 5_000).unwrap();
        assert_eq!(
            store.claim(&descriptor.id, EXPECTED_KIND, CONSUMER_APP, 6_000),
            Err(HandoffError::Missing)
        );

        let expired = store.create_with_ttl(request(), 10_000, 100).unwrap();
        assert_eq!(
            store.claim(&expired.id, EXPECTED_KIND, CONSUMER_APP, 10_100),
            Err(HandoffError::Expired)
        );
        let fresh = store.create(request(), 11_000).unwrap();
        assert_ne!(fresh.id, expired.id);
        let fresh_claim = store
            .claim(&fresh.id, EXPECTED_KIND, CONSUMER_APP, 11_001)
            .unwrap();
        store.ack(&fresh_claim, CONSUMER_APP, 11_002).unwrap();
    }
}
