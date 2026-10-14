#![cfg(unix)]

use devbox_git as git;
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

fn sh(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("owned Git");
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("owned output")
        .trim()
        .to_owned()
}

#[tokio::test]
async fn squash_cannot_commit_concurrent_host_staging() {
    const CHILD: &str = "DEVBOX_SQUASH_INDEX_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let root = tempfile::tempdir().expect("owned wrapper");
        let real_git = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("Git path");
        assert!(real_git.status.success());
        let wrapper = root.path().join("git");
        std::fs::write(&wrapper, r#"#!/bin/sh
previous=; repo=; squash=
for argument do
 if [ "$previous" = -C ]; then repo=$argument; fi
 if [ "$previous:$argument" = merge:--squash ]; then squash=1; fi
 previous=$argument
done
"$DEVBOX_SQUASH_REAL_GIT" "$@" || exit $?
if [ "$squash" = 1 ] && [ -f "$DEVBOX_SQUASH_REPO" ] && [ "$repo" = "$(cat "$DEVBOX_SQUASH_REPO")" ]; then
 printf host-owned-data > "$repo/host-staged"
 # This is a separate host Git operation, not a nested helper using our index.
 env -u GIT_INDEX_FILE "$DEVBOX_SQUASH_REAL_GIT" -C "$repo" add host-staged
 printf %s "$?" > "$DEVBOX_SQUASH_STAGE_RESULT"
fi
"#).expect("owned wrapper");
        std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o700))
            .expect("owned wrapper");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "squash_cannot_commit_concurrent_host_staging",
                "--exact",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env(
                "DEVBOX_SQUASH_REAL_GIT",
                String::from_utf8(real_git.stdout).expect("Git path").trim(),
            )
            .env("DEVBOX_SQUASH_REPO", root.path().join("repo"))
            .env(
                "DEVBOX_SQUASH_STAGE_RESULT",
                root.path().join("stage-result"),
            )
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.path().display(),
                    std::env::var("PATH").expect("PATH")
                ),
            )
            .output()
            .expect("owned child");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return;
    }
    let root = tempfile::tempdir().expect("owned repo");
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).expect("owned repo");
    sh(&repo, &["init", "-q", "-b", "main"]);
    sh(&repo, &["config", "user.name", "fixture"]);
    sh(&repo, &["config", "user.email", "fixture@example.invalid"]);
    sh(&repo, &["config", "commit.gpgSign", "false"]);
    std::fs::write(repo.join("base"), "base").expect("owned file");
    sh(&repo, &["add", "base"]);
    sh(&repo, &["commit", "-qm", "base"]);
    sh(&repo, &["checkout", "-qb", "agent/x"]);
    std::fs::write(repo.join("agent-file"), "agent-data").expect("owned file");
    sh(&repo, &["add", "agent-file"]);
    sh(&repo, &["commit", "-qm", "agent"]);
    sh(&repo, &["checkout", "-q", "main"]);
    std::fs::write(
        std::env::var_os("DEVBOX_SQUASH_REPO").expect("owned flag"),
        repo.to_str().expect("owned repo path"),
    )
    .expect("owned flag");
    let result = git::mutate::merge_into(
        &repo,
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "confirmed agent squash".into(),
        },
    )
    .await;
    assert!(
        result.is_ok() || matches!(result, Err(git::GitError::StateChanged)),
        "{result:?}"
    );
    let committed = sh(&repo, &["ls-tree", "--name-only", "HEAD"]);
    assert!(
        !committed.lines().any(|path| path == "host-staged"),
        "confirmed squash silently acquired host staging: {committed}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("host-staged")).expect("preserved host file"),
        "host-owned-data"
    );
    let attempt = std::fs::read_to_string(
        std::env::var_os("DEVBOX_SQUASH_STAGE_RESULT").expect("owned result"),
    )
    .expect("owned result");
    if attempt == "0" {
        assert!(
            matches!(result, Err(git::GitError::StateChanged)),
            "admitted host staging must remain intact when mutation is rejected"
        );
        assert_eq!(sh(&repo, &["show", ":host-staged"]), "host-owned-data");
    } else {
        assert!(
            result.is_ok(),
            "Git index lock prevented the concurrent staging operation"
        );
        assert_eq!(sh(&repo, &["show", "HEAD:agent-file"]), "agent-data");
    }
}

fn fixture() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("owned repo");
    sh(repo.path(), &["init", "-q", "-b", "main"]);
    sh(repo.path(), &["config", "user.name", "fixture"]);
    sh(
        repo.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    sh(repo.path(), &["config", "commit.gpgSign", "false"]);
    std::fs::write(repo.path().join("base"), "base").expect("owned file");
    sh(repo.path(), &["add", "base"]);
    sh(repo.path(), &["commit", "-qm", "base"]);
    sh(repo.path(), &["checkout", "-qb", "agent/x"]);
    std::fs::write(repo.path().join("agent-file"), "agent-data").expect("owned file");
    sh(repo.path(), &["add", "agent-file"]);
    sh(repo.path(), &["commit", "-qm", "agent"]);
    sh(repo.path(), &["checkout", "-q", "main"]);
    repo
}

