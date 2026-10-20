mod support;
use devbox_core::{Env, Instance, Paths};
use devbox_projects::AddProject;
use devbox_runtime::*;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use support::runtime::RuntimeFixture;

#[tokio::test]
#[ignore = "needs systemd user"]
async fn transient_paths_preserve_percent_spaces_quotes_and_instance_identity() {
    let mut fixture = RuntimeFixture::new();
    fixture.daemon.stop();
    let home = fixture.daemon.home.path().join("home %h \"quoted\"");
    let runtime = home.join("runtime %t space");
    std::fs::create_dir_all(&runtime).expect("owned runtime");
    struct E(PathBuf, PathBuf);
    impl Env for E {
        fn var(&self, key: &str) -> Option<String> {
            match key {
                "HOME" => Some(self.0.display().to_string()),
                "XDG_RUNTIME_DIR" => Some(self.1.display().to_string()),
                _ => None,
            }
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = Paths::resolve(
        Instance::parse(&fixture.daemon.instance).expect("instance"),
        &E(home.clone(), runtime.clone()),
    )
    .expect("paths");
    let services = devbox_cli::daemon::build_services(paths.clone()).expect("owned services");
    std::fs::write(
        paths.instance_config_dir.join("env"),
        devbox_cli::setup::capture_env(b"PATH=/usr/bin:/bin\0LANG=C.UTF-8\0"),
    )
    .expect("captured env");
    let executable = home.join("devbox %i \"binary\"");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_devbox"), &executable)
        .expect("owned executable link");
    let repo = home.join("project %h \"cwd\"");
    std::fs::create_dir(&repo).expect("owned cwd");
    let project = services
        .projects
        .add(AddProject {
            path: repo.display().to_string(),
        })
        .await
        .expect("project")
        .id;
    let mut config = ExecutionConfig::new(paths.clone());
    config.executable = executable;
    config.slice = Some(fixture.slice.clone());
    let runs = Arc::new(
        RuntimeService::new(
            services.db.clone(),
            services.hub.clone(),
            services.projects.clone(),
        )
        .with_execution(config),
    );
    runs.define(DefineRun { project_id: project.clone(), save_to: SaveTo::ThisPc, def: RunDef::Task(TaskDef {
        id: "identity".into(), command: "printf '%s\\n' \"$HOME\" \"$XDG_CONFIG_HOME\" \"$XDG_DATA_HOME\" \"$XDG_STATE_HOME\" \"$XDG_RUNTIME_DIR\" \"$DEVBOX_INSTANCE\" \"$LITERAL\"; pwd".into(), env: [("LITERAL".into(), "%h space \"quote\"".into())].into(), ..Default::default()
    }) }).await.expect("define");
    let started = runs
        .start(StartRun {
            project_id: project.clone(),
            id: "identity".into(),
            worktree: None,
        })
        .await
        .expect("literal transient paths");
    let store =
        exit_record::ExitStore::new(&paths.data_dir.join("runs"), &started.id).expect("store");
    let deadline = Instant::now() + Duration::from_secs(5);
    let exit = loop {
        if let Some(exit) = store.read().expect("exit") {
            break exit;
        }
        assert!(Instant::now() < deadline, "owned exit deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(exit.code, Some(0));
    runs.exited(exit_record::Exited {
        run_id: started.id.clone(),
        attempt_id: exit.attempt_id,
    })
    .await
    .expect("reconcile canonical exit");
    let log = std::fs::read_to_string(store.log_path()).expect("owned log");
    for literal in [
        home.display().to_string(),
        home.join(".config").display().to_string(),
        home.join(".local/share").display().to_string(),
        home.join(".local/state").display().to_string(),
        runtime.display().to_string(),
        fixture.daemon.instance.clone(),
        "%h space \"quote\"".into(),
        repo.display().to_string(),
    ] {
        assert!(
            log.contains(&format!("\to\t{literal}\n")),
            "missing literal output {literal:?}"
        );
    }
    assert!(!home.join(".local/share/devbox/prod").exists());
    let run = runs
        .runs(RunsQuery {
            project_id: Some(project.clone()),
            limit: 100,
        })
        .await
        .expect("runs")
        .items
        .remove(0);
    assert_eq!(run.state, RunState::Exited);
    runs.define(DefineRun {
        project_id: project.clone(),
        save_to: SaveTo::ThisPc,
        def: RunDef::Service(ServiceDef {
            id: "fallback".into(),
            command: "sleep 60".into(),
            ..Default::default()
        }),
    })
    .await
    .expect("define fallback");
    let active = runs
        .start(StartRun {
            project_id: project,
            id: "fallback".into(),
            worktree: None,
        })
        .await
        .expect("owned fallback service");
    assert!(
        active
            .unit
            .starts_with(&format!("devbox-{}-run-", fixture.daemon.instance))
    );
    assert!(active.pid.is_some());
    devbox_core::systemd::command(
        "systemctl",
        &[
            "--user",
            "kill",
            "--kill-whom=main",
            "--signal=KILL",
            "--",
            &active.unit,
        ],
    )
    .await
    .expect("kill exact owned main");
    let store = exit_record::ExitStore::new(&paths.data_dir.join("runs"), &active.id)
        .expect("fallback store");
    let deadline = Instant::now() + Duration::from_secs(5);
    let exit = loop {
        if let Some(exit) = store.read().expect("fallback exit") {
            break exit;
        }
        assert!(Instant::now() < deadline, "stop-post literal path deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(exit.by, exit_record::ExitSource::Systemd);
    assert_eq!(exit.signal.as_deref(), Some("KILL"));
    assert_eq!(Some(exit.attempt_id.as_str()), active.attempt_id.as_deref());
    runs.exited(exit_record::Exited {
        run_id: active.id,
        attempt_id: exit.attempt_id,
    })
    .await
    .expect("fallback reconcile");
}
