//! Git queries and mutations. Commands never invoke a shell.
//! Read-only queries disable external diff drivers.
//! File paths are repository-root-relative even when `dir` is a repository subdirectory.
pub mod diff;
pub mod mutate;
pub mod run;
pub mod status;
use devbox_protocol::domain_error;
pub use diff::{ChangeStatus, Commit, FileChange, changed_files, file_patch, log_range};
pub use status::{StatusSummary, ahead_behind, branches, current_branch, status};
domain_error! {
    pub enum GitError {
        NotRepo = "git.not_repo",
        Failed { op: String, stderr: String } = "git.failed",
        // OS failures must be mapped by RPC callers to Fail::Internal, never a domain detail.
        Internal = "git.internal",
        InvalidInput = "git.invalid_input",
        InvalidEncoding = "git.invalid_encoding",
        InvalidOutput = "git.invalid_output",
        LimitExceeded = "git.limit_exceeded",
        TimedOut = "git.timed_out",
        BaseDirty { files: Vec<String> } = "git.base_dirty",
        WorktreeDirty { files: Vec<String> } = "git.worktree_dirty",
        BaseNotCheckedOut { branch: String } = "git.base_not_checked_out",
        Conflict { files: Vec<String> } = "git.conflict",
        BranchExists { branch: String } = "git.branch_exists",
        WorktreeAddPartial { branch: String, path: String, path_exists: bool, branch_exists: Option<bool>, worktree_registered: Option<bool>, cause: Box<GitError> } = "git.worktree_add_partial",
        PushFailed { stderr: String } = "git.push_failed",
        StateChanged = "git.state_changed",
        OperationInProgress = "git.operation_in_progress",
        UnsafeWorktree = "git.unsafe_worktree",
        GhFailed { stderr: String } = "git.gh_failed",
        GhInvalidOutput = "git.gh_invalid_output",
        GhMissing = "git.gh_missing",
    }
}
