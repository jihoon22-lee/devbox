//! Log Lens claim/preview boundary for the Run Manager/WSL Desktop
//! `log-source/v1` handoff.
//!
//! The generic applink store validates protocol, target, size, and one-time
//! claim state.  This module validates the producer-specific allowlist and
//! converts it to one of Log Lens's fixed read-only `SourceSpec` adapters.
//! No command, environment, credential, or log bytes are accepted.

use super::model::{LogSourceRef, SourceSpec, SourceSummary};
use devbox_applink::{HandoffClaim, HandoffError};
use serde::{Deserialize, Serialize};

pub const HANDOFF_KIND: &str = "log-source/v1";
pub const CONSUMER_APP: &str = "log-lens";
pub const RUN_SOURCE_APP: &str = "run-manager";
pub const WSL_SOURCE_APP: &str = "wsl-desktop";
pub const WSL_FILE_SOURCE_TYPE: &str = "wslFile";
pub const WSL_JOURNAL_SOURCE_TYPE: &str = "wslJournal";
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

// These values cross the Tauri boundary. Keep them opaque and stable so the
// frontend can distinguish a terminal claim from a retryable store/restore
// failure without ever displaying a native error string or payload detail.
pub const ERROR_INVALID: &str = "handoff-invalid";
pub const ERROR_MISSING: &str = "handoff-missing";
pub const ERROR_EXPIRED: &str = "handoff-expired";
pub const ERROR_LEASE_EXPIRED: &str = "handoff-lease-expired";
pub const ERROR_BUSY: &str = "handoff-busy";
pub const ERROR_STORAGE: &str = "handoff-storage-failed";
pub const ERROR_CLAIM_STORAGE: &str = "handoff-claim-storage-failed";
pub const ERROR_RESTORE: &str = "handoff-restore-failed";
pub const ERROR_NOT_OPEN: &str = "handoff-not-open";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WslFilePayload {
    pub source_type: String,
    pub distro: String,
    pub wsl_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WslJournalPayload {
    pub source_type: String,
    pub distro: String,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LogSourcePreview {
    pub id: String,
    pub kind: String,
    pub source_app: String,
    pub expires_at_ms: u64,
    pub lease_until_ms: u64,
    pub source: SourceSummary,
}

/// Parse and validate the producer-specific payload while it is exclusively
/// claimed.  The source app determines the only payload family it may send.
pub fn parse_claim(claim: &HandoffClaim) -> Result<SourceSpec, String> {
    let envelope = &claim.envelope;
    // HandoffStore already enforces these invariants while claiming. Keep
    // them here as a second receiver-side boundary because this parser is
    // also callable from tests and future integrations without going through
    // the store. In particular, never let a future protocol version silently
    // enter a v1 source adapter.
    if envelope.protocol_version != devbox_applink::PROTOCOL_VERSION
        || !valid_opaque_id(&envelope.id)
        || !valid_opaque_id(&claim.claim_token)
        || envelope.created_at_ms == 0
        || envelope.expires_at_ms <= envelope.created_at_ms
        || envelope
            .expires_at_ms
            .saturating_sub(envelope.created_at_ms)
            > devbox_applink::DEFAULT_HANDOFF_TTL_MS
        || claim.lease_until_ms <= envelope.created_at_ms
        || claim.lease_until_ms > envelope.expires_at_ms
        || envelope.kind != HANDOFF_KIND
        || envelope.target_app.as_deref() != Some(CONSUMER_APP)
        || (envelope.source_app != RUN_SOURCE_APP && envelope.source_app != WSL_SOURCE_APP)
    {
        return Err("log source handoff source or target is invalid".to_string());
    }
    let bytes = serde_json::to_vec(&envelope.payload)
        .map_err(|_| "log source handoff payload is invalid".to_string())?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err("log source handoff payload is too large".to_string());
    }
    let source = match envelope.source_app.as_str() {
        RUN_SOURCE_APP => {
            let reference: LogSourceRef = serde_json::from_value(envelope.payload.clone())
                .map_err(|_| "log source handoff payload is invalid".to_string())?;
            reference
                .into_source()
                .map_err(|_| "log source handoff source is invalid".to_string())?
        }
        WSL_SOURCE_APP => parse_wsl_payload(&envelope.payload)?,
        _ => return Err("log source handoff source is invalid".to_string()),
    };
    source
        .validate()
        .map_err(|_| "log source handoff source is invalid".to_string())?;
    Ok(source)
}

fn valid_opaque_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse_wsl_payload(payload: &serde_json::Value) -> Result<SourceSpec, String> {
    if let Ok(file) = serde_json::from_value::<WslFilePayload>(payload.clone()) {
        if file.source_type == WSL_FILE_SOURCE_TYPE {
            return Ok(SourceSpec::WslFile {
                distro: file.distro,
                path: file.wsl_path,
            });
        }
    }
    let journal: WslJournalPayload = serde_json::from_value(payload.clone())
        .map_err(|_| "log source handoff payload is invalid".to_string())?;
    if journal.source_type != WSL_JOURNAL_SOURCE_TYPE {
        return Err("log source handoff source is invalid".to_string());
    }
    Ok(SourceSpec::WslJournal {
        distro: journal.distro,
        unit: journal.unit,
    })
}

impl LogSourcePreview {
    pub fn from_claim(claim: &HandoffClaim, source: &SourceSpec) -> Result<Self, String> {
        Ok(Self {
            id: claim.envelope.id.clone(),
            kind: claim.envelope.kind.clone(),
            source_app: claim.envelope.source_app.clone(),
            expires_at_ms: claim.envelope.expires_at_ms,
            lease_until_ms: claim.lease_until_ms,
            source: source
                .summary()
                .map_err(|_| "log source handoff source is invalid".to_string())?,
        })
    }
}

pub fn map_claim_error(error: &HandoffError) -> &'static str {
    match error {
        HandoffError::Missing => ERROR_MISSING,
        HandoffError::Expired => ERROR_EXPIRED,
        HandoffError::LeaseExpired => ERROR_LEASE_EXPIRED,
        HandoffError::AlreadyClaimed => ERROR_BUSY,
        HandoffError::WrongTarget | HandoffError::WrongKind => ERROR_INVALID,
        HandoffError::Storage | HandoffError::UnsafeStorage => ERROR_CLAIM_STORAGE,
        _ => ERROR_INVALID,
    }
}

