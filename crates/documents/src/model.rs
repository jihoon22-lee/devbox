use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A document identity: a registered root id and a `/`-separated path that is
/// already resolved through symlinks (the screen never sends raw OS paths).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DocKey {
    pub root: String,
    pub path: String,
}

/// What the daemon observed on disk. Large integers travel as strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiskRev {
    pub hash: String,
    pub size: u64,
    pub mtime_ns: String,
    pub ino: String,
}
impl DiskRev {
    /// Compare-and-swap identity. Metadata-only changes (`touch`) keep the same
    /// content and must not turn every later save into a conflict.
    pub fn same_content(&self, other: &DiskRev) -> bool {
        self.hash == other.hash && self.size == other.size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DocKind {
    Note,
    Code,
}
impl DocKind {
    pub fn limit(self) -> u64 {
        match self {
            DocKind::Note => crate::NOTE_LIMIT,
            DocKind::Code => crate::CODE_LIMIT,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DocPolicy {
    pub kind: DocKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Eol {
    Lf,
    Crlf,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReadOnlyReason {
    Binary,
    Encoding,
    TooLarge,
    LeasedElsewhere,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ReadOnly {
    pub reason: ReadOnlyReason,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalView {
    pub seq: u64,
    pub base_rev: Option<DiskRev>,
    pub text: Option<String>,
    /// True when `text` did not fit the response budget; read it with `documents.journal_read`.
    pub deferred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimDocument {
    pub key: DocKey,
    pub policy: DocPolicy,
    pub window_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TakeDocument {
    pub key: DocKey,
    pub window_id: String,
    /// Defaults to `note`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub policy: Option<DocPolicy>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Claimed {
    pub key: DocKey,
    pub lease_id: Option<String>,
    pub rev: Option<DiskRev>,
    pub text: Option<String>,
    pub eol: Eol,
    pub bom: bool,
    pub read_only: Option<ReadOnly>,
    pub journal: Option<JournalView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenewLeases {
    pub lease_ids: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Renewed {
    pub lost: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseRef {
    pub lease_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Expect {
    Present,
    Absent,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveDocument {
    pub lease_id: String,
    pub seq: u64,
    pub base_rev: Option<DiskRev>,
    pub expect: Expect,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub rev: DiskRev,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ReadDocument {
    pub key: DocKey,
    /// Defaults to `note`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub policy: Option<DocPolicy>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiskDocument {
    pub key: DocKey,
    pub rev: Option<DiskRev>,
    pub text: Option<String>,
    pub read_only: Option<ReadOnly>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteJournal {
    pub lease_id: String,
    pub seq: u64,
    pub base_rev: Option<DiskRev>,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DropJournal {
    pub key: DocKey,
    pub upto_seq: u64,
    /// The caller's lease. While another live lease holds the key the drop is refused, so a
    /// read-only window can never remove the holder's journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lease_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalText {
    pub seq: u64,
    pub base_rev: Option<DiskRev>,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameDocument {
    pub lease_id: Option<String>,
    pub key: DocKey,
    pub to: DocKey,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Renamed {
    pub key: DocKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DocumentChanged {
    pub key: DocKey,
    pub rev: Option<DiskRev>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DocumentMoved {
    pub from: DocKey,
    pub to: DocKey,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LeaseLost {
    pub key: DocKey,
    pub lease_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LeaseLoss {
    Taken,
    Expired,
}

domain_error! {
    pub enum DocumentsError {
        Conflict { rev: Option<DiskRev> } = "documents.conflict",
        LeaseLost { reason: LeaseLoss } = "documents.lease_lost",
        OutOfScope = "documents.out_of_scope",
        NotFound = "documents.not_found",
        Exists = "documents.exists",
        TooLarge { limit: u64 } = "documents.too_large",
        StaleJournal = "documents.stale_journal",
        Unsupported = "documents.unsupported",
        InvalidInput = "documents.invalid_input",
    }
}
