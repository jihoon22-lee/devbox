//! Reviewed recovery conversion keeps all existing buffers unless a conflicting
//! path is explicitly replaced. Bounds reject the whole write; nothing is evicted.
use code_pad_lib::core::recovery::{RecoveryEntry, RecoveryFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub snapshot_id: String,
    pub paths: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredRecovery {
    schema_version: u32,
    pub recovery: RecoveryFile,
    pub imports: Vec<Receipt>,
}
impl Default for StoredRecovery {
    fn default() -> Self {
        Self {
            schema_version: 1,
            recovery: RecoveryFile::empty(),
            imports: Vec::new(),
        }
    }
}
impl StoredRecovery {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let raw: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| "invalid_files_store")?;
        let record = if raw.get("schemaVersion").is_some() {
            let value = raw.get("recovery").ok_or("invalid_files_store")?;
            validate_recovery(&serde_json::to_vec(value).map_err(|_| "invalid_files_store")?)?;
            serde_json::from_value(raw).map_err(|_| "invalid_files_store")?
        } else {
            validate_recovery(bytes)?;
            Self {
                recovery: serde_json::from_value(raw).map_err(|_| "invalid_files_store")?,
                ..Self::default()
            }
        };
        Self::validate(&record)?;
        Ok(record)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = if self.imports.is_empty() {
            serde_json::to_vec(&self.recovery)
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
        if self.schema_version != 1 || self.imports.len() > 32 || self.recovery.entries.len() > 64 {
            return Err("invalid_files_store");
        }
        validate_recovery(&serde_json::to_vec(&self.recovery).map_err(|_| "invalid_files_store")?)?;
        for receipt in &self.imports {
            if receipt.snapshot_id.len() != 64
                || !receipt
                    .snapshot_id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || receipt.paths.len() > 64
                || receipt.paths.iter().any(|path| path.trim().is_empty())
                || receipt.paths.iter().collect::<BTreeSet<_>>().len() != receipt.paths.len()
            {
                return Err("invalid_files_store");
            }
        }
        Ok(())
    }
    pub fn merge(&self, entries: &[RecoveryEntry]) -> Result<Self> {
        let mut next = self.clone();
        for entry in entries {
            next.recovery.remove(&entry.path);
            next.recovery.entries.push(entry.clone());
        }
        next.validate()?;
        Ok(next)
    }
}
fn validate_recovery(bytes: &[u8]) -> Result<()> {
    let raw: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid_files_store")?;
    let typed: RecoveryFile =
        serde_json::from_value(raw.clone()).map_err(|_| "invalid_files_store")?;
    let normalized = serde_json::to_value(typed).map_err(|_| "invalid_files_store")?;
    if !super::editor_sessions::known_shape(&raw, &normalized, false) {
        return Err("invalid_files_store");
    }
    code_pad_lib::component::validate_persistent_file("recovery.json", bytes)
}
