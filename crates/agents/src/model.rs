// crates/agents/src/model.rs
use crate::state::{AgentState, CloseHow};
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub kind: String,
    pub message: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    pub at_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub profile: String,
    pub base_branch: String,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub workdir: String,
    pub session_id: Option<String>,
    pub terminal_id: Option<String>,
    pub state: AgentState,
    pub reason: Option<String>,
    pub attention: Option<Attention>,
    pub port: u16,
    pub prompt: String,
    pub follow_ups: Vec<FollowUp>,
    pub tokens: Option<Tokens>,
    pub pr_url: Option<String>,
    pub closed_how: Option<CloseHow>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAgentTask {
    pub project_id: String,
    pub title: String,
    pub profile: String,
    pub base_branch: Option<String>,
    pub worktree: bool,
    pub prompt: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListAgents {
    pub include_closed_hours: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct GetAgent {
    pub id: String,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AgentsList {
    pub rev: String,
    pub items: Vec<AgentTask>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AgentProfiles {
    pub items: Vec<crate::profile::ToolProfile>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentEventParams {
    pub agent_id: String,
    pub source: crate::hooks::HookSource,
    pub payload: String,
    pub code: Option<i32>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AgentsChanged {
    pub rev: String,
    pub id: String,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct AgentsAttention {
    pub id: String,
    pub state: AgentState,
    pub message: String,
}

domain_error! {
    pub enum AgentsError {
        TitleInvalid = "agents.title_invalid",
        SlugExhausted = "agents.slug_exhausted",
        NotFound { id: String } = "agents.not_found",
        ProjectNotFound { id: String } = "agents.project_not_found",
        ProfileNotFound { id: String } = "agents.profile_not_found",
        BadState { state: String } = "agents.bad_state",
        WorktreeDirty { files: Vec<String> } = "agents.worktree_dirty",
        BaseDirty { files: Vec<String> } = "agents.base_dirty",
        BaseNotCheckedOut { branch: String } = "agents.base_not_checked_out",
        Conflict { files: Vec<String> } = "agents.conflict",
        UnmergedCommits { count: u32 } = "agents.unmerged_commits",
        GhMissing = "agents.gh_missing",
        NotRepo = "agents.not_repo",
        StateChanged = "agents.state_changed",
        OperationInProgress = "agents.operation_in_progress",
        UnsafeWorktree = "agents.unsafe_worktree",
        BranchExists { branch: String } = "agents.branch_exists",
        WorktreeAddPartial { branch: String, path: String, path_exists: bool, branch_exists: Option<bool>, worktree_registered: Option<bool> } = "agents.worktree_add_partial",
        InvalidInput = "agents.invalid_input",
        SessionMissing = "agents.session_missing",
        SessionMetadata { diagnostic: crate::tracker::Diagnostic } = "agents.session_metadata",
    }
}

pub fn git_failure(error: devbox_git::GitError) -> devbox_protocol::Fail<AgentsError> {
    use devbox_git::GitError as G;
    use devbox_protocol::Fail;
    let domain = match error {
        G::BaseDirty { files } => AgentsError::BaseDirty { files },
        G::WorktreeDirty { files } => AgentsError::WorktreeDirty { files },
        G::BaseNotCheckedOut { branch } => AgentsError::BaseNotCheckedOut { branch },
        G::Conflict { files } => AgentsError::Conflict { files },
        G::GhMissing => AgentsError::GhMissing,
        G::NotRepo => AgentsError::NotRepo,
        G::InvalidInput => AgentsError::InvalidInput,
        G::StateChanged => AgentsError::StateChanged,
        G::OperationInProgress => AgentsError::OperationInProgress,
        G::UnsafeWorktree => AgentsError::UnsafeWorktree,
        G::BranchExists { branch } => AgentsError::BranchExists { branch },
        G::WorktreeAddPartial {
            branch,
            path,
            path_exists,
            branch_exists,
            worktree_registered,
            cause,
        } => {
            // Keep remaining resources actionable; the underlying process/IO cause stays in logs.
            tracing::error!(?cause, "partial agent worktree creation");
            AgentsError::WorktreeAddPartial {
                branch,
                path,
                path_exists,
                branch_exists,
                worktree_registered,
            }
        }
        other => return Fail::Internal(format!("agent git operation: {other:?}")),
    };
    Fail::Domain(domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_git::GitError as G;
    use devbox_protocol::Fail;
    #[test]
    fn raw_process_causes_never_become_domain_details() {
        for error in [
            G::Failed {
                op: "add".into(),
                stderr: "PRIVATE_MARKER".into(),
            },
            G::PushFailed {
                stderr: "PRIVATE_MARKER".into(),
            },
            G::GhFailed {
                stderr: "PRIVATE_MARKER".into(),
            },
            G::Internal,
        ] {
            assert!(matches!(git_failure(error), Fail::Internal(_)));
        }
        let partial = G::WorktreeAddPartial {
            branch: "agent/fix".into(),
            path: "/tmp/owned".into(),
            path_exists: true,
            branch_exists: Some(true),
            worktree_registered: None,
            cause: Box::new(G::Failed {
                op: "add".into(),
                stderr: "PRIVATE_MARKER".into(),
            }),
        };
        let Fail::Domain(error) = git_failure(partial) else {
            panic!("remaining resources are actionable")
        };
        let json = serde_json::to_string(&error).expect("serialize domain");
        assert!(!json.contains("PRIVATE_MARKER"));
        assert!(json.contains("/tmp/owned"));
    }
}
