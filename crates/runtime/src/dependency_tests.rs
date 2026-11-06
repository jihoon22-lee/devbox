use crate::{
    run_store::{self, StoredRun},
    service_tests::Fixture,
    *,
};

fn recorded(project: &str, id: &str, def: &str, state: RunState, started_ms: u64) -> StoredRun {
    StoredRun {
        source_cwd: None,
        secret_definition: None,
        windows: None,
        schedule: None,
        run: Run {
            id: id.into(),
            project_id: project.into(),
            def_id: def.into(),
            kind: RunKind::Service,
            state,
            exit: None,
            started_ms,
            ended_ms: None,
            restarts: 0,
            unit: format!("devbox-owned-{id}.service"),
            ready: state == RunState::Running,
            pid: None,
            port: None,
            attempt_id: None,
            windows: None,
        },
        restart: RestartPolicy::No,
        start_limit: None,
        worktree: None,
        start_limit_notified: false,
    }
}

fn service(id: &str) -> RunDef {
    RunDef::Service(ServiceDef {
        id: id.into(),
        command: "true".into(),
        ..Default::default()
    })
}

fn task(id: &str) -> RunDef {
    RunDef::Task(TaskDef {
        id: id.into(),
        command: "true".into(),
        ..Default::default()
    })
}

fn graph(f: &Fixture, worktree: Option<&str>, def: &RunDef) -> PreparedGraph {
    PreparedGraph {
        agent_test: false,
        project_id: f.project.clone(),
        schedule: None,
        worktree: worktree.map(Into::into),
        nodes: vec![def.clone()],
        windows: Default::default(),
        automatic_ports: Default::default(),
    }
}

/// Run ids `start_graph` would reuse for `def` in a graph started for `worktree`.
async fn reused(f: &Fixture, worktree: Option<&str>, def: &RunDef) -> Vec<String> {
    f.runs
        .reusable_dependencies(&graph(f, worktree, def), def)
        .await
        .expect("dependency lookup")
        .into_iter()
        .map(|value| value.run.id)
        .collect()
}

#[tokio::test]
async fn dependency_service_reuse_matches_only_a_live_run_of_the_same_definition() {
    let f = Fixture::new().await;
    let project = f.project.clone();
    let mut rows = vec![
        recorded(&project, "restarting", "db", RunState::Restarting, 2),
        recorded(&project, "ready", "db", RunState::Running, 5),
        recorded(&project, "starting", "db", RunState::Starting, 6),
        recorded(&project, "exited", "db", RunState::Exited, 7),
        recorded(&project, "failed", "db", RunState::Failed, 8),
        recorded(&project, "stopped", "db", RunState::Stopped, 9),
        recorded(&project, "cache", "cache", RunState::Running, 3),
    ];
    let mut feature = recorded(&project, "feature", "db", RunState::Running, 4);
    feature.worktree = Some("feature".into());
    let mut task_run = recorded(&project, "task-run", "db", RunState::Running, 10);
    task_run.run.kind = RunKind::Task;
    rows.extend([feature, task_run]);
    f.db.write(move |tx| {
        for row in &rows {
            run_store::put(tx, row)?;
        }
        Ok(())
    })
    .await
    .expect("fixture runs");
    // Same identity: only live service runs, a ready one before newer unready ones.
    assert_eq!(
        reused(&f, None, &service("db")).await,
        ["ready", "starting", "restarting"]
    );
    // Another worktree is another identity, in both directions.
    assert_eq!(
        reused(&f, Some("feature"), &service("db")).await,
        ["feature"]
    );
    assert!(reused(&f, Some("other"), &service("db")).await.is_empty());
    // Another definition id only matches its own run.
    assert_eq!(reused(&f, None, &service("cache")).await, ["cache"]);
    assert!(reused(&f, None, &service("queue")).await.is_empty());
    // A task dependency is one-shot and never reused, even with live runs of its id.
    assert!(reused(&f, None, &task("db")).await.is_empty());
}

/// The start lock of `def`, or `Err` while another start of its identity holds it.
async fn start_lock(
    f: &Fixture,
    worktree: Option<&str>,
    def: &RunDef,
) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, tokio::time::error::Elapsed> {
    let graph = graph(f, worktree, def);
    let lock = f.runs.service_start_lock(&graph, def);
    tokio::time::timeout(std::time::Duration::from_millis(100), lock).await
}

#[tokio::test]
async fn service_starts_of_one_definition_identity_are_serialized() {
    let f = Fixture::new().await;
    let held = start_lock(&f, None, &service("db")).await.expect("free");
    assert!(held.is_some());
    assert!(start_lock(&f, None, &service("db")).await.is_err());
    for (worktree, def) in [(Some("feature"), service("db")), (None, service("cache"))] {
        let other = start_lock(&f, worktree, &def).await;
        assert!(other.expect("another identity does not wait").is_some());
    }
    let task = start_lock(&f, None, &task("db")).await;
    assert!(task.expect("tasks do not wait").is_none());
    drop(held);
    let next = start_lock(&f, None, &service("db")).await;
    assert!(next.expect("released identity").is_some());
}
