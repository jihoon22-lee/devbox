use super::*;

#[test]
fn rebase_stops_owned_scope_and_retains_review_for_success_and_conflict() {
    cases(&["rebase", "rebase-conflict"]);
}
#[test]
fn rebase_stop_failure_blocks_git_mutation() {
    cases(&["rebase-stop-fails"]);
}
#[test]
fn rebase_revalidates_committed_head_and_dirty_content_after_stop() {
    cases(&["rebase-late-head", "rebase-late-dirty"]);
}
pub(super) async fn case(mode: &str) {
    let (mut fixture, task, wt) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .use_scope = true;
    fixture
        .svc
        .update(&task.id, |task| {
            task.state = AgentState::Running;
            true
        })
        .await
        .expect("live owned task");
    let file = if mode == "rebase-conflict" {
        "tracked"
    } else {
        "host"
    };
    fs::write(fixture.repo.join(file), "host edit\n").expect("base advancement");
    git(&fixture.repo, &["add", file]);
    git(&fixture.repo, &["commit", "-qm", "host base advancement"]);
    let head = devbox_git::run::resolve(&wt, "HEAD")
        .await
        .expect("source head");
    let base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("base head");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_WORKTREE_RECEIPT").expect("receipt"),
        wt.to_str().expect("path"),
    )
    .expect("owned scope receipt");
    let result = fixture.svc.rebase(&task.id).await;
    let current = fixture.svc.get(&task.id).await.expect("task");
    assert_eq!(
        devbox_git::run::resolve(&fixture.repo, "main")
            .await
            .expect("base"),
        base
    );
    if mode.ends_with("fails") {
        assert!(result.is_err(), "stop failure blocks rebase: {result:?}");
        assert_eq!(
            devbox_git::run::resolve(&wt, "HEAD").await.expect("source"),
            head
        );
        assert_eq!(current.state, AgentState::Running);
        assert!(!current.tool_exited);
    } else {
        assert_eq!(
            current.state,
            AgentState::Review,
            "a stopped tool stays in review"
        );
        assert!(current.tool_exited);
        if mode.ends_with("head") {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
                "{result:?}"
            );
            assert_ne!(
                devbox_git::run::resolve(&wt, "HEAD")
                    .await
                    .expect("late source"),
                head
            );
        } else if mode.ends_with("dirty") {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::WorktreeDirty { .. }))),
                "{result:?}"
            );
            assert_eq!(
                devbox_git::run::resolve(&wt, "HEAD").await.expect("source"),
                head
            );
            assert_eq!(
                fs::read_to_string(wt.join("tracked")).expect("retained edit"),
                "late uncommitted edit\n"
            );
        } else if mode == "rebase-conflict" {
            assert!(matches!(
                result.expect("owned conflict").result,
                RebaseResult::Conflict
            ));
            assert!(
                fixture
                    .svc
                    .launch_binding(&task.id)
                    .await
                    .expect("binding")
                    .owned_rebase
            );
            fixture
                .svc
                .rebase_abort(&task.id)
                .await
                .expect("owned abort");
            let restored = fixture.svc.get(&task.id).await.expect("restored review");
            assert_eq!(restored.state, AgentState::Review);
            assert!(restored.tool_exited);
            assert!(devbox_git::status(&wt).await.expect("status").clean);
            assert_eq!(
                devbox_git::run::resolve(&wt, "HEAD")
                    .await
                    .expect("restored source"),
                head
            );
        } else {
            assert!(matches!(
                result.expect("owned rebase").result,
                RebaseResult::Done
            ));
            assert_ne!(
                devbox_git::run::resolve(&wt, "HEAD")
                    .await
                    .expect("rebased source"),
                head
            );
            assert_eq!(
                fs::read_to_string(wt.join("host")).expect("base edit included"),
                "host edit\n"
            );
            assert!(
                !fixture
                    .svc
                    .launch_binding(&task.id)
                    .await
                    .expect("binding")
                    .owned_rebase
            );
        }
    }
    let stops = fs::read_to_string(std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("marker"))
        .expect("owned stop called");
    assert_eq!(
        stops.lines().collect::<Vec<_>>(),
        vec![format!(
            "devbox-{}-agent-{}.scope",
            fixture.svc.paths.instance.as_str(),
            task.id
        )]
    );
}
