// crates/agents/src/model.rs
use crate::state::{AgentState, CloseHow};
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub kind: String,
    pub message: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    pub at_ms: i64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instruction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivery: Option<InstructionDelivery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InstructionDelivery {
    Pending,
    Delivered,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    // The owned terminal can outlive the tool. Review alone does not imply exit.
    #[serde(default)]
    pub tool_exited: bool,
    pub state: AgentState,
    pub reason: Option<String>,
    pub attention: Option<Attention>,
    pub port: u16,
    pub prompt: String,
    pub follow_ups: Vec<FollowUp>,
    pub tokens: Option<Tokens>,
    #[serde(default)]
    pub tokens_truncated: bool,
    // Monotonic activity epoch rejects terminal observations captured before hooks/input.
    #[serde(default)]
    pub activity_seq: u64,
    pub pr_url: Option<String>,
    pub closed_how: Option<CloseHow>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub closed_ms: Option<i64>,
    #[serde(default)]
    pub closed_commits: Option<u32>,
    #[serde(default)]
    pub closed_head_oid: Option<String>,
    #[serde(default)]
    pub cleanup_error: Option<String>,
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
    #[serde(default)]
    #[ts(optional)]
    pub after_id: Option<String>,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "nextAfterId"
    )]
    #[ts(optional)]
    pub next_after_id: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendToAgent {
    pub id: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewExpect {
    pub head_oid: String,
    #[ts(optional)]
    pub base_oid: Option<String>,
    #[ts(optional)]
    pub unmerged_commits: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub discard_fingerprint: Option<String>,
}
fn merge_expect<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ReviewExpect, D::Error> {
    let expect = ReviewExpect::deserialize(d)?;
    if expect.head_oid.is_empty() || expect.base_oid.as_ref().is_none_or(String::is_empty) {
        return Err(serde::de::Error::custom(
            "expect.headOid and expect.baseOid are required",
        ));
    }
    Ok(expect)
}
fn discard_expect<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ReviewExpect, D::Error> {
    let expect = ReviewExpect::deserialize(d)?;
    if expect.head_oid.is_empty() || expect.unmerged_commits.is_none() {
        return Err(serde::de::Error::custom(
            "expect.headOid and expect.unmergedCommits are required",
        ));
    }
    Ok(expect)
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MergeKind {
    Merge,
    Squash,
    Rebase,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeAgent {
    pub id: String,
    pub strategy: MergeKind,
    pub message: Option<String>,
    #[serde(deserialize_with = "merge_expect")]
    #[ts(type = "ReviewExpect & { baseOid: string }")]
    pub expect: ReviewExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrAgent {
    pub id: String,
    pub title: Option<String>,
    pub body: Option<String>,
    #[serde(deserialize_with = "merge_expect")]
    #[ts(type = "ReviewExpect & { baseOid: string }")]
    pub expect: ReviewExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscardAgent {
    pub id: String,
    #[serde(deserialize_with = "discard_expect")]
    #[ts(type = "ReviewExpect & { unmergedCommits: number }")]
    pub expect: ReviewExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupCandidates {
    pub project_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupAgents {
    pub project_id: String,
    pub worktrees: Vec<String>,
    pub branches: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct CleanupOptions {
    pub worktrees: Vec<String>,
    pub branches: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentChanges {
    pub base: String,
    pub ahead: u32,
    pub behind: u32,
    pub head_oid: String,
    pub base_oid: String,
    pub files: Vec<devbox_git::FileChange>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentPatch {
    pub patch: String,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentCommits {
    pub items: Vec<devbox_git::Commit>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiscardCheck {
    pub unmerged_commits: u32,
    pub dirty_files: u32,
    pub head_oid: String,
    pub fingerprint: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct GetAgentPatch {
    pub id: String,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RebaseResult {
    Done,
    Conflict,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentRebase {
    pub result: RebaseResult,
    pub files: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentTest {
    pub terminal_id: String,
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
        StateChanged { head: String, base: String, unmerged: u32 } = "agents.state_changed",
        OperationInProgress = "agents.operation_in_progress",
        UnsafeWorktree = "agents.unsafe_worktree",
        BranchExists { branch: String } = "agents.branch_exists",
        WorktreeAddPartial { branch: String, path: String, path_exists: bool, branch_exists: Option<bool>, worktree_registered: Option<bool> } = "agents.worktree_add_partial",
        InvalidInput = "agents.invalid_input",
        SessionMissing = "agents.session_missing",
        SessionMetadata { diagnostic: crate::tracker::Diagnostic } = "agents.session_metadata",
        InputTooLarge { limit_bytes: u32 } = "agents.input_too_large",
        PrTitleTooLong { limit_bytes: u32 } = "agents.pr_title_too_long",
        SquashMessageTooLong { limit_bytes: u32 } = "agents.squash_message_too_long",
        TeardownCommandTooLong { limit_bytes: u32 } = "agents.teardown_command_too_long",
        TeardownCommandInvalid = "agents.teardown_command_invalid",
        DeliveryUnknown = "agents.delivery_unknown",
        ResumeUnavailable = "agents.resume_unavailable",
        ReviewUnavailable = "agents.review_unavailable",
        DiscardSnapshotUnavailable { reason: String } = "agents.discard_snapshot_unavailable",
        ListChanged = "agents.list_changed",
        ProjectTrustRequired { id: String } = "agents.project_trust_required",
    }
}

pub(crate) fn project_trust_failure(
    error: devbox_protocol::Fail<devbox_projects::ProjectsError>,
) -> devbox_protocol::Fail<AgentsError> {
    match error {
        devbox_protocol::Fail::Domain(devbox_projects::ProjectsError::TrustRequired { id }) => {
            AgentsError::ProjectTrustRequired { id }.into()
        }
        devbox_protocol::Fail::Domain(devbox_projects::ProjectsError::NotFound { id }) => {
            AgentsError::ProjectNotFound { id }.into()
        }
        other => devbox_protocol::Fail::Internal(format!(
            "project configuration authorization: {other:?}"
        )),
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
    fn confirmation_requests_require_the_reviewed_fields() {
        let missing_base = r#"{"id":"a","strategy":"merge","expect":{"headOid":"abc"}}"#;
        assert!(serde_json::from_str::<MergeAgent>(missing_base).is_err());
        let missing_count = r#"{"id":"a","expect":{"headOid":"abc"}}"#;
        assert!(serde_json::from_str::<DiscardAgent>(missing_count).is_err());
        let valid = r#"{"id":"a","expect":{"headOid":"abc","unmergedCommits":0}}"#;
        assert!(serde_json::from_str::<DiscardAgent>(valid).is_ok());
        assert!(serde_json::from_str::<MergeAgent>(r#"{"id":"a","strategy":"merge"}"#).is_err());
    }

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
pub struct AgentUsage {
    pub tokens: Option<Tokens>,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentGitStat {
    pub id: String,
    pub changed_files: u32,
    pub ahead: u32,
    pub behind: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentGitStats {
    pub items: Vec<AgentGitStat>,
    #[serde(default)]
    pub failures: Vec<AgentGitFailure>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentGitFailure {
    pub id: String,
    pub code: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentResource {
    pub id: String,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AgentResources {
    pub items: Vec<AgentResource>,
}
