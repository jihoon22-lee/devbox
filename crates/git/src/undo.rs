//! Undo what changed in a work folder since its last commit, for a removal that takes only what
//! the caller can account for. `dir` is the work folder's top: both start from there. Neither
//! enters a submodule or a nested repository, and ignored files stay for the removal.
use crate::{GitError, run::git};
use std::path::Path;

/// Puts tracked files and the index back to HEAD: edits, deletions and staged additions are undone.
/// Submodules are not entered, even where `submodule.recurse` says otherwise, so what a submodule
/// holds stays as it is.
pub async fn restore_tracked(dir: &Path) -> Result<(), GitError> {
    git(
        dir,
        &[
            "restore",
            "--source=HEAD",
            "--staged",
            "--worktree",
            "--no-recurse-submodules",
            "--",
            ".",
        ],
    )
    .await?;
    Ok(())
}

/// Removes untracked files and folders that are not ignored. Without `-x` ignored files stay, and a
/// single `-f` leaves nested repositories alone.
pub async fn clean_untracked(dir: &Path) -> Result<(), GitError> {
    git(dir, &["clean", "-f", "-d"]).await?;
    Ok(())
}
