//! Ephemeral source-owned, bounded/redacted Webhook projection. No raw capture.
use applink::{validate_webhook_log_payload, WebhookLogPayload};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub const TTL_MS: u64 = 120_000;
const MAX_PENDING: usize = 32;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub id: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub payload: WebhookLogPayload,
}
impl Artifact {
    pub fn revision(&self) -> Result<String> {
        if !crate::commands::opaque_id(&self.id)
            || self.created_at_ms == 0
            || self.expires_at_ms.checked_sub(self.created_at_ms) != Some(TTL_MS)
        {
            return Err("webhook_log_invalid");
        }
        validate_webhook_log_payload(&self.payload).map_err(|_| "webhook_log_invalid")?;
        let bytes = serde_json::to_vec(self).map_err(|_| "webhook_log_invalid")?;
        if bytes.len() > 24 * 1024 {
            return Err("webhook_log_limit");
        }
        Ok(Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    pub fn require(&self, id: &str, revision: &str, now: u64) -> Result<()> {
        if self.id != id
            || self.revision()? != revision
            || now < self.created_at_ms
            || now >= self.expires_at_ms
        {
            return Err("webhook_log_stale");
        }
        Ok(())
    }
}
struct Entry {
    artifact: Artifact,
    claim: Option<String>,
    acknowledged: bool,
}
#[derive(Default)]
pub struct Store(BTreeMap<String, Entry>);
impl Store {
    pub fn publish(
        &mut self,
        id: String,
        payload: WebhookLogPayload,
        now: u64,
    ) -> Result<Artifact> {
        self.0.retain(|_, entry| entry.artifact.expires_at_ms > now);
        if self.0.len() >= MAX_PENDING || self.0.contains_key(&id) {
            return Err("webhook_log_limit");
        }
        let artifact = Artifact {
            id: id.clone(),
            created_at_ms: now,
            expires_at_ms: now.checked_add(TTL_MS).ok_or("webhook_log_invalid")?,
            payload,
        };
        artifact.revision()?;
        self.0.insert(
            id,
            Entry {
                artifact: artifact.clone(),
                claim: None,
                acknowledged: false,
            },
        );
        Ok(artifact)
    }
    pub fn claim(
        &mut self,
        id: &str,
        revision: &str,
        operation: &str,
        now: u64,
    ) -> Result<Artifact> {
        if !crate::commands::opaque_id(operation) {
            return Err("webhook_log_invalid");
        }
        let entry = self.0.get_mut(id).ok_or("webhook_log_stale")?;
        entry.artifact.require(id, revision, now)?;
        if entry.acknowledged
            || entry
                .claim
                .as_deref()
                .is_some_and(|claim| claim != operation)
        {
            return Err("webhook_log_claimed");
        }
        entry.claim = Some(operation.into());
        Ok(entry.artifact.clone())
    }
    pub fn acknowledge(
        &mut self,
        id: &str,
        revision: &str,
        operation: &str,
        now: u64,
    ) -> Result<()> {
        let entry = self.0.get_mut(id).ok_or("webhook_log_stale")?;
        entry.artifact.require(id, revision, now)?;
        if entry.claim.as_deref() != Some(operation) {
            return Err("webhook_log_claimed");
        }
        entry.acknowledged = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload() -> WebhookLogPayload {
        applink::webhook_log_payload(
            "POST",
            "/events?token=synthetic-secret",
            1_788_000_000_000,
            &[("Authorization".into(), "Bearer synthetic-secret".into())],
            r#"{"token":"synthetic-secret","message":"ordinary"}"#,
        )
        .unwrap()
    }
    #[test]
    fn source_claims_are_bounded_redacted_revision_bound_and_one_time() {
        let mut store = Store::default();
        let artifact = store.publish("source-one".into(), payload(), 1000).unwrap();
        let revision = artifact.revision().unwrap();
        assert!(!serde_json::to_string(&artifact)
            .unwrap()
            .contains("synthetic-secret"));
        assert!(store
            .claim(&artifact.id, &"0".repeat(64), "op", 1001)
            .is_err());
        store.claim(&artifact.id, &revision, "op", 1001).unwrap();
        store.claim(&artifact.id, &revision, "op", 1002).unwrap();
        assert!(store.claim(&artifact.id, &revision, "other", 1002).is_err());
        assert!(store
            .acknowledge(&artifact.id, &revision, "other", 1003)
            .is_err());
        store
            .acknowledge(&artifact.id, &revision, "op", 1003)
            .unwrap();
        store
            .acknowledge(&artifact.id, &revision, "op", 1004)
            .unwrap();
        assert!(store.claim(&artifact.id, &revision, "op", 1004).is_err());
        assert!(artifact
            .require(&artifact.id, &revision, artifact.expires_at_ms)
            .is_err());
        for n in 1..MAX_PENDING {
            store
                .publish(format!("source-{n}"), payload(), 1000)
                .unwrap();
        }
        assert!(store.publish("full".into(), payload(), 1000).is_err());
        store
            .publish("new".into(), payload(), 1000 + TTL_MS)
            .unwrap();
    }
}
