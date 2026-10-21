use super::*;
use devbox_protocol::windows::*;
use devbox_runtime::exit_record::{ExitStore, now_ms};
use std::path::Path;
async fn insert(f: &RuntimeFixture, project: &str, id: &str, def: &RunDef) -> Run {
    let db = devbox_core::Db::open(
        &f.daemon.paths.db_path,
        &f.daemon.paths.backups_dir,
        env!("CARGO_PKG_VERSION"),
        &[
            devbox_projects::MIGRATIONS,
            devbox_terminal::MIGRATIONS,
            devbox_agents::MIGRATIONS,
            devbox_runtime::MIGRATIONS,
            devbox_secrets::MIGRATIONS,
        ]
        .concat(),
    )
    .expect("fixture result");
    let run = Run {
        id: id.into(),
        project_id: project.into(),
        def_id: def.id().into(),
        kind: RunKind::Service,
        state: RunState::Starting,
        exit: None,
        started_ms: now_ms(),
        ended_ms: None,
        restarts: 0,
        unit: launch::unit_name(id, &f.daemon.paths),
        ready: false,
        pid: None,
        port: None,
        attempt_id: None,
        windows: Some(WindowsRun {
            directory: "C:\\owned copy's\\repo".into(),
            notice: windows::EXECUTION_NOTICE.into(),
        }),
    };
    let body=serde_json::json!({"run":run,"secret_definition":def,"windows":{"cwd":"C:\\owned copy's\\repo","def":def},"schedule":null,"restart":"onFailure","start_limit":{"burst":3,"intervalSec":30},"worktree":null,"start_limit_notified":false}).to_string();
    let row = run.clone();
    db.write(move |tx| {
        tx.execute(
            "INSERT INTO runtime_runs(id,project_id,started_ms,body) VALUES(?1,?2,?3,?4)",
            (&row.id, &row.project_id, row.started_ms, &body),
        )?;
        Ok(())
    })
    .await
    .expect("fixture result");
    run
}
fn definition(f: &RuntimeFixture, id: &str) -> RunDef {
    let mut def = secret_service(id, "Write-Output $env:TOKEN");
    if let RunDef::Service(d) = &mut def {
        d.target = RunTarget::Windows;
        d.cwd = Some(f.daemon.home.path().display().to_string());
        d.ready_timeout_sec = 5;
        d.ports_hint = vec![23456];
    }
    def
}
async fn launch(f: &RuntimeFixture, run: &Run, def: &RunDef) {
    let args = launch::unit_args_with_executable(
        def,
        &run.id,
        &f.daemon.paths,
        Some(&f.slice),
        Path::new(env!("CARGO_BIN_EXE_devbox")),
    );
    devbox_core::systemd::Systemd
        .run_transient(&args)
        .await
        .expect("fixture result");
}
async fn next(app: &Client) -> RunWindows {
    let raw = bounded(app.raw_call("windows.next", serde_json::json!({}), None))
        .await
        .expect("fixture result");
    serde_json::from_str(&raw).expect("fixture result")
}
async fn event(app: &Client, r: &RunWindows, event: WindowsOutput) {
    app.raw_call(
        "windows.output",
        WindowsEvent {
            run_id: r.run_id.clone(),
            attempt_id: r.attempt_id.clone(),
            event,
        },
        None,
    )
    .await
    .expect("fixture result");
}
async fn finish(f: &RuntimeFixture, run: &Run) -> devbox_runtime::exit_record::ExitRecord {
    let store =
        ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &run.id).expect("fixture result");
    bounded(async {
        loop {
            if let Some(exit) = store.read().expect("fixture result") {
                return exit;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
}
#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn windows_native_worker_preserves_logs_secrets_ready_exit_and_restart_policy() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let def = definition(&f, "native");
    let run = insert(&f, &project, "windows-known", &def).await;
    let native = next(&app);
    tokio::pin!(native);
    // Polling is deliberately concurrent with the systemd worker start.
    let (_, request) = tokio::join!(
        async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            launch(&f, &run, &def).await
        },
        &mut native
    );
    assert_eq!(request.run_id, run.id);
    assert_eq!(request.env["TOKEN"], SECRET);
    assert_eq!(request.cwd, "C:\\owned copy's\\repo");
    assert_eq!(
        f.runs(&client, &project)
            .await
            .into_iter()
            .find(|r| r.id == run.id)
            .expect("fixture value")
            .attempt_id,
        None,
        "first Ready precedes database observation"
    );
    event(&app, &request, WindowsOutput::Ready).await;
    bounded(async {
        loop {
            if f.runs(&client, &project)
                .await
                .iter()
                .any(|r| r.id == run.id && r.ready)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    event(
        &app,
        &request,
        WindowsOutput::Stdout {
            data: format!("OUT {}", &SECRET[..8]).into_bytes(),
        },
    )
    .await;
    event(
        &app,
        &request,
        WindowsOutput::Stdout {
            data: format!("{}\n", &SECRET[8..]).into_bytes(),
        },
    )
    .await;
    event(
        &app,
        &request,
        WindowsOutput::Stderr {
            data: format!("ERR {SECRET}\n").into_bytes(),
        },
    )
    .await;
    event(&app, &request, WindowsOutput::Finished { code: 78 }).await;
    let exit = finish(&f, &run).await;
    assert_eq!(exit.code, Some(78));
    assert_eq!(exit.signal, None);
    let next_request = next(&app);
    tokio::pin!(next_request);
    let request2 = bounded(&mut next_request).await;
    assert_eq!(request2.run_id, run.id);
    assert_ne!(request2.attempt_id, request.attempt_id);
    assert!(
        f.show(&run.unit)["NRestarts"]
            .parse::<u32>()
            .expect("fixture result")
            >= 1
    );
    let worker = Client::connect(&f.daemon.socket, ClientKind::Cli, &f.daemon.instance)
        .await
        .expect("fixture result");
    assert!(
        worker
            .raw_call(
                "windows.ready",
                RunIdentity {
                    run_id: run.id.clone(),
                    attempt_id: request.attempt_id.clone()
                },
                None
            )
            .await
            .is_err(),
        "old Ready cannot authorize the new attempt"
    );
    event(&app, &request2, WindowsOutput::Finished { code: 0 }).await;
    bounded(async {
        loop {
            let rows = f.runs(&client, &project).await;
            if rows
                .iter()
                .any(|r| r.id == run.id && r.state == RunState::Exited)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    let log = std::fs::read_to_string(
        ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &run.id)
            .expect("fixture result")
            .log_path(),
    )
    .expect("fixture result");
    assert!(!log.contains(SECRET));
    assert!(log.contains("[REDACTED]"));
    assert!(log.lines().any(|line| line.contains("\to\tOUT [REDACTED]")));
    assert!(log.lines().any(|line| line.contains("\te\tERR [REDACTED]")));
    assert!(log.contains(&devbox_core::run_log::attempt_marker(&request.attempt_id)));
    let rows = f.runs(&client, &project).await;
    assert!(
        !serde_json::to_string(&rows)
            .expect("fixture result")
            .contains(SECRET)
    );
    assert!(!format!("{:?}", f.show(&run.unit)).contains(SECRET));
    server.stop().await;
}
#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn windows_native_disconnect_is_unknown_and_does_not_restart_or_duplicate() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let def = definition(&f, "unknown");
    let run = insert(&f, &project, "windows-unknown", &def).await;
    let request = {
        let native = next(&app);
        tokio::pin!(native);
        tokio::join!(
            async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                launch(&f, &run, &def).await
            },
            &mut native
        )
        .1
    };
    drop(app);
    let exit = finish(&f, &run).await;
    assert_eq!(exit.code, None);
    assert_eq!(exit.signal.as_deref(), Some("USR2"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let props = f.show(&run.unit);
    assert_eq!(props["NRestarts"], "0");
    assert_eq!(props["ActiveState"], "failed");
    let store =
        ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &run.id).expect("fixture result");
    assert!(store.claim_windows_native(&request.attempt_id).is_err());
    server.stop().await;
}

#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn windows_direct_worker_failure_does_not_fallback_and_preserves_masked_streams() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let executable = f.daemon.home.path().join("absent-windows-interop");
    std::fs::write(&executable, "#!/usr/bin/python3\nimport json,sys\nr=json.load(sys.stdin)\nassert r['cwd']==\"C:\\\\owned copy's\\\\repo\"\nassert r['env']['TOKEN']=='owned-fixture-token-42'\nassert not any(r['env']['TOKEN'] in a for a in sys.argv)\nprint('DIRECT OUT '+r['env']['TOKEN'],flush=True)\nprint('DIRECT ERR '+r['env']['TOKEN'],file=sys.stderr,flush=True)\nsys.exit(9)\n").expect("fixture result");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fixture result");
    let server = Server::start(&f).await;
    let app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let mut def = definition(&f, "direct");
    if let RunDef::Service(d) = &mut def {
        d.restart = RestartPolicy::No;
        d.ports_hint.clear();
    }
    let run = insert(&f, &project, "windows-direct", &def).await;
    let poll = app.raw_call("windows.next", serde_json::json!({}), None);
    tokio::pin!(poll);
    let (reply, _) = tokio::join!(
        tokio::time::timeout(Duration::from_millis(500), &mut poll),
        async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            launch(&f, &run, &def).await
        }
    );
    assert!(
        reply.is_err(),
        "a started command failure must not be delegated"
    );
    let exit = finish(&f, &run).await;
    assert_eq!(exit.code, Some(9));
    assert_eq!(exit.signal, None);
    let log = std::fs::read_to_string(
        ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &run.id)
            .expect("fixture result")
            .log_path(),
    )
    .expect("fixture result");
    assert!(log.contains("\to\tDIRECT OUT [REDACTED]"));
    assert!(log.contains("\te\tDIRECT ERR [REDACTED]"));
    assert!(!log.contains(SECRET));
    assert_eq!(f.show(&run.unit)["NRestarts"], "0");
    server.stop().await;
}

#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn windows_quiet_stop_cancels_matching_native_child_and_keeps_other_run_alive() {
    use std::{collections::HashMap, os::unix::fs::PermissionsExt, sync::Mutex};
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let app = Arc::new(bootstrap(&f).await);
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let fake = f.daemon.home.path().join("owned-quiet-native");
    std::fs::write(&fake,"#!/usr/bin/python3\nimport os,sys,json,time\nr=json.load(sys.stdin)\nopen(r['env']['OWNED_PID_FILE'],'w').write(str(os.getpid()))\ntime.sleep(60)\n").expect("fixture result");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700))
        .expect("fixture result");
    let tokens = Arc::new(Mutex::new(HashMap::new()));
    let (output, mut events) = tokio::sync::mpsc::channel::<WindowsEvent>(16);
    let proxy_app = app.clone();
    let proxy_tokens = tokens.clone();
    let proxy = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let key = (event.run_id.clone(), event.attempt_id.clone());
            if proxy_app
                .raw_call("windows.output", event, None)
                .await
                .is_err()
            {
                native_window_protocol::finish_run(
                    &mut proxy_tokens.lock().expect("fixture result"),
                    &key,
                );
            }
        }
    });
    let mut rows = Vec::new();
    let mut children = Vec::new();
    for id in ["quiet-first", "quiet-other"] {
        let pidfile = f.daemon.home.path().join(format!("{id}.pid"));
        let mut def = definition(&f, id);
        if let RunDef::Service(d) = &mut def {
            d.ports_hint.clear();
            d.env
                .insert("OWNED_PID_FILE".into(), pidfile.display().to_string());
        }
        let run = insert(&f, &project, id, &def).await;
        let request = {
            let poll = next(&app);
            tokio::pin!(poll);
            tokio::join!(
                async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    launch(&f, &run, &def).await
                },
                &mut poll
            )
            .1
        };
        let key = (request.run_id.clone(), request.attempt_id.clone());
        let token = CancellationToken::new();
        tokens
            .lock()
            .expect("fixture result")
            .insert(key, token.clone());
        let executable = fake.clone();
        let sender = output.clone();
        children.push(tokio::spawn(async move {
            native_window_protocol::platform_compile::run_with_executable(
                request,
                sender,
                token,
                executable.as_os_str(),
            )
            .await
        }));
        let pid = bounded(async {
            loop {
                if let Ok(text) = std::fs::read_to_string(&pidfile)
                    && let Ok(pid) = text.parse::<u32>()
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        eprintln!("owned quiet native pid={pid} run={}", run.id);
        rows.push((run, pid));
    }
    let started = std::time::Instant::now();
    client
        .call::<RuntimeStop>(&RunId {
            run_id: rows[0].0.id.clone(),
        })
        .await
        .expect("fixture result");
    tokio::time::timeout(Duration::from_secs(4), async {
        while Path::new(&format!("/proc/{}", rows[0].1)).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("quiet native child is reaped after ACK failure");
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(
        Path::new(&format!("/proc/{}", rows[1].1)).exists(),
        "unrelated native child remains alive"
    );
    assert_eq!(f.show(&rows[1].0.unit)["ActiveState"], "active");
    assert_eq!(tokens.lock().expect("fixture result").len(), 1);
    let log = std::fs::read_to_string(
        ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &rows[1].0.id)
            .expect("fixture result")
            .log_path(),
    )
    .expect("fixture result");
    assert!(
        log.lines().all(|line| line.contains("\ts\t")),
        "heartbeat never becomes stdout/stderr log data"
    );
    assert!(
        !f.runs(&client, &project)
            .await
            .into_iter()
            .find(|r| r.id == rows[1].0.id)
            .expect("fixture value")
            .ready,
        "heartbeat does not mark readiness"
    );
    client
        .call::<RuntimeStop>(&RunId {
            run_id: rows[1].0.id.clone(),
        })
        .await
        .expect("fixture result");
    for child in children {
        bounded(child).await.expect("fixture result");
    }
    drop(output);
    bounded(proxy).await.expect("fixture result");
    assert!(!Path::new(&format!("/proc/{}", rows[1].1)).exists());
    server.stop().await;
}
