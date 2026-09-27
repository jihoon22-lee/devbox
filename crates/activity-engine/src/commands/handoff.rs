//! Life Log's explicit native handoff action.
//!
//! A handoff is published only after the native digest has been rebuilt and
//! validated.  The payload is kept in the versioned one-time store; argv gets
//! only the opaque descriptor and the Knowledge executable is launched through
//! the shared launcher contract.

#[cfg(target_os = "windows")]
use crate::commands::digest::build_for_state;
use crate::commands::tracking::AppState;
use crate::core::digest::DigestInput;
use crate::core::draft_history;
#[cfg(target_os = "windows")]
use crate::core::handoff::{self, KNOWLEDGE_DRAFT_KIND};
use devbox_applink::{HandoffStatus, RecordHandoffStatus};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct SendKnowledgeDraftResult {
    pub id: String,
    pub kind: String,
    pub expires_at_ms: u64,
    pub history_id: String,
}

/// Reconcile durable handoff sidecars into the bounded local history. Missing
/// metadata is never guessed as consumed; only an elapsed envelope TTL can
/// move a non-terminal row to expired.
pub fn knowledge_draft_history(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<draft_history::DraftHistoryEntry>, String> {
    let now_ms = current_epoch_ms().unwrap_or(0);
    let store = handoff_store(&state);
    let entries = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "Knowledge draft 이력을 읽을 수 없습니다".to_string())?;
        draft_history::list(&connection)?
    };
    for entry in &entries {
        let sidecar = match store.read_status(&entry.handoff_id) {
            Ok(Some(record))
                if record.kind == "knowledge-draft/v1"
                    && record.source_app == "life-log"
                    && record.target_app.as_deref() == Some("knowledge-base")
                    && record.expires_at_ms == entry.expires_at_ms =>
            {
                Some(record)
            }
            Ok(Some(_)) => {
                // A sidecar with a valid JSON shape but a different identity
                // is corruption, not a missing status. Never fall back to a
                // DB state that could make a mismatched handoff look consumed.
                return Err("Knowledge draft 상태 메타데이터가 올바르지 않습니다".into());
            }
            Ok(None) | Err(devbox_applink::HandoffError::Missing) => None,
            Err(_) => {
                // Storage/corruption errors must be visible to the caller;
                // `.ok().flatten()` would silently turn them into a guessed
                // history state and hide a broken handoff root.
                return Err("Knowledge draft 상태를 확인할 수 없습니다".into());
            }
        };
        let (reconciled, reconciled_at_ms) = match sidecar.as_ref() {
            Some(record)
                if matches!(
                    record.status,
                    devbox_applink::HandoffStatus::Pending | devbox_applink::HandoffStatus::Sent
                ) && now_ms >= entry.expires_at_ms =>
            {
                let expired = record_expired_status(&store, entry, now_ms)?;
                (draft_history::DraftStatus::Expired, expired.updated_at_ms)
            }
            Some(record) => (
                match record.status {
                    devbox_applink::HandoffStatus::Pending => draft_history::DraftStatus::Pending,
                    devbox_applink::HandoffStatus::Sent => draft_history::DraftStatus::Sent,
                    devbox_applink::HandoffStatus::Consumed => draft_history::DraftStatus::Consumed,
                    devbox_applink::HandoffStatus::Expired => draft_history::DraftStatus::Expired,
                },
                record.updated_at_ms,
            ),
            None if now_ms >= entry.expires_at_ms
                && !matches!(
                    entry.status,
                    draft_history::DraftStatus::Consumed | draft_history::DraftStatus::Expired
                ) =>
            {
                let expired = record_expired_status(&store, entry, now_ms)?;
                (draft_history::DraftStatus::Expired, expired.updated_at_ms)
            }
            None => (entry.status, entry.updated_at_ms),
        };
        if reconciled != entry.status {
            let connection = state
                .db
                .lock()
                .map_err(|_| "Knowledge draft 이력을 갱신할 수 없습니다".to_string())?;
            // The sidecar lock is authoritative. A concurrent history
            // refresh may have won the DB CAS first; in that case the next
            // read will converge and no stale write is attempted here.
            if let Err(error) = draft_history::set_status(
                &connection,
                &entry.handoff_id,
                reconciled,
                reconciled_at_ms.max(entry.updated_at_ms),
            ) {
                if !error.contains("다른 작업에 의해 변경") {
                    return Err("Knowledge draft 이력을 갱신할 수 없습니다".into());
                }
            }
        }
    }
    let connection = state
        .db
        .lock()
        .map_err(|_| "Knowledge draft 이력을 읽을 수 없습니다".to_string())?;
    draft_history::list(&connection)
}

