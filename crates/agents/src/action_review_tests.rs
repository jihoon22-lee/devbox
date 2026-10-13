use super::*;
use crate::tracker::{Binding, Resources};
use devbox_protocol::Fail;

#[path = "close_review_tests.rs"]
mod close_review_tests;

async fn action_fixture() -> (Fixture, AgentTask, PathBuf) {
    let mut fixture = Fixture::new().await;
    Arc::get_mut(&mut fixture.svc)
        .expect("owned service")
        .cleanup_after_close = true;
    fs::remove_file(fixture.repo.join(".git/hooks/post-checkout")).expect("owned hook");
    fs::write(fixture.repo.join("tracked"), "base\n").expect("tracked file");
    git(&fixture.repo, &["add", "tracked"]);
    git(&fixture.repo, &["commit", "-m", "base"]);
    let wt = crate::plan::worktree_dir(&fixture.repo, "review").expect("owned path");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "agent/review",
            wt.to_str().expect("path"),
            "main",
        ],
    );
    fs::write(wt.join("tracked"), "agent\n").expect("agent file");
    git(&wt, &["add", "tracked"]);
    git(&wt, &["commit", "-m", "agent"]);
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"review-task","projectId":fixture.project,"title":"Review","profile":"claude","baseBranch":"main",
        "branch":"agent/review","worktreePath":wt,"workdir":wt,"sessionId":null,"terminalId":null,
        "state":"idle","reason":null,"attention":null,"port":20000,"prompt":"owned prompt","followUps":[],
        "tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    })).expect("owned task");
    let row = task.clone();
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
        .expect("owned task reservation");
    (fixture, task, wt)
}

async fn expect(fixture: &Fixture, task: &AgentTask) -> ReviewExpect {
    let changes = fixture.svc.changes(&task.id).await.expect("review");
    ReviewExpect {
        head_oid: changes.head_oid,
        base_oid: Some(changes.base_oid),
        unmerged_commits: None,
        discard_fingerprint: None,
    }
}

async fn reservation(fixture: &Fixture, task: &AgentTask) -> Resources {
    let id = task.id.clone();
    fixture
        .svc
        .db
        .read(move |tx| crate::store::binding(tx, &id))
        .await
        .expect("read binding")
        .expect("binding")
        .resources
}

#[tokio::test]
async fn review_final_squash_cleanup_deletes_confirmed_branch_and_releases_project() {
    let (fixture, task, wt) = action_fixture().await;
    let review = expect(&fixture, &task).await;
    let closed = fixture
        .svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy: MergeKind::Squash,
            message: Some("squash owned change".into()),
            expect: review,
        })
        .await
        .expect("squash");
    assert_eq!(closed.state, AgentState::Closed);
    assert!(closed.cleanup_error.is_none(), "{closed:?}");
    assert!(!wt.exists());
    assert!(
        !devbox_git::branches(&fixture.repo)
            .await
            .expect("branches")
            .contains(&"agent/review".into()),
        "automatic cleanup must remove the squash source branch"
    );
    assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    fixture
        .svc
        .projects
        .remove(devbox_projects::RemoveProject {
            id: fixture.project.clone(),
        })
        .await
        .expect("released project has no resource dependents");
}

