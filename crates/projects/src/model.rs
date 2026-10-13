use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub favorite: bool,
    pub added_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ProjectList {
    pub rev: String,
    pub items: Vec<Project>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ListProjects {}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct AddProject {
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct RemoveProject {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ScanProjects {}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub path: String,
    pub name: String,
    pub is_git: bool,
    pub registered: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ScanResult {
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ProjectsChanged {
    pub rev: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct GetProjectConfig {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectConfigIssue {
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectConfigResult {
    pub config: crate::config::ProjectConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<ProjectConfigIssue>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct SummaryProjects {}

#[derive(Debug, Default, Deserialize, Serialize, TS)]
pub struct ProjectAgentCounts {
    pub open: u32,
    pub attention: u32,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectSummaryIssue {
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub branch: Option<String>,
    pub dirty: Option<bool>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub terminals: u32,
    pub agents: ProjectAgentCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<ProjectSummaryIssue>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectSummaryList {
    pub items: Vec<ProjectSummary>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct LaunchProject {
    pub id: String,
}

#[derive(Debug, Default, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub terminals: u32,
    pub agents: u32,
    pub terminal_ids: Vec<String>,
    pub rev: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct CloneProject {
    pub url: String,
    #[serde(default)]
    pub dir: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct SetFavorite {
    pub id: String,
    pub favorite: bool,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectPrompts {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct ProjectPrompt {
    pub name: String,
    pub body: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct PromptList {
    pub items: Vec<ProjectPrompt>,
}

domain_error! {
    pub enum ProjectsError {
        PathInvalid { path: String } = "projects.path_invalid",
        NotDirectory { path: String } = "projects.not_directory",
        AlreadyExists { path: String } = "projects.already_exists",
        NotFound { id: String } = "projects.not_found",
        InUse { id: String } = "projects.in_use",
        ConfigurationInvalid { message: String } = "projects.configuration_invalid",
        LaunchProfileNotFound { id: String } = "projects.launch_profile_not_found",
        LaunchFailed { terminals: u32, agents: u32, message: String } = "projects.launch_failed",
        CloneInvalidUrl = "projects.clone_invalid_url",
        CloneTargetExists { path: String } = "projects.clone_target_exists",
        CloneFailed { path: String, message: String } = "projects.clone_failed",
        UnsafePromptPath { path: String } = "projects.unsafe_prompt_path",
        PromptLimitExceeded { max_files: u32, max_bytes: u32 } = "projects.prompt_limit_exceeded",
        PromptInvalidEncoding { name: String } = "projects.prompt_invalid_encoding",
    }
}
