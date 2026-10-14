use super::*;

async fn request(fixture: &Fixture, task: &AgentTask) -> DiscardAgent {
    let check = fixture
        .svc
        .discard_check(&task.id)
        .await
        .expect("owned confirmation");
    let confirmation = expect(fixture, task).await;
    DiscardAgent {
        id: task.id.clone(),
        expect: ReviewExpect {
            head_oid: check.head_oid,
            base_oid: confirmation.base_oid,
            unmerged_commits: Some(check.unmerged_commits),
            discard_fingerprint: check.fingerprint,
        },
    }
}
#[tokio::test]
async fn discard_close_database_failure_preserves_git_resources_for_explicit_retry() {
    let (fixture, task, wt) = action_fixture().await;
    let request = request(&fixture, &task).await;
    fixture.svc.db.write(|tx|tx.execute_batch("CREATE TRIGGER owned_discard_failure BEFORE UPDATE ON agents_tasks WHEN NEW.state='closed' BEGIN SELECT RAISE(FAIL,'owned close failure'); END;")).await.expect("owned trigger");
    let first = fixture.svc.discard(request.clone()).await;
    assert!(matches!(first, Err(Fail::Internal(_))), "{first:?}");
    assert!(
        wt.exists(),
        "closure must persist before deleting the owned worktree"
    );
    assert_eq!(
        devbox_git::run::resolve(&fixture.repo, "agent/review")
            .await
            .expect("retained ref"),
        request.expect.head_oid
    );
    assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
    assert_ne!(
        fixture.svc.get(&task.id).await.expect("open task").state,
        AgentState::Closed
    );
    fixture
        .svc
        .db
        .write(|tx| tx.execute_batch("DROP TRIGGER owned_discard_failure"))
        .await
        .expect("remove owned failure");
    let closed = fixture
        .svc
        .discard(request.clone())
        .await
        .expect("explicit retry with intact confirmed resources");
    assert_eq!(closed.state, AgentState::Closed);
    assert_eq!(closed.closed_how, Some(crate::state::CloseHow::Discarded));
    assert!(closed.cleanup_error.is_none());
    assert_eq!(
        closed.closed_head_oid.as_deref(),
        Some(request.expect.head_oid.as_str())
    );
    assert!(!wt.exists());
    assert!(
        !devbox_git::branches(&fixture.repo)
            .await
            .expect("refs")
            .contains(&"agent/review".into())
    );
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    let frozen = fixture
        .svc
        .closed_review(&task.id)
        .await
        .expect("frozen review");
    assert_eq!(frozen.changes.head_oid, request.expect.head_oid);
    assert_eq!(Some(frozen.changes.base_oid), request.expect.base_oid);
}
#[tokio::test]
async fn discard_resource_database_failure_preserves_closed_review_and_cleanup_warning() {
    let (fixture, task, wt) = action_fixture().await;
    let request = request(&fixture, &task).await;
    fixture.svc.db.write(|tx|tx.execute_batch("CREATE TRIGGER owned_discard_failure BEFORE UPDATE ON agents_runtime WHEN json_extract(NEW.body,'$.resources')='Planned' BEGIN SELECT RAISE(FAIL,'owned resource failure'); END;")).await.expect("owned trigger");
    let closed = fixture
        .svc
        .discard(request.clone())
        .await
        .expect("completed closure survives cleanup bookkeeping failure");
    assert_eq!(closed.state, AgentState::Closed);
    assert_eq!(closed.closed_how, Some(crate::state::CloseHow::Discarded));
    assert_eq!(
        closed.closed_head_oid.as_deref(),
        Some(request.expect.head_oid.as_str())
    );
    assert!(closed.cleanup_error.is_some());
    assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
    assert!(!wt.exists());
    assert!(
        !devbox_git::branches(&fixture.repo)
            .await
            .expect("refs")
            .contains(&"agent/review".into())
    );
    let frozen = fixture
        .svc
        .closed_review(&task.id)
        .await
        .expect("original frozen review retained");
    assert_eq!(frozen.changes.head_oid, request.expect.head_oid);
    assert_eq!(Some(frozen.changes.base_oid), request.expect.base_oid);
    assert_eq!(closed.closed_commits, request.expect.unmerged_commits);
}
