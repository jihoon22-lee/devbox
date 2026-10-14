use super::*;

#[test]
fn discard_preserves_a_host_commit_added_after_worktree_removal() {
    const TEST: &str = "service::resource_tests::action_review_tests::discard_cas_tests::discard_preserves_a_host_commit_added_after_worktree_removal";
    if std::env::var_os("DEVBOX_DISCARD_CAS_CHILD").is_some() {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(case());
        return;
    }
    let root = tempfile::tempdir().expect("owned Git wrapper");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("real Git path");
    assert!(real_git.status.success());
    let wrapper = r#"#!/bin/sh
previous=; repo=; target=; remove=
for argument do
 if [ "$previous:$argument" = worktree:remove ]; then remove=1; fi
 if [ "$previous" = -C ]; then repo=$argument; fi
 if [ "$previous" = -- ]; then target=$argument; fi
 previous=$argument
done
"$DEVBOX_DISCARD_REAL_GIT" "$@" || exit $?
if [ "$remove" = 1 ] && [ -f "$DEVBOX_DISCARD_CAS_PATHS" ]; then
 expected_repo=$(sed -n '1p' "$DEVBOX_DISCARD_CAS_PATHS")
 expected_target=$(sed -n '2p' "$DEVBOX_DISCARD_CAS_PATHS")
 host=$(sed -n '3p' "$DEVBOX_DISCARD_CAS_PATHS")
 expected=$(sed -n '4p' "$DEVBOX_DISCARD_CAS_PATHS")
 if [ "$repo" = "$expected_repo" ] && [ "$target" = "$expected_target" ]; then
  "$DEVBOX_DISCARD_REAL_GIT" -C "$repo" update-ref refs/heads/agent/review "$host" "$expected" || exit $?
  printf repointed > "$DEVBOX_DISCARD_CAS_MARKER"
 fi
fi
"#;
    fs::write(root.path().join("git"), wrapper).expect("owned wrapper");
    fs::set_permissions(root.path().join("git"), fs::Permissions::from_mode(0o700))
        .expect("owned executable");
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("child PATH");
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", TEST, "--nocapture"])
        .env("DEVBOX_DISCARD_CAS_CHILD", "1")
        .env(
            "DEVBOX_DISCARD_REAL_GIT",
            String::from_utf8(real_git.stdout).expect("Git path").trim(),
        )
        .env("DEVBOX_DISCARD_CAS_PATHS", root.path().join("paths"))
        .env("DEVBOX_DISCARD_CAS_MARKER", root.path().join("marker"))
        .env("PATH", path)
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
}

async fn case() {
    let (fixture, task, wt) = action_fixture().await;
    let confirmation = expect(&fixture, &task).await;
    let check = fixture
        .svc
        .discard_check(&task.id)
        .await
        .expect("discard confirmation");
    let tree = devbox_git::run::git(
        &fixture.repo,
        &["rev-parse", &format!("{}^{{tree}}", check.head_oid)],
    )
    .await
    .expect("owned tree");
    let host = devbox_git::run::git(
        &fixture.repo,
        &[
            "commit-tree",
            tree.trim(),
            "-p",
            &check.head_oid,
            "-m",
            "owned host commit",
        ],
    )
    .await
    .expect("owned host commit");
    let host = host.trim().to_owned();
    fs::write(
        std::env::var_os("DEVBOX_DISCARD_CAS_PATHS").expect("owned receipt"),
        format!(
            "{}\n{}\n{host}\n{}\n",
            fixture.repo.display(),
            wt.display(),
            check.head_oid
        ),
    )
    .expect("owned receipt");
    let closed = fixture
        .svc
        .discard(DiscardAgent {
            id: task.id.clone(),
            expect: ReviewExpect {
                head_oid: check.head_oid.clone(),
                base_oid: confirmation.base_oid.clone(),
                unmerged_commits: Some(check.unmerged_commits),
                discard_fingerprint: check.fingerprint,
            },
        })
        .await
        .expect("removal closes with a preserved ref and cleanup warning");
    assert!(!wt.exists());
    assert!(
        Path::new(&std::env::var_os("DEVBOX_DISCARD_CAS_MARKER").expect("owned marker")).is_file()
    );
    assert_eq!(
        devbox_git::run::resolve(&fixture.repo, "agent/review")
            .await
            .expect("host ref preserved"),
        host
    );
    assert_eq!(
        devbox_git::run::git(&fixture.repo, &["log", "-1", "--format=%s", &host])
            .await
            .expect("host commit preserved")
            .trim(),
        "owned host commit"
    );
    assert_eq!(closed.state, AgentState::Closed);
    assert_eq!(closed.closed_how, Some(crate::state::CloseHow::Discarded));
    assert_eq!(
        closed.closed_head_oid.as_deref(),
        Some(check.head_oid.as_str())
    );
    assert!(closed.cleanup_error.is_some());
    assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
    let frozen = fixture
        .svc
        .closed_review(&task.id)
        .await
        .expect("confirmed review preserved");
    assert_eq!(frozen.changes.head_oid, check.head_oid);
    assert_eq!(Some(frozen.changes.base_oid), confirmation.base_oid);
    assert_eq!(closed.closed_commits, Some(check.unmerged_commits));
}
