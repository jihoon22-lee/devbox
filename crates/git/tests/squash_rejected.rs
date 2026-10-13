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
        .into()
}

#[test]
fn rejected_squash_cleans_only_the_owned_patch_when_base_and_index_are_unchanged() {
    isolated(
        "rejected_squash_cleans_only_the_owned_patch_when_base_and_index_are_unchanged",
        &["source-moved", "query-failed", "unrelated-edit"],
    );
}

#[test]
fn rejected_squash_preserves_foreign_head_index_lock_and_overlapping_worktree_edits() {
    isolated(
        "rejected_squash_preserves_foreign_head_index_lock_and_overlapping_worktree_edits",
        &[
            "foreign-head",
            "foreign-index",
            "foreign-lock",
            "overlapping-edit",
        ],
    );
}

#[test]
fn failing_git_hook_retains_its_changed_head_and_staged_index() {
    isolated(
        "failing_git_hook_retains_its_changed_head_and_staged_index",
        &["failing-hook"],
    );
}

fn isolated(test: &str, modes: &[&str]) {
    if let Ok(mode) = std::env::var("DEVBOX_REJECTED_SQUASH_CHILD") {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(run_case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned executables");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("Git path");
    assert!(real_git.status.success());
    let wrapper = root.path().join("git");
    std::fs::write(&wrapper, r#"#!/bin/sh
previous=; repo=; squash=; inspect=
for argument do
 if [ "$previous" = -C ]; then repo=$argument; fi
 if [ "$previous:$argument" = merge:--squash ]; then squash=1; fi
 if [ "$previous:$argument" = diff:--cached ]; then inspect=1; fi
 previous=$argument
done
if [ "$inspect" = 1 ] && [ -f "$DEVBOX_REJECTED_DONE" ] && [ "$DEVBOX_REJECTED_SQUASH_CHILD" = query-failed ]; then
 echo 'owned post-squash check failed' >&2
 exit 23
fi
"$DEVBOX_REJECTED_REAL_GIT" "$@" || exit $?
if [ "$squash" = 1 ] && [ -f "$DEVBOX_REJECTED_REPO" ] && [ "$repo" = "$(cat "$DEVBOX_REJECTED_REPO")" ]; then
 : > "$DEVBOX_REJECTED_DONE"
 original=$("$DEVBOX_REJECTED_REAL_GIT" -C "$repo" rev-parse main)
 case "$DEVBOX_REJECTED_SQUASH_CHILD" in
  query-failed|failing-hook) exit 0;;
  foreign-head) "$DEVBOX_REJECTED_REAL_GIT" -C "$repo" update-ref refs/heads/main refs/heads/agent/x;;
  foreign-index)
   printf host-index-data > "$repo/host-file"
   cp "$repo/.git/index" "$DEVBOX_REJECTED_FOREIGN_INDEX"
   GIT_INDEX_FILE="$DEVBOX_REJECTED_FOREIGN_INDEX" "$DEVBOX_REJECTED_REAL_GIT" -C "$repo" add host-file
   cp "$DEVBOX_REJECTED_FOREIGN_INDEX" "$repo/.git/index";;
  foreign-lock) rm "$repo/.git/index.lock"; printf foreign-owned-lock > "$repo/.git/index.lock";;
  overlapping-edit) printf host-edited-agent-file > "$repo/agent-file";;
  unrelated-edit) printf host-unrelated-data > "$repo/host-file";;
 esac
 "$DEVBOX_REJECTED_REAL_GIT" -C "$repo" update-ref refs/heads/agent/x "$original"
