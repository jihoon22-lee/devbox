//! Typed repository execution definitions shared with the runtime domain.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunTarget {
    #[default]
    Linux,
    Windows,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RestartPolicy {
    #[default]
    No,
    #[serde(alias = "on-failure", alias = "on_failure")]
    OnFailure,
    Always,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MissedPolicy {
    #[default]
    Skip,
    #[serde(alias = "run_once", alias = "run-once")]
    RunOnce,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct StartLimit {
    pub burst: u32,
    #[serde(alias = "interval_sec")]
    pub interval_sec: u32,
}
impl Default for StartLimit {
    fn default() -> Self {
        Self {
            burst: 5,
            interval_sec: 60,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TaskDef {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub target: RunTarget,
    #[serde(default, alias = "depends_on")]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServiceDef {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub target: RunTarget,
    #[serde(default, alias = "depends_on")]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default = "restart_sec", alias = "restart_sec")]
    pub restart_sec: u32,
    #[serde(default, alias = "start_limit")]
    pub start_limit: StartLimit,
    #[serde(default, alias = "ports_hint")]
    pub ports_hint: Vec<u16>,
    #[serde(default = "ready_timeout_sec", alias = "ready_timeout_sec")]
    pub ready_timeout_sec: u32,
}
fn restart_sec() -> u32 {
    1
}
fn ready_timeout_sec() -> u32 {
    60
}
impl Default for ServiceDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            command: String::new(),
            cwd: None,
            env: BTreeMap::new(),
            target: RunTarget::Linux,
            depends_on: vec![],
            worktree: None,
            secrets: vec![],
            restart: RestartPolicy::No,
            restart_sec: restart_sec(),
            start_limit: StartLimit::default(),
            ports_hint: vec![],
            ready_timeout_sec: ready_timeout_sec(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ScheduleDef {
    pub id: String,
    pub task: String,
    pub cron: String,
    #[serde(default)]
    pub tz: Option<String>,
    #[serde(default)]
    pub missed: MissedPolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunDef {
    Task(TaskDef),
    Service(ServiceDef),
    Schedule(ScheduleDef),
}
/// Approval UI uses these same typed fields; secret values are never included.
pub type RunDefinitionPreview = RunDef;

impl RunDef {
    pub fn id(&self) -> &str {
        match self {
            Self::Task(d) => &d.id,
            Self::Service(d) => &d.id,
            Self::Schedule(d) => &d.id,
        }
    }
    pub fn dependencies(&self) -> &[String] {
        match self {
            Self::Task(d) => &d.depends_on,
            Self::Service(d) => &d.depends_on,
            Self::Schedule(d) => std::slice::from_ref(&d.task),
        }
    }
    pub fn command(&self) -> Option<&str> {
        match self {
            Self::Task(d) => Some(&d.command),
            Self::Service(d) => Some(&d.command),
            Self::Schedule(_) => None,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub tasks: Vec<TaskDef>,
    pub services: Vec<ServiceDef>,
    pub schedules: Vec<ScheduleDef>,
}
impl RunConfig {
    pub fn definitions(&self) -> Vec<RunDef> {
        self.tasks
            .iter()
            .cloned()
            .map(RunDef::Task)
            .chain(self.services.iter().cloned().map(RunDef::Service))
            .chain(self.schedules.iter().cloned().map(RunDef::Schedule))
            .collect()
    }
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty() && self.services.is_empty() && self.schedules.is_empty()
    }
}
#[derive(Debug, Clone)]
pub struct RunConfigEdit {
    pub expected_digest: String,
    pub change: RunConfigChange,
}
#[derive(Debug, Clone)]
pub enum RunConfigChange {
    Define(RunDef),
    Undefine { id: String },
}
#[derive(Debug)]
pub struct UpdatedRunConfig {
    pub trust: crate::TrustPreviewStatus,
    pub digest: String,
    pub config: RunConfig,
    pub rev: String,
}
#[derive(Debug)]
pub struct RunConfigState {
    /// Opaque identity of the persisted consent record, even when current bytes are stale.
    /// It is a change marker, never an authorization capability.
    pub approval_stamp: Option<String>,
    pub config: RunConfig,
    pub trust: crate::TrustPreviewStatus,
    pub digest: String,
}
