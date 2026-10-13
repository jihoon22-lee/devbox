use super::*;

#[cfg(unix)]
#[tokio::test]
async fn failed_worktree_hook_reports_partial_resources_and_preserves_hook_changes() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    let hook = d.path().join(".git/hooks/post-checkout");
    std::fs::write(
        &hook,
        "#!/bin/sh\nprintf hook-data > hook-created\necho hook-failed >&2\nexit 1\n",
    )
    .expect("owned fixture");
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
    let error = git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect_err("fixture operation must fail");
    assert!(
        matches!(&error, git::GitError::WorktreeAddPartial { branch, path, path_exists: true, branch_exists: Some(true), worktree_registered: Some(true), cause } if branch == "agent/x" && path == wt.to_str().expect("owned fixture") && matches!(cause.as_ref(), git::GitError::Failed { stderr, .. } if stderr.contains("hook-failed"))),
        "{error:?}"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("hook-created")).expect("owned fixture"),
        "hook-data"
    );
    assert_eq!(
        git::current_branch(&wt)
            .await
            .expect("owned fixture")
            .as_deref(),
        Some("agent/x")
    );
    assert!(
        git::branches(d.path())
            .await
            .expect("owned fixture")
            .contains(&"agent/x".into())
    );
}
#[tokio::test]
async fn checkout_shorthand_cannot_delete_a_different_literal_branch() {
    let d = repo();
    sh(d.path(), &["checkout", "-qb", "agent/x"]);
    sh(d.path(), &["checkout", "-q", "main"]);
    assert!(
        git::mutate::delete_branch(d.path(), "@{-1}", true)
            .await
            .is_err()
    );
    assert!(
        git::branches(d.path())
            .await
            .expect("owned fixture")
            .contains(&"agent/x".into())
    );
}

#[tokio::test]
async fn merge_at_rechecks_the_confirmed_base_and_source_branch() {
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("owned");
    git::mutate::worktree_add(d.path(), &wt, "agent/confirmed", "main")
        .await
        .expect("owned fixture");
    let base = git::run::resolve(d.path(), "main")
        .await
        .expect("owned fixture");
    commit_file(&wt, "new.txt", "agent\n");
    let head = git::run::resolve(&wt, "HEAD").await.expect("owned fixture");
    sh(d.path(), &["commit", "--allow-empty", "-qm", "late base"]);
    let current = git::run::resolve(d.path(), "main")
        .await
        .expect("owned fixture");
    let error = git::mutate::merge_into_at(
        d.path(),
        "main",
        &head,
        git::mutate::MergeStrategy::Merge,
        Some(&base),
        Some("agent/confirmed"),
    )
    .await
    .expect_err("fixture operation must fail");
    assert!(matches!(error, git::GitError::StateChanged));
    assert_eq!(
        git::run::resolve(d.path(), "HEAD")
            .await
            .expect("owned fixture"),
        current
    );
    sh(&wt, &["checkout", "-qb", "foreign"]);
    let error = git::mutate::merge_into_at(
        d.path(),
        "main",
        &head,
        git::mutate::MergeStrategy::Rebase {
            worktree: wt.clone(),
        },
        Some(&current),
        Some("agent/confirmed"),
    )
    .await
    .expect_err("fixture operation must fail");
    assert!(matches!(error, git::GitError::StateChanged));
    assert_eq!(
        git::current_branch(&wt)
            .await
            .expect("owned fixture")
            .as_deref(),
        Some("foreign")
    );
}