async fn child_pr(mode: &str) {
    let (fixture, task, wt) = action_fixture().await;
    let remote = fixture.root.path().join("remote.git");
    git(
        &fixture.repo,
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    git(
        &fixture.repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    if mode != "clean" {
        let name = if mode == "untracked" {
            "new-untracked"
        } else {
            "tracked"
        };
        fs::write(wt.join(name), "unreviewed edit\n").expect("dirty edit");
        if mode == "staged" {
            git(&wt, &["add", "tracked"]);
        }
    }
    let review = expect(&fixture, &task).await;
    let result = fixture
        .svc
        .pr(PrAgent {
            id: task.id.clone(),
            title: None,
            body: None,
            expect: review,
        })
        .await;
    if mode == "clean" {
        let closed = result.expect("owned PR");
        assert_eq!(closed.state, AgentState::Closed);
        assert!(!wt.exists());
        assert!(
            !devbox_git::branches(&fixture.repo)
                .await
                .expect("branches")
                .contains(&"agent/review".into())
        );
        assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
    } else {
        assert!(
            matches!(result, Err(Fail::Domain(AgentsError::WorktreeDirty { .. }))),
            "{mode}: {result:?}"
        );
        assert_eq!(
            fixture.svc.get(&task.id).await.expect("task").state,
            AgentState::Idle
        );
        assert!(wt.exists());
        assert!(
            devbox_git::branches(&remote)
                .await
                .expect("remote branches")
                .is_empty(),
            "dirty PR must not push"
        );
        assert!(
            !Path::new(&std::env::var_os("DEVBOX_REVIEW_GH_MARKER").expect("marker")).exists(),
            "dirty PR must not run gh"
        );
    }
}

#[test]
fn review_final_pr_requires_clean_tracked_index_and_untracked_state_before_push() {
    isolated_cases("pr", &["unstaged", "staged", "untracked", "clean"]);
}

async fn child_rebase(mode: &str) {
    let (fixture, task, wt) = action_fixture().await;
    fs::write(fixture.repo.join("tracked"), "base conflict\n").expect("base edit");
    git(&fixture.repo, &["add", "tracked"]);
    git(&fixture.repo, &["commit", "-m", "base conflict"]);
    assert!(
        fixture.svc.rebase(&task.id).await.is_err(),
        "status inspection is deliberately failed after the owned rebase starts"
    );
    let id = task.id.clone();
    let binding = fixture
        .svc
        .db
        .read(move |tx| crate::store::binding(tx, &id))
        .await
        .expect("binding")
        .expect("row");
    if mode == "no-markers" {
        assert!(
            !binding.owned_rebase,
            "a proven absent operation releases ownership"
        );
        return;
    }
    assert!(
        binding.owned_rebase,
        "{mode}: failed status/marker queries may not revoke the active operation's abort ownership"
    );
    fs::remove_file(std::env::var_os("DEVBOX_REVIEW_REBASE_FLAG").expect("flag"))
        .expect("restore inspection");
    fixture
        .svc
        .rebase_abort(&task.id)
        .await
        .expect("owned abort remains available");
    assert!(devbox_git::status(&wt).await.expect("status").clean);
}

async fn child_cleanup(mode: &str) {
    let (fixture, task, wt) = action_fixture().await;
    if mode == "recreated" {
        git(
            &fixture.repo,
            &[
                "config",
                "--local",
                "branch.agent/review.remote",
                "owned-tracking",
            ],
        );
    }
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
    fixture
        .svc
        .close_record(
            &task.id,
            crate::state::CloseHow::PullRequest,
            Some("https://example.invalid/pr/1".into()),
            confirmed.head_oid.clone(),
            fixture.svc.closure_usage(&task.id).await,
            review,
        )
        .await
        .expect("closed task");
    fixture
        .svc
        .quiesce(&task)
        .await
        .expect("owned resources stopped");
    fixture
        .svc
        .cleanup_closed_after_quiesce(&task.id, &None)
        .await;
    assert!(!wt.exists(), "owned clean worktree was removed");
    let closed = fixture.svc.get(&task.id).await.expect("closed task");
    if mode == "gone" {
        assert!(
            closed.cleanup_error.is_none(),
            "already removed owned branch is complete: {closed:?}"
        );
        assert_eq!(reservation(&fixture, &task).await, Resources::Planned);
        assert!(
            !devbox_git::branches(&fixture.repo)
                .await
                .expect("branches")
                .contains(&"agent/review".into())
        );
    } else {
        assert!(
            closed.cleanup_error.is_some(),
            "changed branch must leave an explicit retained-resource error"
        );
        assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
        assert_ne!(
            devbox_git::run::resolve(&fixture.repo, "agent/review")
                .await
                .expect("retained branch"),
            confirmed.head_oid
        );
        if mode == "recreated" {
            let (_, remote, _) = devbox_git::run::output(
                &fixture.repo,
                &["config", "--local", "--get", "branch.agent/review.remote"],
            )
            .await
            .expect("tracking metadata");
            assert_eq!(
                remote.trim(),
                "owned-tracking",
                "preserve metadata of the re-created branch"
            );
        }
    }
}

#[test]
fn review_final_auto_cleanup_releases_absent_branch_and_preserves_changed_branch() {
    isolated_cases("cleanup", &["gone", "moved", "recreated"]);
}

#[test]
fn review_final_rebase_inspection_failure_keeps_owned_abort_available() {
    isolated_cases("rebase", &["status", "markers", "no-markers"]);
}

fn isolated_cases(kind: &str, modes: &[&str]) {
    const CHILD: &str = "DEVBOX_REVIEW_ACTION_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            match kind {
                "pr" => child_pr(&mode).await,
                "cleanup" => child_cleanup(&mode).await,
                _ => child_rebase(&mode).await,
            }
        });
        return;
    }
    let root = tempfile::tempdir().expect("owned executables");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("git path");
    assert!(real_git.status.success());
    let real_git = String::from_utf8(real_git.stdout)
        .expect("Git path")
        .trim()
        .to_owned();
    let wrapper = r#"#!/bin/sh
