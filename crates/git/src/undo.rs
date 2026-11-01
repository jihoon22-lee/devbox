//! Undo what changed in a work folder since its last commit, for a removal that takes only what
//! the caller can account for. `dir` is the work folder's top: both start from there. Neither
//! enters a submodule or a nested repository, and ignored files stay for the removal.
use crate::{
    GitError,
    run::{git, git_bytes},
};
use std::path::{Path, PathBuf};

/// The index of a large work folder lists one entry per file; past this, nothing is undone, and the
/// submodule check has no answer, so nothing is removed either.
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

/// Whether Git refuses to remove the work folder `dir` without force for its submodules, by the
/// check its removal makes: the folder's own Git directory has a `modules` folder, where an
/// initialized submodule's repository lives, or a gitlink in the index has a `.git` in its folder.
/// A gitlink that was never checked out is an empty folder and does not count. Decided from what
/// is on disk, never from Git's message.
pub async fn has_submodules(dir: &Path) -> Result<bool, GitError> {
    let modules = git(
        dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "modules",
        ],
    )
    .await?;
    if std::fs::metadata(modules.trim_end_matches('\n')).is_ok_and(|meta| meta.is_dir()) {
        return Ok(true);
    }
    // `<mode> <object> <stage>\t<path>` per entry.
    let index = git_bytes(dir, &["ls-files", "--stage", "-z"], INDEX_BYTES).await?;
    Ok(index
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let tab = entry.iter().position(|byte| *byte == b'\t')?;
            entry
                .starts_with(b"160000 ")
                .then(|| listed(&entry[tab + 1..]))
        })
        .any(|gitlink| std::fs::symlink_metadata(dir.join(gitlink).join(".git")).is_ok()))
}

/// A path as the index lists it, byte for byte: names need not be UTF-8.
fn listed(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        <std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(bytes).into()
    }
    #[cfg(not(unix))]
    {
        // A name that is not UTF-8 then reads as another, which has no `.git`: Git still refuses.
        String::from_utf8_lossy(bytes).into_owned().into()
    }
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