#[tokio::test]
async fn fresh_confirmed_retry_of_applied_squash_returns_existing_base_without_commit() {
    let repo = fixture();
    let source = sh(repo.path(), &["rev-parse", "agent/x"]);
    let original = sh(repo.path(), &["rev-parse", "main"]);
    let first = git::mutate::merge_into_at(
        repo.path(),
        "main",
        &source,
        git::mutate::MergeStrategy::Squash {
            message: "first squash".into(),
        },
        Some(&original),
        Some("agent/x"),
    )
    .await
    .expect("first squash");
    let count = sh(repo.path(), &["rev-list", "--count", "main"]);
    let retry = git::mutate::merge_into_at(
        repo.path(),
        "main",
        &source,
        git::mutate::MergeStrategy::Squash {
            message: "fresh confirmed retry".into(),
        },
        Some(&first.sha),
        Some("agent/x"),
    )
    .await
    .expect("fresh confirmed already-applied squash succeeds");
    assert_eq!(retry.sha, first.sha);
    assert_eq!(retry.source_sha, source);
    assert_eq!(sh(repo.path(), &["rev-list", "--count", "main"]), count);
    assert!(git::status(repo.path()).await.expect("owned status").clean);
    assert!(!repo.path().join(".git/index.lock").exists());
}

#[tokio::test]
async fn preexisting_foreign_index_lock_is_preserved_without_mutation() {
    let repo = fixture();
    let head = sh(repo.path(), &["rev-parse", "HEAD"]);
    let original = std::fs::read(repo.path().join(".git/index")).expect("owned index");
    std::fs::write(repo.path().join(".git/index.lock"), "foreign-owned-lock")
        .expect("owned fixture simulates foreign lock");
    let result = git::mutate::merge_into(
        repo.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "must not mutate".into(),
        },
    )
    .await;
    assert!(
        matches!(result, Err(git::GitError::StateChanged)),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join(".git/index.lock"))
            .expect("preserved foreign lock"),
        "foreign-owned-lock"
    );
    assert_eq!(
        std::fs::read(repo.path().join(".git/index")).expect("preserved index"),
        original
    );
    assert_eq!(sh(repo.path(), &["rev-parse", "HEAD"]), head);
    assert!(!repo.path().join("agent-file").exists());
}

#[tokio::test]
async fn hook_replacement_of_index_lock_is_never_removed_or_published() {
    let repo = fixture();
    let original = std::fs::read(repo.path().join(".git/index")).expect("owned index");
    let hook = repo.path().join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\nrm .git/index.lock\nprintf foreign-replacement > .git/index.lock\nexit 1\n",
    )
    .expect("owned hook");
    std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700))
        .expect("owned hook mode");
    let head = sh(repo.path(), &["rev-parse", "HEAD"]);
    let result = git::mutate::merge_into(
        repo.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "rejected squash".into(),
        },
    )
    .await;
    assert!(
        matches!(result, Err(git::GitError::StateChanged)),
        "{result:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join(".git/index.lock"))
            .expect("preserved replacement"),
        "foreign-replacement"
    );
    assert_eq!(
        std::fs::read(repo.path().join(".git/index")).expect("preserved shared index"),
        original
    );
    assert_eq!(sh(repo.path(), &["rev-parse", "HEAD"]), head);
}

#[tokio::test]
async fn squash_conflict_rolls_back_private_index_and_worktree() {
    let repo = fixture();
    sh(repo.path(), &["checkout", "-q", "agent/x"]);
    std::fs::write(repo.path().join("base"), "agent-base").expect("owned file");
    sh(repo.path(), &["add", "base"]);
    sh(repo.path(), &["commit", "-qm", "agent conflict"]);
    sh(repo.path(), &["checkout", "-q", "main"]);
    std::fs::write(repo.path().join("base"), "host-base").expect("owned file");
    sh(repo.path(), &["add", "base"]);
    sh(repo.path(), &["commit", "-qm", "host conflict"]);
    let head = sh(repo.path(), &["rev-parse", "HEAD"]);
    let result = git::mutate::merge_into(
        repo.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "conflicting squash".into(),
        },
    )
    .await;
    assert!(
        matches!(result, Err(git::GitError::Conflict { ref files }) if files == &["base"]),
        "{result:?}"
    );
    assert_eq!(sh(repo.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("base")).expect("preserved host baseline"),
        "host-base"
    );
    assert!(git::status(repo.path()).await.expect("owned status").clean);
    assert!(!repo.path().join("agent-file").exists());
    assert!(!repo.path().join(".git/index.lock").exists());
}

#[tokio::test]
async fn squash_preserves_split_index_configuration_and_publishes_a_clean_index() {
    let repo = fixture();
    sh(repo.path(), &["config", "core.splitIndex", "true"]);
    sh(repo.path(), &["update-index", "--split-index"]);
    let outcome = git::mutate::merge_into(
        repo.path(),
        "main",
        "agent/x",
        git::mutate::MergeStrategy::Squash {
            message: "split-index squash".into(),
        },
    )
    .await
    .expect("split index remains supported");
    assert_eq!(
        sh(
            repo.path(),
            &["show", &format!("{}:agent-file", outcome.sha)]
        ),
        "agent-data"
    );
    assert!(git::status(repo.path()).await.expect("owned status").clean);
    assert_eq!(
        sh(repo.path(), &["config", "--get", "core.splitIndex"]),
        "true"
    );
    assert!(!repo.path().join(".git/index.lock").exists());
}
