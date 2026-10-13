use super::*;

#[tokio::test]
async fn squash_close_database_failure_recovers_after_fresh_confirmation_without_duplicate_commit()
{
    let (fixture, task, wt) = action_fixture().await;
    let initial = expect(&fixture, &task).await;
    let before = initial.base_oid.clone().expect("initial base");
    fixture.svc.db.write(|tx| tx.execute_batch("CREATE TRIGGER owned_block_closed BEFORE UPDATE ON agents_tasks WHEN NEW.state='closed' BEGIN SELECT RAISE(FAIL, 'owned close failure'); END;")).await.expect("owned close failure trigger");
    let request = |expect: ReviewExpect| MergeAgent {
        id: task.id.clone(),
        strategy: MergeKind::Squash,
        message: Some("owned retry squash".into()),
        expect,
    };
    let first = fixture.svc.merge(request(initial.clone())).await;
    assert!(
        matches!(first, Err(Fail::Internal(_))),
        "DB closure is deliberately failed: {first:?}"
    );
    let applied = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("applied squash");
    assert_ne!(applied, before, "Git completed before the DB error");
    assert_ne!(
        fixture.svc.get(&task.id).await.expect("open record").state,
        AgentState::Closed
    );
    assert!(wt.exists(), "no cleanup before closure persistence");
    fixture
        .svc
        .db
        .write(|tx| tx.execute_batch("DROP TRIGGER owned_block_closed"))
        .await
        .expect("remove owned failure");
    let stale = fixture.svc.merge(request(initial.clone())).await;
    assert!(matches!(
        stale,
        Err(Fail::Domain(AgentsError::StateChanged { .. }))
    ));
    let fresh = expect(&fixture, &task).await;
    let closed = fixture
        .svc
        .merge(request(fresh.clone()))
        .await
        .expect("fresh confirmation recovers completed squash");
    assert_eq!(closed.state, AgentState::Closed);
    assert_eq!(
        devbox_git::run::resolve(&fixture.repo, "main")
            .await
            .expect("same applied commit"),
        applied,
        "retry creates no extra commit"
    );
    let frozen = fixture
        .svc
        .closed_review(&task.id)
        .await
        .expect("last confirmed review");
    assert_eq!(frozen.changes.base_oid, fresh.base_oid.expect("fresh base"));
    assert_eq!(frozen.changes.head_oid, initial.head_oid);
    assert!(!wt.exists());
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
}
