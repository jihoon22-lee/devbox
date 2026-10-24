use crate::{DiskRev, DocKey, ReadOnly};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EntryInfo {
    pub key: DocKey,
    pub kind: EntryKind,
    pub rev: DiskRev,
    pub read_only: Option<ReadOnly>,
}