fn handoff_store(state: &AppState) -> devbox_applink::HandoffStore {
    let root = state
        .integration_root
        .clone()
        .unwrap_or_else(devbox_integration::common_root);
    devbox_applink::HandoffStore::new(devbox_applink::handoff_root_in(&root))
}

fn record_expired_status(
    store: &devbox_applink::HandoffStore,
    entry: &draft_history::DraftHistoryEntry,
    now_ms: u64,
) -> Result<devbox_applink::HandoffStatusRecord, String> {
    store
        .record_status(devbox_applink::RecordHandoffStatus {
            id: entry.handoff_id.clone(),
            kind: entry.kind.clone(),
            source_app: "life-log".into(),
            target_app: Some("knowledge-base".into()),
            status: devbox_applink::HandoffStatus::Expired,
            updated_at_ms: now_ms.max(entry.expires_at_ms),
            expires_at_ms: entry.expires_at_ms,
        })
        .map_err(|_| "Knowledge draft 만료 상태를 기록하지 못했습니다".to_string())
}

/// Build and send a Life Log digest to Knowledge.  Browser preview and
/// non-Windows builds never publish a pending handoff or attempt a launch.
pub async fn send_digest_to_knowledge(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
    regenerated_from: Option<String>,
) -> Result<SendKnowledgeDraftResult, String> {
    let _ = (state, input, regenerated_from);
    Err("Knowledge 앱을 실행할 수 없습니다".into())
}

