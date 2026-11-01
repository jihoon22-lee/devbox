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
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
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

async fn undo_of(dir: &Path) -> git::mutate::IndexUndo {
    git::mutate::index_undo(dir).await.expect("index flags")
}

#[tokio::test]
async fn index_undo_counts_assume_unchanged_always_and_skip_worktree_only_where_its_file_is() {
    use git::mutate::IndexUndo::{HiddenEdit, Safe};
    let root = tempfile::tempdir().expect("owned fixture");
    let dir = repo(root.path(), "work");
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.join(name), "base\n").expect("owned fixture");
    }
    sh(&dir, &["add", "."]);
    sh(&dir, &["commit", "-qm", "init"]);
    assert_eq!(undo_of(&dir).await, Safe);
    // An edit status shows is not hidden.
    std::fs::write(dir.join("c.txt"), "shown\n").expect("owned fixture");
    assert_eq!(undo_of(&dir).await, Safe);
    // Assume-unchanged counts with or without an edit: status would show neither.
    sh(&dir, &["update-index", "--assume-unchanged", "a.txt"]);
    assert_eq!(undo_of(&dir).await, HiddenEdit);
    std::fs::write(dir.join("a.txt"), "the user's edit\n").expect("owned fixture");
    assert_eq!(sh(&dir, &["status", "--porcelain=v1"]), " M c.txt\n");
    assert_eq!(undo_of(&dir).await, HiddenEdit);
    sh(&dir, &["update-index", "--no-assume-unchanged", "a.txt"]);
    std::fs::write(dir.join("a.txt"), "base\n").expect("owned fixture");
    // Skip-worktree counts while its file is in the folder, where it can hold an edit status does
    // not show...
    sh(&dir, &["update-index", "--skip-worktree", "b.txt"]);
    std::fs::write(dir.join("b.txt"), "the user's edit\n").expect("owned fixture");
    assert_eq!(sh(&dir, &["status", "--porcelain=v1"]), " M c.txt\n");
    assert_eq!(undo_of(&dir).await, HiddenEdit);
    // ...and not once the file is gone: an absent file holds no edit.
    std::fs::remove_file(dir.join("b.txt")).expect("owned fixture");
    assert_eq!(undo_of(&dir).await, Safe);
}

#[tokio::test]
async fn index_undo_lets_a_sparse_checkout_be_undone_and_keeps_out_what_restore_cannot_match() {
    use git::mutate::IndexUndo::{HiddenEdit, NoPlainEntry, Safe};
    let root = tempfile::tempdir().expect("owned fixture");
    let main = repo(root.path(), "main");
    for name in ["top.txt", "keep/kept.txt", "out/outside.txt"] {
        let file = main.join(name);
        std::fs::create_dir_all(file.parent().expect("owned fixture")).expect("owned fixture");
        std::fs::write(file, "base\n").expect("owned fixture");
    }
    sh(&main, &["add", "."]);
    sh(&main, &["commit", "-qm", "init"]);
    sh(&main, &["sparse-checkout", "set", "--cone", "keep"]);
    // A worktree added from a sparse checkout is sparse too: the entry outside the cone is
    // skip-worktree and its file is not there.
    let add = |name: &str| {
        let dir = root.path().join(name);
        let path = dir.to_str().expect("owned fixture");
        sh(&main, &["worktree", "add", "-q", "-b", name, path]);
        dir
    };
    let wt = add("wt");
    assert_eq!(
        sh(&wt, &["ls-files", "-v"]),
        "H keep/kept.txt\nS out/outside.txt\nH top.txt\n"
    );
    assert!(!wt.join("out").exists());
    assert_eq!(undo_of(&wt).await, Safe);
    // The undo's own steps work there: the edit and the new file go, and the cone stays.
    std::fs::write(wt.join("keep/kept.txt"), "edited\n").expect("owned fixture");
    std::fs::write(wt.join("teardown.log"), "x\n").expect("owned fixture");
    git::mutate::restore_tracked(&wt)
        .await
        .expect("restore in a sparse checkout");
    git::mutate::clean_untracked(&wt)
        .await
        .expect("clean in a sparse checkout");
    git::mutate::worktree_clean(&wt)
        .await
        .expect("nothing is left");
    assert_eq!(read(&wt.join("keep/kept.txt")), "base\n");
    assert!(!wt.join("teardown.log").exists());
    assert!(!wt.join("out").exists());
    // A file outside the cone that the user put back, still skip-worktree, can hold an edit.
    sh(
        &wt,
        &[
            "config",
            "--worktree",
            "sparse.expectFilesOutsideOfPatterns",
            "true",
        ],
    );
    std::fs::create_dir(wt.join("out")).expect("owned fixture");
    std::fs::write(wt.join("out/outside.txt"), "the user's copy\n").expect("owned fixture");
    assert_eq!(sh(&wt, &["status", "--porcelain=v1"]), "");
    assert_eq!(undo_of(&wt).await, HiddenEdit);
    // With every entry skip-worktree, `git restore -- .` matches nothing and fails.
    let none = add("none");
    sh(
        &none,
        &["sparse-checkout", "set", "--no-cone", "/nothing-matches"],
    );
    assert_eq!(undo_of(&none).await, NoPlainEntry);
    assert!(git::mutate::restore_tracked(&none).await.is_err());
}

