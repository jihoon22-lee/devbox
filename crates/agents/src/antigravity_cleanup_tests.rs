//! The hooks folder devbox puts beside an Antigravity task's work folder goes with that folder:
//! on discard, on automatic cleanup and on the cleanup list. The list never names it. A folder
//! that holds anything devbox did not write there stays whole.
use super::*;

/// The hooks folder beside `wt`, as the task's first launch made it.
fn hooks_beside(task: &AgentTask, wt: &Path) -> PathBuf {
    let hooks = crate::antigravity::hooks_dir(wt.to_str().expect("UTF-8"));
    crate::antigravity::write_hooks_dir(&hooks, "/owned/devbox", &task.id, true)
        .expect("first launch");
    hooks
}

/// The same with a file the agent wrote there: agy's workspace holds the folder.
fn hooks_with_notes(task: &AgentTask, wt: &Path) -> PathBuf {
    let hooks = hooks_beside(task, wt);
    fs::write(hooks.join("scratch.md"), "written by the agent").expect("owned file");
    hooks
}

/// The folder and what the agent wrote there are as they were.
fn kept_whole(hooks: &Path) {
    assert_eq!(
        fs::read_to_string(hooks.join("scratch.md")).expect("agent file"),
        "written by the agent"
    );
    assert!(hooks.join(".agents/hooks.json").is_file());
}

/// A folder of a hooks folder's name that is not this task's.
fn foreign_beside(wt: &Path) -> PathBuf {
    let foreign = wt.with_file_name("other.agy-hooks");
    crate::antigravity::write_hooks_dir(&foreign, "/owned/devbox", "another-task", true)
        .expect("another task's folder");
    foreign
}

async fn merged(fixture: &Fixture, task: &AgentTask) {
    let closed = fixture
        .svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: MergeKind::Merge,
            message: None,
            expect: expect(fixture, task).await,
        })
        .await
        .expect("merged");
    assert_eq!(closed.state, AgentState::Closed);
}

#[tokio::test]
async fn discard_removes_the_tasks_hooks_folder_with_the_work_folder() {
    let (fixture, task, wt) = action_fixture().await;
    let hooks = hooks_beside(&task, &wt);
    let foreign = foreign_beside(&wt);
    // Its prompt is still queued: agy never showed its prompt box.
    fixture
        .svc
        .update(&task.id, |task| {
            crate::first_instruction::set(task, InstructionDelivery::Queued, 1);
            true
        })
        .await
        .expect("queued");
    let confirmation = expect(&fixture, &task).await;
    let check = fixture.svc.discard_check(&task.id).await.expect("check");
    let closed = fixture
        .svc
        .discard(DiscardAgent {
            id: task.id.clone(),
            expect: ReviewExpect {
                head_oid: check.head_oid.clone(),
                base_oid: confirmation.base_oid,
                unmerged_commits: Some(check.unmerged_commits),
                discard_fingerprint: check.fingerprint,
            },
        })
        .await
        .expect("discard");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(closed.cleanup_error.is_none(), "{closed:?}");
    assert!(!wt.exists());
    assert!(fs::symlink_metadata(&hooks).is_err(), "left as an orphan");
    // A discarded task's queued prompt is recorded as never sent.
    assert!(crate::first_instruction::is(
        &closed,
        InstructionDelivery::NotSent
    ));
    // Another task's folder is not this task's to remove.
    assert!(foreign.join(".agents/hooks.json").is_file());
}

#[tokio::test]
async fn automatic_cleanup_after_a_merge_keeps_a_hooks_folder_the_agent_wrote_in() {
    let (fixture, task, wt) = action_fixture().await;
    let hooks = hooks_with_notes(&task, &wt);
    merged(&fixture, &task).await;
    assert!(!wt.exists());
    kept_whole(&hooks);
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
}

#[tokio::test]
async fn the_cleanup_list_never_names_the_hooks_folder_and_cleanup_removes_it() {
    let (mut fixture, task, wt) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .cleanup_after_close = false;
    let hooks = hooks_beside(&task, &wt);
    merged(&fixture, &task).await;
    assert!(wt.exists() && hooks.is_dir(), "no automatic cleanup here");
    let listed = fixture
        .svc
        .cleanup_candidates(CleanupCandidates {
            project_id: fixture.project.clone(),
        })
        .await
        .expect("list");
    assert_eq!(listed.worktrees, [wt.to_string_lossy()]);
    assert_eq!(listed.branches, ["agent/review"]);
    fixture
        .svc
        .cleanup(CleanupAgents {
            project_id: fixture.project.clone(),
            worktrees: listed.worktrees,
            branches: listed.branches,
        })
        .await
        .expect("cleanup");
    assert!(!wt.exists());
    assert!(fs::symlink_metadata(&hooks).is_err(), "left as an orphan");
}

#[tokio::test]
async fn cleanup_of_a_branch_whose_work_folder_is_gone_keeps_a_hooks_folder_the_agent_wrote_in() {
    let (mut fixture, task, wt) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .cleanup_after_close = false;
    let hooks = hooks_with_notes(&task, &wt);
    merged(&fixture, &task).await;
    // The user removed the work folder by hand.
    devbox_git::mutate::worktree_remove(&fixture.repo, &wt, false)
        .await
        .expect("owned work folder");
    assert!(hooks.is_dir());
    fixture
        .svc
        .cleanup(CleanupAgents {
            project_id: fixture.project.clone(),
            worktrees: vec![],
            branches: vec!["agent/review".into()],
        })
        .await
        .expect("branch cleanup");
    kept_whole(&hooks);
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
}
