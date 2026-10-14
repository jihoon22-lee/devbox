use super::*;

#[test]
fn inline_discard_rechecks_head_and_optional_base_after_quiesce() {
    cases(&["inline-discard-late-head", "inline-discard-late-base"]);
}
#[test]
fn inline_discard_freezes_confirmed_review_after_quiesce() {
    cases(&["inline-discard"]);
}

pub(super) async fn case(mode: &str) {
    let (mut fixture, task, _) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .use_scope = true;
    git(&fixture.repo, &["checkout", "-b", "inline", "main"]);
    fs::write(fixture.repo.join("tracked"), "inline edit\n").expect("inline content");
    git(&fixture.repo, &["add", "tracked"]);
    git(&fixture.repo, &["commit", "-qm", "inline commit"]);
    let workdir = fixture.repo.to_str().expect("path").to_owned();
    fixture
        .svc
        .update(&task.id, move |task| {
            task.branch = None;
            task.worktree_path = None;
            task.workdir = workdir;
            task.state = AgentState::Running;
            true
        })
        .await
        .expect("inline task");
    fixture
        .svc
        .record_resources(&task.id, Resources::Planned)
        .await
        .expect("inline resources");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_WORKTREE_RECEIPT").expect("receipt"),
        fixture.repo.to_str().expect("path"),
    )
    .expect("owned stop target");
    let confirmed = fixture
        .svc
        .discard_check(&task.id)
        .await
        .expect("confirmation");
    let changes = fixture
        .svc
        .changes(&task.id)
        .await
        .expect("confirmed changes");
    let result = fixture
        .svc
        .discard(DiscardAgent {
            id: task.id.clone(),
            expect: ReviewExpect {
                head_oid: confirmed.head_oid.clone(),
                // The optional request field must not let base changes during stop escape.
                base_oid: if mode.ends_with("base") {
                    None
                } else {
                    Some(changes.base_oid.clone())
                },
                unmerged_commits: Some(confirmed.unmerged_commits),
                discard_fingerprint: None,
            },
        })
        .await;
    let head = devbox_git::run::resolve(&fixture.repo, "HEAD")
        .await
        .expect("current head");
    let base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("current base");
    if mode.contains("-late-") {
        assert!(
            matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
            "late Git state requires fresh confirmation: {result:?}"
        );
        assert_ne!(
            fixture.svc.get(&task.id).await.expect("task").state,
            AgentState::Closed
        );
        assert!(
            fixture
                .svc
                .db
                .read(move |tx| crate::store::closed_review(tx, &task.id))
                .await
                .expect("no frozen review")
                .is_none()
        );
        if mode.ends_with("head") {
            assert_ne!(head, confirmed.head_oid);
        } else {
            assert_eq!(head, confirmed.head_oid);
            assert_ne!(base, changes.base_oid);
        }
    } else {
        let closed = result.expect("unchanged inline discard");
        assert_eq!(closed.state, AgentState::Closed);
        assert_eq!(closed.closed_how, Some(crate::state::CloseHow::Discarded));
        assert_eq!(
            closed.closed_head_oid.as_deref(),
            Some(confirmed.head_oid.as_str())
        );
        let frozen = fixture
            .svc
            .closed_review(&task.id)
            .await
            .expect("frozen review");
        assert_eq!(frozen.changes.head_oid, head);
        assert_eq!(frozen.changes.base_oid, base);
        assert_eq!(frozen.commits.len(), confirmed.unmerged_commits as usize);
        assert!(fixture.repo.exists(), "inline checkout is preserved");
        assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    }
    let stops = fs::read_to_string(std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("marker"))
        .expect("scope stopped");
    assert_eq!(stops.lines().count(), 1);
}
