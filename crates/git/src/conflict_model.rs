use crate::model::SourceTarget;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictToken {
    pub target: SourceTarget,
    pub path: String,
    pub worktree_hash: String,
    pub stage_oids: [Option<String>; 3],
    pub identity: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    ModifyModify,
    DeleteModify,
    AddAdd,
    Other,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictEntry {
    pub path: String,
    pub kind: ConflictKind,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictLabels {
    pub base: String,
    pub ours: String,
    pub theirs: String,
    pub rebase: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictPreview {
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub merged: Option<String>,
    pub labels: ConflictLabels,
    pub token: ConflictToken,
    pub text_supported: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictApply {
    pub target: SourceTarget,
    pub path: String,
    // Explicit null means deletion; an omitted field must never authorize deletion.
    #[serde(deserialize_with = "required_resolution")]
    pub resolution: Option<String>,
    pub token: ConflictToken,
}

fn required_resolution<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
