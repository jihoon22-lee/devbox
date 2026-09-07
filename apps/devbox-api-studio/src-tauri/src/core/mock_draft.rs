//! One-time, data-only response draft. The recipient alone constructs a rule;
//! preview/accept never changes the listener or the active rule collection.
use applink::{CreateHandoff, HandoffClaim, HandoffStore, OpenRequest, OpenTarget};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use webhook_core::core::rules::{self, ResponseRule};

pub const KIND: &str = "mock-rule-draft/v1";
pub const TARGET: &str = "webhook-lab";
const INVALID: &str = "mock_draft_invalid";
const EXPIRED: &str = "mock_draft_expired";
const STORAGE: &str = "mock_draft_unavailable";
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MediaType {
    Json,
    #[default]
    Text,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Payload {
    schema_version: u8,
    // This is an HTTP matching target, never a filesystem path or executable.
    request_target: String,
    request_method: Option<String>,
    response_status: u16,
    response_body: String,
    media_type: MediaType,
    redacted: bool,
}
impl Payload {
    fn rule(&self) -> Result<ResponseRule, String> {
        if self.schema_version != 1
            || (!self.response_body.is_empty()
                && applink::validate_handoff_text(&self.response_body).is_err())
            || applink::validate_handoff_text(&self.request_target).is_err()
        {
            return Err(INVALID.into());
        }
        let rule = ResponseRule {
            id: String::new(),
            priority: 0,
            method: self.request_method.clone(),
            path: self.request_target.clone(),
            status: self.response_status,
            headers: vec![(
                "Content-Type".into(),
                match self.media_type {
                    MediaType::Json => "application/json",
                    MediaType::Text => "text/plain; charset=utf-8",
                }
                .into(),
            )],
            body: self.response_body.clone(),
            delay_ms: 0,
            sequence: vec![],
        };
        rules::validate_rule(&rule).map_err(|_| INVALID.to_string())?;
        Ok(rule)
    }
}
pub fn prepare(component: &str, args: Value) -> Result<(CreateHandoff, bool), String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        output: String,
        source: Option<transforms_core::core::export_policy::OutputSource>,
        #[serde(default = "default_status")]
        status: u16,
        #[serde(default)]
        media_type: MediaType,
        #[serde(default = "default_target")]
        request_target: String,
        request_method: Option<String>,
    }
    fn default_status() -> u16 {
        200
    }
    fn default_target() -> String {
        "/".into()
    }
    let Input {
        output,
        source,
        status,
        media_type,
        request_target,
        request_method,
    } = serde_json::from_value(args).map_err(|_| INVALID)?;
    let output = zeroize::Zeroizing::new(output);
    let producer = match component {
        "api-studio.api" if source.is_none() => "api-playground",
        "api-studio.transforms" => {
            source
                .ok_or("transform_export_denied")?
                .require_exportable()?;
            "developer-toolbox"
        }
        _ => return Err(INVALID.into()),
    };
    if output.len() > rules::MAX_BODY_BYTES || output.chars().count() > rules::MAX_BODY_CHARS {
        return Err(INVALID.into());
    }
    let (body, redacted) = if output.is_empty() {
        (String::new(), false)
    } else {
        let masked = applink::redact_handoff_text(&output).map_err(|_| INVALID)?;
        (masked.text, masked.redacted)
    };
    let payload = Payload {
        schema_version: 1,
        request_target,
        request_method,
        response_status: status,
        response_body: body,
        media_type,
        redacted,
    };
    payload.rule()?;
    Ok((
        CreateHandoff {
            kind: KIND.into(),
            source_app: producer.into(),
            target_app: Some(TARGET.into()),
            payload: serde_json::to_value(payload).map_err(|_| INVALID)?,
        },
        redacted,
    ))
}