#[tokio::test]
async fn has_submodules_agrees_with_what_git_refuses_to_remove() {
    let root = tempfile::tempdir().expect("owned fixture");
    let lib = repo(root.path(), "lib");
    std::fs::write(lib.join("file.txt"), "one\n").expect("owned fixture");
    sh(&lib, &["add", "."]);
    sh(&lib, &["commit", "-qm", "lib"]);
    let main = repo(root.path(), "main");
    std::fs::write(main.join("top.txt"), "top\n").expect("owned fixture");
    sh(&main, &["add", "."]);
    sh(&main, &["commit", "-qm", "init"]);
    let source = lib.to_str().expect("owned fixture");
    sh(
        &main,
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
    sh(&main, &["commit", "-qm", "submodule"]);
    let add = |name: &str| {
        let dir = root.path().join(name);
        let path = dir.to_str().expect("owned fixture");
        sh(&main, &["worktree", "add", "-q", "-b", name, path]);
        dir
    };
    // Checked out without its submodule, the gitlink is an empty folder.
    let plain = add("plain");
    assert!(
        !git::mutate::has_submodules(&plain)
            .await
            .expect("submodule check")
    );
    // Initialized, the submodule's repository is in the folder's own Git directory.
    let initialized = add("initialized");
    sh(
        &initialized,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "update",
            "--init",
            "-q",
        ],
    );
    assert!(main.join(".git/worktrees/initialized/modules").is_dir());
    assert!(
        git::mutate::has_submodules(&initialized)
            .await
            .expect("submodule check")
    );
    // A repository of its own in the gitlink's folder, with no `modules` folder.
    let cloned = add("cloned");
    sh(
        root.path(),
        &[
            "clone",
            "-q",
            source,
            cloned.join("lib").to_str().expect("owned fixture"),
        ],
    );
    assert!(!main.join(".git/worktrees/cloned/modules").exists());
    assert!(
        git::mutate::has_submodules(&cloned)
            .await
            .expect("submodule check")
    );
    // Git, without force, refuses to remove the last two and removes the first.
    for (dir, refused) in [(&initialized, true), (&cloned, true), (&plain, false)] {
        let removal = Command::new("git")
            .current_dir(&main)
            .args(["worktree", "remove", "--"])
            .arg(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("owned removal");
        assert_eq!(!removal.status.success(), refused, "{dir:?}");
        assert_eq!(dir.exists(), refused, "{dir:?}");
    }
}
