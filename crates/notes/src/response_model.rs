//! API response append contracts. Response bytes are supplied only by daemon orchestration.
use crate::Appended;
use devbox_documents::DiskRev;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ResponseNoteTarget {
    Daily,
    Existing { path: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SelectResponseNote {
    pub target: ResponseNoteTarget,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseNoteSelection {
    pub selection: String,
    pub path: String,
    pub target: ResponseNoteTarget,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppendResponseNote {
    pub history_id: String,
    pub selection: String,
    // A fresh explicit intent gets a fresh UUID; uncertain retries keep this ID.
    pub operation_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseNoteAppended {
    pub path: String,
    pub rev: DiskRev,
    pub operation_id: String,
}
impl ResponseNoteAppended {
    pub(crate) fn new(operation_id: String, result: Appended) -> Self {
        Self {
            path: result.path,
            rev: result.rev,
            operation_id,
        }
    }
}
