//! Owner adapters retain the existing transport and secret envelope. These
//! metadata values grant no authority to resolve, decrypt or export anything.
use crate::{ProblemCode, Provenance};
use applink::{
    CreateHandoff, HandoffStore, OpenRequest, ToolboxTextPayload, TOOLBOX_TEXT_HANDOFF_KIND,
    TOOLBOX_TEXT_TARGET_APP,
};
use devbox_secrets::SecretReference;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReference {
    pub provenance: Provenance,
    pub recipient: String,
    pub link: OpenRequest,
}

/// Durable producer-owned draft identity, distinct from a one-time handoff.
/// Resolution requires the native owner's authority; metadata is never a grant.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedArtifactReference {
    pub provenance: Provenance,
    pub id: String,
    pub kind: OwnedArtifactKind,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum OwnedArtifactKind {
    #[serde(rename = "knowledge-draft/v1")]
    KnowledgeDraft,
}

/// Native API adapter for an explicit exportable selection. Callers must first
/// apply their operation's export policy (e.g. Toolbox HMAC is non-exportable).
/// The existing source allowlist, redaction and one-time storage remain intact.
pub fn publish_api_selection(
    store: &HandoffStore,
    provenance: Provenance,
    selection: &str,
    now_ms: u64,
) -> Result<ArtifactReference, ProblemCode> {
    validate_owner(&provenance, SecretOwner::ApiEnvironment)?;
    let (payload, _) = ToolboxTextPayload::from_selected_text("api-playground", selection)
        .map_err(|_| ProblemCode::InvalidRequest)?;
    let descriptor = store
        .create(
            CreateHandoff {
                kind: TOOLBOX_TEXT_HANDOFF_KIND.into(),
                source_app: "api-playground".into(),
                target_app: Some(TOOLBOX_TEXT_TARGET_APP.into()),
                payload: serde_json::to_value(payload).map_err(|_| ProblemCode::InvalidRequest)?,
            },
            now_ms,
        )
        .map_err(|_| ProblemCode::Unavailable)?;
    Ok(ArtifactReference {
        provenance,
        recipient: TOOLBOX_TEXT_TARGET_APP.into(),
        link: OpenRequest {
            target: descriptor.into(),
            from: Some("api-playground".into()),
        },
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SecretOwner {
    ApiEnvironment,
    RuntimeEnvironment,
    ProjectEnvironment,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedSecretReference {
    pub provenance: Provenance,
    pub owner: SecretOwner,
    pub reference: SecretReference,
}

impl OwnedSecretReference {
    pub fn new(
        provenance: Provenance,
        owner: SecretOwner,
        reference: SecretReference,
    ) -> Result<Self, ProblemCode> {
        let value = Self {
            provenance,
            owner,
            reference,
        };
        value.validate_for(owner)?;
        Ok(value)
    }

    /// Expected owner is native configuration, never copied from the request.
    /// This does not resolve a value; the actual platform Sealer stays local.
    pub fn validate_for(&self, expected: SecretOwner) -> Result<(), ProblemCode> {
        if self.owner != expected {
            return Err(ProblemCode::Unauthorized);
        }
        validate_owner(&self.provenance, expected)?;
        let name = &self.reference.name;
        if !self.reference.is_project_environment()
            || name.len() > 128
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || name.as_bytes()[0].is_ascii_digit()
        {
            return Err(ProblemCode::InvalidRequest);
        }
        Ok(())
    }
}

fn validate_owner(provenance: &Provenance, owner: SecretOwner) -> Result<(), ProblemCode> {
    let (product, component) = match owner {
        SecretOwner::ApiEnvironment => ("api-studio", "api-studio.api"),
        SecretOwner::RuntimeEnvironment => ("workspace", "workspace.runtime"),
        SecretOwner::ProjectEnvironment => ("workspace", "workspace.project"),
    };
    if provenance.product != product || provenance.component != component {
        return Err(ProblemCode::Unauthorized);
    }
    if provenance.revision == 0
        || provenance.revision > 9_007_199_254_740_991
        || provenance.request_id.is_empty()
        || provenance.request_id.len() > 64
        || !provenance
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ProblemCode::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use applink::{build_argv, parse_argv, HandoffError, OpenTarget};
    use serde_json::{json, Value};

    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../packages/product-shell/fixtures/references.json"
        ))
        .unwrap()
    }
    fn provenance() -> Provenance {
        serde_json::from_value(fixture()["artifact"]["provenance"].clone()).unwrap()
    }

    #[test]
    fn api_reference_keeps_applink_redaction_claim_restore_and_ack() {
        let root = tempfile::tempdir().unwrap();
        let store = HandoffStore::new(root.path().join("handoff"));
        let artifact = publish_api_selection(
            &store,
            provenance(),
            "safe body\nAuthorization: Bearer synthetic-fixture",
            1000,
        )
        .unwrap();
        let mut value = serde_json::to_value(&artifact).unwrap();
        let OpenTarget::Handoff { id, kind } = &artifact.link.target else {
            panic!("handoff required")
        };
        assert!(!value.to_string().contains("safe body"));
        assert!(!value.to_string().contains("synthetic-fixture"));
        value["link"]["target"]["id"] = fixture()["artifact"]["link"]["target"]["id"].clone();
        assert_eq!(value, fixture()["artifact"]);
        let mut argv = vec!["fixture.exe".into()];
        argv.extend(build_argv(&artifact.link).unwrap());
        assert_eq!(parse_argv(&argv).unwrap(), Some(artifact.link.clone()));
        assert_eq!(
            store.claim(id, kind, "knowledge-base", 1001),
            Err(HandoffError::WrongTarget)
        );
        let claim = store
            .claim(id, kind, TOOLBOX_TEXT_TARGET_APP, 1002)
            .unwrap();
        let payload = ToolboxTextPayload::from_claim(&claim).unwrap();
        assert!(payload.text.contains("[REDACTED]"));
        let mut forged = claim.clone();
        forged.claim_token = "0".repeat(32);
        assert_eq!(
            store.ack(&forged, TOOLBOX_TEXT_TARGET_APP, 1003),
            Err(HandoffError::TokenMismatch)
        );
        store
            .restore(&claim, TOOLBOX_TEXT_TARGET_APP, 1004)
            .unwrap();
        let restored = store
            .claim(id, kind, TOOLBOX_TEXT_TARGET_APP, 1005)
            .unwrap();
        assert_ne!(restored.claim_token, claim.claim_token);
        store.ack(&restored, TOOLBOX_TEXT_TARGET_APP, 1006).unwrap();
        assert!(store
            .claim(id, kind, TOOLBOX_TEXT_TARGET_APP, 1007)
            .is_err());
    }

    #[test]
    fn secret_reference_reuses_v1_and_checks_native_owner_without_secret_bytes() {
        let reference = OwnedSecretReference::new(
            provenance(),
            SecretOwner::ApiEnvironment,
            SecretReference::project_environment("API_TOKEN"),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&reference).unwrap(),
            fixture()["secret"]
        );
        assert_eq!(
            reference.validate_for(SecretOwner::RuntimeEnvironment),
            Err(ProblemCode::Unauthorized)
        );
        for (field, value) in [
            ("kind", json!("secret-ref/v2")),
            ("name", json!("../value")),
            ("name", json!("")),
            ("plaintext", json!("synthetic")),
        ] {
            let mut modified = fixture()["secret"].clone();
            modified["reference"][field] = value;
            assert!(
                serde_json::from_value::<OwnedSecretReference>(modified).map_or(true, |v| v
                    .validate_for(SecretOwner::ApiEnvironment)
                    .is_err())
            );
        }
        let mut foreign = provenance();
        foreign.component = "workspace.runtime".into();
        let root = tempfile::tempdir().unwrap();
        assert!(publish_api_selection(
            &HandoffStore::new(root.path().join("handoff")),
            foreign,
            "safe",
            1000
        )
        .is_err());
        assert!(!root.path().join("handoff").exists());
    }
}
