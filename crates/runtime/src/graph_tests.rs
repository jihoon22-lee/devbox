use super::*;

#[tokio::test]
async fn graph_port_selection_preserves_explicit_hints_and_allocates_distinct_defaults() {
    let f = Fixture::new().await;
    f.define("first", "true", SaveTo::ThisPc).await;
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Service(ServiceDef {
                id: "second".into(),
                command: "true".into(),
                depends_on: vec!["first".into()],
                ..Default::default()
            }),
        })
        .await
        .expect("define");
    let p = StartRun {
        project_id: f.project.clone(),
        id: "second".into(),
        worktree: Some("main".into()),
    };
    let prepared = f.runs.authorize_run_graph(&p).await.expect("prepared");
    let ports: Vec<u16> = prepared
        .nodes
        .iter()
        .map(|def| {
            match def {
                RunDef::Task(d) => &d.env,
                RunDef::Service(d) => &d.env,
                _ => unreachable!(),
            }
            .get("DEVBOX_PORT")
            .expect("resolved port")
            .parse()
            .expect("port")
        })
        .collect();
    assert!(ports.iter().all(|p| *p > 0));
    assert_ne!(ports[0], ports[1]);
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Service(ServiceDef {
                id: "second".into(),
                command: "true".into(),
                ports_hint: vec![31234],
                env: [("DEVBOX_PORT".into(), "31235".into())].into(),
                ..Default::default()
            }),
        })
        .await
        .expect("define");
    assert!(matches!(
        f.runs.authorize_run_graph(&p).await,
        Err(devbox_protocol::Fail::Domain(
            RuntimeError::DefInvalid { .. }
        ))
    ));
}

#[tokio::test]
async fn graph_rejects_unconnected_windows_and_secret_inputs_before_run_creation() {
    let f = Fixture::new().await;
    for def in [
        TaskDef {
            id: "bad".into(),
            command: "true".into(),
            target: RunTarget::Windows,
            ..Default::default()
        },
        TaskDef {
            id: "bad".into(),
            command: "true".into(),
            secrets: vec!["KEY".into()],
            ..Default::default()
        },
        TaskDef {
            id: "bad".into(),
            command: "echo {{secret:KEY}}".into(),
            ..Default::default()
        },
        TaskDef {
            id: "bad".into(),
            command: "true".into(),
            env: [("HOME".into(), "/foreign".into())].into(),
            ..Default::default()
        },
    ] {
        f.runs
            .define(DefineRun {
                project_id: f.project.clone(),
                save_to: SaveTo::ThisPc,
                def: RunDef::Task(def),
            })
            .await
            .expect("define");
        assert!(matches!(
            f.runs
                .authorize_run_graph(&StartRun {
                    project_id: f.project.clone(),
                    id: "bad".into(),
                    worktree: None
                })
                .await,
            Err(devbox_protocol::Fail::Domain(
                RuntimeError::DefInvalid { .. }
            ))
        ));
    }
    assert!(
        f.runs
            .runs(RunsQuery {
                project_id: None,
                limit: 100
            })
            .await
            .expect("runs")
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn graph_preflights_the_complete_child_argv_before_any_dependency() {
    let f = Fixture::new().await;
    f.define("first", "true", SaveTo::ThisPc).await;
    for (command, env) in [
        (
            "true".into(),
            [("BIG".into(), "x".repeat(64 * 1024))].into(),
        ),
        ("x".repeat(64 * 1024), Default::default()),
    ] {
        f.runs
            .define(DefineRun {
                project_id: f.project.clone(),
                save_to: SaveTo::ThisPc,
                def: RunDef::Task(TaskDef {
                    id: "last".into(),
                    command,
                    env,
                    depends_on: vec!["first".into()],
                    ..Default::default()
                }),
            })
            .await
            .expect("define admitted text");
        assert!(matches!(
            f.runs
                .authorize_run_graph(&StartRun {
                    project_id: f.project.clone(),
                    id: "last".into(),
                    worktree: None
                })
                .await,
            Err(devbox_protocol::Fail::Domain(
                RuntimeError::DefInvalid { .. }
            ))
        ));
        assert!(
            f.runs
                .runs(RunsQuery {
                    project_id: None,
                    limit: 100
                })
                .await
                .expect("runs")
                .items
                .is_empty()
        );
    }
}

#[tokio::test]
async fn graph_uses_the_agent_resolver_directory_and_port() {
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
        devbox_core::Instance::parse("test-port").expect("instance"),
        &E(f.root.path().display().to_string()),
    )
    .expect("paths");
    paths.ensure_dirs().expect("fixture dirs");
    std::fs::write(
        paths.instance_config_dir.join("env"),
        "PATH=\"/usr/bin:/bin\"\n",
    )
    .expect("fixture env");
    let mut config = ExecutionConfig::new(paths);
    let expected = f.project.clone();
    let directory = f.root.path().join("repo").display().to_string();
    config.worktree = Some(Arc::new(move |project, agent| {
        assert_eq!(project, expected);
        assert_eq!(agent, "agent-id");
        let path = directory.clone();
        Box::pin(async move { Ok(WorktreeLocation { path, port: 20100 }) })
    }));
    let runs =
        RuntimeService::new(f.db.clone(), f.hub.clone(), f.projects.clone()).with_execution(config);
    runs.define(DefineRun {
        project_id: f.project.clone(),
        save_to: SaveTo::ThisPc,
        def: RunDef::Task(TaskDef {
            id: "agent-task".into(),
            command: "true".into(),
            worktree: Some("agent-id".into()),
            ..Default::default()
        }),
    })
    .await
    .expect("define");
    let prepared = runs
        .authorize_run_graph(&StartRun {
            project_id: f.project.clone(),
            id: "agent-task".into(),
            worktree: None,
        })
        .await
        .expect("prepared");
    let RunDef::Task(def) = &prepared.nodes[0] else {
        panic!("task")
    };
    assert_eq!(
        def.env.get("DEVBOX_PORT").map(String::as_str),
        Some("20100")
    );
    assert_eq!(
        def.cwd.as_deref(),
        Some(f.root.path().join("repo").to_str().expect("path"))
    );
}
