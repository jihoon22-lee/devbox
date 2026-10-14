use super::*;

#[cfg(unix)]
#[tokio::test]
async fn failed_worktree_checkout_reports_partial_resources_and_preserves_written_files() {
    if checkout_failure_child() {
        return;
    }
    let d = repo();
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    let flag = std::env::var_os("DEVBOX_CHECKOUT_FAILURE_FLAG").expect("owned child flag");
    std::fs::write(flag, format!("{}\n{}\n", d.path().display(), wt.display()))
        .expect("owned fixture paths");
    let error = git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect_err("fixture operation must fail");
    assert!(
        matches!(&error, git::GitError::WorktreeAddPartial { branch, path, path_exists: true, branch_exists: Some(true), worktree_registered: Some(true), cause } if branch == "agent/x" && path == wt.to_str().expect("owned fixture") && matches!(cause.as_ref(), git::GitError::Failed { stderr, .. } if stderr.contains("owned checkout response failure"))),
        "{error:?}"
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("checkout-created")).expect("owned fixture"),
        "owned-checkout-data"
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

#[cfg(unix)]
fn checkout_failure_child() -> bool {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("DEVBOX_CHECKOUT_FAILURE_FLAG").is_some() {
        return false;
    }
    let root = tempfile::tempdir().expect("owned wrapper");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("Git path");
    assert!(real_git.status.success());
    let wrapper = root.path().join("git");
    std::fs::write(
        &wrapper,
        r#"#!/bin/sh
checkout=; previous=; target=; current_repo=
for argument do
 if [ "$previous:$argument" = worktree:add ]; then checkout=1; fi
 if [ "$previous" = -- ]; then target=$argument; fi
 if [ "$previous" = -C ]; then current_repo=$argument; fi
 previous=$argument
done
"$DEVBOX_CHECKOUT_REAL_GIT" "$@" || exit $?
if [ "$checkout" = 1 ] && [ -f "$DEVBOX_CHECKOUT_FAILURE_FLAG" ]; then
 repo=$(sed -n '1p' "$DEVBOX_CHECKOUT_FAILURE_FLAG")
 expected=$(sed -n '2p' "$DEVBOX_CHECKOUT_FAILURE_FLAG")
 if [ "$current_repo" = "$repo" ] && [ "$target" = "$expected" ]; then
  printf owned-checkout-data > "$target/checkout-created"
  echo 'owned checkout response failure' >&2
  exit 3
 fi
fi
"#,
    )
    .expect("owned wrapper");
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("wrapper mode");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["safety_cases::failed_worktree_checkout_reports_partial_resources_and_preserves_written_files", "--exact", "--nocapture"])
        .env("DEVBOX_CHECKOUT_FAILURE_FLAG", root.path().join("paths"))
        .env("DEVBOX_CHECKOUT_REAL_GIT", String::from_utf8(real_git.stdout).expect("Git path").trim())
        .env("PATH", format!("{}:{}", root.path().display(), std::env::var("PATH").expect("PATH")))
        .output().expect("owned child");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "child must execute exactly the selected test"
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    true
}
