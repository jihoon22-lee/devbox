//! Undo what changed in a work folder since its last commit, for a removal that takes only what
//! the caller can account for. `dir` is the work folder's top: both start from there. Neither
//! enters a submodule or a nested repository, and ignored files stay for the removal.
use crate::{
    GitError,
    run::{git, git_bytes},
};
use std::path::Path;

/// The index of a large work folder lists one entry per file; past this, nothing is undone.
const INDEX_BYTES: usize = 32 * 1024 * 1024;

/// Whether the index marks an entry assume-unchanged (a lowercase tag in `git ls-files -v`) or
/// skip-worktree (`S`). `git status` shows no edit to such a file, so the folder can look clean
/// while it holds one, maybe the user's: nothing may be undone there.
pub async fn hides_edits(dir: &Path) -> Result<bool, GitError> {
    let listing = git_bytes(dir, &["ls-files", "-v", "-z"], INDEX_BYTES).await?;
    Ok(listing
        .split(|byte| *byte == 0)
        .filter_map(|entry| entry.first())
        .any(|tag| tag.is_ascii_lowercase() || *tag == b'S'))
}

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
