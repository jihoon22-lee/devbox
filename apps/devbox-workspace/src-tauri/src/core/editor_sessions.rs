//! Product session metadata with retained receipt decoding. No file
//! path here grants access: the native Files owner supplies eligible paths.
use code_pad_lib::core::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub snapshot_id: String,
    // Original document IDs are retained verbatim in the destination session.
    pub document_ids: Vec<String>,
    pub recent_files: Vec<String>,
    pub source_workspace: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredSession {
    schema_version: u32,
    pub session: Session,
    pub imports: Vec<Receipt>,
}
impl Default for StoredSession {
    fn default() -> Self {
        Self {
            schema_version: 1,
            session: Session::empty(),
            imports: Vec::new(),
        }
    }
}
impl StoredSession {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| "invalid_files_store")?;
        let session_bytes = if raw.get("schemaVersion").is_some() {
            serde_json::to_vec(raw.get("session").ok_or("invalid_files_store")?)
                .map_err(|_| "invalid_files_store")?
        } else {
            bytes.to_vec()
        };
        let text = std::str::from_utf8(&session_bytes).map_err(|_| "invalid_files_store")?;
        let parsed = Session::from_json(text).map_err(|_| "invalid_files_store")?;
        let normalized = serde_json::to_value(parsed).map_err(|_| "invalid_files_store")?;
        let input: serde_json::Value =
            serde_json::from_slice(&session_bytes).map_err(|_| "invalid_files_store")?;
        if !known_shape(&input, &normalized, true) {
            return Err("invalid_files_store");
        }
        let record: Self = if raw.get("schemaVersion").is_some() {
            serde_json::from_value(raw).map_err(|_| "invalid_files_store")?
        } else {
            code_pad_lib::component::validate_persistent_file("session.json", bytes)?;
            Self {
                session: serde_json::from_value(raw).map_err(|_| "invalid_files_store")?,
                ..Self::default()
            }
        };
        record.validate()?;
        Ok(record)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        // Preserve the established raw shape until an import needs an atomic
        // receipt. Subsequent editor saves retain the receipt in this file.
        let bytes = if self.imports.is_empty() {
            serde_json::to_vec(&self.session)
        } else {
            serde_json::to_vec(self)
        }
        .map_err(|_| "invalid_files_store")?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("files_store_limit");
        }
        Ok(bytes)
    }
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || self.imports.len() > 32 || self.session.validate().is_err() {
            return Err("invalid_files_store");
        }
        for receipt in &self.imports {
            if receipt.snapshot_id.len() != 64
                || !receipt
                    .snapshot_id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || receipt.document_ids.len() > 64
                || receipt.document_ids.iter().any(|id| id.trim().is_empty())
                || receipt.document_ids.iter().collect::<BTreeSet<_>>().len()
                    != receipt.document_ids.len()
            {
                return Err("invalid_files_store");
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_envelopes_reject_future_or_unknown_fields_without_dropping_import_receipts() {
        let session = Session::empty();
        let mut unknown = serde_json::to_value(&session).unwrap();
        unknown["future_setting"] = serde_json::json!(true);
        assert!(StoredSession::decode(&serde_json::to_vec(&unknown).unwrap()).is_err());
        let future = serde_json::json!({"schemaVersion":2,"session":session,"imports":[]});
        assert!(StoredSession::decode(&serde_json::to_vec(&future).unwrap()).is_err());
        let invalid = serde_json::json!({"schemaVersion":1,"session":session,"imports":[{"snapshotId":"../foreign","documentIds":[],"recentFiles":[],"sourceWorkspace":null}]});
        assert!(StoredSession::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
}

use serde_json::Value;
fn known_shape(raw: &Value, normalized: &Value, cursor_aliases: bool) -> bool {
    match (raw, normalized) {
        (Value::Object(raw), Value::Object(normalized)) => raw.iter().all(|(key, value)| {
            let key =
                if cursor_aliases && matches!(key.as_str(), "cursorPosition" | "cursor_position") {
                    "cursor"
                } else if key == "source_id" && normalized.contains_key("sourceId") {
                    "sourceId"
                } else if key == "container_id" && normalized.contains_key("containerId") {
                    "containerId"
                } else {
                    key
                };
            normalized
                .get(key)
                .is_some_and(|expected| known_shape(value, expected, cursor_aliases))
        }),
        (Value::Array(raw), Value::Array(normalized)) => {
            raw.len() == normalized.len()
                && raw
                    .iter()
                    .zip(normalized)
                    .all(|(a, b)| known_shape(a, b, cursor_aliases))
        }
        _ => raw == normalized,
    }
}
