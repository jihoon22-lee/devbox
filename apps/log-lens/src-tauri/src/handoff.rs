//! Tauri command layer for Log Lens `log-source/v1` previews.
//!
//! A handoff is claimed only when an explicit inbound AppLink request is
//! consumed.  Preview keeps the claim in process memory; cancel restores it,
//! and an explicit Add source action acknowledges it.  The claim token never
//! crosses the WebView boundary.

use crate::core::handoff::{self, LogSourcePreview};
use crate::core::SourceSpec;
use devbox_applink::{HandoffClaim, HandoffError, HandoffStore};
use std::sync::{Mutex, MutexGuard};

struct ClaimedLogSource {
    claim: HandoffClaim,
    source: SourceSpec,
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

    fn put_if_empty(&self, claimed: ClaimedLogSource) -> Result<(), Box<ClaimedLogSource>> {
        let mut slot = self.slot();
        if slot.is_some() {
            return Err(Box::new(claimed));
        }
        *slot = Some(claimed);
        Ok(())
    }

    fn take(&self, id: &str) -> Result<ClaimedLogSource, String> {
        let mut slot = self.slot();
        let Some(current) = slot.as_ref() else {
            return Err("Log Lens source preview is not open".to_string());
        };
        if current.claim.envelope.id != id {
            return Err("another Log Lens source preview is open".to_string());
        }
        slot.take()
            .ok_or_else(|| "Log Lens source preview is not open".to_string())
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

/// Claim and validate a pending producer envelope.  The response contains a
/// bounded source summary and no claim token, path, command, or log bytes.
#[tauri::command]
pub fn preview_log_source(
    pending: tauri::State<'_, PendingLogSource>,
    id: String,
) -> Result<LogSourcePreview, String> {
    if !valid_handoff_id(&id) {
        return Err("Log Lens source handoff is unavailable".to_string());
    }
    if pending.slot().is_some() {
        return Err("Log Lens source handoff is already being previewed".to_string());
    }
    let now = now_ms();
    if now == 0 {
        return Err("Log Lens source handoff is unavailable".to_string());
    }
    let claim = handoff_store()
        .claim(&id, handoff::HANDOFF_KIND, handoff::CONSUMER_APP, now)
        .map_err(|error| handoff::map_claim_error(&error).to_string())?;
    let source = match handoff::parse_claim(&claim) {
        Ok(source) => source,
        Err(_) => {
            let _ = handoff_store().restore(&claim, handoff::CONSUMER_APP, now);
            return Err("Log Lens source handoff is invalid".to_string());
        }
    };
    let preview = match LogSourcePreview::from_claim(&claim, &source) {
        Ok(preview) => preview,
        Err(_) => {
            let _ = handoff_store().restore(&claim, handoff::CONSUMER_APP, now);
            return Err("Log Lens source handoff is invalid".to_string());
        }
    };
    if let Err(claimed) = pending.put_if_empty(ClaimedLogSource { claim, source }) {
        // Another request won the in-process slot while this claim was being
        // parsed. Restore this envelope rather than losing it.
        let claimed = *claimed;
        let _ = handoff_store().restore(&claimed.claim, handoff::CONSUMER_APP, now);
        return Err("Log Lens source handoff is already being previewed".to_string());
    }
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
    let claimed = pending.take(&id)?;
    let now = now_ms();
    if now == 0 {
        let _ = pending.put_if_empty(claimed);
        return Err("Log Lens source handoff is unavailable. Send it again.".to_string());
    }
    if now >= claimed.claim.envelope.expires_at_ms {
        let _ = handoff_store().ack(&claimed.claim, handoff::CONSUMER_APP, now);
        return Err("Log Lens source handoff has expired. Send it again.".to_string());
    }
    match handoff_store().ack(&claimed.claim, handoff::CONSUMER_APP, now) {
        Ok(()) => Ok(claimed.source),
        Err(
            error @ (HandoffError::Expired | HandoffError::LeaseExpired | HandoffError::Missing),
        ) => Err(handoff::map_claim_error(&error).to_string()),
        Err(error) => {
            let _ = pending.put_if_empty(claimed);
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
    let claimed = pending.take(&id)?;
    match handoff_store().restore(&claimed.claim, handoff::CONSUMER_APP, now_ms()) {
        Ok(()) => Ok(()),
        Err(HandoffError::Expired | HandoffError::LeaseExpired | HandoffError::Missing) => {
            Err("Log Lens source handoff has expired. Send it again.".to_string())
        }
        Err(error) => {
            let _ = pending.put_if_empty(claimed);
            Err(handoff::map_claim_error(&error).to_string())
        }
    }
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
        return Err("Log Lens source preview is not open".to_string());
    };
    if current.claim.envelope.id != id {
        return Err("another Log Lens source preview is open".to_string());
    }
    let renewed = match handoff_store().renew(
        &current.claim,
        handoff::CONSUMER_APP,
        now_ms(),
        devbox_applink::DEFAULT_CLAIM_LEASE_MS,
    ) {
        Ok(claim) => claim,
        Err(error) => {
            if matches!(
                &error,
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
        assert!(pending
            .put_if_empty(ClaimedLogSource {
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
                source: SourceSpec::Run {
                    source_id: "run-manager:run-1:stdout".into()
                },
            })
            .is_ok());
        assert!(pending.take(&"c".repeat(32)).is_err());
        assert!(pending.take(&"a".repeat(32)).is_ok());
        assert!(pending.take(&"a".repeat(32)).is_err());
    }
}
