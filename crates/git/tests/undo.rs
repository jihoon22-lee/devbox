//! The undo helpers a closed task's cleanup runs before it removes a work folder: what they put
//! back, what they remove, and what they must leave alone.
use devbox_git as git;
use std::{path::Path, process::Command};

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test")
        .output()
        .expect("fixture git");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("fixture output")
}
fn repo(root: &Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    std::fs::create_dir(&dir).expect("owned fixture");
    sh(&dir, &["init", "-q", "-b", "main"]);
    sh(&dir, &["config", "commit.gpgSign", "false"]);
    dir
}
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("owned fixture")
}

#[tokio::test]
async fn restore_tracked_puts_edits_deletions_and_staged_files_back_to_head() {
    let root = tempfile::tempdir().expect("owned fixture");
    let dir = repo(root.path(), "work");
    std::fs::write(dir.join("a.txt"), "a\n").expect("owned fixture");
    std::fs::write(dir.join("b.txt"), "b\n").expect("owned fixture");
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("a.txt"), "edited\n").expect("owned fixture");
    std::fs::remove_file(dir.join("b.txt")).expect("owned fixture");
    std::fs::write(dir.join("staged.txt"), "staged\n").expect("owned fixture");
    sh(&dir, &["add", "staged.txt"]);
    std::fs::write(dir.join("untracked.txt"), "untracked\n").expect("owned fixture");
    git::mutate::restore_tracked(&dir)
        .await
        .expect("restore owned fixture");
    assert_eq!(read(&dir.join("a.txt")), "a\n");
    assert_eq!(read(&dir.join("b.txt")), "b\n");
    // A file staged since HEAD is not in HEAD, so both the index entry and the file go.
    assert!(!dir.join("staged.txt").exists());
    // Untracked files are `clean_untracked`'s to remove, not this one's.
    assert_eq!(read(&dir.join("untracked.txt")), "untracked\n");
    assert_eq!(
        sh(&dir, &["status", "--porcelain=v1"]),
        "?? untracked.txt\n"
    );
}

#[tokio::test]
async fn clean_untracked_keeps_ignored_files_and_nested_repositories() {
    let root = tempfile::tempdir().expect("owned fixture");
    let dir = repo(root.path(), "work");
    std::fs::write(dir.join(".gitignore"), "*.log\n").expect("owned fixture");
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    std::fs::write(dir.join("left.txt"), "left\n").expect("owned fixture");
    std::fs::create_dir_all(dir.join("scratch/deep")).expect("owned fixture");
    std::fs::write(dir.join("scratch/deep/file"), "x\n").expect("owned fixture");
    std::fs::write(dir.join("keep.log"), "ignored\n").expect("owned fixture");
    let nested = dir.join("vendor/tool");
    std::fs::create_dir_all(&nested).expect("owned fixture");
    sh(&nested, &["init", "-q"]);
    std::fs::write(nested.join("data"), "nested\n").expect("owned fixture");
    git::mutate::clean_untracked(&dir)
        .await
        .expect("clean owned fixture");
    assert!(!dir.join("left.txt").exists());
    assert!(!dir.join("scratch").exists());
    // No `-x`: an ignored file stays, for the removal to take with the folder.
    assert_eq!(read(&dir.join("keep.log")), "ignored\n");
    // A single `-f`: Git skips a nested repository, so it is still there and still dirty.
    assert!(nested.join(".git").is_dir());
    assert_eq!(read(&nested.join("data")), "nested\n");
    assert!(matches!(
        git::mutate::worktree_clean(&dir).await,
        Err(git::GitError::WorktreeDirty { files }) if files == ["vendor/tool/"]
    ));
}

#[tokio::test]
async fn restore_tracked_never_enters_a_submodule_even_when_recursion_is_configured() {
    let root = tempfile::tempdir().expect("owned fixture");
    let lib = repo(root.path(), "lib");
    std::fs::write(lib.join("file.txt"), "one\n").expect("owned fixture");
    sh(&lib, &["add", "."]);
    sh(&lib, &["commit", "-qm", "lib"]);
    let dir = repo(root.path(), "work");
    std::fs::write(dir.join("top.txt"), "top\n").expect("owned fixture");
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    let source = lib.to_str().expect("owned fixture");
    sh(
        &dir,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            source,
            "lib",
        ],
    );
    sh(&dir, &["commit", "-qm", "submodule"]);
    // Without `--no-recurse-submodules` this setting makes `git restore` reset the submodule's
    // files and detach its HEAD.
    sh(&dir, &["config", "submodule.recurse", "true"]);
    let sub = dir.join("lib");
    sh(&sub, &["config", "commit.gpgSign", "false"]);
    std::fs::write(sub.join("file.txt"), "unpushed\n").expect("owned fixture");
    sh(&sub, &["commit", "-qam", "unpushed"]);
    let unpushed = sh(&sub, &["rev-parse", "HEAD"]);
    std::fs::write(sub.join("file.txt"), "edited in the submodule\n").expect("owned fixture");
    std::fs::write(dir.join("top.txt"), "edited\n").expect("owned fixture");
    git::mutate::restore_tracked(&dir)
        .await
        .expect("restore owned fixture");
    assert_eq!(read(&dir.join("top.txt")), "top\n");
    assert_eq!(read(&sub.join("file.txt")), "edited in the submodule\n");
    assert_eq!(sh(&sub, &["rev-parse", "HEAD"]), unpushed);
    assert_eq!(
        sh(&sub, &["symbolic-ref", "HEAD"]).trim_end(),
        "refs/heads/main"
    );
    // The submodule's change still shows, so a removal check still refuses the folder.
    assert!(matches!(
        git::mutate::worktree_clean(&dir).await,
        Err(git::GitError::WorktreeDirty { files }) if files == ["lib"]
    ));
}

#[tokio::test]
async fn worktree_clean_is_the_check_a_removal_without_force_makes() {
    let root = tempfile::tempdir().expect("owned fixture");
    let dir = repo(root.path(), "work");
    std::fs::write(dir.join(".gitignore"), "*.log\n").expect("owned fixture");
    std::fs::write(dir.join("a.txt"), "a\n").expect("owned fixture");
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    git::mutate::worktree_clean(&dir)
        .await
        .expect("a committed folder is clean");
    std::fs::write(dir.join("build.log"), "ignored\n").expect("owned fixture");
    git::mutate::worktree_clean(&dir)
        .await
        .expect("ignored files do not count");
    std::fs::write(dir.join("a.txt"), "edited\n").expect("owned fixture");
    std::fs::write(dir.join("new.txt"), "new\n").expect("owned fixture");
    assert!(matches!(
        git::mutate::worktree_clean(&dir).await,
        Err(git::GitError::WorktreeDirty { files }) if files == ["a.txt", "new.txt"]
    ));
}
