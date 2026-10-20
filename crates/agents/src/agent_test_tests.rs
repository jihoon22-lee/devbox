use super::*;
use std::time::Duration;

async fn consent(f: &Fixture) {
    fs::create_dir_all(f.repo.join(".devbox")).unwrap();
    fs::write(
        f.repo.join(".devbox/devbox.toml"),
        "[agent]\ntest='printf test-only'\n",
    )
    .unwrap();
    let preview = f
        .svc
        .projects
        .trust_preview(
            devbox_projects::TrustPreviewParams {
                id: f.project.clone(),
                operation: devbox_projects::TrustOperation::Test,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .unwrap();
    f.svc
        .projects
        .trust_approve(devbox_projects::TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.unwrap(),
            expected_scope_version: preview.scope_version,
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn agent_test_runner_receives_only_approved_test_and_holds_action_lock_until_start_finishes()
{
    let (f, task, directory) = owned().await;
    consent(&f).await;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let svc = f.svc.clone();
    let id = task.id.clone();
    let project = f.project.clone();
    let runner = tokio::spawn(async move {
        svc.test_with_runner(&id, |spec| async move {
            assert_eq!(spec.project_id, project);
            assert_eq!(spec.cwd, directory);
            assert_eq!(spec.command, "printf test-only");
            assert_eq!(
                spec.snapshot.config().agent.test.as_deref(),
                Some(spec.command.as_str())
            );
            entered_tx.send(()).unwrap();
            release_rx.await.unwrap();
            Ok(AgentTest {
                run_id: "owned-run".into(),
            })
        })
        .await
    });
    entered_rx.await.unwrap();
    assert!(
        f.svc.actions.try_lock().is_err(),
        "runner await must retain action lock"
    );
    let mut rebase = Box::pin(f.svc.rebase(&task.id));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut rebase)
            .await
            .is_err()
    );
    drop(rebase);
    let expect =
        serde_json::from_value(serde_json::json!({"headOid":"unused", "unmergedCommits":0}))
            .unwrap();
    let mut discard = Box::pin(f.svc.discard(crate::DiscardAgent {
        id: task.id.clone(),
        expect,
    }));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut discard)
            .await
            .is_err()
    );
    drop(discard);
    release_tx.send(()).unwrap();
    assert_eq!(runner.await.unwrap().unwrap().run_id, "owned-run");
    assert!(f.svc.actions.try_lock().is_ok());
    unchanged(&f, &task).await;
}
#[tokio::test]
async fn agent_test_closed_unsafe_and_unapproved_never_call_runner() {
    for mode in ["closed", "unsafe", "unapproved"] {
        let (f, mut task, _) = owned().await;
        consent(&f).await;
        match mode {
            "closed" => task.state = AgentState::Closed,
            "unsafe" => task.workdir = f.repo.to_string_lossy().into_owned(),
            _ => fs::write(
                f.repo.join(".devbox/devbox.toml"),
                "[agent]\ntest='changed'\n",
            )
            .unwrap(),
        }
        save(&f, &task).await;
        let result = f
            .svc
            .test_with_runner(&task.id, |_| async {
                panic!("rejected runner must not run")
            })
            .await;
        assert!(matches!(
            (mode, result),
            ("closed", Err(Fail::Domain(AgentsError::BadState { .. })))
                | ("unsafe", Err(Fail::Domain(AgentsError::UnsafeWorktree)))
                | (
                    "unapproved",
                    Err(Fail::Domain(AgentsError::ProjectTrustRequired { .. }))
                )
        ));
        unchanged(&f, &task).await;
    }
}
#[tokio::test]
async fn agent_test_runner_failure_releases_action_lock() {
    let (f, task, _) = owned().await;
    consent(&f).await;
    let result = f
        .svc
        .test_with_runner(&task.id, |_| async {
            Err(AgentsError::InvalidInput.into())
        })
        .await;
    assert!(matches!(
        result,
        Err(Fail::Domain(AgentsError::InvalidInput))
    ));
    assert!(f.svc.actions.try_lock().is_ok());
}
