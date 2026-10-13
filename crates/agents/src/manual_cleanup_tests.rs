use super::*;

#[test]
fn manual_cleanup_preserves_a_host_ref_repointed_after_validation() {
    const CHILD: &str = "DEVBOX_MANUAL_CLEANUP_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned Git wrapper");
    let wrapper = r#"#!/bin/sh
case " $* " in
 *" branch -d agent/review "*|*" update-ref --no-deref -d refs/heads/agent/review "*)
   if [ ! -f "$DEVBOX_MANUAL_CLEANUP_MARKER" ]; then
     : > "$DEVBOX_MANUAL_CLEANUP_MARKER"
     previous=; repo=
     for argument do
       if [ "$previous" = -C ]; then repo=$argument; fi
       previous=$argument
     done
     host=$(/usr/bin/git -C "$repo" rev-parse main) || exit $?
     /usr/bin/git -C "$repo" update-ref refs/heads/agent/review "$host" || exit $?
   fi;;
esac
exec /usr/bin/git "$@"
"#;
    fs::write(root.path().join("git"), wrapper).expect("wrapper");
    fs::set_permissions(root.path().join("git"), fs::Permissions::from_mode(0o700)).expect("mode");
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let mut errors = vec![];
    for mode in ["absent", "worktree"] {
        let output = Command::new(std::env::current_exe().expect("owned executable")).args(["--exact", "service::resource_tests::action_review_tests::manual_cleanup_tests::manual_cleanup_preserves_a_host_ref_repointed_after_validation", "--nocapture"]).env(CHILD, mode).env("PATH", &path).env("DEVBOX_MANUAL_CLEANUP_MARKER",root.path().join(mode)).output().expect("owned child");
        if !output.status.success() {
            errors.push(format!(
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
async fn case(mode: &str) {
    let (fixture, task, wt) = action_fixture().await;
    let confirmed = expect(&fixture, &task).await;
    let review = fixture
        .svc
        .capture_review(
            &task,
            &fixture.repo,
            &confirmed.head_oid,
            confirmed.base_oid.as_deref().expect("base"),
        )
        .await
        .expect("review");
    git(
        &fixture.repo,
        &["merge", "--no-ff", "--no-edit", "agent/review"],
    );
    git(
        &fixture.repo,
        &["commit", "--allow-empty", "-qm", "host advancement"],
    );
    let host = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("host ref");
    fixture
        .svc
        .close_record(
            &task.id,
            crate::state::CloseHow::Merged,
            None,
            confirmed.head_oid,
            fixture.svc.closure_usage(&task.id).await,
            review,
        )
        .await
        .expect("closed task");
    if mode == "absent" {
        devbox_git::mutate::worktree_remove(&fixture.repo, &wt, false)
            .await
            .expect("owned worktree already removed");
    }
    let result = fixture
        .svc
        .cleanup(CleanupAgents {
            project_id: fixture.project.clone(),
            worktrees: if mode == "worktree" {
                vec![wt.to_string_lossy().into_owned()]
            } else {
                vec![]
            },
            branches: vec!["agent/review".into()],
        })
        .await;
    assert!(
        result.is_err(),
        "changed branch must not be deleted: {result:?}"
    );
    assert_eq!(
        devbox_git::run::resolve(&fixture.repo, "agent/review")
            .await
            .expect("retained host ref"),
        host
    );
    assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
    assert!(Path::new(&std::env::var_os("DEVBOX_MANUAL_CLEANUP_MARKER").expect("marker")).exists());
}
