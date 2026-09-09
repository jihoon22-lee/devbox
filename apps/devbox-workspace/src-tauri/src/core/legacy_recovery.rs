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
    pub fn conflicts(&self, candidate: &Candidate) -> Vec<String> {
        candidate
            .recovery
            .entries
            .iter()
            .filter(|entry| {
                self.recovery
                    .get(&entry.path)
                    .is_some_and(|old| old != *entry)
            })
            .map(|entry| entry.path.clone())
            .collect()
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
    pub fn apply(&self, candidate: &Candidate, replace_existing: bool) -> Result<Self> {
        if self.imports.contains(&candidate.receipt) {
            return Ok(self.clone());
        }
        if !replace_existing && !self.conflicts(candidate).is_empty() {
            return Err("legacy_recovery_conflict");
        }
        if self.imports.len() >= 32 {
            return Err("legacy_recovery_limit");
        }
        let mut next = self.merge(&candidate.recovery.entries)?;
        next.imports.push(candidate.receipt.clone());
        next.validate()?;
        Ok(next)
    }
}
fn validate_recovery(bytes: &[u8]) -> Result<()> {
    if super::legacy_inventory::inspect(
        super::legacy_inventory::Source::CodePad,
        "recovery.json",
        bytes,
    )?
    .issue
    .is_some()
    {
        return Err("invalid_files_store");
    }
    code_pad_lib::component::validate_persistent_file("recovery.json", bytes)
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub recovery: RecoveryFile,
    pub receipt: Receipt,
    pub skipped_entries: usize,
}
pub fn candidate(
    snapshot_id: String,
    source: &RecoveryFile,
    mut eligible: impl FnMut(&str) -> bool,
) -> Result<Candidate> {
    validate_recovery(&serde_json::to_vec(source).map_err(|_| "legacy_recovery_unavailable")?)?;
    let mut recovery = source.clone();
    recovery.entries.retain(|entry| eligible(&entry.path));
    if recovery.entries.len() > 64 {
        return Err("legacy_recovery_limit");
    }
    let paths = recovery
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(Candidate {
        skipped_entries: source.entries.len() - recovery.entries.len(),
        recovery,
        receipt: Receipt { snapshot_id, paths },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(path: &str, content: &str) -> RecoveryEntry {
        RecoveryEntry {
            path: path.into(),
            content: content.into(),
            base_hash: None,
            snapshot_at_ms: 1,
        }
    }
    #[test]
    fn reviewed_merge_preserves_other_buffers_and_repeat_keeps_later_discard() {
        let source = RecoveryFile {
            version: 1,
            entries: vec![entry("/a", "old"), entry("/outside", "keep source")],
        };
        let candidate = candidate("a".repeat(64), &source, |path| path == "/a").unwrap();
        assert_eq!(candidate.skipped_entries, 1);
        let stored = StoredRecovery::default()
            .merge(&[entry("/a", "new"), entry("/b", "unrelated")])
            .unwrap();
        assert_eq!(stored.conflicts(&candidate), vec!["/a"]);
        assert!(matches!(
            stored.apply(&candidate, false),
            Err("legacy_recovery_conflict")
        ));
        let mut applied = stored.apply(&candidate, true).unwrap();
        assert_eq!(applied.recovery.get("/b").unwrap().content, "unrelated");
        applied.recovery.remove("/a");
        let loaded = StoredRecovery::decode(&applied.encode().unwrap()).unwrap();
        assert!(loaded
            .apply(&candidate, false)
            .unwrap()
            .recovery
            .get("/a")
            .is_none());
        assert_eq!(source.entries.len(), 2);
    }
    #[test]
    fn oversized_merge_never_evicts_existing_buffers_and_unknown_schema_is_preserved() {
        let large = "x".repeat(128_000);
        let stored = StoredRecovery::default()
            .merge(
                &(0..4)
                    .map(|i| entry(&format!("/{i}"), &large))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        assert!(stored.merge(&[entry("/new", "x")]).is_err());
        assert_eq!(stored.recovery.entries.len(), 4);
        for raw in [
            r#"{"version":1,"entries":[],"unknown":true}"#,
            r#"{"schemaVersion":2,"recovery":{"version":1,"entries":[]},"imports":[]}"#,
        ] {
            assert!(StoredRecovery::decode(raw.as_bytes()).is_err());
        }
    }
}
