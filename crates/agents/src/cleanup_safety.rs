//! What a closed task's cleanup may undo before it removes the work folder. Automatic cleanup and
//! the cleanup list share these steps, under the `actions` lock each already holds.
//!
//! devbox never removes a work folder with force: that would also skip Git's refusal of a folder
//! with initialized submodules, whose commits can exist only under `.git/worktrees/<name>/modules`.
//! Such a folder is refused before the undo with a code that says how to remove it (Ruling 64 (c)).
//! A teardown (or a process of the closed task) that writes into the folder used to make every
//! cleanup refuse with `agents.worktree_dirty`, again on each retry, since each retry runs the
//! teardown again. So when the folder was clean at the closed head right before the teardown, what
//! changed since is cleanup's own and is undone, and only then is the folder removed. A folder
//! with changes of its own is refused as before (Ruling 61).
use crate::{
    cleanup::cleanup_in_progress,
    model::*,
    service::{AgentsService, Result},
    state::{AgentState, CloseHow},
};
use devbox_projects::trust::AuthorizedConfig;
use devbox_protocol::Fail;
use std::path::Path;

/// Why cleanup may delete a closed task's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Release {
    /// Every commit of the branch is on the base branch.
    Merged,
    /// The branch's own commits are not on the base branch, but what they changed is: in the
    /// squash commit the close made, or in the PR (Ruling 60). The cleanup list says so.
    OffBase,
}

impl AgentsService {
    /// The one rule the cleanup list and cleanup itself apply to a closed task's branch, so the
    /// list never offers what cleanup then refuses. A branch whose commits are all on the base
    /// branch may go, as before. Otherwise only when all of these hold:
    /// - the task closed with a note of a cleanup that stopped, so the user wanted it cleaned up;
    /// - it closed as a merge or a PR, never as a discard;
    /// - the branch is still at the head the task closed with (a folder that is left must also be
    ///   clean at that head, which its callers check);
    /// - for a merge, the commit the close made is still on the base branch: a reset of the base
    ///   past it leaves the branch the only copy of the work. A task closed before that commit was
    ///   recorded keeps the strict rule.
    pub(crate) async fn release(
        &self,
        repo: &Path,
        task: &AgentTask,
        branch: &str,
    ) -> Result<Option<Release>> {
        if devbox_git::mutate::unmerged_count(repo, branch, &task.base_branch)
            .await
            .map_err(git_failure)?
            == 0
        {
            return Ok(Some(Release::Merged));
        }
        let Some(pin) = task.closed_head_oid.as_deref() else {
            return Ok(None);
        };
        if task.state != AgentState::Closed || task.cleanup_error.is_none() {
            return Ok(None);
        }
        let tip = devbox_git::run::resolve(repo, &format!("refs/heads/{branch}"))
            .await
            .map_err(git_failure)?;
        if tip != pin {
            return Ok(None);
        }
        let off_base = match (task.closed_how, task.closed_merge_oid.as_deref()) {
            (Some(CloseHow::PullRequest), _) => true,
            (Some(CloseHow::Merged), Some(result)) => {
                match devbox_git::run::is_ancestor(repo, result, &task.base_branch).await {
                    Ok(on_base) => on_base,
                    // No answer keeps the branch; the rest of the list still shows.
                    Err(error) => {
                        tracing::warn!(agent = %task.id, %error, "merge commit ancestry unknown; branch kept");
                        false
                    }
                }
            }
            _ => false,
        };
        Ok(off_base.then_some(Release::OffBase))
    }

    /// Right before the teardown: whether what the folder shows after it is cleanup's own. It is
    /// when a teardown is configured, and the folder passes the removal's own clean check and is at
    /// the head the task closed with. Then any change made since came from the teardown or from
    /// the closed task's processes. Without a teardown, removal is as before.
    pub(crate) async fn undoable(
        &self,
        task: &AgentTask,
        wt: &Path,
        snapshot: &Option<AuthorizedConfig>,
    ) -> bool {
        if snapshot
            .as_ref()
            .is_none_or(|s| s.config().agent.teardown.is_empty())
        {
            return false;
        }
        let Some(pin) = task.closed_head_oid.as_deref() else {
            return false;
        };
        devbox_git::mutate::worktree_clean(wt).await.is_ok()
            && devbox_git::run::resolve(wt, "HEAD")
                .await
                .is_ok_and(|head| head == pin)
    }

    /// After the teardown of an [`Self::undoable`] folder: HEAD and the branch must still be at the
    /// head the task closed with. A teardown that committed, reset or moved the branch made changes
    /// no undo accounts for, so nothing is removed.
    pub(crate) async fn pins_held(
        &self,
        task: &AgentTask,
        repo: &Path,
        wt: &Path,
        branch: &str,
    ) -> Result<()> {
        let pin = task
            .closed_head_oid
            .as_deref()
            .ok_or(AgentsError::UnsafeWorktree)?;
        let head = devbox_git::run::resolve(wt, "HEAD")
            .await
            .map_err(git_failure)?;
        let tip = devbox_git::run::resolve(repo, &format!("refs/heads/{branch}"))
            .await
            .ok();
        if head == pin && tip.as_deref() == Some(pin) {
            return Ok(());
        }
        let moved = if head != pin {
            head
        } else {
            tip.unwrap_or(head)
        };
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let unmerged = devbox_git::mutate::unmerged_count(repo, &moved, &base)
            .await
            .map_err(git_failure)?;
        Err(AgentsError::StateChanged {
            head: moved,
            base,
            unmerged,
        }
        .into())
    }

