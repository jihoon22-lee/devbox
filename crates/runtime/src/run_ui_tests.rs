use crate::{
    run_store::{self, StoredRun},
    service_tests::Fixture,
    *,
};
use devbox_projects::{ProjectsError, RemoveProject};
use devbox_protocol::Fail;

fn recorded(project: &str, index: u64, def: &str, state: RunState) -> StoredRun {
    StoredRun {
        source_cwd: None,
        secret_definition: None,
        windows: None,
        schedule: None,
        run: Run {
            id: format!("run-{index}"),
            project_id: project.into(),
            def_id: def.into(),
            kind: RunKind::Service,
            state,
            exit: None,
            started_ms: index,
            ended_ms: None,
            restarts: 0,
            unit: format!("devbox-owned-{index}.service"),
            ready: true,
            pid: Some(index as u32 + 10),
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

#[tokio::test]
async fn ui_definition_page_keeps_old_live_service_and_latest_result() {
    let f = Fixture::new().await;
    f.define("service", "true", SaveTo::ThisPc).await;
    f.define("task", "true", SaveTo::ThisPc).await;
    let project = f.project.clone();
    f.db.write(move |tx| {
        for i in 0..300 {
            run_store::put(
                tx,
                &recorded(
                    &project,
                    i,
                    if i == 0 { "service" } else { "task" },
                    if i == 0 {
                        RunState::Running
                    } else {
                        RunState::Exited
                    },
                ),
            )?;
        }
        Ok(())
    })
    .await
    .expect("fixture result");
    let page = f
        .runs
        .list(ListRuns {
            project_id: f.project,
            cursor: None,
        })
        .await
        .expect("fixture result");
    assert!(
        page.runs.iter().any(|r| r.id == "run-0"),
        "old live service must survive recent history bounds"
    );
    assert!(page.runs.iter().any(|r| r.id == "run-299"));
    assert_eq!(
        page.runs.len(),
        2,
        "bounded one representative per definition"
    );
}

#[tokio::test]
async fn ui_get_returns_old_selection_with_current_revision_and_original_cwd() {
    let f = Fixture::new().await;
    f.define("task", "true", SaveTo::ThisPc).await;
    let mut run = recorded(&f.project, 1, "task", RunState::Running);
    run.source_cwd = Some("/owned/original".into());
    f.runs.save_run(&run).await.expect("fixture result");
    let first = f
        .runs
        .get(RunId {
            run_id: "run-1".into(),
        })
        .await
        .expect("fixture result");
    run.run.pid = Some(999);
    run.run.restarts = 2;
    f.runs.save_run(&run).await.expect("fixture result");
    f.define("task", "echo edited", SaveTo::ThisPc).await;
    let detail = f
        .runs
        .get(RunId {
            run_id: "run-1".into(),
        })
        .await
        .expect("fixture result");
    assert!(
        detail.rev.parse::<u64>().expect("fixture result")
            > first.rev.parse().expect("fixture result")
    );
    assert_eq!(detail.run.pid, Some(999));
    assert_eq!(detail.run.restarts, 2);
    assert_eq!(
        f.runs
            .location_context("run-1")
            .await
            .expect("fixture result")
            .cwd
            .as_deref(),
        Some("/owned/original")
    );
    assert!(matches!(
        f.runs
            .get(RunId {
                run_id: "missing".into()
            })
            .await,
        Err(devbox_protocol::Fail::Domain(
            RuntimeError::RunNotFound { .. }
        ))
    ));
}

#[tokio::test]
async fn ui_definition_pages_are_bounded_and_include_later_definitions() {
    let f = Fixture::new().await;
    let project = f.project.clone();
    f.db.write(move |tx| {
        for i in 0..270 {
            let id = format!("task-{i:03}");
            crate::store::define(
                tx,
                &project,
                &RunDef::Task(TaskDef {
                    id: id.clone(),
                    command: "true".into(),
                    ..Default::default()
                }),
            )?;
            run_store::put(tx, &recorded(&project, i, &id, RunState::Exited))?;
        }
        Ok(())
    })
    .await
    .expect("fixture result");
    let first = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
        .await
        .expect("fixture result");
    assert_eq!(first.defs.len(), 256);
    assert_eq!(first.runs.len(), 256);
    assert!(
        serde_json::to_vec(&first).expect("fixture result").len() <= crate::service::PAGE_BYTES
    );
    let next = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: first.next_cursor,
        })
        .await
        .expect("fixture result");
    assert_eq!(next.defs.len(), 14);
    assert_eq!(next.runs.len(), 14);
    assert!(next.next_cursor.is_none());
    assert_eq!(
        next.defs.last().expect("fixture value").def.id(),
        "task-269"
    );
}

#[tokio::test]
async fn published_start_has_a_readable_log_before_the_wrapper_and_restart_preserves_bytes() {
    use std::io::{Read, Write};
    let f = Fixture::new().await;
    struct E(String);
    impl devbox_core::Env for E {
        fn var(&self, key: &str) -> Option<String> {
            matches!(key, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.clone())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = devbox_core::Paths::resolve(
        devbox_core::Instance::parse("test-log-start").expect("instance"),
        &E(f.root.path().display().to_string()),
    )
    .expect("paths");
    let runs = RuntimeService::new(f.db.clone(), f.hub.clone(), f.projects.clone())
        .with_execution(ExecutionConfig::new(paths));
    let mut events = f.hub.subscribe("runtime.changed");
    let run = recorded(&f.project, 1, "task", RunState::Starting);
    // No wrapper exists yet: publication must make the stream source available.
    runs.save_starting_run(&run)
        .await
        .expect("publish starting");
    tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("published start");
    let path = runs.log_path(&run.run.id).await.expect("recorded log");
    let mut reader = std::fs::File::open(&path).expect("published run log is already readable");
    let mut before = String::new();
    reader
        .read_to_string(&mut before)
        .expect("read empty source");
    assert!(before.is_empty());
    // The late wrapper appends to the same file already opened by a log consumer.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("wrapper opens")
        .write_all(b"late wrapper output\n")
        .expect("wrapper writes");
    reader
        .read_to_string(&mut before)
        .expect("follow late output");
    assert_eq!(before, "late wrapper output\n");
    runs.save_starting_run(&run)
        .await
        .expect("restart publication");
    assert_eq!(
        std::fs::read_to_string(&path).expect("retained history"),
        before
    );
    // An externally replaced log must not redirect preparation outside the owned file.
    std::fs::remove_file(&path).expect("replace owned fixture");
    let outside = f.root.path().join("outside");
    std::fs::write(&outside, "preserve").expect("outside fixture");
    std::os::unix::fs::symlink(&outside, &path).expect("link fixture");
    tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("restart event");
    assert!(runs.save_starting_run(&run).await.is_err());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err(),
        "failed preparation never publishes"
    );
    assert_eq!(
        std::fs::read_to_string(outside).expect("preserved"),
        "preserve"
    );
}

/// Starts service `api` as a recorded live run, then deletes its definition.
/// The unit name is unique, so a stop only finds the unit missing.
async fn orphaned_api(f: &Fixture) -> StoredRun {
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            def: RunDef::Service(ServiceDef {
                id: "api".into(),
                command: "true".into(),
                ..Default::default()
            }),
            save_to: SaveTo::ThisPc,
        })
        .await
        .expect("define api");
    let mut run = recorded(&f.project, 1, "api", RunState::Running);
    run.run.unit = format!(
        "devbox-test-orphan-{}.service",
        uuid::Uuid::new_v4().simple()
    );
    f.runs.save_run(&run).await.expect("started api");
    f.runs
        .undefine(UndefineRun {
            project_id: f.project.clone(),
            id: "api".into(),
        })
        .await
        .expect("delete api");
    run
}

