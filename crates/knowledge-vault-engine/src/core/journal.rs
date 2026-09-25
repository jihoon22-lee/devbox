//! Local recovery entries are scoped to the native canonical vault root.
use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 8;
pub const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATH_CHARS: usize = 1024;
const MAX_ROOT_BYTES: usize = 32 * 1024;
const MAX_REVISION_BYTES: usize = 256;
pub const MAX_FILE_BYTES: usize = MAX_ENTRIES
    * (MAX_CONTENT_BYTES + MAX_ROOT_BYTES + MAX_PATH_CHARS * 4 + MAX_REVISION_BYTES + 256)
    * 6
    + 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalEntry {
    pub vault_root: String,
    pub path: String,
    pub content: String,
    pub base_revision: String,
    pub saved_at_ms: u64,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct JournalEntryView {
    pub path: String,
    pub content: String,
    pub base_revision: String,
    pub saved_at_ms: u64,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct JournalView {
    pub entries: Vec<JournalEntryView>,
    pub other_vault_count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalFile {
    pub schema_version: u32,
    pub entries: Vec<JournalEntry>,
}
impl Default for JournalFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            entries: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    Invalid,
    Limit,
    FutureSchema,
}
fn validate(entry: &JournalEntry) -> Result<(), JournalError> {
    if entry.vault_root.trim().is_empty()
        || entry.vault_root.len() > MAX_ROOT_BYTES
        || entry.vault_root.contains('\0')
        || entry.path.trim().is_empty()
        || entry.path.chars().count() > MAX_PATH_CHARS
        || entry.path.contains(['\0', '\\'])
        || std::path::Path::new(&entry.path).is_absolute()
        || entry
            .path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || entry.base_revision.len() > MAX_REVISION_BYTES
        || entry.saved_at_ms > 8_640_000_000_000_000
    {
        return Err(JournalError::Invalid);
    }
    if entry.content.len() > MAX_CONTENT_BYTES {
        return Err(JournalError::Limit);
    }
    Ok(())
}
impl JournalFile {
    fn validate(&self) -> Result<(), JournalError> {
        if self.schema_version > 1 {
            return Err(JournalError::FutureSchema);
        }
        if self.schema_version != 1 || self.entries.len() > MAX_ENTRIES {
            return Err(JournalError::Invalid);
        }
        let mut seen = std::collections::HashSet::new();
        for entry in &self.entries {
            validate(entry)?;
            if !seen.insert((&entry.vault_root, &entry.path)) {
                return Err(JournalError::Invalid);
            }
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, JournalError> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(JournalError::Limit);
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Header {
            schema_version: u32,
        }
        let header: Header = serde_json::from_slice(bytes).map_err(|_| JournalError::Invalid)?;
        if header.schema_version > 1 {
            return Err(JournalError::FutureSchema);
        }
        let file: Self = serde_json::from_slice(bytes).map_err(|_| JournalError::Invalid)?;
        file.validate()?;
        Ok(file)
    }
    pub fn encode(&self) -> Result<Vec<u8>, JournalError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| JournalError::Invalid)
    }
    pub fn upsert(&mut self, entry: JournalEntry) -> Result<(), JournalError> {
        validate(&entry)?;
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.vault_root == entry.vault_root && e.path == entry.path)
        {
            *existing = entry;
        } else {
            if self.entries.len() >= MAX_ENTRIES {
                return Err(JournalError::Limit);
            }
            self.entries.push(entry);
        }
        Ok(())
    }
    pub fn remove(&mut self, root: &str, path: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| e.vault_root != root || e.path != path);
        before != self.entries.len()
    }
    pub fn view(&self, root: &str) -> JournalView {
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.vault_root == root)
            .map(|e| JournalEntryView {
                path: e.path.clone(),
                content: e.content.clone(),
                base_revision: e.base_revision.clone(),
                saved_at_ms: e.saved_at_ms,
            })
            .collect();
        JournalView {
            other_vault_count: self.entries.len() - entries.len(),
            entries,
        }
    }
    pub fn discard_other(&mut self, root: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.vault_root == root);
        before != self.entries.len()
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
