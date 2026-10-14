#![cfg(unix)]

use devbox_git as git;
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command};

fn sh(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("owned Git");
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("Git output")
        .trim()
        .into()
}

#[test]
fn refused_normal_merge_preserves_edit_staged_after_the_last_baseline_check() {
    if std::env::var_os("DEVBOX_NORMAL_MERGE_CHILD").is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(case());
        return;
    }
    let root = tempfile::tempdir().expect("owned wrapper");
    let real_git = which::which("git").expect("Git path");
    let wrapper = root.path().join("git");
    std::fs::write(
        &wrapper,
        r#"#!/bin/sh
previous=; repo=; merging=; resetting=
for argument do
 if [ "$previous" = -C ]; then repo=$argument; fi
 if [ "$previous:$argument" = merge:--no-ff ]; then merging=1; fi
 if [ "$argument" = reset ]; then resetting=1; fi
 previous=$argument
done
if [ -f "$DEVBOX_NORMAL_MERGE_REPO" ] && [ "$repo" = "$(cat "$DEVBOX_NORMAL_MERGE_REPO")" ]; then
 if [ "$merging" = 1 ]; then
  printf 'host staged edit\n' > "$repo/host-file"
  "$DEVBOX_NORMAL_MERGE_REAL_GIT" -C "$repo" add host-file || exit $?
  : > "$DEVBOX_NORMAL_MERGE_STAGED"
 fi
 if [ "$resetting" = 1 ]; then : > "$DEVBOX_NORMAL_MERGE_RESET"; fi
fi
exec "$DEVBOX_NORMAL_MERGE_REAL_GIT" "$@"
"#,
    )
    .expect("owned Git wrapper");
    std::fs::set_permissions(wrapper, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let out = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "refused_normal_merge_preserves_edit_staged_after_the_last_baseline_check",
            "--nocapture",
        ])
        .env("DEVBOX_NORMAL_MERGE_CHILD", "1")
        .env("DEVBOX_NORMAL_MERGE_REAL_GIT", real_git)
        .env("DEVBOX_NORMAL_MERGE_REPO", root.path().join("repo"))
        .env("DEVBOX_NORMAL_MERGE_STAGED", root.path().join("staged"))
        .env("DEVBOX_NORMAL_MERGE_RESET", root.path().join("reset"))
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
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed; 0 failed"));
}

async fn case() {
    let root = tempfile::tempdir().expect("owned repository");
    let repo = root.path();
    sh(repo, &["init", "-q", "-b", "main"]);
    sh(repo, &["config", "user.name", "fixture"]);
    sh(repo, &["config", "user.email", "fixture@example.invalid"]);
    sh(repo, &["config", "commit.gpgSign", "false"]);
    sh(
        repo,
        &[
            "config",
            "core.hooksPath",
            repo.join(".git/hooks").to_str().expect("hooks"),
        ],
    );
    std::fs::write(repo.join("host-file"), "original host content\n").expect("base file");
    sh(repo, &["add", "host-file"]);
    sh(repo, &["commit", "-qm", "base"]);
    sh(repo, &["checkout", "-qb", "agent/x"]);
    std::fs::write(repo.join("agent-file"), "agent content\n").expect("agent file");
    sh(repo, &["add", "agent-file"]);
    sh(repo, &["commit", "-qm", "agent"]);
    let source = sh(repo, &["rev-parse", "HEAD"]);
    sh(repo, &["checkout", "-q", "main"]);
    let original = sh(repo, &["rev-parse", "HEAD"]);
    std::fs::write(
        std::env::var_os("DEVBOX_NORMAL_MERGE_REPO").expect("owned receipt"),
        repo.to_str().expect("repo path"),
    )
    .expect("arm staging fixture");
    let result = git::mutate::merge_into_at(
        repo,
        "main",
        &source,
        git::mutate::MergeStrategy::Merge,
        Some(&original),
        Some("agent/x"),
    )
    .await;
    assert!(
        matches!(result, Err(git::GitError::Failed { ref op, .. }) if op == "merge"),
        "Git refusal must remain visible: {result:?}"
    );
    assert!(
        Path::new(&std::env::var_os("DEVBOX_NORMAL_MERGE_STAGED").expect("receipt")).exists(),
        "the staged edit arrived at merge invocation after the baseline checks"
    );
    assert!(
        !repo.join(".git/MERGE_HEAD").exists(),
        "Git refused before creating merge state"
    );
    assert_eq!(sh(repo, &["rev-parse", "HEAD"]), original);
    assert_eq!(sh(repo, &["rev-parse", "agent/x"]), source);
    assert_eq!(
        std::fs::read_to_string(repo.join("host-file")).expect("host data"),
        "host staged edit\n",
        "failure cleanup must preserve external working-tree data"
    );
    assert_eq!(
        sh(repo, &["show", ":host-file"]),
        "host staged edit",
        "failure cleanup must preserve external staging"
    );
    assert!(
        !Path::new(&std::env::var_os("DEVBOX_NORMAL_MERGE_RESET").expect("receipt")).exists(),
        "no reset without evidence of the owned merge"
    );
}
