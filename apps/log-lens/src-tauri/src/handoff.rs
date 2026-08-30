//! Tauri command layer for Log Lens source previews.
//!
//! A handoff is claimed only when an explicit inbound AppLink request is
//! consumed.  Preview keeps the claim in process memory; cancel restores it,
//! and an explicit Add source action acknowledges it.  The claim token never
//! crosses the WebView boundary.

use crate::core::handoff::{self, LogSourcePreview};
use crate::core::SourceSpec;
use devbox_applink::{HandoffClaim, HandoffStore};
use std::sync::{Mutex, MutexGuard};

struct ClaimedLogSource {
    claim: HandoffClaim,
    // Keep the claim even when a bounded preview cannot be decoded.  A
    // restore/storage failure must retain the exact claim so the frontend can
    // retry that same id/token instead of silently losing ownership.
    source: Option<SourceSpec>,
}

/// At most one source handoff can be previewed in one Log Lens process.  This
/// keeps the one-time claim lifecycle explicit and prevents a stale modal from
/// accidentally acknowledging a newer request.
pub struct PendingLogSource(Mutex<Option<ClaimedLogSource>>);

impl PendingLogSource {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }

    fn slot(&self) -> MutexGuard<'_, Option<ClaimedLogSource>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for PendingLogSource {
    fn default() -> Self {
        Self::new()
    }
}

