use super::*;
use crate::tracker::{Binding, Resources};
use devbox_protocol::Fail;

async fn owned() -> (Fixture, AgentTask, PathBuf) {
    let fixture = Fixture::new().await;
    let directory =
        crate::plan::worktree_dir(&fixture.repo, "run-selector").expect("owned worktree path");
    git(
        &fixture.repo,
        &[
            "worktree",
            "add",
            "-b",
            "agent/run-selector",
            directory.to_str().expect("UTF-8 fixture path"),
            "main",
        ],
    );
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"run-selector", "projectId":fixture.project, "title":"Owned runtime selector",
        "profile":"missing-profile", "baseBranch":"main", "branch":"agent/run-selector",
        "worktreePath":directory, "workdir":directory, "sessionId":null, "terminalId":null,
        "state":"idle", "reason":null, "attention":null, "port":20120, "prompt":"",
        "followUps":[], "tokens":null, "prUrl":null, "closedHow":null,
        "createdMs":0, "updatedMs":0, "closedMs":null
    }))
    .expect("fixture task");
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
        .expect("fixture task and ownership");
    (fixture, task, directory)
}
async fn save(fixture: &Fixture, task: &AgentTask) {
    let row = task.clone();
    fixture
        .svc
        .db
        .write(move |tx| crate::store::put(tx, &row))
        .await
        .expect("fixture task update");
}
async fn unchanged(fixture: &Fixture, task: &AgentTask) {
    assert_eq!(
        fixture.svc.get(&task.id).await.expect("persisted task"),
        *task
    );
    assert!(
        task.terminal_id.is_none(),
        "fixture never starts a terminal or tool"
    );
}

#[tokio::test]
async fn run_worktree_returns_owned_canonical_directory_and_allocated_port() {
    let (fixture, task, directory) = owned().await;
    // Local uncommitted work is expected in an agent worktree and must survive lookup.
    fs::write(directory.join("in-progress"), "owned unfinished work").expect("owned file");
    let result = fixture
        .svc
        .run_worktree(&fixture.project, &task.id)
        .await
        .expect("owned selector");
    assert_eq!(result, (directory.to_string_lossy().into_owned(), 20120));
    assert_eq!(
        fs::read_to_string(directory.join("in-progress")).expect("preserved work"),
        "owned unfinished work"
    );
    unchanged(&fixture, &task).await;
}

#[tokio::test]
async fn run_worktree_rejects_a_different_project() {
    let (fixture, task, _) = owned().await;
    assert!(matches!(
        fixture
            .svc
            .run_worktree("different-project", &task.id)
            .await,
        Err(Fail::Domain(AgentsError::UnsafeWorktree))
    ));
    unchanged(&fixture, &task).await;
}

#[tokio::test]
async fn run_worktree_rejects_closed_and_preparing_agents() {
    for state in [AgentState::Closed, AgentState::Preparing] {
        let (fixture, mut task, _) = owned().await;
        task.state = state;
        save(&fixture, &task).await;
        assert!(matches!(
            fixture.svc.run_worktree(&fixture.project, &task.id).await,
            Err(Fail::Domain(AgentsError::BadState { .. }))
        ));
        unchanged(&fixture, &task).await;
    }
}

#[tokio::test]
async fn run_worktree_rejects_unowned_or_inline_directories() {
    for mode in ["planned", "partial", "inline", "external"] {
        let (fixture, mut task, _) = owned().await;
        match mode {
            "planned" | "partial" => {
                let id = task.id.clone();
                let partial = mode == "partial";
                fixture
                    .svc
                    .db
                    .write(move |tx| {
                        let mut binding = Binding::new(0);
                        if partial {
                            binding.resources = Resources::Partial {
                                path_exists: true,
                                branch_exists: Some(true),
                                worktree_registered: Some(true),
                            };
                        }
                        crate::store::put_binding(tx, &id, &binding)
                    })
                    .await
                    .expect("unowned fixture binding");
            }
            "inline" => {
                task.worktree_path = None;
                task.branch = None;
                task.workdir = fixture.repo.to_string_lossy().into_owned();
            }
            "external" => {
                let external = fixture.root.path().join("foreign");
                fs::create_dir(&external).expect("foreign fixture directory");
                fs::write(external.join("preserve"), "external fixture")
                    .expect("foreign fixture data");
                task.worktree_path = Some(external.to_string_lossy().into_owned());
                task.workdir = external.to_string_lossy().into_owned();
            }
            _ => unreachable!(),
        }
        save(&fixture, &task).await;
        assert!(
            matches!(
                fixture.svc.run_worktree(&fixture.project, &task.id).await,
                Err(Fail::Domain(AgentsError::UnsafeWorktree))
            ),
            "{mode}"
        );
        unchanged(&fixture, &task).await;
        if mode == "external" {
            assert_eq!(
                fs::read_to_string(fixture.root.path().join("foreign/preserve"))
                    .expect("foreign data preserved"),
                "external fixture"
            );
        }
    }
}

#[tokio::test]
async fn run_worktree_rejects_tampered_workdir_symlink_and_locked_registration() {
    for mode in ["workdir", "symlink", "locked"] {
        let (fixture, mut task, directory) = owned().await;
        match mode {
            "workdir" => task.workdir = fixture.repo.to_string_lossy().into_owned(),
            "symlink" => {
                let moved = fixture.root.path().join("moved-worktree");
                fs::rename(&directory, &moved).expect("move fixture worktree");
                std::os::unix::fs::symlink(&moved, &directory).expect("fixture symlink");
            }
            "locked" => git(
                &fixture.repo,
                &[
                    "worktree",
                    "lock",
                    directory.to_str().expect("fixture path"),
                ],
            ),
            _ => unreachable!(),
        }
        save(&fixture, &task).await;
        assert!(
            matches!(
                fixture.svc.run_worktree(&fixture.project, &task.id).await,
                Err(Fail::Domain(AgentsError::UnsafeWorktree))
            ),
            "{mode}"
        );
        unchanged(&fixture, &task).await;
    }
}

#[tokio::test]
async fn run_worktree_rejects_changed_or_detached_branch() {
    for detached in [false, true] {
        let (fixture, task, directory) = owned().await;
        if detached {
            git(&directory, &["checkout", "--detach"]);
        } else {
            git(&directory, &["checkout", "-b", "user-edited-branch"]);
        }
        assert!(matches!(
            fixture.svc.run_worktree(&fixture.project, &task.id).await,
            Err(Fail::Domain(AgentsError::UnsafeWorktree))
        ));
        unchanged(&fixture, &task).await;
    }
}

#[path = "agent_test_tests.rs"]
mod agent_test_tests;
