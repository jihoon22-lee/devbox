use crate::RunDef;
use devbox_projects::TrustPreviewStatus;
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DefinitionSource {
    ProjectFile,
    ThisPc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Definition {
    pub source: DefinitionSource,
    pub def: RunDef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SaveTo {
    Project,
    ThisPc,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListRuns {
    pub project_id: String,
    #[serde(default)]
    pub cursor: Option<DefinitionCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DefinitionCursor {
    pub id: String,
    pub shadowed: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionList {
    pub rev: String,
    pub defs: Vec<Definition>,
    pub shadowed: Vec<Definition>,
    pub runs: Vec<crate::Run>,
    pub trust: TrustPreviewStatus,
    pub next_cursor: Option<DefinitionCursor>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DefineRun {
    pub project_id: String,
    pub def: RunDef,
    pub save_to: SaveTo,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct DefineResult {
    pub rev: String,
    pub definition: Definition,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UndefineRun {
    pub project_id: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeChange {
    pub rev: String,
    pub project_id: String,
}

domain_error! {
    pub enum RuntimeError {
        DefInvalid { id: String, reason: String } = "runtime.def_invalid",
        ProjectNotFound { id: String } = "runtime.project_not_found",
        ProjectTrustRequired { id: String } = "projects.trust_required",
        ProjectConfigChanged { id: String } = "projects.trust_preview_stale",
        ProjectConfigInvalid { message: String } = "runtime.project_config_invalid",
        RunNotFound { id: String } = "runtime.run_not_found",
        NotReady { id: String } = "runtime.not_ready",
        DependencyFailed { id: String } = "runtime.dependency_failed",
        InvalidCursor = "runtime.invalid_cursor",
    }
}
