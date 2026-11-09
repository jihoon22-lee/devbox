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
#[test]
fn rebase_rejects_existing_dirty_without_quiescing() {
    cases(&["rebase-existing-dirty"]);
}
/// A folder that was dirty before the rebase is refused while its tool still runs: the terminal,
/// the scope, the task row, the head and the edit are all as they were.
async fn existing_dirty() {
    let (mut fixture, task, wt) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .use_scope = true;
    fs::write(
        fixture.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux configuration");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_TMUX_OWNER").expect("owner path"),
        fixture.svc.terminal.tmux().socket(),
    )
    .expect("owned socket receipt");
    let terminal = fixture
        .svc
        .terminal
        .create_internal(
            devbox_terminal::SessionKind::Agent,
            Some(fixture.project.clone()),
            wt.clone(),
            "Owned rebase fixture".into(),
            "cat".into(),
            &[],
        )
        .await
        .expect("owned agent terminal");
    let terminal_id = terminal.id.clone();
    fixture
        .svc
        .update(&task.id, move |task| {
            task.state = AgentState::Running;
            task.terminal_id = Some(terminal_id);
            true
        })
        .await
        .expect("live owned task");
    fs::write(fixture.repo.join("host"), "host edit\n").expect("base advancement");
    git(&fixture.repo, &["add", "host"]);
    git(&fixture.repo, &["commit", "-qm", "host base advancement"]);
    fs::write(wt.join("tracked"), "existing uncommitted edit\n").expect("existing edit");
    let before = fixture.svc.get(&task.id).await.expect("task");
    let head = devbox_git::run::resolve(&wt, "HEAD")
        .await
        .expect("source head");
    let result = fixture.svc.rebase(&task.id).await;
    assert!(
        matches!(&result, Err(Fail::Domain(AgentsError::WorktreeDirty { files })) if files == &["tracked"]),
        "{result:?}"
    );
    let stops = std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("marker");
    assert!(
        !Path::new(&stops).exists(),
        "a refused rebase may not stop the tool scope: {:?}",
        fs::read_to_string(&stops)
    );
    let sessions = fixture
        .svc
        .terminal
        .list(devbox_terminal::ListSessions {
            project_id: Some(fixture.project.clone()),
            after_id: None,
        })
        .await
        .expect("terminal records");
    assert!(
        sessions
            .items
            .iter()
            .any(|item| item.id == terminal.id && item.alive),
        "a refused rebase keeps the agent terminal: {:?}",
        sessions.items
    );
    let current = fixture.svc.get(&task.id).await.expect("task");
    assert_eq!(current.state, AgentState::Running);
    assert!(!current.tool_exited);
    assert_eq!(
        current, before,
        "a refused rebase leaves the task as it was"
    );
    assert!(
        !fixture
            .svc
            .launch_binding(&task.id)
            .await
            .expect("binding")
            .owned_rebase
    );
    assert_eq!(
        devbox_git::run::resolve(&wt, "HEAD").await.expect("source"),
        head
    );
    assert_eq!(
        fs::read_to_string(wt.join("tracked")).expect("retained edit"),
        "existing uncommitted edit\n"
    );
    // The fixture's own terminal: the parent kills the tmux server only if this is not reached.
    fixture
        .svc
        .terminal
        .close_internal(&terminal.id)
        .await
        .expect("close owned terminal");
}
pub(super) async fn case(mode: &str) {
    if mode == "rebase-existing-dirty" {
        existing_dirty().await;
        return;
    }
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