/// The product supplies a native delivery callback for an opaque draft reference.
/// This producer never receives authority to read or write the destination vault.
pub(crate) async fn send_with_delivery<F>(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
    regenerated_from: Option<String>,
    require_installation: bool,
    deferred_delivery: bool,
    deliver: F,
) -> Result<SendKnowledgeDraftResult, String>
where
    F: FnOnce(&devbox_applink::OpenRequest) -> Result<(), String> + Send,
{
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            state,
            input,
            regenerated_from,
            require_installation,
            deferred_delivery,
            deliver,
        );
        Err("Knowledge handoff는 Windows 데스크톱에서 사용할 수 없습니다".into())
    }

    #[cfg(target_os = "windows")]
    {
        // Resolve first so a missing installation does not leave a pending
        // payload behind.  A launch race can still leave an expiring pending
        // item, which contains only the bounded summary and is retryable.
        draft_history::validate_regenerated_from(regenerated_from.as_deref())?;
        if require_installation {
            return Err("Knowledge 앱을 실행할 수 없습니다".into());
        }
        // Reuse the digest command's single-flight/cancellation boundary so
        // an explicit handoff cannot race a visible digest generation or
        // publish a result from a cancelled generation.
        let operation = state.digest_operations.begin()?;
        let cancellation = operation.cancellation();
        let response = build_for_state(&state, input, cancellation).await?;
        if operation.is_cancelled() {
            return Err("digest_cancelled".into());
        }
        let payload = handoff::build_knowledge_draft(&response)?;
        let payload_value = serde_json::to_value(payload.clone())
            .map_err(|_| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        if operation.is_cancelled() {
            return Err("digest_cancelled".into());
        }
        let now_ms = current_epoch_ms()
            .ok_or_else(|| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        let store = handoff_store(&state);
        let descriptor = store
            .create(
                devbox_applink::CreateHandoff {
                    kind: KNOWLEDGE_DRAFT_KIND.into(),
                    source_app: "life-log".into(),
                    target_app: Some("knowledge-base".into()),
                    payload: payload_value,
                },
                now_ms,
            )
            .map_err(|_| "Knowledge draft를 준비하지 못했습니다".to_string())?;
        let expires_at_ms = now_ms.saturating_add(devbox_applink::DEFAULT_HANDOFF_TTL_MS);
        if let Err(error) = store.record_status(RecordHandoffStatus {
            id: descriptor.id.clone(),
            kind: descriptor.kind.clone(),
            source_app: "life-log".into(),
            target_app: Some("knowledge-base".into()),
            status: HandoffStatus::Pending,
            updated_at_ms: now_ms,
            expires_at_ms,
        }) {
            discard_producer_state(&state, &store, &descriptor);
            let _ = error;
            return Err("Knowledge draft 상태를 기록하지 못했습니다".into());
        }
        {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    discard_producer_state(&state, &store, &descriptor);
                    return Err("Knowledge draft 이력을 저장하지 못했습니다".into());
                }
            };
            if let Err(error) = draft_history::insert(
                &connection,
                draft_history::DraftHistoryInsert {
                    handoff_id: &descriptor.id,
                    summary: &payload.summary,
                    sources: &payload.sources,
                    status: draft_history::DraftStatus::Pending,
                    created_at_ms: now_ms,
                    expires_at_ms,
                    regenerated_from: regenerated_from.as_deref(),
                },
            ) {
                let _ = store.discard_created(&descriptor);
                return Err(error);
            }
        }
        let request = devbox_applink::OpenRequest {
            target: descriptor.clone().into(),
            from: Some("life-log".into()),
        };
        // Mark the envelope as sent before dispatching the open request. This
        // closes the small race where Knowledge can claim/save the envelope
        // before the producer records its post-launch state. A failed launch
        // is explicitly put back into pending so it remains retryable.
        let sent_at_ms = current_epoch_ms().unwrap_or(now_ms);
        if !deferred_delivery {
            if store
                .record_status(RecordHandoffStatus {
                    id: descriptor.id.clone(),
                    kind: descriptor.kind.clone(),
                    source_app: "life-log".into(),
                    target_app: Some("knowledge-base".into()),
                    status: HandoffStatus::Sent,
                    updated_at_ms: sent_at_ms,
                    expires_at_ms,
                })
                .is_err()
            {
                discard_producer_state(&state, &store, &descriptor);
                return Err("Knowledge draft 상태를 기록하지 못했습니다".into());
            }
            let connection = state.db.lock().map_err(|_| {
                discard_producer_state(&state, &store, &descriptor);
                "Knowledge draft 이력을 갱신하지 못했습니다".to_string()
            })?;
            if let Err(error) = draft_history::set_status(
                &connection,
                &descriptor.id,
                draft_history::DraftStatus::Sent,
                sent_at_ms,
            ) {
                let _ = store.record_status(RecordHandoffStatus {
                    id: descriptor.id.clone(),
                    kind: descriptor.kind.clone(),
                    source_app: "life-log".into(),
                    target_app: Some("knowledge-base".into()),
                    status: HandoffStatus::Pending,
                    updated_at_ms: current_epoch_ms().unwrap_or(sent_at_ms),
                    expires_at_ms,
                });
                let _ = draft_history::remove(&connection, &descriptor.id);
                drop(connection);
                let _ = store.discard_created(&descriptor);
                let _ = error;
                return Err("Knowledge draft 이력을 갱신하지 못했습니다".into());
            }
            drop(connection);
        }
        if operation.is_cancelled() {
            discard_producer_state(&state, &store, &descriptor);
            return Err("digest_cancelled".into());
        }
        if let Err(error) = deliver(&request) {
            // A consumer may have claimed the envelope while the launcher
            // was returning an error. Only regress to pending when the
            // sidecar still agrees; if it already reached a terminal state,
            // mirror that state into the DB instead of overwriting consumed
            // history with a launch-failure result.
            reconcile_after_launch_failure(&state, &store, &descriptor, expires_at_ms, sent_at_ms);
            return Err(error);
        }
        Ok(SendKnowledgeDraftResult {
            id: descriptor.id.clone(),
            kind: descriptor.kind,
            expires_at_ms,
            history_id: descriptor.id,
        })
    }
}

