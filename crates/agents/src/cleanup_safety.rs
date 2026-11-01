//! What a closed task's cleanup may undo before it removes the work folder. Automatic cleanup and
//! the cleanup list share these steps, under the `actions` lock each already holds.
//!
//! devbox never removes a work folder with force: that would also skip Git's refusal of a folder
//! with initialized submodules, whose commits can exist only under `.git/worktrees/<name>/modules`.
//! A teardown (or a process of the closed task) that writes into the folder used to make every
//! cleanup refuse with `agents.worktree_dirty`, again on each retry, since each retry runs the
//! teardown again. So when the folder was clean at the closed head right before the teardown, what
//! changed since is cleanup's own and is undone, and only then is the folder removed. A folder
//! with changes of its own is refused as before (Ruling 61).
use crate::{
    cleanup::cleanup_in_progress,
    model::*,
    service::{AgentsService, Result},
};
use devbox_projects::trust::AuthorizedConfig;
use std::path::Path;

impl AgentsService {
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

    /// The last step before an [`Self::undoable`] folder's removal, after every other check passed:
    /// tracked files and the index go back to HEAD, and untracked files that are not ignored go.
    /// Submodules, nested repositories and ignored files are left as they are, so the removal's
    /// clean check, made again here, still refuses a folder whose submodule or nested repository
    /// changed. Git itself still refuses a folder with an initialized submodule.
    pub(crate) async fn undo_teardown(&self, task: &AgentTask, wt: &Path) -> Result<()> {
        if cleanup_in_progress(wt).await? {
            return Err(AgentsError::OperationInProgress.into());
        }
        let changed = match devbox_git::mutate::worktree_clean(wt).await {
            Ok(()) => return Ok(()),
            Err(devbox_git::GitError::WorktreeDirty { files }) => files.len(),
            Err(error) => return Err(git_failure(error)),
        };
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
