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
async fn squash_and_safe_worktree_removal() {
    let d = repo();
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    std::fs::write(wt.join("new"), "new").expect("owned fixture");
    sh(&wt, &["add", "."]);
    sh(&wt, &["commit", "-qm", "new"]);
    sh(d.path(), &["config", "user.name", "test"]);
    sh(d.path(), &["config", "user.email", "test@test"]);
    git::mutate::merge_into(
        d.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "squash".into(),
        },
    )
    .await
    .expect("owned fixture");
    assert!(d.path().join("new").exists());
    assert!(
        git::mutate::worktree_remove(d.path(), d.path(), true)
            .await
            .is_err()
    );
    git::mutate::worktree_remove(d.path(), &wt, false)
        .await
        .expect("owned fixture");
}
fn identity(d: &Path) {
    sh(d, &["config", "user.name", "test"]);
    sh(d, &["config", "user.email", "test@test"]);
}
fn commit_file(d: &Path, name: &str, contents: &str) {
    std::fs::write(d.join(name), contents).expect("owned fixture");
    sh(d, &["add", "."]);
    sh(d, &["commit", "-qm", "change"]);
}
#[tokio::test]
async fn conflicts_restore_base_and_rebase_conflicts_remain_until_abort() {
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    commit_file(&wt, "a.txt", "side\n");
    commit_file(d.path(), "a.txt", "base\n");
    let before = git::run::resolve(d.path(), "HEAD")
        .await
        .expect("owned fixture");
    for strategy in [
        git::mutate::MergeStrategy::Merge,
        git::mutate::MergeStrategy::Squash {
            message: "squash".into(),
        },
        git::mutate::MergeStrategy::Rebase {
            worktree: wt.clone(),
        },
    ] {
        let error = git::mutate::merge_into(d.path(), "main", "agent/x", strategy)
            .await
            .expect_err("fixture operation must fail");
        assert!(
            matches!(error, git::GitError::Conflict { ref files } if files == &["a.txt"]),
            "{error:?}"
        );
        assert_eq!(
            git::run::resolve(d.path(), "HEAD")
                .await
                .expect("owned fixture"),
            before
        );
        assert!(git::status(d.path()).await.expect("owned fixture").clean);
        assert!(git::status(&wt).await.expect("owned fixture").clean);
    }
    assert!(matches!(
        git::mutate::rebase_onto(&wt, "main")
            .await
            .expect("owned fixture"),
        git::mutate::RebaseOutcome::Conflict { .. }
    ));
    assert!(matches!(
        git::mutate::rebase_onto(&wt, "main").await,
        Err(git::GitError::OperationInProgress)
    ));
    git::mutate::rebase_abort(&wt).await.expect("owned fixture");
    assert!(git::status(&wt).await.expect("owned fixture").clean);
}
#[tokio::test]
async fn rebase_success_counts_and_ancestry_cleanup() {
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    commit_file(&wt, "side", "side");
    commit_file(d.path(), "base", "base");
    assert_eq!(
        git::mutate::unmerged_count(d.path(), "agent/x", "main")
            .await
            .expect("owned fixture"),
        1
    );
    assert!(
        git::mutate::merged_agent_branches(d.path(), "main")
            .await
            .expect("owned fixture")
            .is_empty()
    );
    git::mutate::merge_into(
        d.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Rebase {
            worktree: wt.clone(),
        },
    )
    .await
    .expect("owned fixture");
    assert!(d.path().join("side").exists());
    assert_eq!(
        git::mutate::merged_agent_branches(d.path(), "main")
            .await
            .expect("owned fixture"),
        vec!["agent/x"]
    );
    git::mutate::worktree_remove(d.path(), &wt, false)
        .await
        .expect("owned fixture");
    git::mutate::delete_branch(d.path(), "agent/x", false)
        .await
        .expect("owned fixture");
}
#[tokio::test]
async fn guards_reject_dirty_wrong_foreign_locked_existing_and_options() {
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    assert!(matches!(
        git::mutate::worktree_add(d.path(), &wt, "agent/x", "main").await,
        Err(git::GitError::BranchExists { .. })
    ));
    let existing = holder.path().join("existing");
    std::fs::create_dir(&existing).expect("owned fixture");
    assert!(
        git::mutate::worktree_add(d.path(), &existing, "agent/new", "main")
            .await
            .is_err()
    );
    assert!(
        !git::branches(d.path())
            .await
            .expect("owned fixture")
            .contains(&"agent/new".into())
    );
    std::fs::write(d.path().join("untracked"), "u").expect("owned fixture");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Merge
        )
        .await,
        Err(git::GitError::BaseDirty { .. })
    ));
    std::fs::remove_file(d.path().join("untracked")).expect("owned fixture");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "other",
            "agent/x",
            git::mutate::MergeStrategy::Merge
        )
        .await,
        Err(git::GitError::BaseNotCheckedOut { .. })
    ));
    std::fs::write(wt.join("untracked"), "u").expect("owned fixture");
    assert!(matches!(
        git::mutate::worktree_remove(d.path(), &wt, false).await,
        Err(git::GitError::WorktreeDirty { .. })
    ));
    sh(
        d.path(),
        &["worktree", "lock", wt.to_str().expect("owned fixture")],
    );
    assert!(
        git::mutate::worktree_remove(d.path(), &wt, true)
            .await
            .is_err()
    );
    sh(
        d.path(),
        &["worktree", "unlock", wt.to_str().expect("owned fixture")],
    );
    let foreign = repo();
    assert!(
        git::mutate::worktree_remove(d.path(), foreign.path(), true)
            .await
            .is_err()
    );
    assert!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Rebase {
                worktree: foreign.path().into()
            }
        )
        .await
        .is_err()
    );
    for branch in ["--all", "HEAD", "bad..name"] {
        assert!(
            git::mutate::delete_branch(d.path(), branch, true)
                .await
                .is_err()
        );
    }
    git::mutate::worktree_remove(d.path(), &wt, true)
        .await
        .expect("owned fixture");
}
#[cfg(unix)]
#[tokio::test]
async fn symlink_worktree_alias_is_never_removed() {
    let d = repo();
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    let alias = holder.path().join("alias");
    std::os::unix::fs::symlink(&wt, &alias).expect("owned fixture");
    assert!(
        git::mutate::worktree_remove(d.path(), &alias, true)
            .await
            .is_err()
    );
    assert!(wt.exists());
}
#[cfg(unix)]
#[tokio::test]
async fn failed_squash_commit_rolls_back_and_preserves_failure() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    commit_file(&wt, "new", "new");
    let hook = d.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho rejected >&2\nexit 1\n").expect("owned fixture");
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
    let before = git::run::resolve(d.path(), "HEAD")
        .await
        .expect("owned fixture");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Squash {
                message: "squash".into()
            }
        )
        .await,
        Err(git::GitError::Failed { .. })
    ));
    assert_eq!(
        git::run::resolve(d.path(), "HEAD")
            .await
            .expect("owned fixture"),
        before
    );
    assert!(git::status(d.path()).await.expect("owned fixture").clean);
    assert!(!d.path().join("new").exists());
}
#[cfg(unix)]
#[test]
fn fake_gh_cases_use_isolated_process_environment() {
    for case in [
        "success",
        "failed",
        "empty",
        "push-failed",
        "missing",
        "switched",
    ] {
        use std::os::unix::fs::PermissionsExt;
        let fixture = tempfile::tempdir().expect("owned fixture");
        let bin = fixture.path().join("bin");
        std::fs::create_dir(&bin).expect("owned fixture");
        std::os::unix::fs::symlink(which::which("git").expect("owned fixture"), bin.join("git"))
            .expect("owned fixture");
        let marker = fixture.path().join("body");
        let script = "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then exit 0; fi\nfor arg do last=$arg; done\n/bin/cat \"$last\" > \"$DEVBOX_GIT_BODY_MARKER\"\ncase \"$DEVBOX_GIT_GH_CASE\" in failed) echo gh-error >&2; exit 1;; empty) exit 0;; esac\nprintf 'https://example.invalid/pr/1\\n'\n";
        if case != "missing" {
            std::fs::write(bin.join("gh"), script).expect("owned fixture");
            std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o700))
                .expect("owned fixture");
        }
        let out = Command::new(std::env::current_exe().expect("owned fixture"))
            .args(["--exact", "fake_gh_child", "--nocapture"])
            .env("DEVBOX_GIT_GH_CASE", case)
            .env("PATH", &bin)
            .env("DEVBOX_GIT_BODY_MARKER", &marker)
            .output()
            .expect("owned fixture");
        assert!(
            out.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn fake_gh_child() {
    let Ok(case) = std::env::var("DEVBOX_GIT_GH_CASE") else {
        return;
    };
    let d = repo();
    identity(d.path());
    let fixture = tempfile::tempdir().expect("owned fixture");
    let origin = fixture.path().join("origin.git");
    sh(
        fixture.path(),
        &[
            "init",
            "--bare",
            "-q",
            origin.to_str().expect("owned fixture"),
        ],
    );
    sh(
        d.path(),
        &[
            "remote",
            "add",
            "origin",
            origin.to_str().expect("owned fixture"),
        ],
    );
    sh(d.path(), &["checkout", "-qb", "agent/x"]);
    commit_file(d.path(), "side", "side");
    let oid = git::run::resolve(d.path(), "HEAD")
        .await
        .expect("owned fixture");
    let marker =
        Path::new(&std::env::var("DEVBOX_GIT_BODY_MARKER").expect("owned fixture")).to_path_buf();
    if case == "push-failed" {
        sh(
            d.path(),
            &[
                "remote",
                "set-url",
                "origin",
                fixture
                    .path()
                    .join("absent.git")
                    .to_str()
                    .expect("owned fixture"),
            ],
        );
    }
    if case == "switched" {
        sh(d.path(), &["checkout", "-q", "main"]);
    }
    let body = "line one\n\nline two\n$(literal) `literal`";
    let result =
        git::mutate::push_and_pr_at(d.path(), "agent/x", &oid, "main", "Title", body).await;
    match case.as_str() {
        "success" => assert_eq!(
            result.expect("owned fixture"),
            "https://example.invalid/pr/1"
        ),
        "failed" => assert!(matches!(result, Err(git::GitError::GhFailed { .. }))),
        "empty" => assert!(matches!(result, Err(git::GitError::GhInvalidOutput))),
        "push-failed" => assert!(matches!(result, Err(git::GitError::PushFailed { .. }))),
        "missing" => assert!(matches!(result, Err(git::GitError::GhMissing))),
        "switched" => assert!(matches!(result, Err(git::GitError::StateChanged))),
        _ => panic!("unknown case"),
    }
    if matches!(case.as_str(), "success" | "failed" | "empty") {
        assert_eq!(
            std::fs::read_to_string(marker).expect("owned fixture"),
            body
        );
        assert_eq!(
            git::run::resolve(&origin, "agent/x")
                .await
                .expect("owned fixture"),
            oid
        );
    } else {
        assert!(!marker.exists());
        assert!(git::run::resolve(&origin, "agent/x").await.is_err());
    }
}
#[cfg(unix)]
#[tokio::test]
async fn rollback_preserves_head_changed_by_failing_hook_and_add_rejects_parent_alias() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    commit_file(&wt, "new", "new");
    sh(d.path(), &["branch", "outside", "agent/x"]);
    let outside = git::run::resolve(d.path(), "outside")
        .await
        .expect("owned fixture");
    let hook = d.path().join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\ngit update-ref refs/heads/main refs/heads/outside\nexit 1\n",
    )
    .expect("owned fixture");
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Squash {
                message: "squash".into()
            }
        )
        .await,
        Err(git::GitError::StateChanged)
    ));
    assert_eq!(
        git::run::resolve(d.path(), "HEAD")
            .await
            .expect("owned fixture"),
        outside
    );
    let alias = holder.path().join("alias");
    std::os::unix::fs::symlink(holder.path(), &alias).expect("owned fixture");
    assert!(
        git::mutate::worktree_add(d.path(), &alias.join("new"), "agent/new", "main")
            .await
            .is_err()
    );
    assert!(
        !git::branches(d.path())
            .await
            .expect("owned fixture")
            .contains(&"agent/new".into())
    );
}
#[tokio::test]
async fn existing_merge_state_is_preserved_and_wrong_rebase_worktree_is_rejected() {
    let d = repo();
    identity(d.path());
    let holder = tempfile::tempdir().expect("owned fixture");
    let wt = holder.path().join("worker");
    git::mutate::worktree_add(d.path(), &wt, "agent/x", "main")
        .await
        .expect("owned fixture");
    let other = holder.path().join("other");
    git::mutate::worktree_add(d.path(), &other, "agent/other", "main")
        .await
        .expect("owned fixture");
    commit_file(&other, "other", "other");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Rebase { worktree: other }
        )
        .await,
        Err(git::GitError::StateChanged)
    ));
    commit_file(&wt, "a.txt", "side");
    commit_file(d.path(), "a.txt", "base");
    assert!(
        git::run::git(d.path(), &["merge", "agent/x"])
            .await
            .is_err()
    );
    let index = std::fs::read(d.path().join(".git/index")).expect("owned fixture");
    assert!(matches!(
        git::mutate::merge_into(
            d.path(),
            "main",
            "agent/x",
            git::mutate::MergeStrategy::Merge
        )
        .await,
        Err(git::GitError::OperationInProgress)
    ));
    assert_eq!(
        std::fs::read(d.path().join(".git/index")).expect("owned fixture"),
        index
    );
    assert!(d.path().join(".git/MERGE_HEAD").exists());
}

#[path = "support/mutate_safety.rs"]
mod safety_cases;