fn current_epoch_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|now| *now > 0)
}

#[cfg(target_os = "windows")]
fn reconcile_after_launch_failure(
    state: &AppState,
    store: &devbox_applink::HandoffStore,
    descriptor: &devbox_applink::HandoffDescriptor,
    expires_at_ms: u64,
    fallback_updated_at_ms: u64,
) {
    let status = match store.record_status(RecordHandoffStatus {
        id: descriptor.id.clone(),
        kind: descriptor.kind.clone(),
        source_app: "life-log".into(),
        target_app: Some("knowledge-base".into()),
        status: HandoffStatus::Pending,
        updated_at_ms: current_epoch_ms().unwrap_or(fallback_updated_at_ms),
        expires_at_ms,
    }) {
        Ok(record) => Some(record.status),
        Err(_) => store
            .read_status(&descriptor.id)
            .ok()
            .flatten()
            .map(|record| record.status),
    };
    let Some(status) = status else {
        // A lock/storage failure is retried by the next history read; do not
        // make the DB claim pending without a matching durable sidecar.
        return;
    };
    let history_status = match status {
        HandoffStatus::Pending => draft_history::DraftStatus::Pending,
        HandoffStatus::Sent => draft_history::DraftStatus::Sent,
        HandoffStatus::Consumed => draft_history::DraftStatus::Consumed,
        HandoffStatus::Expired => draft_history::DraftStatus::Expired,
    };
    let updated_at_ms = current_epoch_ms().unwrap_or(fallback_updated_at_ms).max(
        if history_status == draft_history::DraftStatus::Expired {
            expires_at_ms
        } else {
            1
        },
    );
    if let Ok(connection) = state.db.lock() {
        let _ =
            draft_history::set_status(&connection, &descriptor.id, history_status, updated_at_ms);
    }
}

#[cfg(target_os = "windows")]
fn discard_producer_state(
    state: &AppState,
    store: &devbox_applink::HandoffStore,
    descriptor: &devbox_applink::HandoffDescriptor,
) {
    if let Ok(connection) = state.db.lock() {
        let _ = draft_history::remove(&connection, &descriptor.id);
    }
    let _ = store.discard_created(descriptor);
}