#[tokio::test]
async fn list_includes_live_service_runs_whose_definition_was_removed() {
    let f = Fixture::new().await;
    let orphan = orphaned_api(&f).await;
    let project = f.project.clone();
    f.db.write(move |tx| {
        // 257 tasks fill the first page and the one row it reads ahead, so the
        // defined live service `web` is beyond everything the first page reads.
        for i in 0..257 {
            let id = format!("task-{i:03}");
            crate::store::define(
                tx,
                &project,
                &RunDef::Task(TaskDef {
                    id,
                    command: "true".into(),
                    ..Default::default()
                }),
            )?;
        }
        crate::store::define(
            tx,
            &project,
            &RunDef::Service(ServiceDef {
                id: "web".into(),
                command: "true".into(),
                ..Default::default()
            }),
        )?;
        run_store::put(tx, &recorded(&project, 2, "web", RunState::Running))?;
        // A finished service and live non-service runs of a gone definition stay out.
        run_store::put(tx, &recorded(&project, 3, "gone", RunState::Exited))?;
        for (index, kind) in [
            (4, RunKind::Task),
            (5, RunKind::Scheduled),
            (6, RunKind::AgentTest),
        ] {
            let mut run = recorded(&project, index, "gone", RunState::Running);
            run.run.kind = kind;
            run_store::put(tx, &run)?;
        }
        Ok(())
    })
    .await
    .expect("fixture runs");
    let first = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
        .await
        .expect("first page");
    assert_eq!(first.defs.len(), 256);
    assert!(first.defs.iter().all(|row| row.def.id() != "api"));
    assert_eq!(first.runs, [orphan.run], "only the orphan, once");
    let next = f
        .runs
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: first.next_cursor,
        })
        .await
        .expect("next page");
    let defs: Vec<_> = next.defs.iter().map(|row| row.def.id()).collect();
    assert_eq!(defs, ["task-256", "web"]);
    let runs: Vec<_> = next.runs.iter().map(|run| run.id.as_str()).collect();
    assert_eq!(runs, ["run-2"], "a later page never repeats the orphan");
}

#[tokio::test]
async fn stopping_an_orphan_service_by_run_id_clears_live_services() {
    let f = Fixture::new().await;
    orphaned_api(&f).await;
    let remove = || {
        f.projects.remove(RemoveProject {
            id: f.project.clone(),
        })
    };
    assert!(
        matches!(
            remove().await,
            Err(Fail::Domain(ProjectsError::InUse { .. }))
        ),
        "the live service still holds its project"
    );
    let list = || {
        f.runs.list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
    };
    // The user stops the run that the list offers, by its run id.
    let listed = list().await.expect("list");
    let [run] = listed.runs.as_slice() else {
        panic!("one orphan run: {:?}", listed.runs);
    };
    let stopped = f
        .runs
        .stop(RunId {
            run_id: run.id.clone(),
        })
        .await
        .expect("stop by run id");
    assert_eq!(stopped.state, RunState::Stopped);
    let project = f.project.clone();
    let live =
        f.db.read(move |tx| run_store::live_services(tx, &project, None))
            .await
            .expect("live services");
    assert!(live.is_empty(), "{live:?}");
    assert!(list().await.expect("list").runs.is_empty());
    remove().await.expect("nothing holds the project any more");
}
