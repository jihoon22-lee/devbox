use super::*;
use devbox_core::{Instance, Migration, paths::Env};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

struct TestEnv(PathBuf);
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
            "/owned/devbox".into(),
            false,
            false,
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
        let hook = fixture.repo.join(".git/hooks/post-checkout");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf owned-hook-data > hook-created\nexit 3\n",
        )
        .expect("hook");
        fs::set_permissions(hook, fs::Permissions::from_mode(0o700)).expect("hook mode");
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
async fn partial_worktree_failure_retains_hook_written_files_and_ownership() {
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
            Path::new(task.worktree_path.as_deref().expect("path")).join("hook-created")
        )
        .expect("retained hook file"),
        "owned-hook-data"
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
        while fixture.svc.background.len() == 0 {
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
        .join("hook-created")
        .is_file()
    );
}

#[tokio::test]
async fn restart_reconciles_exact_preparing_resources_without_touching_review_or_closed() {
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
    unmade.workdir = unmade.worktree_path.clone().unwrap();
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
        fixture.svc.get("review").await.unwrap().state,
        AgentState::Review
    );
    assert_eq!(
        fixture.svc.get("closed").await.unwrap().state,
        AgentState::Closed
    );
    let unmade = fixture.svc.get("unmade").await.unwrap();
    assert_eq!(unmade.state, AgentState::Failed);
    assert!(unmade.branch.is_none());
    assert!(unmade.worktree_path.is_none());
    assert_eq!(unmade.workdir, fixture.repo.to_string_lossy());
    assert_eq!(
        fs::read_to_string(
            Path::new(recovered.worktree_path.as_deref().unwrap()).join("hook-created")
        )
        .unwrap(),
        "owned-hook-data"
    );
}
