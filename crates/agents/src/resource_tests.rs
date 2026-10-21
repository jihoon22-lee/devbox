use super::*;
use devbox_core::{Instance, Migration, paths::Env};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

#[path = "resource_checkout_fixture.rs"]
mod checkout_fixture;

#[path = "tool_recovery_tests.rs"]
mod tool_recovery_tests;

#[path = "resume_review_tests.rs"]
mod resume_review_tests;

#[path = "action_review_tests.rs"]
mod action_review_tests;

#[tokio::test]
async fn list_pages_count_escaped_json_bytes_and_return_every_task() {
    let fixture = Fixture::new().await;
    let project = fixture.project.clone();
    fixture
        .svc
        .db
        .write(move |tx| {
            for index in 0..32 {
                let task: AgentTask = serde_json::from_value(serde_json::json!({
                    "id":format!("fixture-{index:03}"),"projectId":project,
                    "title":"Fixture","profile":"fixture","baseBranch":"main",
                    "branch":null,"worktreePath":null,"workdir":"/owned/fixture",
                    "sessionId":null,"terminalId":null,"state":"idle","reason":null,
                    "attention":null,"port":20000,"prompt":"\"".repeat(60 * 1024),
                    "followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
                    "createdMs":0,"updatedMs":0,"closedMs":null
                }))
                .expect("task");
                crate::store::put(tx, &task)?;
            }
            Ok(())
        })
        .await
        .expect("seed owned rows");
    let mut cursor = None;
    let mut ids = std::collections::HashSet::new();
    loop {
        let mut params = serde_json::json!({"includeClosedHours":24});
        if let Some(cursor) = &cursor {
            params["afterId"] = serde_json::json!(cursor);
        }
        let params: ListAgents = serde_json::from_value(params).expect("pagination parameters");
        let page = fixture.svc.list(params).await.expect("bounded page");
        let serialized = serde_json::to_vec(&page).expect("page JSON");
        assert!(
            serialized.len() <= 768 * 1024,
            "escaped JSON, metadata and envelope must fit the response budget: {}",
            serialized.len()
        );
        for item in &page.items {
            assert!(
                ids.insert(item.id.clone()),
                "pages must not duplicate a task"
            );
        }
        let value = serde_json::to_value(&page).expect("page");
        cursor = value["nextAfterId"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids.len(), 32, "pagination may not impose a task-count cap");
}

#[path = "base_ref_tests.rs"]
mod base_ref_tests;

struct TestEnv(PathBuf);
#[tokio::test]
async fn output_watcher_observes_waiting_and_resumed_tasks_beyond_the_first_rpc_page() {
    let fixture = Fixture::new().await;
    let project = fixture.project.clone();
    fixture.svc.db.write(move |tx| {
        for id in ["a-first", "z-last"] {
            let task: AgentTask = serde_json::from_value(serde_json::json!({
                "id":id,"projectId":project,"title":"Fixture","profile":"codex","baseBranch":"main",
                "branch":null,"worktreePath":null,"workdir":"/owned/fixture","sessionId":null,"terminalId":format!("terminal-{id}"),
                "state":"running","reason":null,"attention":null,"port":20000,"prompt":"p".repeat(64*1024),
                "followUps":(0..7).map(|at| serde_json::json!({"atMs":at,"text":"x".repeat(48*1024)})).collect::<Vec<_>>(),
                "tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
            })).expect("task");
            crate::store::put(tx,&task)?;
        }
        Ok(())
    }).await.expect("seed owned rows");
    assert_eq!(
        fixture
            .svc
            .list(ListAgents::default())
            .await
            .expect("bounded page")
            .items
            .len(),
        1
    );
    let watched = fixture.svc.watched_tasks().await.expect("observer tasks");
    let hidden = watched
        .into_iter()
        .find(|task| task.id == "z-last")
        .expect("second RPC page must stay eligible for output observation");
    let waiting = fixture
        .svc
        .output_event(
            crate::watch::Snapshot {
                task: hidden,
                binding: None,
            },
            true,
        )
        .await
        .expect("observe waiting")
        .expect("transition");
    assert_eq!(waiting.state, AgentState::Waiting);
    assert_eq!(
        waiting.attention.as_ref().expect("attention").kind,
        "output"
    );
    let hidden = fixture
        .svc
        .watched_tasks()
        .await
        .expect("observer tasks")
        .into_iter()
        .find(|task| task.id == "z-last")
        .expect("hidden waiting task must remain observed");
    let resumed = fixture
        .svc
        .output_event(
            crate::watch::Snapshot {
                task: hidden,
                binding: None,
            },
            false,
        )
        .await
        .expect("observe resume")
        .expect("transition");
    assert_eq!(resumed.state, AgentState::Running);
}
impl Env for TestEnv {
    fn var(&self, key: &str) -> Option<String> {
        match key {
            "HOME" => Some(self.0.to_string_lossy().into_owned()),
            "XDG_RUNTIME_DIR" => Some(self.0.join("runtime").to_string_lossy().into_owned()),
            _ => None,
        }
    }
    fn uid(&self) -> u32 {
        1000
    }
}

struct Fixture {
    svc: Arc<AgentsService>,
    root: tempfile::TempDir,
    repo: PathBuf,
    project: String,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("owned fixture");
        let paths = Paths::resolve(
            Instance::parse(&format!(
                "test-resource-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ))
            .expect("instance"),
            &TestEnv(root.path().into()),
        )
        .expect("paths");
        paths.ensure_dirs().expect("directories");
        let migrations: Vec<Migration> = crate::MIGRATIONS
            .iter()
            .chain(devbox_projects::MIGRATIONS)
            .chain(devbox_terminal::MIGRATIONS)
            .copied()
            .collect();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).expect("db");
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(ProjectsService::new(
            db.clone(),
            hub.clone(),
            paths.home.clone(),
            vec![],
        ));
        let terminal = Arc::new(TerminalService::new(
            db.clone(),
            hub.clone(),
            paths.clone(),
            projects.clone(),
            devbox_core::config::TerminalConfig::default(),
        ));
        let svc = Arc::new(AgentsService::new(
            db,
            hub,
            paths,
            projects,
            terminal,
            vec![],
            crate::AgentRuntime {
                devbox_bin: "/owned/devbox".into(),
                use_scope: false,
                cleanup_after_close: false,
            },
        ));
        let repo = root.path().join("project");
        fs::create_dir(&repo).expect("repo");
        git(&repo, &["init", "-b", "main"]);
        git(
            &repo,
            &[
                "config",
                "core.hooksPath",
                repo.join(".git/hooks").to_str().expect("hook path"),
            ],
        );
        git(&repo, &["config", "commit.gpgSign", "false"]);
        git(&repo, &["config", "user.name", "fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        git(&repo, &["commit", "--allow-empty", "-m", "fixture"]);
        let project = svc
            .projects
            .add(devbox_projects::AddProject {
                path: repo.to_string_lossy().into_owned(),
            })
            .await
            .expect("register")
            .id;
        let fixture = Self {
            svc,
            root,
            repo,
            project,
        };
        if let Some(flag) = std::env::var_os("DEVBOX_RESOURCE_CHECKOUT_FLAG") {
            fs::write(flag, fixture.repo.to_str().expect("owned fixture path"))
                .expect("owned checkout flag");
        }
        fixture
    }
    fn request(&self) -> CreateAgentTask {
        CreateAgentTask {
            project_id: self.project.clone(),
            title: "owned".into(),
            profile: "claude".into(),
            base_branch: Some("main".into()),
            worktree: true,
            prompt: String::new(),
        }
    }
    async fn drain(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(15), self.svc.shutdown())
            .await
            .expect("bounded preparation drain");
    }
}
fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .env("HOME", repo.parent().expect("owned fixture home"))
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn partial_worktree_failure_retains_written_files_and_ownership() {
    if checkout_fixture::run_child(
        "service::resource_tests::partial_worktree_failure_retains_written_files_and_ownership",
    ) {
        return;
    }
    let fixture = Fixture::new().await;
    let reserved = fixture
        .svc
        .create(fixture.request())
        .await
        .expect("reserve");
    assert_eq!(reserved.state, AgentState::Preparing);
    fixture.drain().await;
    let task = fixture.svc.get(&reserved.id).await.expect("task");
    assert_eq!(task.state, AgentState::Failed);
    assert_eq!(task.reason.as_deref(), Some("worktree_add_partial"));
    assert_eq!(task.branch, reserved.branch);
    assert_eq!(task.worktree_path, reserved.worktree_path);
    assert_eq!(
        fs::read_to_string(
            Path::new(task.worktree_path.as_deref().expect("path")).join("checkout-created")
        )
        .expect("retained checkout file"),
        "owned-checkout-data"
    );
    let binding = fixture
        .svc
        .db
        .read(move |tx| store::binding(tx, &reserved.id))
        .await
        .expect("binding read")
        .expect("binding");
    assert!(matches!(
        binding.resources,
        crate::tracker::Resources::Partial {
            path_exists: true,
            branch_exists: Some(true),
            worktree_registered: Some(true)
        }
    ));
}

#[tokio::test]
async fn concurrent_reservations_skip_foreign_paths_and_branches() {
    if checkout_fixture::run_child(
        "service::resource_tests::concurrent_reservations_skip_foreign_paths_and_branches",
    ) {
        return;
    }
    let fixture = Fixture::new().await;
    git(&fixture.repo, &["branch", "agent/owned"]);
    let foreign = fixture.root.path().join("project-agents/owned-2");
    fs::create_dir_all(&foreign).expect("foreign fixture");
    fs::write(foreign.join("preserve"), "foreign").expect("foreign file");
    let (a, b) = tokio::join!(
        fixture.svc.create(fixture.request()),
        fixture.svc.create(fixture.request())
    );
    let a = a.expect("first reservation");
    let b = b.expect("second reservation");
    assert_ne!(a.port, b.port);
    assert_ne!(a.branch, b.branch);
    assert_ne!(a.worktree_path, b.worktree_path);
    for task in [&a, &b] {
        assert_ne!(task.branch.as_deref(), Some("agent/owned"));
        assert_ne!(task.worktree_path.as_deref(), foreign.to_str());
    }
    fixture.drain().await;
    assert_eq!(
        fs::read_to_string(foreign.join("preserve")).expect("retained foreign fixture"),
        "foreign"
    );
    assert!(
        devbox_git::branches(&fixture.repo)
            .await
            .expect("branches")
            .contains(&"agent/owned".into())
    );
}

#[tokio::test]
async fn canceled_create_waiter_still_drains_committed_preparation() {
    if checkout_fixture::run_child(
        "service::resource_tests::canceled_create_waiter_still_drains_committed_preparation",
    ) {
        return;
    }
    let fixture = Fixture::new().await;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let db = fixture.svc.db.clone();
    let blocker = tokio::spawn(async move {
        db.write_checked::<(), (), _>(move |_| {
            entered_tx.send(()).expect("signal writer barrier");
            release_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("release writer before deadline");
            Ok(Err(()))
        })
        .await
        .expect("writer barrier")
    });
    entered_rx.await.expect("writer entered");
    let svc = fixture.svc.clone();
    let request = fixture.request();
    let create = tokio::spawn(async move { svc.create(request).await });
    // create constructs its tracker token immediately before synchronously
    // enqueueing the reservation; its next suspension awaits the blocked writer.
    let reached = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture.svc.background.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    create.abort();
    let canceled = create.await;
    release_tx.send(()).expect("release writer");
    assert!(
        reached.is_ok(),
        "create reached committed-reservation token"
    );
    assert!(canceled.expect_err("caller canceled").is_cancelled());
    assert_eq!(blocker.await.expect("barrier task"), Err(()));
    fixture.drain().await;
    assert_eq!(fixture.svc.background.len(), 0);
    let tasks = fixture
        .svc
        .list(ListAgents::default())
        .await
        .expect("durable task list")
        .items;
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].state, AgentState::Failed);
    assert_eq!(tasks[0].reason.as_deref(), Some("worktree_add_partial"));
    assert!(
        Path::new(
            tasks[0]
                .worktree_path
                .as_deref()
                .expect("retained worktree")
        )
        .join("checkout-created")
        .is_file()
    );
}