/// Split native producer preparation from the UI-owned offer. Pending is honest
/// if the response is lost; acknowledgement cannot regress a consumed draft.
pub(crate) async fn prepare_product_draft(
    state: tauri::State<'_, Arc<AppState>>,
    input: DigestInput,
    regenerated_from: Option<String>,
) -> Result<serde_json::Value, String> {
    let offered = std::sync::Mutex::new(None);
    let result = send_with_delivery(state, input, regenerated_from, false, true, |request| {
        *offered.lock().map_err(|_| "draft_delivery_unavailable")? = Some(request.clone());
        Ok(())
    })
    .await?;
    let request = offered
        .into_inner()
        .map_err(|_| "draft_delivery_unavailable")?
        .ok_or("draft_delivery_unavailable")?;
    Ok(serde_json::json!({"request":request,"result":result}))
}
fn finish_status(
    store: &devbox_applink::HandoffStore,
    id: &str,
    expires: u64,
    delivered: bool,
    now: u64,
) -> Result<devbox_applink::HandoffStatusRecord, String> {
    let record = store
        .read_status(id)
        .map_err(|_| "draft_delivery_unavailable")?
        .ok_or("draft_delivery_unavailable")?;
    if record.kind != "knowledge-draft/v1"
        || record.source_app != "life-log"
        || record.target_app.as_deref() != Some("knowledge-base")
        || record.expires_at_ms != expires
    {
        return Err("draft_delivery_invalid".into());
    }
    if matches!(
        record.status,
        HandoffStatus::Consumed | HandoffStatus::Expired
    ) {
        return Ok(record);
    }
    let update = RecordHandoffStatus {
        id: id.into(),
        kind: record.kind.clone(),
        source_app: record.source_app.clone(),
        target_app: record.target_app.clone(),
        expires_at_ms: expires,
        updated_at_ms: now.max(record.updated_at_ms),
        status: if now >= expires {
            HandoffStatus::Expired
        } else if delivered {
            HandoffStatus::Sent
        } else {
            HandoffStatus::Pending
        },
    };
    match store.record_status(update) {
        Ok(record) => Ok(record),
        Err(_) => {
            let latest = store
                .read_status(id)
                .map_err(|_| "draft_delivery_unavailable")?
                .ok_or("draft_delivery_unavailable")?;
            if latest.kind == record.kind
                && latest.source_app == record.source_app
                && latest.target_app == record.target_app
                && latest.expires_at_ms == expires
                && matches!(
                    latest.status,
                    HandoffStatus::Consumed | HandoffStatus::Expired
                )
            {
                Ok(latest)
            } else {
                Err("draft_delivery_unavailable".into())
            }
        }
    }
}
pub(crate) fn finish_product_draft(
    state: &AppState,
    id: &str,
    delivered: bool,
) -> Result<(), String> {
    let entry = {
        let connection = state.db.lock().map_err(|_| "draft_delivery_unavailable")?;
        draft_history::list(&connection)?
            .into_iter()
            .find(|entry| entry.handoff_id == id)
            .ok_or("draft_delivery_invalid")?
    };
    let now = current_epoch_ms().ok_or("draft_delivery_unavailable")?;
    let record = finish_status(
        &handoff_store(state),
        id,
        entry.expires_at_ms,
        delivered,
        now,
    )?;
    let status = match record.status {
        HandoffStatus::Pending => draft_history::DraftStatus::Pending,
        HandoffStatus::Sent => draft_history::DraftStatus::Sent,
        HandoffStatus::Consumed => draft_history::DraftStatus::Consumed,
        HandoffStatus::Expired => draft_history::DraftStatus::Expired,
    };
    let connection = state.db.lock().map_err(|_| "draft_delivery_unavailable")?;
    draft_history::set_status(
        &connection,
        id,
        status,
        record.updated_at_ms.max(entry.updated_at_ms),
    )
}
#[cfg(test)]
mod delivery_tests {
    use super::*;
    #[test]
    fn native_delivery_ack_preserves_terminal_receipts_and_rejects_other_envelopes() {
        let root = tempfile::tempdir().unwrap();
        let store = devbox_applink::HandoffStore::new(root.path());
        let created = store
            .create(
                devbox_applink::CreateHandoff {
                    kind: "knowledge-draft/v1".into(),
                    source_app: "life-log".into(),
                    target_app: Some("knowledge-base".into()),
                    payload: serde_json::json!({}),
                },
                100,
            )
            .unwrap();
        let expires = 100 + devbox_applink::DEFAULT_HANDOFF_TTL_MS;
        store
            .record_status(RecordHandoffStatus {
                id: created.id.clone(),
                kind: created.kind.clone(),
                source_app: "life-log".into(),
                target_app: Some("knowledge-base".into()),
                status: HandoffStatus::Pending,
                updated_at_ms: 100,
                expires_at_ms: expires,
            })
            .unwrap();
        assert!(finish_status(&store, &created.id, expires + 1, true, 101).is_err());
        assert_eq!(
            finish_status(&store, &created.id, expires, true, 101)
                .unwrap()
                .status,
            HandoffStatus::Sent
        );
        store
            .record_status(RecordHandoffStatus {
                id: created.id.clone(),
                kind: created.kind,
                source_app: "life-log".into(),
                target_app: Some("knowledge-base".into()),
                status: HandoffStatus::Consumed,
                updated_at_ms: 102,
                expires_at_ms: expires,
            })
            .unwrap();
        assert_eq!(
            finish_status(&store, &created.id, expires, false, 103)
                .unwrap()
                .status,
            HandoffStatus::Consumed
        );
    }
}
