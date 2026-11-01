//! Ruling 60: a task closed by squash or PR whose automatic cleanup stopped is offered in the
//! cleanup list, though its branch's own commits are not on the base branch, while everything is
//! as it closed: the folder clean at the closed head, the branch at that head and, for a squash, the
//! commit the close made still on the base branch. The list and cleanup decide by the same rule, so
//! what the list offers, cleanup removes.
use super::*;
use crate::closed_review::Closure;

/// The note automatic cleanup records when it stops, recorded the same way. Its code does not
/// matter to the rule.
async fn leave_note(f: &Fixture, id: &str) {
    let error = Fail::Domain(AgentsError::TeardownFailed {
        index: 1,
        exit_code: Some(3),
    });
    f.svc.record_cleanup_error(id, &error).await;
}

/// The fixture's task closed by squash with automatic cleanup off, so its folder and branch stay;
/// with `note`, it also has the note of a cleanup that stopped.
async fn squashed(note: bool) -> (Fixture, AgentTask, PathBuf) {
    let (mut f, task, wt) = action_fixture().await;
    Arc::get_mut(&mut f.svc)
        .expect("owned service")
        .cleanup_after_close = false;
    let review = expect(&f, &task).await;
    f.svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: MergeKind::Squash,
            message: Some("squash owned change".into()),
            expect: review,
        })
        .await
        .expect("squash");
    if note {
        leave_note(&f, &task.id).await;
    }
    (f, task, wt)
}

/// Closes `task` as `closure` records it once the merge, PR or discard is done, without making one.
async fn close_as(f: &Fixture, task: &AgentTask, closure: Closure) {
    let confirmed = expect(f, task).await;
    let review = f
        .svc
        .capture_review(
            task,
            &f.repo,
            &confirmed.head_oid,
            confirmed.base_oid.as_deref().expect("base"),
        )
        .await
        .expect("review");
    f.svc
        .close_record(
            &task.id,
            closure,
            confirmed.head_oid,
            f.svc.closure_usage(&task.id).await,
            review,
        )
        .await
        .expect("closed task");
}

fn pull_request() -> Closure {
    Closure::PullRequest {
        url: "https://example.invalid/pr/1".into(),
    }
}