struct Pending {
    id: String,
    producer: String,
    expires: u64,
    claim: Option<HandoffClaim>,
}
pub struct Receiver {
    store: HandoffStore,
    pending: Mutex<Option<Pending>>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: String,
    pub producer: String,
    pub expires_at_ms: u64,
    pub rule: ResponseRule,
    pub redacted: bool,
}
impl Receiver {
    pub fn new(store: HandoffStore) -> Self {
        Self {
            store,
            pending: Mutex::new(None),
        }
    }
    pub fn offer(&self, request: OpenRequest, now: u64) -> Result<(), String> {
        let OpenTarget::Handoff { id, kind } = request.target else {
            return Err(INVALID.into());
        };
        let producer = request.from.ok_or(INVALID)?;
        if kind != KIND
            || id.len() != 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || !matches!(producer.as_str(), "api-playground" | "developer-toolbox")
        {
            return Err(INVALID.into());
        }
        let mut slot = self.pending.lock().map_err(|_| STORAGE)?;
        if slot.as_ref().is_some_and(|p| p.expires > now) {
            return Err("mock_draft_busy".into());
        }
        *slot = Some(Pending {
            id,
            producer,
            expires: now
                .checked_add(applink::DEFAULT_HANDOFF_TTL_MS)
                .ok_or(INVALID)?,
            claim: None,
        });
        Ok(())
    }
    pub fn preview(&self, now: u64) -> Result<Option<Preview>, String> {
        let mut slot = self.pending.lock().map_err(|_| STORAGE)?;
        let Some(pending) = slot.as_mut() else {
            return Ok(None);
        };
        let claim = match &pending.claim {
            Some(claim) => self
                .store
                .renew(claim, TARGET, now, applink::DEFAULT_CLAIM_LEASE_MS),
            None => self.store.claim(&pending.id, KIND, TARGET, now),
        };
        let claim = match claim {
            Ok(claim) => claim,
            Err(error) => {
                if matches!(
                    error,
                    applink::HandoffError::Expired
                        | applink::HandoffError::LeaseExpired
                        | applink::HandoffError::Missing
                ) {
                    slot.take();
                    return Err(EXPIRED.into());
                }
                return Err(STORAGE.into());
            }
        };
        let payload = Self::payload(&claim, &pending.producer);
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                let _ = self.store.restore(&claim, TARGET, now);
                slot.take();
                return Err(error);
            }
        };
        let rule = payload.rule()?;
        let result = Preview {
            id: pending.id.clone(),
            producer: pending.producer.clone(),
            expires_at_ms: claim.envelope.expires_at_ms,
            rule,
            redacted: payload.redacted,
        };
        pending.claim = Some(claim);
        Ok(Some(result))
    }
    fn payload(claim: &HandoffClaim, producer: &str) -> Result<Payload, String> {
        if claim.envelope.source_app != producer
            || claim.envelope.kind != KIND
            || claim.envelope.target_app.as_deref() != Some(TARGET)
        {
            return Err(INVALID.into());
        }
        let payload: Payload =
            serde_json::from_value(claim.envelope.payload.clone()).map_err(|_| INVALID)?;
        payload.rule()?;
        Ok(payload)
    }
    pub fn finish(&self, id: &str, now: u64) -> Result<ResponseRule, String> {
        let mut slot = self.pending.lock().map_err(|_| STORAGE)?;
        let pending = slot.as_ref().filter(|p| p.id == id).ok_or(INVALID)?;
        let claim = pending.claim.as_ref().ok_or(INVALID)?;
        let rule = Self::payload(claim, &pending.producer)?.rule()?;
        if let Err(error) = self.store.ack(claim, TARGET, now) {
            if matches!(
                error,
                applink::HandoffError::Expired
                    | applink::HandoffError::LeaseExpired
                    | applink::HandoffError::Missing
            ) {
                slot.take();
                return Err(EXPIRED.into());
            }
            return Err(STORAGE.into());
        }
        slot.take();
        Ok(rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rule_is_masked_bounded_and_cannot_inherit_headers_or_execution() {
        let (draft, redacted) = prepare(
            "api-studio.api",
            json!({"output":"safe\nAuthorization: Bearer synthetic", "status":201}),
        )
        .unwrap();
        assert!(redacted);
        assert!(!draft.payload.to_string().contains("synthetic"));
        let payload: Payload = serde_json::from_value(draft.payload).unwrap();
        let rule = payload.rule().unwrap();
        assert_eq!(
            (
                rule.id.as_str(),
                rule.path.as_str(),
                rule.status,
                rule.priority,
                rule.delay_ms
            ),
            ("", "/", 201, 0, 0)
        );
        assert!(rule.sequence.is_empty());
        assert_eq!(rule.headers.len(), 1);
        for input in [
            json!({"output":"safe","headers":[["Authorization","secret"]]}),
            json!({"output":"safe","status":999}),
            json!({"output":"safe","requestTarget":"file:///tmp/file"}),
        ] {
            assert!(prepare("api-studio.api", input).is_err());
        }
        assert!(prepare(
            "api-studio.transforms",
            json!({"output":"tag","source":{"kind":"tool","toolId":"hmac"}})
        )
        .is_err());
    }
    #[test]
    fn receiver_retains_busy_preview_and_consumes_exactly_once() {
        let root = tempfile::tempdir().unwrap();
        let store = HandoffStore::new(root.path().join("handoff"));
        let receiver = Receiver::new(store.clone());
        let (draft, _) = prepare("api-studio.api", json!({"output":"fixture"})).unwrap();
        let descriptor = store.create(draft, 1000).unwrap();
        let id = descriptor.id.clone();
        let request = OpenRequest {
            target: descriptor.into(),
            from: Some("api-playground".into()),
        };
        receiver.offer(request.clone(), 1000).unwrap();
        assert_eq!(receiver.offer(request, 1001), Err("mock_draft_busy".into()));
        let preview = receiver.preview(1002).unwrap().unwrap();
        assert_eq!(preview.rule.body, "fixture");
        assert!(store.claim(&id, KIND, TARGET, 1003).is_err());
        assert!(receiver.finish(&"b".repeat(32), 1003).is_err());
        assert!(receiver.preview(1004).unwrap().is_some());
        assert_eq!(receiver.finish(&id, 1005).unwrap().body, "fixture");
        assert!(receiver.finish(&id, 1006).is_err());
        assert!(store.claim(&id, KIND, TARGET, 1006).is_err());
        assert!(receiver.preview(1006).unwrap().is_none());
    }
}
