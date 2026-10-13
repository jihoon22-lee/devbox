use devbox_git as git;
use std::{path::Path, process::Command};
fn sh(dir: &Path, args: &[&str]) {
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
}
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("fixture");
    sh(d.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(d.path().join("a.txt"), "one\n").expect("write");
    sh(d.path(), &["add", "."]);
    sh(d.path(), &["commit", "-qm", "init"]);
    d
}
#[tokio::test]
async fn reports_branch_changes_commits_and_ahead_behind() {
    let d = repo();
    sh(d.path(), &["checkout", "-qb", "agent/x"]);
    std::fs::write(d.path().join("a.txt"), "one\ntwo\n").expect("write");
    sh(d.path(), &["commit", "-qam", "add two"]);
    std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\n").expect("write");
    std::fs::write(d.path().join("b.txt"), "new").expect("write");
    assert_eq!(
        git::current_branch(d.path()).await.unwrap().as_deref(),
        Some("agent/x")
    );
    let st = git::status(d.path()).await.unwrap();
    assert!(!st.clean);
    assert_eq!(st.untracked, 1);
    assert_eq!(
        git::ahead_behind(d.path(), "main", "HEAD").await.unwrap(),
        (1, 0)
    );
    let files = git::changed_files(d.path(), "main").await.unwrap();
    let a = files.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!((a.added, a.removed), (2, 0));
    let b = files.iter().find(|f| f.path == "b.txt").unwrap();
    assert_eq!(b.added, 1);
    assert!(
        git::file_patch(d.path(), "main", "a.txt")
            .await
            .unwrap()
            .contains("+three")
    );
    assert_eq!(
        git::log_range(d.path(), "main").await.unwrap()[0].subject,
        "add two"
    );
}
#[tokio::test]
async fn non_repository_and_unborn_and_detached_are_distinct() {
    let d = tempfile::tempdir().unwrap();
    assert!(matches!(
        git::status(d.path()).await,
        Err(git::GitError::NotRepo)
    ));
    sh(d.path(), &["init", "-q", "-b", "main"]);
    assert_eq!(
        git::status(d.path()).await.unwrap().branch.as_deref(),
        Some("main")
    );
    assert!(git::ahead_behind(d.path(), "main", "HEAD").await.is_err());
    let d = repo();
    sh(d.path(), &["checkout", "--detach", "-q"]);
    assert_eq!(git::current_branch(d.path()).await.unwrap(), None);
}
#[tokio::test]
async fn literal_unusual_paths_renames_binary_and_empty_files() {
    let d = repo();
    for name in ["a space\t한글\n.txt", ":(glob)*", "-option", "empty"] {
        std::fs::write(
            d.path().join(name),
            if name == "empty" {
                ""
            } else {
                "line without newline"
            },
        )
        .unwrap();
    }
    std::fs::write(d.path().join("binary"), b"\0one\n").unwrap();
    let files = git::changed_files(d.path(), "main").await.unwrap();
    for name in ["a space\t한글\n.txt", ":(glob)*", "-option"] {
        assert_eq!(files.iter().find(|f| f.path == name).unwrap().added, 1);
        assert!(
            git::file_patch(d.path(), "main", name)
                .await
                .unwrap()
                .contains("+line without newline")
        );
    }
    assert_eq!(files.iter().find(|f| f.path == "empty").unwrap().added, 0);
    assert_eq!(files.iter().find(|f| f.path == "binary").unwrap().added, 0);
    assert_eq!(
        git::file_patch(d.path(), "main", "a.txt").await.unwrap(),
        ""
    );
    assert!(
        git::file_patch(d.path(), "main", "../outside")
            .await
            .is_err()
    );
    assert!(
        git::file_patch(d.path(), "main", "/etc/passwd")
            .await
            .is_err()
    );
    assert!(git::ahead_behind(d.path(), "--all", "HEAD").await.is_err());
    sh(d.path(), &["mv", "a.txt", "renamed space\t\n.txt"]);
    let files = git::changed_files(d.path(), "main").await.unwrap();
    let renamed = files
        .iter()
        .find(|f| f.path == "renamed space\t\n.txt")
        .unwrap();
    assert_eq!(renamed.status, git::ChangeStatus::Renamed);
    assert_eq!(renamed.old_path.as_deref(), Some("a.txt"));
    assert_eq!(git::status(d.path()).await.unwrap().staged, 1);
}
#[tokio::test]
async fn conflicts_keep_complete_paths_and_branch_log_queries() {
    let d = repo();
    let name = "conflict space\t한글\n.txt";
    std::fs::write(d.path().join(name), "base\n").unwrap();
    sh(d.path(), &["add", "."]);
    sh(d.path(), &["commit", "-qm", "base"]);
    sh(d.path(), &["checkout", "-qb", "side"]);
    std::fs::write(d.path().join(name), "side\n").unwrap();
    sh(d.path(), &["commit", "-qam", "side"]);
    sh(d.path(), &["checkout", "-q", "main"]);
    std::fs::write(d.path().join(name), "main\n").unwrap();
    sh(d.path(), &["commit", "-qam", "main"]);
    let result = Command::new("git")
        .args([
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@test",
            "merge",
            "side",
        ])
        .current_dir(d.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(git::status(d.path()).await.unwrap().conflicted, vec![name]);
    assert!(!git::status(d.path()).await.unwrap().clean);
    let files = git::changed_files(d.path(), "HEAD").await.unwrap();
    assert!(
        files
            .iter()
            .any(|f| f.path == name && f.status == git::ChangeStatus::Unmerged)
    );
    std::fs::create_dir(d.path().join("nested")).unwrap();
    let subdir_files = git::changed_files(&d.path().join("nested"), "HEAD")
        .await
        .unwrap();
    assert_eq!(
        subdir_files, files,
        "subdirectory queries keep repository-root paths, including conflicts"
    );
    assert_eq!(git::branches(d.path()).await.unwrap(), vec!["main", "side"]);
    let log = git::log_range(d.path(), "side").await.unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "main");
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_diff_never_reads_target_and_invalid_utf8_is_rejected() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let d = repo();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "private target data").unwrap();
    symlink(outside.path().join("secret"), d.path().join("link")).unwrap();
    let patch = git::file_patch(d.path(), "main", "link").await.unwrap();
    assert!(!patch.contains("private target data"));
    std::fs::write(
        d.path().join(std::ffi::OsString::from_vec(vec![b'x', 255])),
        "bad",
    )
    .unwrap();
    assert!(matches!(
        git::changed_files(d.path(), "main").await,
        Err(git::GitError::InvalidEncoding)
    ));
}
#[tokio::test]
async fn branch_divergence_and_subdirectory_queries_are_consistent() {
    let d = repo();
    sh(d.path(), &["checkout", "-qb", "agent"]);
    std::fs::create_dir(d.path().join("nested")).unwrap();
    std::fs::write(d.path().join("nested/item"), "item\n").unwrap();
    sh(d.path(), &["add", "."]);
    sh(d.path(), &["commit", "-qm", "nested"]);
    sh(d.path(), &["branch", "--set-upstream-to=main"]);
    sh(d.path(), &["checkout", "-q", "main"]);
    std::fs::write(d.path().join("other"), "other\n").unwrap();
    sh(d.path(), &["add", "."]);
    sh(d.path(), &["commit", "-qm", "other"]);
    sh(d.path(), &["checkout", "-q", "agent"]);
    assert_eq!(
        git::ahead_behind(d.path(), "main", "HEAD").await.unwrap(),
        (1, 1)
    );
    let status = git::status(d.path()).await.unwrap();
    assert_eq!((status.ahead, status.behind), (1, 1));
    let files = git::changed_files(&d.path().join("nested"), "main")
        .await
        .unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "nested/item");
    assert!(
        git::file_patch(&d.path().join("nested"), "main", "nested/item")
            .await
            .unwrap()
            .contains("+item")
    );
    assert_eq!(git::log_range(d.path(), "main").await.unwrap().len(), 1);
}
#[cfg(unix)]
#[tokio::test]
async fn untracked_fifo_is_ignored_without_opening() {
    let d = repo();
    assert!(
        Command::new("mkfifo")
            .arg(d.path().join("pipe"))
            .status()
            .unwrap()
            .success()
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        git::file_patch(d.path(), "main", "pipe"),
    )
    .await;
    assert_eq!(
        result
            .expect("FIFO query must not block")
            .expect("Git ignores non-files"),
        ""
    );
}
#[tokio::test]
async fn staged_delete_recreated_untracked_is_a_combined_change() {
    let d = repo();
    sh(d.path(), &["rm", "-q", "a.txt"]);
    sh(d.path(), &["config", "core.splitIndex", "true"]);
    sh(d.path(), &["config", "core.sparseCheckout", "true"]);
    let index_before = std::fs::read(d.path().join(".git/index")).unwrap();
    let git_files = || {
        let mut files: Vec<_> = std::fs::read_dir(d.path().join(".git"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        files.sort();
        files
    };
    let files_before = git_files();
    std::fs::write(d.path().join("a.txt"), "two\n").unwrap();
    let files = git::changed_files(d.path(), "main").await.unwrap();
    let a = files.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!(
        (a.status, a.added, a.removed),
        (git::ChangeStatus::Modified, 1, 1)
    );
    let patch = git::file_patch(d.path(), "main", "a.txt").await.unwrap();
    assert!(patch.contains("-one") && patch.contains("+two"));
    std::fs::write(d.path().join("a.txt"), "one\n").unwrap();
    assert!(
        !git::changed_files(d.path(), "main")
            .await
            .unwrap()
            .iter()
            .any(|f| f.path == "a.txt")
    );
    assert_eq!(
        git::file_patch(d.path(), "main", "a.txt").await.unwrap(),
        ""
    );
    assert_eq!(
        std::fs::read(d.path().join(".git/index")).unwrap(),
        index_before
    );
    assert_eq!(
        git_files(),
        files_before,
        "queries create no shared index or other repository files"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn recreated_file_queries_do_not_run_index_hooks() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    sh(d.path(), &["rm", "-q", "a.txt"]);
    std::fs::write(d.path().join("a.txt"), "replacement\n").unwrap();
    let hook = d.path().join(".git/hooks/post-index-change");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf hook-ran > .git/query-hook-marker\n",
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).unwrap();
    let original_index = std::fs::read(d.path().join(".git/index")).unwrap();
    git::changed_files(d.path(), "main").await.unwrap();
    git::file_patch(d.path(), "main", "a.txt").await.unwrap();
    assert!(
        !d.path().join(".git/query-hook-marker").exists(),
        "read-only queries must not execute project hooks"
    );
    assert_eq!(
        std::fs::read(d.path().join(".git/index")).unwrap(),
        original_index
    );
}