fn handoff_store() -> HandoffStore {
    HandoffStore::new(devbox_applink::handoff_root_in(
        &devbox_integration::common_root(),
    ))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn valid_handoff_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Restore the exact claim held in the in-process slot.  Terminal lifecycle
/// errors mean the claim is no longer usable and remove the slot; all other
/// failures retain it for an explicit bounded retry.
fn restore_slot(
    slot: &mut Option<ClaimedLogSource>,
    store: &HandoffStore,
    now: u64,
) -> Result<(), String> {
    let result = match slot.as_ref() {
        Some(current) => store.restore(&current.claim, handoff::CONSUMER_APP, now),
        None => return Err(handoff::ERROR_NOT_OPEN.to_string()),
    };
    match result {
        Ok(()) => {
            slot.take();
            Ok(())
        }
        Err(error) if handoff::is_terminal_claim_error(&error) => {
            slot.take();
            Err(handoff::map_restore_error(&error).to_string())
        }
        Err(error) => Err(handoff::map_restore_error(&error).to_string()),
    }
}

fn preview_restore_error(
    slot: &mut Option<ClaimedLogSource>,
    store: &HandoffStore,
    now: u64,
) -> String {
    restore_slot(slot, store, now)
        .err()
        .unwrap_or_else(|| handoff::ERROR_INVALID.to_string())
}

/// Claim and validate a pending producer envelope.  The response contains a
/// bounded source summary and no claim token, path, command, or log bytes.
#[tauri::command]
pub fn preview_log_source(
    pending: tauri::State<'_, PendingLogSource>,
    id: String,
    handoff_kind: String,
) -> Result<LogSourcePreview, String> {
    if !valid_handoff_id(&id) || !handoff::supported_kind(&handoff_kind) {
        return Err(handoff::ERROR_INVALID.to_string());
    }
    let now = now_ms();
    if now == 0 {
        return Err(handoff::ERROR_STORAGE.to_string());
    }
    // Hold the slot lock across claim/validation/restoration.  This removes
    // the take/restore gap in which a second inbound request could race the
    // first claim and makes the retained id/token unambiguous.
    let mut slot = pending.slot();
    if slot.is_some() {
        return Err(handoff::ERROR_BUSY.to_string());
    }
    let store = handoff_store();
    let claim = store
        .claim(&id, &handoff_kind, handoff::CONSUMER_APP, now)
        .map_err(|error| handoff::map_claim_error(&error).to_string())?;
    *slot = Some(ClaimedLogSource {
        claim,
        source: None,
    });
    let source = match handoff::parse_claim(&slot.as_ref().expect("claim slot").claim) {
        Ok(source) => source,
        Err(_) => {
            return Err(preview_restore_error(&mut slot, &store, now));
        }
    };
    let preview =
        match LogSourcePreview::from_claim(&slot.as_ref().expect("claim slot").claim, &source) {
            Ok(preview) => preview,
            Err(_) => {
                return Err(preview_restore_error(&mut slot, &store, now));
            }
        };
    slot.as_mut().expect("claim slot").source = Some(source);
    Ok(preview)
}

/// A confirmed preview becomes a Log Lens source and consumes the one-time
/// envelope. The source itself remains read-only and is loaded separately by
/// the existing bounded reader.
#[tauri::command]
pub fn accept_log_source(
    pending: tauri::State<'_, PendingLogSource>,
    id: String,
) -> Result<SourceSpec, String> {
    let mut slot = pending.slot();
    let Some(current) = slot.as_ref() else {
        return Err(handoff::ERROR_NOT_OPEN.to_string());
    };
    if current.claim.envelope.id != id {
        return Err(handoff::ERROR_BUSY.to_string());
    }
    let Some(source) = current.source.clone() else {
        return Err(handoff::ERROR_INVALID.to_string());
    };
    let now = now_ms();
    let store = handoff_store();
    let result = store.ack(&current.claim, handoff::CONSUMER_APP, now);
    match result {
        Ok(()) => {
            slot.take();
            Ok(source)
        }
        Err(error) => {
            if handoff::is_terminal_claim_error(&error) {
                slot.take();
            }
            Err(handoff::map_claim_error(&error).to_string())
        }
    }
}

/// Restore a claimed preview without adding a source.
#[tauri::command]
pub fn discard_log_source(
    pending: tauri::State<'_, PendingLogSource>,
    id: String,
) -> Result<(), String> {
    let mut slot = pending.slot();
    let Some(current) = slot.as_ref() else {
        return Err(handoff::ERROR_NOT_OPEN.to_string());
    };
    if current.claim.envelope.id != id {
        return Err(handoff::ERROR_BUSY.to_string());
    }
    restore_slot(&mut slot, &handoff_store(), now_ms())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenewLogSourceResult {
    pub lease_until_ms: u64,
}

/// Keep an open preview within the generic 60-second lease without extending
/// the envelope's ten-minute TTL.
#[tauri::command]
pub fn renew_log_source(
    pending: tauri::State<'_, PendingLogSource>,
    id: String,
) -> Result<RenewLogSourceResult, String> {
    let mut slot = pending.slot();
    let Some(current) = slot.as_mut() else {
        return Err(handoff::ERROR_NOT_OPEN.to_string());
    };
    if current.claim.envelope.id != id {
        return Err(handoff::ERROR_BUSY.to_string());
    }
    let renewed = match handoff_store().renew(
        &current.claim,
        handoff::CONSUMER_APP,
        now_ms(),
        devbox_applink::DEFAULT_CLAIM_LEASE_MS,
    ) {
        Ok(claim) => claim,
        Err(error) => {
            if handoff::is_terminal_claim_error(&error) {
                slot.take();
            }
            return Err(handoff::map_claim_error(&error).to_string());
        }
    };
    current.claim = renewed.clone();
    Ok(RenewLogSourceResult {
        lease_until_ms: renewed.lease_until_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_ids_are_strictly_opaque() {
        assert!(valid_handoff_id(&"a".repeat(32)));
        assert!(!valid_handoff_id("../secret"));
        assert!(!valid_handoff_id(&"A".repeat(32)));
        assert!(!valid_handoff_id(&"a".repeat(33)));
    }

    #[test]
    fn pending_slot_is_one_shot_and_rejects_other_ids() {
        let pending = PendingLogSource::new();
        let claimed = ClaimedLogSource {
            claim: HandoffClaim {
                envelope: devbox_applink::HandoffEnvelope {
                    protocol_version: devbox_applink::PROTOCOL_VERSION,
                    id: "a".repeat(32),
                    kind: handoff::HANDOFF_KIND.into(),
                    source_app: handoff::RUN_SOURCE_APP.into(),
                    target_app: Some(handoff::CONSUMER_APP.into()),
                    created_at_ms: 1,
                    expires_at_ms: 10,
                    payload: serde_json::json!({
                        "kind": "log-source/v1",
                        "sourceId": "run-manager:run-1:stdout",
                        "runId": "run-1",
                        "stream": "stdout"
                    }),
                },
                claim_token: "b".repeat(32),
                lease_until_ms: 5,
            },
            source: Some(SourceSpec::Run {
                source_id: "run-manager:run-1:stdout".into(),
            }),
        };
        {
            let mut slot = pending.slot();
            assert!(slot.is_none());
            *slot = Some(claimed);
        }
        assert_eq!(
            pending
                .slot()
                .as_ref()
                .map(|current| current.claim.envelope.id.as_str()),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_ne!(
            pending
                .slot()
                .as_ref()
                .map(|current| current.claim.envelope.id.as_str()),
            Some("cccccccccccccccccccccccccccccccc")
        );
        {
            let mut slot = pending.slot();
            assert!(slot.take().is_some());
        }
        assert!(pending.slot().is_none());
    }
}
