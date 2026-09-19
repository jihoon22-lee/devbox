//! Explicitly saved, masked API Studio result consumed by native Knowledge.
use crate::references::{OwnedArtifactKind, OwnedArtifactReference};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub const KIND: &str = "knowledge-result/v1";
pub const PRODUCER: &str = "devbox-api-studio";
pub const COMPONENTS: [&str; 2] = ["api-studio.api", "api-studio.transforms"];
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Draft {
    pub artifact: OwnedArtifactReference,
    pub created_at_ms: u64,
    pub title: String,
    pub body: String,
    pub redacted: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub artifact: OwnedArtifactReference,
    pub created_at_ms: u64,
    pub title: String,
    pub redacted: bool,
}
impl Draft {
    pub fn summary(&self) -> Summary {
        Summary {
            artifact: self.artifact.clone(),
            created_at_ms: self.created_at_ms,
            title: self.title.clone(),
            redacted: self.redacted,
        }
    }
}

impl Draft {
    pub fn validate_for(&self, component: &str) -> Result<(), &'static str> {
        let provenance = &self.artifact.provenance;
        let id = self.artifact.id.as_bytes();
        let valid_id = id.len() == 36
            && id.iter().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
                }
            });
        let label = match component {
            "api-studio.api" => "Requests",
            "api-studio.transforms" => "Transforms",
            _ => return Err("knowledge_owner_invalid"),
        };
        if self.artifact.kind != OwnedArtifactKind::KnowledgeDraft
            || !valid_id
            || provenance.product != "api-studio"
            || provenance.component != component
            || provenance.revision == 0
            || provenance.revision > 9_007_199_254_740_991
            || provenance.request_id.is_empty()
            || provenance.request_id.len() > 64
            || !provenance
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.created_at_ms == 0
            || self.created_at_ms > 9_007_199_254_740_991
            || self.title != format!("API Studio · {label} 결과")
            || self.body.len() > 512 * 1024
            || self.body.trim().is_empty()
            || self.body.chars().count() > 256_000
            || self
                .body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || applink::validate_handoff_text(&self.body).is_err()
        {
            return Err("knowledge_draft_invalid");
        }
        Ok(())
    }
    pub fn revision(&self) -> Result<String, &'static str> {
        self.validate_for(&self.artifact.provenance.component)?;
        Ok(
            Sha256::digest(serde_json::to_vec(self).map_err(|_| "knowledge_draft_invalid")?)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> Draft {
        Draft {
            artifact: OwnedArtifactReference {
                provenance: crate::Provenance {
                    product: "api-studio".into(),
                    component: "api-studio.transforms".into(),
                    request_id: "fixture".into(),
                    revision: 1,
                },
                id: "11111111-1111-4111-8111-111111111111".into(),
                kind: OwnedArtifactKind::KnowledgeDraft,
            },
            created_at_ms: 1000,
            title: "API Studio · Transforms 결과".into(),
            body: "synthetic result".into(),
            redacted: false,
        }
    }
    #[test]
    fn received_results_reject_foreign_owner_and_track_content_revision() {
        let source = draft();
        assert!(source.validate_for("api-studio.transforms").is_ok());
        assert!(source.validate_for("api-studio.api").is_err());
        assert!(source.validate_for("api-studio.protocols").is_err());
        let mut changed = source.clone();
        changed.body.push_str(" changed");
        assert_ne!(source.revision().unwrap(), changed.revision().unwrap());
        changed.artifact.provenance.product = "workspace".into();
        assert!(changed.revision().is_err());
        let mut value = serde_json::to_value(source).unwrap();
        value["rawHeaders"] = serde_json::json!({});
        assert!(serde_json::from_value::<Draft>(value).is_err());
    }
}