fi
"#).expect("owned wrapper");
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o700)).expect("owned mode");
    let mut failures = Vec::new();
    for mode in modes {
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([test, "--exact", "--nocapture"])
            .env("DEVBOX_REJECTED_SQUASH_CHILD", mode)
            .env(
                "DEVBOX_REJECTED_REAL_GIT",
                String::from_utf8(real_git.stdout.clone())
                    .expect("Git path")
                    .trim(),
            )
            .env(
                "DEVBOX_REJECTED_REPO",
                root.path().join(format!("repo-{mode}")),
            )
            .env(
                "DEVBOX_REJECTED_DONE",
                root.path().join(format!("done-{mode}")),
            )
            .env(
                "DEVBOX_REJECTED_FOREIGN_INDEX",
                root.path().join(format!("index-{mode}")),
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
        print!(
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success()
            || !String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed")
        {
            failures.push(*mode);
        }
    }
    assert!(failures.is_empty(), "failed owned cases: {failures:?}");
}

async fn run_case(mode: &str) {
    let root = tempfile::tempdir().expect("owned repo");
    let repo = root.path();
    sh(repo, &["init", "-q", "-b", "main"]);
    sh(repo, &["config", "user.name", "fixture"]);
    sh(repo, &["config", "user.email", "fixture@example.invalid"]);
    sh(repo, &["config", "commit.gpgSign", "false"]);
    std::fs::write(repo.join("base"), "base-data").expect("owned file");
    sh(repo, &["add", "base"]);
    sh(repo, &["commit", "-qm", "base"]);
    sh(repo, &["checkout", "-qb", "agent/x"]);
    std::fs::write(repo.join("agent-file"), "agent-data").expect("owned file");
    sh(repo, &["add", "agent-file"]);
    sh(repo, &["commit", "-qm", "agent"]);
    let source = sh(repo, &["rev-parse", "agent/x"]);
    sh(repo, &["checkout", "-q", "main"]);
    let original = sh(repo, &["rev-parse", "HEAD"]);
    let original_index = std::fs::read(repo.join(".git/index")).expect("owned index");
    std::fs::write(
        std::env::var_os("DEVBOX_REJECTED_REPO").expect("owned flag"),
        repo.to_str().expect("owned path"),
    )
    .expect("owned flag");
    if mode == "failing-hook" {
        let hook = repo.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\ngit update-ref refs/heads/main refs/heads/agent/x\nexit 3\n",
        )
        .expect("owned legitimate hook");
        std::fs::set_permissions(hook, std::fs::Permissions::from_mode(0o700))
            .expect("owned hook mode");
    }
    let result = git::mutate::merge_into_at(
        repo,
        "main",
        &source,
        git::mutate::MergeStrategy::Squash {
            message: "rejected squash".into(),
        },
        Some(&original),
        Some("agent/x"),
    )
    .await;
    assert!(result.is_err(), "changed validation must fail: {result:?}");
    match mode {
        "query-failed" => assert!(
            matches!(&result, Err(git::GitError::Failed { op, stderr }) if op == "inspect squash index" && stderr.contains("owned post-squash check failed")),
            "preserve the query failure: {result:?}"
        ),
        "overlapping-edit" => assert!(
            matches!(&result, Err(git::GitError::Failed { op, .. }) if op == "reset"),
            "overlapping edits must make safe rollback refuse: {result:?}"
        ),
        _ => assert!(
            matches!(&result, Err(git::GitError::StateChanged)),
            "changed confirmation/ownership must be explicit: {result:?}"
        ),
    }

    assert_eq!(
        sh(repo, &["rev-parse", "agent/x"]),
        if matches!(mode, "query-failed" | "failing-hook") {
            source.as_str()
        } else {
            original.as_str()
        },
        "do not rewind the externally changed source ref"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("base")).expect("preserved baseline"),
        "base-data"
    );
    assert_eq!(
        sh(repo, &["rev-parse", "HEAD"]),
        if matches!(mode, "foreign-head" | "failing-hook") {
            source.as_str()
        } else {
            original.as_str()
        }
    );
    if mode == "failing-hook" {
        assert_eq!(
            sh(repo, &["show", ":agent-file"]),
            "agent-data",
            "retain the legitimate failing hook's staged result"
        );
        assert!(
            git::status(repo)
                .await
                .expect("hook-owned head/index consistent")
                .clean
        );
    } else if mode == "foreign-index" {
        assert_eq!(sh(repo, &["show", ":host-file"]), "host-index-data");
        assert_eq!(
            std::fs::read(repo.join(".git/index")).expect("preserved foreign index"),
            std::fs::read(
                std::env::var_os("DEVBOX_REJECTED_FOREIGN_INDEX").expect("owned index snapshot")
            )
            .expect("owned snapshot")
        );
    } else {
        assert_eq!(
            std::fs::read(repo.join(".git/index")).expect("shared index"),
            original_index,
            "a rejected private squash must not replace the shared index"
        );
    }
    match mode {
        "source-moved" | "query-failed" | "unrelated-edit" => {
            assert!(
                !repo.join("agent-file").exists(),
                "rejected owned patch must be removed when original base is still current"
            );
            if mode == "unrelated-edit" {
                assert_eq!(
                    std::fs::read_to_string(repo.join("host-file"))
                        .expect("preserved unrelated edit"),
                    "host-unrelated-data"
                );
            } else {
                assert!(git::status(repo).await.expect("restored status").clean);
            }
        }
        "overlapping-edit" => assert_eq!(
            std::fs::read_to_string(repo.join("agent-file")).expect("preserved overlapping edit"),
            "host-edited-agent-file"
        ),
        _ => assert_eq!(
            std::fs::read_to_string(repo.join("agent-file"))
                .expect("preserved worktree when ownership is uncertain"),
            "agent-data"
        ),
    }
    if mode == "foreign-lock" {
        assert_eq!(
            std::fs::read_to_string(repo.join(".git/index.lock")).expect("preserved foreign lock"),
            "foreign-owned-lock"
        );
    } else {
        assert!(!repo.join(".git/index.lock").exists());
    }
}
