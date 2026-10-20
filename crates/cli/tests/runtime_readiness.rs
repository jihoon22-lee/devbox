mod support;
use devbox_core::sh_quote;
use devbox_protocol::Fail;
use devbox_runtime::*;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use support::runtime::{RuntimeFixture, task};

async fn direct(
    fixture: &mut RuntimeFixture,
    readiness: Readiness,
) -> (Arc<RuntimeService>, String) {
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    drop(client);
    fixture.daemon.stop();
    let services =
        devbox_cli::daemon::build_services(fixture.daemon.paths.clone()).expect("owned services");
    let mut execution = ExecutionConfig::new(fixture.daemon.paths.clone());
    execution.executable = env!("CARGO_BIN_EXE_devbox").into();
    execution.slice = Some(fixture.slice.clone());
    execution.readiness = readiness;
    let runs = Arc::new(
        RuntimeService::new(
            services.db.clone(),
            services.hub.clone(),
            services.projects.clone(),
        )
        .with_execution(execution),
    );
    (runs, project)
}
async fn define(runs: &RuntimeService, project: &str, def: RunDef) {
    runs.define(DefineRun {
        project_id: project.into(),
        def,
        save_to: SaveTo::ThisPc,
    })
    .await
    .expect("define fixture");
}
fn port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("owned free port")
        .local_addr()
        .expect("port")
        .port()
}
fn listening() -> Readiness {
    Arc::new(|check| {
        Box::pin(async move {
            for port in check.ports {
                if !tokio::time::timeout(
                    Duration::from_millis(100),
                    tokio::net::TcpStream::connect(("127.0.0.1", port)),
                )
                .await
                .is_ok_and(|r| r.is_ok())
                {
                    return false;
                }
            }
            true
        })
    })
}
#[tokio::test]
#[ignore = "needs systemd user"]
async fn dependent_service_waits_for_readiness_then_times_out() {
    let mut fixture = RuntimeFixture::new();
    let (runs, project) = direct(&mut fixture, listening()).await;
    let selected = port();
    let opened = fixture.daemon.home.path().join("opened");
    let dependent = fixture.daemon.home.path().join("dependent");
    let python = format!(
        "import socket,time; s=socket.socket(); s.bind(('127.0.0.1',{selected})); s.listen(); open({:?},'w').write('ready'); time.sleep(60)",
        opened.display().to_string()
    );
    define(
        &runs,
        &project,
        RunDef::Service(ServiceDef {
            id: "upstream".into(),
            command: format!("sleep 3; exec /usr/bin/python3 -c {}", sh_quote(&python)),
            ports_hint: vec![selected],
            ready_timeout_sec: 6,
            ..Default::default()
        }),
    )
    .await;
    define(
        &runs,
        &project,
        RunDef::Service(ServiceDef {
            id: "dependent".into(),
            command: format!(
                "test -f {} && touch {}; sleep 60",
                sh_quote(&opened.display().to_string()),
                sh_quote(&dependent.display().to_string())
            ),
            depends_on: vec!["upstream".into()],
            ..Default::default()
        }),
    )
    .await;
    let before = Instant::now();
    let run = runs
        .start(StartRun {
            project_id: project.clone(),
            id: "dependent".into(),
            worktree: None,
        })
        .await
        .expect("prepared service chain");
    assert!(before.elapsed() >= Duration::from_secs(3));
    assert!(run.ready);
    support::runtime::file_exists(&dependent).await;
    assert!(opened.is_file());
    let timeout_port = port();
    define(
        &runs,
        &project,
        RunDef::Service(ServiceDef {
            id: "unready".into(),
            command: "sleep 60".into(),
            ports_hint: vec![timeout_port],
            ready_timeout_sec: 2,
            ..Default::default()
        }),
    )
    .await;
    let absent = fixture.daemon.home.path().join("never-start");
    let mut final_task = task(
        "after-timeout",
        &format!("touch {}", sh_quote(&absent.display().to_string())),
    );
    if let RunDef::Task(def) = &mut final_task {
        def.depends_on.push("unready".into());
    }
    define(&runs, &project, final_task).await;
    let before = Instant::now();
    assert!(
        matches!(runs.start(StartRun { project_id: project.clone(), id: "after-timeout".into(), worktree: None }).await, Err(Fail::Domain(RuntimeError::NotReady { id })) if id == "unready")
    );
    assert!(
        before.elapsed() >= Duration::from_secs(2) && before.elapsed() < Duration::from_secs(4)
    );
    assert!(!absent.exists());
    assert!(
        !runs
            .runs(RunsQuery {
                project_id: Some(project),
                limit: 100
            })
            .await
            .expect("runs")
            .items
            .iter()
            .any(|r| r.def_id == "after-timeout")
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn stalled_readiness_checker_obeys_the_same_deadline() {
    let mut fixture = RuntimeFixture::new();
    let (runs, project) = direct(
        &mut fixture,
        Arc::new(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                true
            })
        }),
    )
    .await;
    define(
        &runs,
        &project,
        RunDef::Service(ServiceDef {
            id: "stalled".into(),
            command: "sleep 60".into(),
            ports_hint: vec![port()],
            ready_timeout_sec: 2,
            ..Default::default()
        }),
    )
    .await;
    let before = Instant::now();
    assert!(matches!(
        runs.start(StartRun {
            project_id: project,
            id: "stalled".into(),
            worktree: None
        })
        .await,
        Err(Fail::Domain(RuntimeError::NotReady { .. }))
    ));
    assert!(
        before.elapsed() >= Duration::from_secs(2) && before.elapsed() < Duration::from_secs(4)
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn stopped_service_cannot_be_marked_ready_by_an_inflight_checker() {
    let mut fixture = RuntimeFixture::new();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let ready: Readiness = Arc::new({
        let entered = entered.clone();
        let release = release.clone();
        move |_| {
            let entered = entered.clone();
            let release = release.clone();
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                true
            })
        }
    });
    let (runs, project) = direct(&mut fixture, ready).await;
    define(
        &runs,
        &project,
        RunDef::Service(ServiceDef {
            id: "upstream".into(),
            command: "sleep 60".into(),
            ports_hint: vec![port()],
            ready_timeout_sec: 10,
            ..Default::default()
        }),
    )
    .await;
    let marker = fixture.daemon.home.path().join("forbidden-dependent");
    let mut def = task(
        "dependent",
        &format!("touch {}", sh_quote(&marker.display().to_string())),
    );
    if let RunDef::Task(def) = &mut def {
        def.depends_on.push("upstream".into());
    }
    define(&runs, &project, def).await;
    let launched = tokio::spawn({
        let runs = runs.clone();
        let project = project.clone();
        async move {
            runs.start(StartRun {
                project_id: project,
                id: "dependent".into(),
                worktree: None,
            })
            .await
        }
    });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .expect("checker pending");
    let upstream = runs
        .runs(RunsQuery {
            project_id: Some(project.clone()),
            limit: 100,
        })
        .await
        .expect("runs")
        .items
        .remove(0);
    let stopped = runs
        .stop(RunId {
            run_id: upstream.id,
        })
        .await
        .expect("stop upstream");
    assert_eq!(stopped.state, RunState::Stopped);
    release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(3), launched)
        .await
        .expect("start terminates")
        .expect("start join");
    assert!(matches!(
        result,
        Err(Fail::Domain(RuntimeError::DependencyFailed { .. }))
    ));
    assert!(!marker.exists());
    let items = runs
        .runs(RunsQuery {
            project_id: Some(project),
            limit: 100,
        })
        .await
        .expect("runs")
        .items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, RunState::Stopped);
    assert!(!items[0].ready);
}
