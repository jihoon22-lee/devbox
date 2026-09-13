//! Suite recovery decisions. Native adapters acquire proofs and persist each
//! transition before its side effect; renderer requests cannot supply proofs.
use product_contract::installation::{Manifest, PRODUCTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, &'static str>;
const MAX_ITEMS: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Inventory,
    Stage,
    Verify,
    Snapshot,
    Import,
    Validate,
    Quiesce,
    Activate,
    Health,
    Commit,
    Cleanup,
    Complete,
    Recover,
    Recovered,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Transfer {
    Transferred,
    Rebuilt,
    ReconnectRequired,
    Skipped,
    Failed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportReceipt {
    pub owner: String,
    pub source_snapshot_id: String,
    pub source_record_id: String,
    pub destination_id: String,
    pub outcome: Transfer,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Backup {
    pub id: String,
    pub owner: String,
    pub sha256: String,
    pub bytes: u64,
    pub schema: u32,
    pub acquisition: String,
    // No source paths, credentials or raw user records in a shared journal.
    pub source_revision: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub schema_version: u32,
    pub operation_id: String,
    pub installation_key: String,
    pub revision: u64,
    pub phase: Phase,
    pub previous: Option<Manifest>,
    pub candidate: Manifest,
    pub backup: Vec<Backup>,
    pub imports: Vec<ImportReceipt>,
    pub cleanup_pending: BTreeSet<String>,
    pub committed: bool,
    pub failure: Option<String>,
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn full_generation(manifest: &Manifest) -> Result<()> {
    manifest.validate(&manifest.suite_version)?;
    if manifest.members.len() != PRODUCTS.len()
        || manifest.members.iter().any(|member| {
            member.executable
                != format!(
                    "generations/{}/products/{}/devbox-{}.exe",
                    manifest.generation, member.product, member.product
                )
        })
    {
        return Err("suite_generation_incomplete");
    }
    Ok(())
}
impl Journal {
    pub fn begin(
        operation_id: String,
        installation_key: String,
        previous: Option<Manifest>,
        candidate: Manifest,
    ) -> Result<Self> {
        let journal = Self {
            schema_version: 1,
            operation_id,
            installation_key,
            revision: 0,
            phase: Phase::Inventory,
            previous,
            candidate,
            backup: Vec::new(),
            imports: Vec::new(),
            cleanup_pending: BTreeSet::new(),
            committed: false,
            failure: None,
        };
        journal.validate()?;
        Ok(journal)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.revision == u64::MAX
            || !id(&self.operation_id)
            || !hash(&self.installation_key)
            || self.backup.len() > MAX_ITEMS
            || self.imports.len() > 65536
            || self.cleanup_pending.len() > 15
            || self.cleanup_pending.iter().any(|v| !id(v))
            || self.failure.as_ref().is_some_and(|v| !id(v))
        {
            return Err("suite_journal_invalid");
        }
        full_generation(&self.candidate)?;
        if let Some(previous) = &self.previous {
            full_generation(previous)?;
            if previous.installation_id != self.candidate.installation_id
                || previous.generation == self.candidate.generation
            {
                return Err("suite_generation_conflict");
            }
        }
        if self.committed != matches!(self.phase, Phase::Cleanup | Phase::Complete) {
            return Err("suite_commit_inconsistent");
        }
        let mut snapshots = BTreeSet::new();
        for backup in &self.backup {
            if !id(&backup.id)
                || !snapshots.insert(&backup.id)
                || !PRODUCTS.contains(&backup.owner.as_str())
                || !hash(&backup.sha256)
                || !hash(&backup.source_revision)
                || backup.bytes == 0
                || backup.bytes > 256 * 1024 * 1024
                || !matches!(
                    backup.acquisition.as_str(),
                    "sqlite-online-backup/v1" | "closed-browser-copy/v1" | "closed-json-copy/v1"
                )
            {
                return Err("suite_backup_invalid");
            }
        }
        let mut records = BTreeSet::new();
        for receipt in &self.imports {
            if !PRODUCTS.contains(&receipt.owner.as_str())
                || !id(&receipt.source_snapshot_id)
                || !id(&receipt.source_record_id)
                || !id(&receipt.destination_id)
                || !self.backup.iter().any(|backup| {
                    backup.id == receipt.source_snapshot_id && backup.owner == receipt.owner
                })
                || !records.insert((
                    &receipt.owner,
                    &receipt.source_snapshot_id,
                    &receipt.source_record_id,
                ))
            {
                return Err("suite_import_invalid");
            }
        }
        Ok(())
    }
    /// Called only with a newly acquired owner snapshot; repeating identical
    /// acquisition receipts is idempotent, changed source bytes need a new plan.
    pub fn record_backup(&mut self, expected: u64, backup: Backup) -> Result<()> {
        self.require(expected, Phase::Snapshot)?;
        if let Some(old) = self.backup.iter().find(|old| old.id == backup.id) {
            return if old == &backup {
                Ok(())
            } else {
                Err("suite_snapshot_conflict")
            };
        }
        let mut next = self.clone();
        next.backup.push(backup);
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn record_import(&mut self, expected: u64, receipt: ImportReceipt) -> Result<()> {
        self.require(expected, Phase::Import)?;
        if let Some(old) = self.imports.iter().find(|old| {
            old.owner == receipt.owner
                && old.source_snapshot_id == receipt.source_snapshot_id
                && old.source_record_id == receipt.source_record_id
        }) {
            return if old == &receipt {
                Ok(())
            } else {
                Err("suite_import_conflict")
            };
        }
        let mut next = self.clone();
        next.imports.push(receipt);
        next.revision += 1;
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn require(&self, expected: u64, phase: Phase) -> Result<()> {
        self.validate()?;
        if expected != self.revision || self.phase != phase {
            return Err("suite_plan_stale");
        }
        Ok(())
    }
    /// Proof revisions are native observations of package, data and writer
    /// owners. They are deliberately not deserializable request arguments.
    pub fn advance(&mut self, expected: u64, proof: Proof) -> Result<()> {
        self.require(expected, proof.phase)?;
        if proof.generation != self.candidate.generation || !hash(&proof.revision) {
            return Err("suite_proof_stale");
        }
        if matches!(
            self.phase,
            Phase::Validate | Phase::Quiesce | Phase::Activate | Phase::Health | Phase::Commit
        ) && self.imports.iter().any(|r| r.outcome == Transfer::Failed)
        {
            return Err("suite_import_incomplete");
        }
        let next = match self.phase {
            Phase::Inventory => Phase::Stage,
            Phase::Stage => Phase::Verify,
            Phase::Verify => Phase::Snapshot,
            Phase::Snapshot => Phase::Import,
            Phase::Import => Phase::Validate,
            Phase::Validate => Phase::Quiesce,
            Phase::Quiesce => Phase::Activate,
            Phase::Activate => Phase::Health,
            Phase::Health => Phase::Commit,
            Phase::Commit => Phase::Cleanup,
            Phase::Cleanup if self.cleanup_pending.is_empty() => Phase::Complete,
            Phase::Recover => Phase::Recovered,
            _ => return Err("suite_phase_incomplete"),
        };
        self.phase = next;
        self.committed = matches!(next, Phase::Cleanup | Phase::Complete);
        self.revision += 1;
        Ok(())
    }
    pub fn fail(&mut self, expected: u64, code: &str) -> Result<()> {
        self.require(expected, self.phase)?;
        if !id(code) || matches!(self.phase, Phase::Complete | Phase::Recovered) {
            return Err("suite_failure_invalid");
        }
        self.failure = Some(code.into());
        if !self.committed {
            self.phase = Phase::Recover;
        }
        // A cleanup failure preserves an activated suite and exact pending IDs.
        self.revision += 1;
        Ok(())
    }
    pub fn recovery(&self) -> Result<Recovery> {
        self.validate()?;
        Ok(if self.committed {
            Recovery::KeepCommittedAndRetryCleanup
        } else if matches!(
            self.phase,
            Phase::Activate | Phase::Health | Phase::Commit | Phase::Recover
        ) {
            Recovery::BlockWritersAndRestorePrevious
        } else {
            Recovery::KeepPreviousAndResumeStage
        })
    }
}
#[derive(Debug)]
pub struct Proof {
    pub phase: Phase,
    pub generation: String,
    pub revision: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Recovery {
    KeepPreviousAndResumeStage,
    BlockWritersAndRestorePrevious,
    KeepCommittedAndRetryCleanup,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate() -> Manifest {
        Manifest {
            schema_version: 1,
            installation_id: "fixture-install".into(),
            generation: "next".into(),
            suite_version: "0.8.0".into(),
            protocol_version: 1,
            members: PRODUCTS
                .iter()
                .map(|p| product_contract::installation::Member {
                    product: (*p).into(),
                    executable: format!("generations/next/products/{p}/devbox-{p}.exe"),
                    sha256: "a".repeat(64),
                })
                .collect(),
        }
    }
    fn journal() -> Journal {
        Journal::begin(
            "fixture-operation".into(),
            "b".repeat(64),
            None,
            candidate(),
        )
        .unwrap()
    }
    fn advance(j: &mut Journal) {
        j.advance(
            j.revision,
            Proof {
                phase: j.phase,
                generation: j.candidate.generation.clone(),
                revision: "c".repeat(64),
            },
        )
        .unwrap();
    }
    #[test]
    fn crash_recovery_never_activates_uncommitted_writers_and_cleanup_failure_keeps_commit() {
        let mut j = journal();
        for _ in 0..7 {
            advance(&mut j);
        }
        assert_eq!(j.phase, Phase::Activate);
        for _ in 0..3 {
            assert_eq!(
                j.recovery().unwrap(),
                Recovery::BlockWritersAndRestorePrevious
            );
            let restored: Journal =
                serde_json::from_slice(&serde_json::to_vec(&j).unwrap()).unwrap();
            restored.validate().unwrap();
            advance(&mut j);
        }
        assert!(j.committed);
        j.cleanup_pending.insert("known-legacy-install".into());
        j.fail(j.revision, "cleanup_locked").unwrap();
        assert_eq!(j.phase, Phase::Cleanup);
        assert_eq!(
            j.recovery().unwrap(),
            Recovery::KeepCommittedAndRetryCleanup
        );
        assert!(j
            .advance(
                j.revision,
                Proof {
                    phase: j.phase,
                    generation: "next".into(),
                    revision: "c".repeat(64)
                }
            )
            .is_err());
    }
    #[test]
    fn mixed_generation_partial_suite_and_stale_review_are_rejected() {
        let mut manifest = candidate();
        manifest.members.pop();
        assert!(Journal::begin("fixture".into(), "b".repeat(64), None, manifest).is_err());
        let mut j = journal();
        advance(&mut j);
        assert!(j
            .advance(
                0,
                Proof {
                    phase: Phase::Stage,
                    generation: "next".into(),
                    revision: "c".repeat(64)
                }
            )
            .is_err());
        let mut altered = j.clone();
        altered.committed = true;
        assert!(altered.validate().is_err());
    }
    #[test]
    fn source_mapping_replay_is_idempotent_and_conflicting_destination_is_preserved() {
        let mut j = journal();
        for _ in 0..3 {
            advance(&mut j);
        }
        let backup = Backup {
            id: "snapshot".into(),
            owner: "knowledge".into(),
            sha256: "a".repeat(64),
            bytes: 4096,
            schema: 1,
            acquisition: "sqlite-online-backup/v1".into(),
            source_revision: "d".repeat(64),
        };
        j.record_backup(j.revision, backup).unwrap();
        advance(&mut j);
        let receipt = ImportReceipt {
            owner: "knowledge".into(),
            source_snapshot_id: "snapshot".into(),
            source_record_id: "record".into(),
            destination_id: "destination".into(),
            outcome: Transfer::Transferred,
        };
        j.record_import(j.revision, receipt.clone()).unwrap();
        let revision = j.revision;
        j.record_import(j.revision, receipt.clone()).unwrap();
        assert_eq!(revision, j.revision);
        let mut conflict = receipt;
        conflict.destination_id = "different".into();
        assert!(j.record_import(j.revision, conflict).is_err());
        assert_eq!(j.imports[0].destination_id, "destination");
    }
}