if [ -f "$DEVBOX_REVIEW_REBASE_FLAG" ]; then
 case " $* " in
  *" status "*) exit 23;;
  *" --git-path rebase-"*) if [ "$DEVBOX_REVIEW_ACTION_CHILD" = markers ]; then exit 24; fi;;
 esac
fi
case " $* " in
 *" update-ref --no-deref -d "*)
    if [ "$DEVBOX_REVIEW_ACTION_CHILD" = recreated ]; then
      "$DEVBOX_REVIEW_REAL_GIT" "$@" || exit $?
      previous=; repo=
      for argument do
        if [ "$previous" = -C ]; then repo=$argument; fi
        previous=$argument
      done
      base=$("$DEVBOX_REVIEW_REAL_GIT" -C "$repo" rev-parse main)
      "$DEVBOX_REVIEW_REAL_GIT" -C "$repo" update-ref refs/heads/agent/review "$base"
      exit $?
    fi;;
 *" worktree remove "*)
    "$DEVBOX_REVIEW_REAL_GIT" "$@"
    result=$?
    if [ $result -eq 0 ]; then
      previous=; repo=
      for argument do
        if [ "$previous" = -C ]; then repo=$argument; fi
        previous=$argument
      done
      case "$DEVBOX_REVIEW_ACTION_CHILD" in
        gone) "$DEVBOX_REVIEW_REAL_GIT" -C "$repo" branch -D agent/review >/dev/null;;
        moved) old=$("$DEVBOX_REVIEW_REAL_GIT" -C "$repo" rev-parse agent/review)
          new=$(printf 'external owned-fixture commit\n' | "$DEVBOX_REVIEW_REAL_GIT" -C "$repo" commit-tree "$old^{tree}" -p "$old")
          "$DEVBOX_REVIEW_REAL_GIT" -C "$repo" update-ref refs/heads/agent/review "$new" "$old";;
      esac
    fi
    exit $result;;
 *" rebase "*) case " $* " in *" --abort "*) exec "$DEVBOX_REVIEW_REAL_GIT" "$@";; esac
    if [ "$DEVBOX_REVIEW_ACTION_CHILD" = no-markers ]; then exit 23; fi
    "$DEVBOX_REVIEW_REAL_GIT" "$@"
    result=$?
    if [ $result -ne 0 ]; then : > "$DEVBOX_REVIEW_REBASE_FLAG"; fi
    exit $result;;
esac
exec "$DEVBOX_REVIEW_REAL_GIT" "$@"
"#;
    fs::write(root.path().join("git"), wrapper).expect("git wrapper");
    fs::write(
        root.path().join("gh"),
        "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then exit 0; fi\n: > \"$DEVBOX_REVIEW_GH_MARKER\"\nprintf 'https://example.invalid/pr/1\\n'\n",
    )
    .expect("gh fixture");
    for name in ["git", "gh"] {
        fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o700))
            .expect("mode");
    }
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let test = match kind {
        "pr" => "review_final_pr_requires_clean_tracked_index_and_untracked_state_before_push",
        "cleanup" => {
            "review_final_auto_cleanup_releases_absent_branch_and_preserves_changed_branch"
        }
        _ => "review_final_rebase_inspection_failure_keeps_owned_abort_available",
    };
    let mut failures = vec![];
    for mode in modes {
        let marker = root.path().join(format!("gh-{mode}"));
        let flag = root.path().join(format!("rebase-{mode}"));
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                &format!("service::resource_tests::action_review_tests::{test}"),
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("PATH", &path)
            .env("DEVBOX_REVIEW_REAL_GIT", &real_git)
            .env("DEVBOX_REVIEW_GH_MARKER", marker)
            .env("DEVBOX_REVIEW_REBASE_FLAG", flag)
            .output()
            .expect("owned child");
        if !output.status.success() {
            failures.push(format!(
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