/// Another open task of the fixture's project, in its own folder on `agent/{slug}` with a commit of
/// its own.
async fn another_task(f: &Fixture, slug: &str, port: u16) -> (AgentTask, PathBuf) {
    let wt = crate::plan::worktree_dir(&f.repo, slug).expect("owned path");
    let branch = format!("agent/{slug}");
    git(
        &f.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            wt.to_str().expect("path"),
            "main",
        ],
    );
    fs::write(wt.join(slug), "work\n").expect("work file");
    git(&wt, &["add", slug]);
    git(&wt, &["commit", "-qm", slug]);
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":format!("{slug}-task"),"projectId":f.project,"title":slug,"profile":"claude","baseBranch":"main",
        "branch":branch,"worktreePath":wt,"workdir":wt,"sessionId":null,"terminalId":null,
        "state":"idle","reason":null,"attention":null,"port":port,"prompt":"owned prompt","followUps":[],
        "tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    }))
    .expect("owned task");
    let row = task.clone();
    f.svc
        .db
        .write(move |tx| {
            crate::store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.resources = Resources::Owned;
            crate::store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("owned task reservation");
    (task, wt)
}

async fn listed(f: &Fixture) -> CleanupOptions {
    f.svc
        .cleanup_candidates(CleanupCandidates {
            project_id: f.project.clone(),
        })
        .await
        .expect("candidates")
}

async fn nothing_listed(f: &Fixture) {
    let options = listed(f).await;
    assert!(
        options.worktrees.is_empty() && options.branches.is_empty() && options.off_base.is_empty(),
        "{options:?}"
    );
}

/// Everything the list offers, selected.
fn offered(f: &Fixture, options: &CleanupOptions) -> CleanupAgents {
    CleanupAgents {
        project_id: f.project.clone(),
        worktrees: options.worktrees.clone(),
        branches: options.branches.clone(),
    }
}

fn path_of(wt: &Path) -> String {
    wt.to_string_lossy().into_owned()
}

async fn has_branch(f: &Fixture, branch: &str) -> bool {
    devbox_git::branches(&f.repo)
        .await
        .expect("branches")
        .contains(&branch.to_owned())
}

async fn note(f: &Fixture, task: &AgentTask) -> Option<String> {
    f.svc.get(&task.id).await.expect("task").cleanup_error
}

/// Cleanup refuses `agent/review`, with its folder when one is given, and removes nothing.
async fn refused(f: &Fixture, wt: Option<&Path>) {
    let result = f
        .svc
        .cleanup(CleanupAgents {
            project_id: f.project.clone(),
            worktrees: wt.map(path_of).into_iter().collect(),
            branches: vec!["agent/review".into()],
        })
        .await;
    assert!(
        matches!(result, Err(Fail::Domain(AgentsError::UnsafeWorktree))),
        "{result:?}"
    );
    if let Some(wt) = wt {
        assert!(wt.exists());
    }
    assert!(has_branch(f, "agent/review").await);
}

/// Cleanup of the folder alone, as the list allows: the branch stays.
async fn remove_folder_only(f: &Fixture, wt: &Path) {
    f.svc
        .cleanup(CleanupAgents {
            project_id: f.project.clone(),
            worktrees: vec![path_of(wt)],
            branches: vec![],
        })
        .await
        .expect("work folder only");
    assert!(!wt.exists());
}

#[tokio::test]
async fn a_squash_closed_task_whose_cleanup_stopped_is_listed_and_removed() {
    let (f, task, wt) = squashed(true).await;
    // The close recorded the squash commit it made on the base branch.
    let squash = devbox_git::run::resolve(&f.repo, "main")
        .await
        .expect("base head");
    assert_eq!(
        f.svc.get(&task.id).await.expect("task").closed_merge_oid,
        Some(squash)
    );
    let options = listed(&f).await;
    assert_eq!(options.worktrees, [path_of(&wt)]);
    assert_eq!(options.branches, ["agent/review"]);
    assert_eq!(options.off_base, [path_of(&wt), "agent/review".into()]);
    f.svc
        .cleanup(offered(&f, &options))
        .await
        .expect("cleanup of what the list offers");
    assert!(!wt.exists());
    assert!(!has_branch(&f, "agent/review").await);
    assert_eq!(reservation(&f, &task).await, Resources::Planned);
    assert_eq!(note(&f, &task).await, None);
}

#[tokio::test]
async fn a_pr_closed_task_whose_cleanup_stopped_is_listed_and_removed() {
    let (f, task, wt) = action_fixture().await;
    close_as(&f, &task, pull_request()).await;
    // A PR close made no commit on the base branch.
    assert_eq!(
        f.svc.get(&task.id).await.expect("task").closed_merge_oid,
        None
    );
    // Without a note the user kept the branch: the strict rule.
    nothing_listed(&f).await;
    leave_note(&f, &task.id).await;
    let options = listed(&f).await;
    assert_eq!(options.worktrees, [path_of(&wt)]);
    assert_eq!(options.branches, ["agent/review"]);
    assert_eq!(options.off_base, [path_of(&wt), "agent/review".into()]);
    f.svc
        .cleanup(offered(&f, &options))
        .await
        .expect("cleanup of what the list offers");
    assert!(!wt.exists());
    assert!(!has_branch(&f, "agent/review").await);
    assert_eq!(reservation(&f, &task).await, Resources::Planned);
    assert_eq!(note(&f, &task).await, None);
}

#[tokio::test]
async fn without_a_note_a_squash_closed_task_keeps_the_strict_rule() {
    let (f, _, wt) = squashed(false).await;
    nothing_listed(&f).await;
    refused(&f, Some(&wt)).await;
}

#[tokio::test]
async fn a_task_whose_folder_moved_on_is_not_listed() {
    let (f, _, wt) = squashed(true).await;
    git(&wt, &["commit", "--allow-empty", "-qm", "later work"]);
    nothing_listed(&f).await;
    refused(&f, Some(&wt)).await;
}

#[tokio::test]
async fn a_branch_that_moved_on_after_its_folder_went_is_not_listed() {
    let (f, task, wt) = squashed(true).await;
    remove_folder_only(&f, &wt).await;
    let pin = f
        .svc
        .get(&task.id)
        .await
        .expect("task")
        .closed_head_oid
        .expect("closed head");
    // A commit on top of the closed head, made elsewhere and pointed to by the branch.
    let output = Command::new("git")
        .env("HOME", f.root.path())
        .arg("-C")
        .arg(&f.repo)
        .args([
            "commit-tree",
            "-m",
            "later work",
            "-p",
            &pin,
            &format!("{pin}^{{tree}}"),
        ])
        .output()
        .expect("git");
    assert!(output.status.success());
    let later = String::from_utf8(output.stdout)
        .expect("OID")
        .trim()
        .to_owned();
    git(
        &f.repo,
        &["update-ref", "refs/heads/agent/review", &later, &pin],
    );
    nothing_listed(&f).await;
    refused(&f, None).await;
    assert_eq!(
        devbox_git::run::resolve(&f.repo, "agent/review")
            .await
            .expect("kept branch"),
        later
    );
}

#[tokio::test]
async fn a_discarded_task_is_not_listed() {
    let (f, task, wt) = action_fixture().await;
    close_as(&f, &task, Closure::Discarded).await;
    leave_note(&f, &task.id).await;
    nothing_listed(&f).await;
    refused(&f, Some(&wt)).await;
}

#[tokio::test]
async fn after_a_cleanup_of_the_folder_alone_the_branch_is_still_listed_and_removable() {
    let (f, task, wt) = squashed(true).await;
    remove_folder_only(&f, &wt).await;
    assert!(
        note(&f, &task).await.is_some(),
        "the note stays while the branch is left"
    );
    let options = listed(&f).await;
    assert!(options.worktrees.is_empty(), "{options:?}");
    assert_eq!(options.branches, ["agent/review"]);
    assert_eq!(options.off_base, ["agent/review"]);
    f.svc
        .cleanup(offered(&f, &options))
        .await
        .expect("branch cleanup");
    assert!(!has_branch(&f, "agent/review").await);
    assert_eq!(reservation(&f, &task).await, Resources::Planned);
    assert_eq!(note(&f, &task).await, None);
}

/// The folder went, then the branch deletion failed: another Git process held the branch's ref
/// lock. The branch is still at the closed head.
#[tokio::test]
async fn the_branch_a_failed_deletion_left_is_listed_and_removable() {
    let (f, task, wt) = action_fixture().await;
    let lock = f.repo.join(".git/refs/heads/agent/review.lock");
    fs::write(&lock, "").expect("held ref lock");
    let review = expect(&f, &task).await;
    let closed = f
        .svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: MergeKind::Squash,
            message: Some("squash owned change".into()),
            expect: review,
        })
        .await
        .expect("squash");
    assert!(!wt.exists(), "the folder went first");
    assert!(closed.cleanup_error.is_some(), "{closed:?}");
    assert!(has_branch(&f, "agent/review").await);
    fs::remove_file(&lock).expect("lock released");
    let options = listed(&f).await;
    assert!(options.worktrees.is_empty(), "{options:?}");
    assert_eq!(options.branches, ["agent/review"]);
    assert_eq!(options.off_base, ["agent/review"]);
    f.svc
        .cleanup(offered(&f, &options))
        .await
        .expect("branch cleanup");
    assert!(!has_branch(&f, "agent/review").await);
    assert_eq!(reservation(&f, &task).await, Resources::Planned);
    assert_eq!(note(&f, &task).await, None);
}