/// Map an operation that is trying to put a claimed envelope back into the
/// pending queue. Terminal lifecycle failures mean that this process no
/// longer owns a usable claim; storage failures keep the exact claim in the
/// native slot so a bounded frontend retry can try the same id/token again.
pub fn map_restore_error(error: &HandoffError) -> &'static str {
    match error {
        HandoffError::Missing => ERROR_MISSING,
        HandoffError::Expired => ERROR_EXPIRED,
        HandoffError::LeaseExpired => ERROR_LEASE_EXPIRED,
        HandoffError::Storage | HandoffError::UnsafeStorage => ERROR_RESTORE,
        HandoffError::Corrupt | HandoffError::TooLarge => ERROR_INVALID,
        _ => map_claim_error(error),
    }
}

/// Errors that prove the native claim is no longer usable.  Storage failures
/// deliberately stay outside this set so an action can retain its exact
/// claim for a bounded retry.
pub fn is_terminal_claim_error(error: &HandoffError) -> bool {
    matches!(
        error,
        HandoffError::InvalidRequest
            | HandoffError::InvalidPayload
            | HandoffError::TooLarge
            | HandoffError::Missing
            | HandoffError::AlreadyClaimed
            | HandoffError::WrongTarget
            | HandoffError::WrongKind
            | HandoffError::Expired
            | HandoffError::LeaseExpired
            | HandoffError::TokenMismatch
            | HandoffError::Corrupt
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claim(source_app: &str, payload: serde_json::Value) -> HandoffClaim {
        HandoffClaim {
            envelope: devbox_applink::HandoffEnvelope {
                protocol_version: devbox_applink::PROTOCOL_VERSION,
                id: "a".repeat(32),
                kind: HANDOFF_KIND.into(),
                source_app: source_app.into(),
                target_app: Some(CONSUMER_APP.into()),
                created_at_ms: 1,
                expires_at_ms: 10,
                payload,
            },
            claim_token: "b".repeat(32),
            lease_until_ms: 5,
        }
    }

    #[test]
    fn run_payload_keeps_the_existing_strict_identity_contract() {
        let source = parse_claim(&claim(
            RUN_SOURCE_APP,
            json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            }),
        ))
        .expect("run source");
        assert_eq!(
            source,
            SourceSpec::Run {
                source_id: "run-manager:run-1:stdout".into()
            }
        );
    }

    #[test]
    fn wsl_file_and_journal_payloads_are_allowlisted_and_fixed() {
        let file = parse_claim(&claim(
            WSL_SOURCE_APP,
            json!({
                "sourceType": "wslFile",
                "distro": "Ubuntu",
                "wslPath": "/var/log/app.log"
            }),
        ))
        .expect("wsl file");
        assert_eq!(
            file,
            SourceSpec::WslFile {
                distro: "Ubuntu".into(),
                path: "/var/log/app.log".into()
            }
        );

        let journal = parse_claim(&claim(
            WSL_SOURCE_APP,
            json!({
                "sourceType": "wslJournal",
                "distro": "Ubuntu",
                "unit": null
            }),
        ))
        .expect("wsl journal");
        assert_eq!(
            journal,
            SourceSpec::WslJournal {
                distro: "Ubuntu".into(),
                unit: None
            }
        );
    }

    #[test]
    fn wrong_source_family_and_raw_path_fields_are_rejected() {
        let wsl_from_run = claim(
            RUN_SOURCE_APP,
            json!({
                "sourceType": "wslFile",
                "distro": "Ubuntu",
                "wslPath": "/var/log/app.log"
            }),
        );
        assert!(parse_claim(&wsl_from_run).is_err());

        let extra = claim(
            RUN_SOURCE_APP,
            json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout",
                "path": "/secret/run.log"
            }),
        );
        assert!(parse_claim(&extra).is_err());

        let unsafe_wsl_path = claim(
            WSL_SOURCE_APP,
            json!({
                "sourceType": "wslFile",
                "distro": "Ubuntu",
                "wslPath": "/"
            }),
        );
        assert!(parse_claim(&unsafe_wsl_path).is_err());
    }

    #[test]
    fn protocol_identity_and_lease_are_rechecked_at_the_receiver_boundary() {
        let mut unsupported = claim(
            RUN_SOURCE_APP,
            json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            }),
        );
        unsupported.envelope.protocol_version += 1;
        assert!(parse_claim(&unsupported).is_err());

        let mut malformed_id = claim(
            RUN_SOURCE_APP,
            json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            }),
        );
        malformed_id.envelope.id = "../secret".into();
        assert!(parse_claim(&malformed_id).is_err());

        let mut expired_lease = claim(
            RUN_SOURCE_APP,
            json!({
                "kind": "log-source/v1",
                "sourceId": "run-manager:run-1:stdout",
                "runId": "run-1",
                "stream": "stdout"
            }),
        );
        expired_lease.lease_until_ms = 1;
        assert!(parse_claim(&expired_lease).is_err());
    }

    #[test]
    fn claimed_source_can_restore_for_retry_then_ack_once() {
        let root = tempfile::tempdir().expect("handoff root");
        let store = devbox_applink::HandoffStore::new(root.path().join("handoff/v1"));
        let payload = json!({
            "kind": "log-source/v1",
            "sourceId": "run-manager:run-1:stderr",
            "runId": "run-1",
            "stream": "stderr"
        });
        let descriptor = store
            .create(
                devbox_applink::CreateHandoff {
                    kind: HANDOFF_KIND.into(),
                    source_app: RUN_SOURCE_APP.into(),
                    target_app: Some(CONSUMER_APP.into()),
                    payload,
                },
                1,
            )
            .expect("publish");
        let first = store
            .claim(&descriptor.id, HANDOFF_KIND, CONSUMER_APP, 2)
            .expect("claim");
        assert!(parse_claim(&first).is_ok());
        store.restore(&first, CONSUMER_APP, 3).expect("restore");
        let retry = store
            .claim(&descriptor.id, HANDOFF_KIND, CONSUMER_APP, 4)
            .expect("retry claim");
        store.ack(&retry, CONSUMER_APP, 5).expect("ack");
        assert!(matches!(
            store.claim(&descriptor.id, HANDOFF_KIND, CONSUMER_APP, 6),
            Err(devbox_applink::HandoffError::Missing)
        ));
    }

    #[test]
    fn public_error_mapping_separates_terminal_claims_from_restore_failures() {
        assert_eq!(map_claim_error(&HandoffError::Missing), ERROR_MISSING);
        assert_eq!(map_claim_error(&HandoffError::Expired), ERROR_EXPIRED);
        assert_eq!(
            map_claim_error(&HandoffError::LeaseExpired),
            ERROR_LEASE_EXPIRED
        );
        assert_eq!(map_claim_error(&HandoffError::Storage), ERROR_CLAIM_STORAGE);
        assert_eq!(map_restore_error(&HandoffError::Storage), ERROR_RESTORE);
        assert!(!map_restore_error(&HandoffError::Storage).contains("secret"));
        assert!(is_terminal_claim_error(&HandoffError::Expired));
        assert!(is_terminal_claim_error(&HandoffError::Corrupt));
        assert!(is_terminal_claim_error(&HandoffError::AlreadyClaimed));
        assert!(!is_terminal_claim_error(&HandoffError::Storage));
    }
}
