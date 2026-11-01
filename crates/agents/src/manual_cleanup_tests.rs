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
    let note = leave_note(&fixture, &task.id).await;
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
    // The cleanup failed (after removing the folder, in one mode): what it left stays on record.
    assert_eq!(cleanup_note(&fixture, &task.id).await, Some(note));
    assert!(Path::new(&std::env::var_os("DEVBOX_MANUAL_CLEANUP_MARKER").expect("marker")).exists());
}

/// The note automatic cleanup records when it stops on files left in the work folder.
async fn leave_note(fixture: &Fixture, id: &str) -> String {
    let error = Fail::Domain(AgentsError::WorktreeDirty {
        files: vec!["teardown.log".into()],
    });
    fixture.svc.record_cleanup_error(id, &error).await;
    cleanup_note(fixture, id).await.expect("recorded note")
}

async fn cleanup_note(fixture: &Fixture, id: &str) -> Option<String> {
    fixture.svc.get(id).await.expect("task").cleanup_error
}

/// The fixture's task, closed by a plain merge (so the cleanup list offers it) with its work
/// folder and branch still in place and a note of what cleanup left.
async fn merged_with_note() -> (Fixture, AgentTask, PathBuf, String) {
    let (mut fixture, task, wt) = action_fixture().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .cleanup_after_close = false;
    let review = expect(&fixture, &task).await;
    fixture
        .svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: MergeKind::Merge,
            message: None,
            expect: review,
        })
        .await
        .expect("merged");
    assert!(wt.exists(), "no automatic cleanup in this fixture");
    let note = leave_note(&fixture, &task.id).await;
    (fixture, task, wt, note)
}

fn selection(fixture: &Fixture, wt: Option<&Path>, branch: Option<&str>) -> CleanupAgents {
    CleanupAgents {
        project_id: fixture.project.clone(),
        worktrees: wt
            .map(|p| p.to_string_lossy().into_owned())
            .into_iter()
            .collect(),
        branches: branch.map(str::to_owned).into_iter().collect(),
    }
}

#[tokio::test]
async fn manual_cleanup_clears_the_note_of_the_task_whose_resources_it_removed() {
    let (fixture, task, wt, _) = merged_with_note().await;
    fixture
        .svc
        .cleanup(selection(&fixture, Some(&wt), Some("agent/review")))
        .await
        .expect("cleanup");
    assert!(!wt.exists());
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    assert_eq!(cleanup_note(&fixture, &task.id).await, None);
}

#[tokio::test]
async fn manual_cleanup_keeps_the_note_until_the_last_resource_is_gone() {
    let (fixture, task, wt, note) = merged_with_note().await;
    // Only the work folder goes: the branch is still the task's, so something is still left.
    fixture
        .svc
        .cleanup(selection(&fixture, Some(&wt), None))
        .await
        .expect("work folder cleanup");
    assert!(!wt.exists());
    assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
    assert_eq!(cleanup_note(&fixture, &task.id).await, Some(note));
    // The branch alone is then offered, and removing it ends the task's leftovers.
    fixture
        .svc
        .cleanup(selection(&fixture, None, Some("agent/review")))
        .await
        .expect("branch cleanup");
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    assert_eq!(cleanup_note(&fixture, &task.id).await, None);
}

#[tokio::test]
async fn manual_cleanup_never_touches_the_note_of_another_task() {
    let (fixture, task, wt, _) = merged_with_note().await;
    // A second closed task of the same project, as clean and as removable, with a note of its own.
    let other_wt = crate::plan::worktree_dir(&fixture.repo, "other").expect("owned path");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "agent/other",
            other_wt.to_str().expect("path"),
            "main",
        ],
    );
    fs::write(other_wt.join("other"), "other\n").expect("other file");
    git(&other_wt, &["add", "other"]);
    git(&other_wt, &["commit", "-m", "other"]);
    git(
        &fixture.repo,
        &["merge", "--no-ff", "--no-edit", "agent/other"],
    );
    let head = devbox_git::run::resolve(&other_wt, "HEAD")
        .await
        .expect("other head");
    let other: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"other-task","projectId":fixture.project,"title":"Other","profile":"claude","baseBranch":"main",
        "branch":"agent/other","worktreePath":other_wt,"workdir":other_wt,"sessionId":null,"terminalId":null,
        "state":"closed","reason":null,"attention":null,"port":20001,"prompt":"","followUps":[],
        "tokens":null,"prUrl":null,"closedHow":"merged","createdMs":0,"updatedMs":0,"closedMs":1,
        "closedHeadOid":head
    }))
    .expect("other task");
    let row = other.clone();
    fixture
        .svc
        .db
        .write(move |tx| {
            crate::store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.resources = Resources::Owned;
            crate::store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("other task reservation");
    leave_note(&fixture, &other.id).await;
    let before = fixture.svc.get(&other.id).await.expect("other task");
    let candidates = fixture
        .svc
        .cleanup_candidates(CleanupCandidates {
            project_id: fixture.project.clone(),
        })
        .await
        .expect("candidates");
    assert!(
        candidates
            .worktrees
            .contains(&other_wt.to_string_lossy().into_owned())
    );
    fixture
        .svc
        .cleanup(selection(&fixture, Some(&wt), Some("agent/review")))
        .await
        .expect("cleanup of the first task only");
    assert_eq!(cleanup_note(&fixture, &task.id).await, None);
    // The other task keeps its folder, its branch, its reservation and its note, untouched.
    assert!(other_wt.exists());
    assert_eq!(reservation(&fixture, &other).await, Resources::Owned);
    assert_eq!(
        fixture.svc.get(&other.id).await.expect("other task"),
        before
    );
}
