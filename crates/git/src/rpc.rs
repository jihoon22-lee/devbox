use crate::{
    Commit, blame::BlameLine, cleanup::*, hunks::DiffSnapshot, model::*, source_queries::*,
    stash::StashEntry,
};
use devbox_protocol::{Done, Exporter, method, topic};
method!(pub GitStatus = "git.status", query, GitTargetQuery => SourceStatus, SourceGitError);
method!(pub GitDiff = "git.diff", query, GitDiffQuery => DiffSnapshot, SourceGitError);
method!(pub GitBranches = "git.branches", query, GitTargetQuery => Vec<SourceBranch>, SourceGitError);
method!(pub GitLog = "git.log", query, GitLogQuery => Vec<Commit>, SourceGitError);
method!(pub GitBlame = "git.blame", query, GitPathQuery => Vec<BlameLine>, SourceGitError);
method!(pub GitWorktrees = "git.worktrees", query, GitTargetQuery => Vec<SourceWorktree>, SourceGitError);
method!(pub GitStashList = "git.stash_list", query, GitTargetQuery => Vec<StashEntry>, SourceGitError);
method!(pub GitCleanupCandidates = "git.cleanup_candidates", query, GitTargetQuery => Vec<CleanupCandidate>, SourceGitError);
method!(pub GitStage = "git.stage", mutation(reversible), GitFileChange => Done, SourceGitError);
method!(pub GitUnstage = "git.unstage", mutation(reversible), GitFileChange => Done, SourceGitError);
method!(pub GitDiscard = "git.discard", mutation(always), GitFileChange => Done, SourceGitError);
method!(pub GitCommit = "git.commit", mutation, GitCommitParams => Done, SourceGitError);
method!(pub GitCheckout = "git.checkout", mutation, GitCheckoutParams => Done, SourceGitError);
method!(pub GitBranchDelete = "git.branch_delete", mutation(always), GitBranchDeleteParams => Done, SourceGitError);
method!(pub GitStash = "git.stash", mutation, GitStashParams => Done, SourceGitError);
method!(pub GitStashDrop = "git.stash_drop", mutation(always), GitStashDropParams => Done, SourceGitError);
method!(pub GitFetch = "git.fetch", mutation, GitTargetQuery => GitOperation, SourceGitError);
method!(pub GitPull = "git.pull", mutation(always), GitPullParams => GitOperation, SourceGitError);
method!(pub GitPush = "git.push", mutation(always), GitPushParams => GitOperation, SourceGitError);
method!(pub GitOperationStatus = "git.operation_status", query, GitOperationQuery => GitOperation, SourceGitError);
method!(pub GitOperationCancel = "git.operation_cancel", mutation, GitOperationQuery => GitOperation, SourceGitError);
method!(pub GitCleanup = "git.cleanup", mutation(always), GitCleanupParams => Done, SourceGitError);
topic!(pub GitChangedTopic = "git.changed", GitChanged);
topic!(pub GitOperationChanged = "git.operation_changed", GitOperation);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<GitStatus>();
    x.method::<GitDiff>();
    x.method::<GitBranches>();
    x.method::<GitLog>();
    x.method::<GitBlame>();
    x.method::<GitWorktrees>();
    x.method::<GitStashList>();
    x.method::<GitCleanupCandidates>();
    x.method::<GitStage>();
    x.method::<GitUnstage>();
    x.method::<GitDiscard>();
    x.method::<GitCommit>();
    x.method::<GitCheckout>();
    x.method::<GitBranchDelete>();
    x.method::<GitStash>();
    x.method::<GitStashDrop>();
    x.method::<GitFetch>();
    x.method::<GitPull>();
    x.method::<GitPush>();
    x.method::<GitOperationStatus>();
    x.method::<GitOperationCancel>();
    x.method::<GitCleanup>();
    x.topic::<GitChangedTopic>();
    x.topic::<GitOperationChanged>();
}