#[tokio::test]
async fn restart_reconciles_exact_preparing_resources_without_touching_review_or_closed() {
    if checkout_fixture::run_child(
        "service::resource_tests::restart_reconciles_exact_preparing_resources_without_touching_review_or_closed",
    ) {
        return;
    }
    let fixture = Fixture::new().await;
    let reserved = fixture
        .svc
        .create(fixture.request())
        .await
        .expect("reserve");
    fixture.drain().await;
    let mut preparing = fixture.svc.get(&reserved.id).await.expect("task");
    preparing.state = AgentState::Preparing;
    let mut review = preparing.clone();
    review.id = "review".into();
    review.state = AgentState::Review;
    let mut closed = preparing.clone();
    closed.id = "closed".into();
    closed.state = AgentState::Closed;
    let mut unmade = preparing.clone();
    unmade.id = "unmade".into();
    unmade.branch = Some("agent/unmade".into());
    unmade.worktree_path = Some(
        fixture
            .root
            .path()
            .join("project-agents/unmade")
            .to_string_lossy()
            .into_owned(),
    );
    unmade.workdir = unmade.worktree_path.clone().expect("owned fixture");
    fixture
        .svc
        .db
        .write(move |tx| {
            for t in [&preparing, &review, &closed, &unmade] {
                store::put(tx, t)?;
                store::put_binding(tx, &t.id, &Binding::new(t.created_ms))?;
            }
            Ok(())
        })
        .await
        .expect("simulate interrupted preparation");
    fixture.svc.reconcile().await.expect("restart reconcile");
    let recovered = fixture.svc.get(&reserved.id).await.expect("recovered");
    assert_eq!(recovered.state, AgentState::Failed);
    assert_eq!(recovered.reason.as_deref(), Some("session_lost"));
    assert_eq!(recovered.branch, reserved.branch);
    assert_eq!(recovered.worktree_path, reserved.worktree_path);
    assert_eq!(
        fixture
            .svc
            .get("review")
            .await
            .expect("owned fixture")
            .state,
        AgentState::Review
    );
    assert_eq!(
        fixture
            .svc
            .get("closed")
            .await
            .expect("owned fixture")
            .state,
        AgentState::Closed
    );
    let unmade = fixture.svc.get("unmade").await.expect("owned fixture");
    assert_eq!(unmade.state, AgentState::Failed);
    assert!(unmade.branch.is_none());
    assert!(unmade.worktree_path.is_none());
    assert_eq!(unmade.workdir, fixture.repo.to_string_lossy());
    assert_eq!(
        fs::read_to_string(
            Path::new(recovered.worktree_path.as_deref().expect("owned fixture"))
                .join("checkout-created")
        )
        .expect("owned fixture"),
        "owned-checkout-data"
    );
}

#[path = "prepare_binding_tests.rs"]
mod prepare_binding_tests;

#[path = "run_worktree_tests.rs"]
mod run_worktree_tests;
