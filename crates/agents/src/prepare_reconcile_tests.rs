use super::*;
use crate::tracker::Resources;

/// A worktree task whose preparation stopped after it recorded `resources`: the daemon
/// restarts and finds the task `Preparing` without a terminal.
async fn interrupted(fixture: &Fixture, slug: &str, resources: Resources) -> (AgentTask, PathBuf) {
    let wt = crate::plan::worktree_dir(&fixture.repo, slug).expect("owned path");
    let branch = format!("agent/{slug}");
    let path = wt.to_str().expect("UTF-8 fixture path");
    git(
        &fixture.repo,
        &["worktree", "add", "-b", &branch, path, "main"],
    );
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":slug,"projectId":fixture.project,"title":"Interrupted","profile":"claude",
        "baseBranch":"main","branch":branch,"worktreePath":wt,"workdir":wt,"sessionId":null,
        "terminalId":null,"state":"preparing","reason":null,"attention":null,"port":20000,
        "prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
        "createdMs":0,"updatedMs":0,"closedMs":null
    }))
    .expect("owned task");
    let row = task.clone();
    fixture
        .svc
        .db
        .write(move |tx| {
            store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.launch_id = Some("interrupted".into());
            binding.resources = resources;
            store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("interrupted preparation");
    (task, wt)
}

async fn resources(fixture: &Fixture, id: &str) -> Resources {
    let id = id.to_owned();
    fixture
        .svc
        .db
        .read(move |tx| store::binding(tx, &id))
        .await
        .expect("read binding")
        .expect("binding")
        .resources
}

#[tokio::test]
async fn reconcile_keeps_owned_resources_after_session_loss() {
    let fixture = Fixture::new().await;
    let (task, wt) = interrupted(&fixture, "owned", Resources::Owned).await;
    fixture.svc.reconcile().await.expect("restart reconcile");
    let failed = fixture.svc.get(&task.id).await.expect("task");
    assert_eq!(failed.state, AgentState::Failed);
    assert_eq!(failed.reason.as_deref(), Some("session_lost"));
    assert_eq!(resources(&fixture, &task.id).await, Resources::Owned);
    // Discarding the failed task is its cleanup: it removes the worktree and the branch.
    let check = fixture
        .svc
        .discard_check(&task.id)
        .await
        .expect("discard confirmation");
    let closed = fixture
        .svc
        .discard(DiscardAgent {
            id: task.id.clone(),
            expect: ReviewExpect {
                head_oid: check.head_oid,
                base_oid: None,
                unmerged_commits: Some(check.unmerged_commits),
                discard_fingerprint: check.fingerprint,
            },
        })
        .await
        .expect("discard owned resources");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(closed.cleanup_error.is_none(), "{closed:?}");
    assert!(!wt.exists(), "owned worktree removed");
    assert!(
        !devbox_git::branches(&fixture.repo)
            .await
            .expect("branches")
            .contains(&"agent/owned".into()),
        "owned branch removed"
    );
    assert_eq!(resources(&fixture, &task.id).await, Resources::Planned);
}

#[tokio::test]
async fn reconcile_keeps_resources_partial_unless_all_owned_ones_match() {
    let fixture = Fixture::new().await;
    // The branch ref is gone although its worktree is still there.
    let (gone, _) = interrupted(&fixture, "gone", Resources::Owned).await;
    git(
        &fixture.repo,
        &["update-ref", "-d", "refs/heads/agent/gone"],
    );
    // The path is registered, but on another branch.
    let (moved, moved_wt) = interrupted(&fixture, "moved", Resources::Owned).await;
    git(&moved_wt, &["switch", "-c", "user-branch"]);
    // Everything is there, but preparation never recorded it as owned.
    let (planned, _) = interrupted(&fixture, "planned", Resources::Planned).await;
    fixture.svc.reconcile().await.expect("restart reconcile");
    for (task, branch_exists) in [(&gone, false), (&moved, true), (&planned, true)] {
        assert_eq!(
            resources(&fixture, &task.id).await,
            Resources::Partial {
                path_exists: true,
                branch_exists: Some(branch_exists),
                worktree_registered: Some(true),
            },
            "{}",
            task.id
        );
        let failed = fixture.svc.get(&task.id).await.expect("task");
        assert_eq!(failed.state, AgentState::Failed);
        assert!(
            failed.worktree_path.is_some(),
            "partial resources stay recorded"
        );
    }
}