    /// The teardown of an [`Self::undoable`] folder failed. What it wrote is undone as before a
    /// removal, after the same checks (ownership, pins, state, branch, operation), so the folder is
    /// clean at the closed head again and the task stays in the cleanup list until the command is
    /// fixed. When a check does not hold, a pin moved for one, the folder stays as the command left
    /// it. The caller reports the teardown's failure either way.
    pub(crate) async fn undo_failed_teardown(
        &self,
        task: &AgentTask,
        repo: &Path,
        wt: &Path,
        branch: &str,
    ) {
        let undone: Result<()> = async {
            self.owned_worktree(task).await?;
            self.pins_held(task, repo, wt, branch).await?;
            if self.get(&task.id).await?.state != AgentState::Closed {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            if devbox_git::current_branch(wt)
                .await
                .map_err(git_failure)?
                .as_deref()
                != Some(branch)
            {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            // The folder stays either way: one with submodules is not undone.
            self.refuse_submodules(wt, true).await?;
            self.undo_teardown(task, wt).await
        }
        .await;
        if let Err(error) = undone {
            let code = match &error {
                Fail::Domain(error) => error.to_string(),
                Fail::Internal(_) => "internal".into(),
            };
            tracing::warn!(agent = %task.id, %code, "kept what the failed teardown left");
        }
    }

    /// Right before the undo, after every other check passed. Git removes a folder with initialized
    /// submodules only with force, which devbox never uses, so such a folder is refused here with
    /// `agents.worktree_has_submodules`, which names it, and nothing is undone. The teardown has
    /// run by then, so what it stops is stopped. The force that code advises would lose changes
    /// and an operation in progress, so a folder that was not [`Self::undoable`] is first refused
    /// for changes the removal does not take, and any folder for an operation in progress. Without
    /// an answer to any of these, nothing is removed.
    pub(crate) async fn refuse_submodules(&self, wt: &Path, undoable: bool) -> Result<()> {
        if !undoable {
            devbox_git::mutate::worktree_clean(wt)
                .await
                .map_err(git_failure)?;
        }
        if cleanup_in_progress(wt).await? {
            return Err(AgentsError::OperationInProgress.into());
        }
        if devbox_git::mutate::has_submodules(wt)
            .await
            .map_err(git_failure)?
        {
            return Err(AgentsError::WorktreeHasSubmodules {
                path: wt.to_string_lossy().into_owned(),
            }
            .into());
        }
        Ok(())
    }

    /// The last step before an [`Self::undoable`] folder's removal, after every other check passed:
    /// tracked files and the index go back to HEAD, and untracked files that are not ignored go.
    /// Nested repositories and ignored files are left as they are, so the removal's clean check,
    /// made again here, still refuses a folder whose nested repository changed. A folder with
    /// initialized submodules never gets here ([`Self::refuse_submodules`]). In a folder with
    /// assume-unchanged or skip-worktree entries nothing is undone.
    pub(crate) async fn undo_teardown(&self, task: &AgentTask, wt: &Path) -> Result<()> {
        if cleanup_in_progress(wt).await? {
            return Err(AgentsError::OperationInProgress.into());
        }
        let changed = match devbox_git::mutate::worktree_clean(wt).await {
            Ok(()) => return Ok(()),
            Err(devbox_git::GitError::WorktreeDirty { files }) => files.len(),
            Err(error) => return Err(git_failure(error)),
        };
        // An edit `git status` does not show may be the user's, from before cleanup. Then nothing
        // is undone, and the removal refuses what is left, as it did before the undo existed.
        match devbox_git::mutate::hides_edits(wt).await {
            Ok(false) => (),
            Ok(true) => {
                tracing::info!(agent = %task.id, "work folder has entries git status does not show; nothing undone");
                return Ok(());
            }
            Err(error) => {
                tracing::warn!(agent = %task.id, %error, "work folder index flags unknown; nothing undone");
                return Ok(());
            }
        }
        // Paths stay out of the log; the count says how much cleanup undid.
        tracing::info!(agent = %task.id, changed, "undoing what changed in the work folder during cleanup");
        devbox_git::mutate::restore_tracked(wt)
            .await
            .map_err(git_failure)?;
        devbox_git::mutate::clean_untracked(wt)
            .await
            .map_err(git_failure)?;
        devbox_git::mutate::worktree_clean(wt)
            .await
            .map_err(git_failure)
    }
}
