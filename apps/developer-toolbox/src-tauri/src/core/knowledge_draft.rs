//! Strict Developer Toolbox -> Knowledge `knowledge-draft/v2` payload.

use serde::{Deserialize, Serialize};

pub const HANDOFF_KIND: &str = "knowledge-draft/v2";
pub const SOURCE_APP: &str = "developer-toolbox";
pub const TARGET_APP: &str = "knowledge-base";
pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_BODY_BYTES: usize = 512 * 1024;
pub const MAX_BODY_CHARS: usize = 256_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolboxKnowledgeDraftPayload {
    pub schema_version: u32,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub created_date: String,
}

pub fn build_payload(
    output: &str,
    created_date: &str,
) -> Result<(ToolboxKnowledgeDraftPayload, bool), &'static str> {
    if output.len() > MAX_BODY_BYTES
        || output.chars().count() > MAX_BODY_CHARS
        || !valid_date(created_date)
    {
        return Err("knowledge-draft-invalid");
    }
    let redacted =
        devbox_applink::redact_handoff_text(output).map_err(|_| "knowledge-draft-invalid")?;
    if redacted.text.len() > MAX_BODY_BYTES
        || redacted.text.chars().count() > MAX_BODY_CHARS
        || devbox_applink::validate_handoff_text(&redacted.text).is_err()
    {
        return Err("knowledge-draft-invalid");
    }
    Ok((
        ToolboxKnowledgeDraftPayload {
            schema_version: SCHEMA_VERSION,
            title: format!("Developer Toolbox result · {created_date}"),
            body: redacted.text,
            tags: vec!["developer-toolbox".to_string(), "draft".to_string()],
            created_date: created_date.to_string(),
        },
        redacted.redacted,
    ))
}

fn valid_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return false;
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_fixed_metadata_draft_and_redacts_secret_lines() {
        let (payload, redacted) =
            build_payload("safe\npassword=raw-value\nresult", "2026-08-30").unwrap();
        assert!(redacted);
        assert_eq!(payload.schema_version, 2);
        assert_eq!(payload.title, "Developer Toolbox result · 2026-08-30");
        assert_eq!(payload.tags, ["developer-toolbox", "draft"]);
        assert_eq!(payload.body, "safe\n[REDACTED]\nresult");
    }

    #[test]
    fn rejects_invalid_dates_empty_controls_and_bounds() {
        assert!(build_payload("safe", "2026-02-29").is_err());
        assert!(build_payload("\0", "2026-08-30").is_err());
        assert!(build_payload(&"x".repeat(MAX_BODY_BYTES + 1), "2026-08-30").is_err());
    }
}
