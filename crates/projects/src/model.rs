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

domain_error! {
    pub enum ProjectsError {
        PathInvalid { path: String } = "projects.path_invalid",
        NotDirectory { path: String } = "projects.not_directory",
        AlreadyExists { path: String } = "projects.already_exists",
        NotFound { id: String } = "projects.not_found",
    }
}
