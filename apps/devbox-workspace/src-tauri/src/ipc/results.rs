//! Result envelopes built by the native Workspace owners.
use product_contract::ProjectContext;
use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceTaskSource {
    pub path: String,
    pub target_kind: runtime_engine::core::models::TargetKind,
    pub target_distro: Option<String>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceTaskSourceReply {
    pub context: ProjectContext,
    pub source: WorkspaceTaskSource,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HandoffReply {
    pub handoff_id: String,
}
#[derive(Serialize, ts_rs::TS)]

pub(crate) struct PortLogReply {
    pub handoff_id: String,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionReply {
    pub handoff_id: String,
    pub redacted: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReconnectedLogs {
    pub sources: Vec<logs_engine::core::SourceSpec>,
    pub filter: logs_engine::core::FilterSpec,
    pub unavailable_sources: usize,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileChoice {
    pub id: String,
    pub name: String,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileCommand {
    pub id: String,
    pub name: String,
    pub revision: String,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalWindow {
    pub id: String,
    pub context: Option<ProjectContext>,
    pub generation: u64,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalCommands {
    pub profiles: Vec<ProfileCommand>,
    pub windows: Vec<TerminalWindow>,
    pub default_shortcut: (),
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalSummon {
    pub id: String,
    pub visible: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalProfiles {
    pub revision: String,
    pub profiles: Vec<terminal_engine::component::WorkspaceProfile>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedTerminalProfile {
    pub revision: String,
    pub profile: terminal_engine::component::WorkspaceProfile,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeletedTerminalProfile {
    pub revision: String,
    pub profile: (),
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WslControlStatus {
    pub state: String,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevelopmentCandidate {
    pub id: String,
    pub name: String,
    pub kind: runtime_engine::core::models::JobKind,
    pub target_kind: runtime_engine::core::models::TargetKind,
    pub target_distro: Option<String>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevelopmentCandidates {
    pub jobs: Vec<DevelopmentCandidate>,
    pub truncated: bool,
    pub profiles: Vec<ProfileChoice>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevelopmentSessionRow {
    #[serde(flatten)]
    pub session: crate::core::development_sessions::Session,
    pub can_archive: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevelopmentSessions {
    pub sessions: Vec<DevelopmentSessionRow>,
    pub resources: BTreeMap<String, crate::core::development_sessions::Resource>,
    pub intents: BTreeMap<String, crate::development_host::Intent>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DevelopmentPlan {
    pub session: crate::core::development_sessions::Session,
    pub jobs: Vec<runtime_engine::core::models::Job>,
    pub profile: Option<terminal_engine::component::WorkspaceProfile>,
    pub preflight: crate::session_preflight::Report,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSummaryPreview {
    pub operation_id: String,
    pub draft: product_contract::session_summary::Draft,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProblemSummary {
    pub toolchains: Vec<String>,
    pub secret_references: usize,
    pub environment_reference: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProblemsReply {
    #[serde(flatten)]
    pub snapshot: crate::core::problems::Snapshot,
    pub running_tasks: Option<usize>,
    pub summary: Option<ProblemSummary>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProblemResolution {
    pub context: ProjectContext,
    pub target: ResolvedProblemTarget,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProblemLogRequest {
    pub id: String,
    pub source: ProblemLogSource,
    pub offset: Option<String>,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(untagged)]
pub(crate) enum ResolvedProblemTarget {
    Native(crate::core::problems::Target),
    Navigation(ProblemNavigation),
}
#[derive(Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ProblemNavigation {
    Task { job_id: String },
    Log { request: ProblemLogRequest },
}
#[derive(Serialize, ts_rs::TS)]
#[serde(untagged)]
pub(crate) enum ProcessActionReply {
    Native(ports_engine::component::ListenerActionResult),
    Owned(OwnedTaskAction),
}
#[derive(Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum OwnedTaskAction {
    OwnedTask { task_id: String },
}

#[derive(serde::Serialize, ts_rs::TS)]
pub(crate) struct EmptyReply {}
#[derive(serde::Serialize, ts_rs::TS)]
pub(crate) struct ContextCleared {
    pub context: Option<product_contract::ProjectContext>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ProblemLogSource {
    RuntimeRun {
        run_id: String,
        #[ts(type = "\"stdout\" | \"stderr\"")]
        stream: String,
        revision: String,
    },
}