#[tokio::test]
async fn every_item_the_list_offers_passes_cleanup() {
    let (f, squash, squash_wt) = squashed(true).await;
    // Closed as a PR; its folder went, but the branch deletion failed.
    let (pr, pr_wt) = another_task(&f, "pr", 20001).await;
    close_as(&f, &pr, pull_request()).await;
    devbox_git::mutate::worktree_remove(&f.repo, &pr_wt, false)
        .await
        .expect("folder gone");
    leave_note(&f, &pr.id).await;
    // Merged without a note: offered by the strict rule.
    let (merged, merged_wt) = another_task(&f, "merged", 20002).await;
    let review = expect(&f, &merged).await;
    f.svc
        .merge(MergeAgent {
            id: merged.id.clone(),
            strategy: MergeKind::Merge,
            message: None,
            expect: review,
        })
        .await
        .expect("merge");
    let options = listed(&f).await;
    let mut worktrees = vec![path_of(&squash_wt), path_of(&merged_wt)];
    worktrees.sort();
    assert_eq!(options.worktrees, worktrees);
    assert_eq!(
        options.branches,
        ["agent/merged", "agent/pr", "agent/review"]
    );
    let mut off_base = vec![
        path_of(&squash_wt),
        "agent/pr".to_owned(),
        "agent/review".to_owned(),
    ];
    off_base.sort();
    assert_eq!(options.off_base, off_base);
    f.svc
        .cleanup(offered(&f, &options))
        .await
        .expect("cleanup of every listed item");
    for path in &options.worktrees {
        assert!(!Path::new(path).exists(), "{path}");
    }
    for branch in &options.branches {
        assert!(!has_branch(&f, branch).await, "{branch}");
    }
    for task in [&squash, &pr, &merged] {
        assert_eq!(
            reservation(&f, task).await,
            Resources::Planned,
            "{}",
            task.id
        );
    }
    nothing_listed(&f).await;
}

#[tokio::test]
async fn a_base_branch_reset_past_the_squash_commit_takes_the_task_off_the_list() {
    let (f, _, wt) = squashed(true).await;
    assert_eq!(listed(&f).await.off_base.len(), 2, "offered at first");
    // The user undoes the squash on the base branch before pushing it: the branch is now the only
    // copy of the work.
    git(&f.repo, &["reset", "-q", "--keep", "HEAD~1"]);
    nothing_listed(&f).await;
    refused(&f, Some(&wt)).await;
    // Nor is the branch offered alone once the folder is gone.
    devbox_git::mutate::worktree_remove(&f.repo, &wt, false)
        .await
        .expect("folder gone");
    nothing_listed(&f).await;
    refused(&f, None).await;
}

#[tokio::test]
async fn a_task_merged_before_the_merge_commit_was_recorded_keeps_the_strict_rule() {
    let (f, task, wt) = squashed(true).await;
    // As an earlier version wrote the closed task: without the field.
    let id = task.id.clone();
    let changed = f
        .svc
        .db
        .write(move |tx| {
            tx.execute(
                "UPDATE agents_tasks SET body = json_remove(body, '$.closedMergeOid') WHERE id = ?1",
                [&id],
            )
        })
        .await
        .expect("an earlier record")
        .0;
    assert_eq!(changed, 1);
    assert_eq!(
        f.svc.get(&task.id).await.expect("task").closed_merge_oid,
        None
    );
    nothing_listed(&f).await;
    refused(&f, Some(&wt)).await;
}
