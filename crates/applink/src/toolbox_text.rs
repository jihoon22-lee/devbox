//! Typed, privacy-bounded text handoff consumed by Developer Toolbox.

use crate::{redact_handoff_text, validate_handoff_text, HandoffClaim};
use serde::{Deserialize, Serialize};

pub const TOOLBOX_TEXT_HANDOFF_KIND: &str = "toolbox-text/v1";
pub const TOOLBOX_TEXT_TARGET_APP: &str = "developer-toolbox";
pub const TOOLBOX_TEXT_MAX_BYTES: usize = 512 * 1024;
pub const TOOLBOX_TEXT_MAX_CHARS: usize = 256_000;

const ALLOWED_SOURCES: [&str; 3] = ["api-playground", "devbox-launcher", "log-lens"];

/// The v1 payload deliberately stays a single text field.  Producer identity
/// comes from the signed-by-storage envelope, while Developer Toolbox performs
/// local structure detection after the user applies the preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolboxTextPayload {
    pub text: String,
}

impl ToolboxTextPayload {
    /// Build a payload from an explicit selection, redacting credential-shaped
    /// lines before the shared handoff store performs its own validation.
    pub fn from_selected_text(source_app: &str, text: &str) -> Result<(Self, bool), &'static str> {
        if !valid_source(source_app)
            || text.len() > TOOLBOX_TEXT_MAX_BYTES
            || text.chars().count() > TOOLBOX_TEXT_MAX_CHARS
        {
            return Err("toolbox-text-invalid");
        }
        let redacted = redact_handoff_text(text).map_err(|_| "toolbox-text-invalid")?;
        let payload = Self {
            text: redacted.text,
        };
        payload.validate()?;
        Ok((payload, redacted.redacted))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.text.trim().is_empty()
            || self.text.len() > TOOLBOX_TEXT_MAX_BYTES
            || self.text.chars().count() > TOOLBOX_TEXT_MAX_CHARS
            || self
                .text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            || validate_handoff_text(&self.text).is_err()
        {
            return Err("toolbox-text-invalid");
        }
        Ok(())
    }

    pub fn from_claim(claim: &HandoffClaim) -> Result<Self, &'static str> {
        if claim.envelope.kind != TOOLBOX_TEXT_HANDOFF_KIND
            || !valid_source(&claim.envelope.source_app)
            || claim.envelope.target_app.as_deref() != Some(TOOLBOX_TEXT_TARGET_APP)
        {
            return Err("toolbox-text-invalid");
        }
        let payload: Self = serde_json::from_value(claim.envelope.payload.clone())
            .map_err(|_| "toolbox-text-invalid")?;
        payload.validate()?;
        Ok(payload)
    }
}

fn valid_source(source_app: &str) -> bool {
    ALLOWED_SOURCES.contains(&source_app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HandoffEnvelope, PROTOCOL_VERSION};

    fn claim(source_app: &str, payload: serde_json::Value) -> HandoffClaim {
        HandoffClaim {
            envelope: HandoffEnvelope {
                protocol_version: PROTOCOL_VERSION,
                id: "a".repeat(32),
                kind: TOOLBOX_TEXT_HANDOFF_KIND.to_string(),
                source_app: source_app.to_string(),
                target_app: Some(TOOLBOX_TEXT_TARGET_APP.to_string()),
                created_at_ms: 1,
                expires_at_ms: 10,
                payload,
            },
            claim_token: "b".repeat(32),
            lease_until_ms: 9,
        }
    }

    #[test]
    fn producer_masks_credentials_before_building_the_payload() {
        let (payload, redacted) = ToolboxTextPayload::from_selected_text(
            "api-playground",
            "status=ok\nAuthorization: Bearer raw-value\nbody",
        )
        .unwrap();
        assert!(redacted);
        assert_eq!(payload.text, "status=ok\n[REDACTED]\nbody");
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn claim_requires_an_exact_source_target_kind_and_shape() {
        let payload = serde_json::json!({"text": "safe selected text"});
        assert_eq!(
            ToolboxTextPayload::from_claim(&claim("log-lens", payload.clone()))
                .unwrap()
                .text,
            "safe selected text"
        );
        assert!(ToolboxTextPayload::from_claim(&claim("unknown-app", payload.clone())).is_err());
        assert!(ToolboxTextPayload::from_claim(&claim(
            "api-playground",
            serde_json::json!({"text": "safe", "extra": true})
        ))
        .is_err());

        let mut wrong_target = claim("devbox-launcher", payload);
        wrong_target.envelope.target_app = Some("knowledge-base".to_string());
        assert!(ToolboxTextPayload::from_claim(&wrong_target).is_err());
    }

    #[test]
    fn text_is_bounded_and_raw_credentials_are_rejected_on_claim() {
        assert!(ToolboxTextPayload::from_selected_text("unknown-app", "safe").is_err());
        assert!(ToolboxTextPayload::from_selected_text("log-lens", "\0").is_err());
        assert!(ToolboxTextPayload::from_selected_text(
            "log-lens",
            &"x".repeat(TOOLBOX_TEXT_MAX_BYTES + 1)
        )
        .is_err());
        assert!(ToolboxTextPayload::from_claim(&claim(
            "api-playground",
            serde_json::json!({"text": "Bearer raw-value"})
        ))
        .is_err());
    }
}
