use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Ecosystem {
    Npm,
    Cargo,
    Go,
    Python,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum LockKind {
    Pnpm,
    Npm,
    Yarn,
    Cargo,
    Go,
    Poetry,
    Uv,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum DependencySource {
    Registry,
    Git,
    Local,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DependencyVersion {
    pub version: String,
    pub source: DependencySource,
    pub direct: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub versions: Vec<DependencyVersion>,
    pub direct: bool,
}
domain_error! { #[derive(Deserialize)] pub enum DepsError {
 InvalidProject = "deps.invalid_project",
 NoLockfile = "deps.no_lockfile",
 UnsupportedLock { lock: LockKind } = "deps.unsupported_lock",
 MalformedLock { lock: LockKind } = "deps.malformed_lock",
 MissingManifest { lock: LockKind } = "deps.missing_manifest",
 LimitExceeded = "deps.limit_exceeded",
 UnsafePath = "deps.unsafe_path",
 ReadFailed = "deps.read_failed",
 Busy = "deps.busy",
 JobNotFound = "deps.job_not_found",
}}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInventory {
    pub fingerprint: String,
    pub locks: Vec<LockKind>,
    pub dependencies: Vec<Dependency>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AuditFailure {
    Network,
    Http,
    InvalidResponse,
    Incomplete,
    LimitExceeded,
    InputChanged,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Advisory {
    pub id: String,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuditPackage {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: String,
    pub advisories: Vec<Advisory>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuditReport {
    pub fingerprint: String,
    pub completed_at_ms: u64,
    pub packages: Vec<AuditPackage>,
    pub skipped: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AuditState {
    Running,
    Succeeded,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuditJob {
    pub id: String,
    pub project_id: String,
    pub fingerprint: String,
    pub state: AuditState,
    pub completed: u32,
    pub total: u32,
    pub cached: bool,
    pub error: Option<AuditFailure>,
    pub result: Option<AuditReport>,
    pub last_success: Option<AuditReport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectQuery {
    pub project_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct JobQuery {
    pub project_id: String,
    pub id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AuditProgress {
    pub id: String,
    pub project_id: String,
    pub state: AuditState,
    pub completed: u32,
    pub total: u32,
}
