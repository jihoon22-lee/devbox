use devbox_git::{GitError, mutate, run};
use std::{path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("owned Git");
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}
async fn fixture() -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().expect("owned repository");
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.name", "fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(root.path(), &["config", "commit.gpgSign", "false"]);
    git(root.path(), &["commit", "--allow-empty", "-m", "base"]);
    git(root.path(), &["checkout", "-b", "agent/review"]);
    git(
        root.path(),
        &["commit", "--allow-empty", "-m", "unmerged source"],
    );
    let head = run::resolve(root.path(), "HEAD").await.expect("source OID");
    git(root.path(), &["checkout", "main"]);
    (root, head)
}

#[tokio::test]
async fn confirmed_unmerged_branch_can_be_deleted_after_approved_closure() {
    let (root, head) = fixture().await;
    mutate::delete_branch_at(root.path(), "agent/review", &head)
        .await
        .expect("confirmed deletion");
    assert!(
        !devbox_git::branches(root.path())
            .await
            .expect("branches")
            .contains(&"agent/review".into())
    );
}

#[tokio::test]
async fn already_absent_confirmed_branch_is_complete() {
    let (root, head) = fixture().await;
    git(root.path(), &["branch", "-D", "agent/review"]);
    mutate::delete_branch_at(root.path(), "agent/review", &head)
        .await
        .expect("absent branch needs no deletion");
}

#[tokio::test]
async fn moved_branch_and_checked_out_branch_are_preserved() {
    let (root, head) = fixture().await;
    git(root.path(), &["checkout", "agent/review"]);
    assert!(matches!(
        mutate::delete_branch_at(root.path(), "agent/review", &head).await,
        Err(GitError::UnsafeWorktree)
    ));
    git(
        root.path(),
        &["commit", "--allow-empty", "-m", "later source"],
    );
    let later = run::resolve(root.path(), "HEAD").await.expect("later OID");
    git(root.path(), &["checkout", "main"]);
    assert!(matches!(
        mutate::delete_branch_at(root.path(), "agent/review", &head).await,
        Err(GitError::StateChanged)
    ));
    assert_eq!(
        run::resolve(root.path(), "agent/review")
            .await
            .expect("preserved ref"),
        later
    );
}

#[cfg(unix)]
#[test]
fn atomic_delete_ref_rechecks_expected_oid_at_the_git_mutation() {
    const CHILD: &str = "DEVBOX_BRANCH_CAS_CHILD";
    if std::env::var_os(CHILD).is_some() {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async {
                let (root, head) = fixture().await;
                let base = run::resolve(root.path(), "main").await.expect("base OID");
                assert!(matches!(
                    mutate::delete_branch_at(root.path(), "agent/review", &head).await,
                    Err(GitError::StateChanged)
                ));
                assert_eq!(
                    run::resolve(root.path(), "agent/review")
                        .await
                        .expect("preserved concurrently moved branch"),
                    base
                );
            });
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("owned wrapper");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("Git executable");
    assert!(real_git.status.success());
    let real_git = String::from_utf8(real_git.stdout)
        .expect("path")
        .trim()
        .to_owned();
    std::fs::write(
        root.path().join("git"),
        r#"#!/bin/sh
case " $* " in
 *" update-ref --no-deref -d "*)
    previous=; repo=
    for argument do
      if [ "$previous" = -C ]; then repo=$argument; fi
      previous=$argument
    done
    base=$("$DEVBOX_BRANCH_CAS_REAL_GIT" -C "$repo" rev-parse main)
    "$DEVBOX_BRANCH_CAS_REAL_GIT" -C "$repo" update-ref refs/heads/agent/review "$base" || exit 25
    ;;
esac
exec "$DEVBOX_BRANCH_CAS_REAL_GIT" "$@"
"#,
    )
    .expect("wrapper");
    std::fs::set_permissions(
        root.path().join("git"),
        std::fs::Permissions::from_mode(0o700),
    )
    .expect("mode");
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "atomic_delete_ref_rechecks_expected_oid_at_the_git_mutation",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("PATH", path)
        .env("DEVBOX_BRANCH_CAS_REAL_GIT", real_git)
        .output()
        .expect("owned child");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
