mod support;
use devbox_cli::client::ClientError;
use devbox_core::notify::{Notification, NotificationReason};
use devbox_runtime::*;
use std::{path::Path, time::Duration};
use support::{
    bounded,
    runtime::{RuntimeFixture, file_exists, service, task},
};

#[tokio::test]
#[ignore = "needs systemd user"]
async fn task_runs_to_completion_and_reports_exit() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            task(
                "build",
                "printf 'owned-out\\n'; printf 'owned-err\\n' >&2; exit 0",
            ),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "build").await;
    let run = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(run.exit.as_ref().and_then(|exit| exit.code), Some(0));
    assert_eq!(run.restarts, 0);
    assert!(run.ended_ms.is_some());
    assert!(run.attempt_id.is_some());
    let log = std::fs::read_to_string(
        fixture
            .daemon
            .paths
            .data_dir
            .join("runs")
            .join(format!("{}.log", run.id)),
    )
    .expect("owned log");
    assert!(log.contains("\to\towned-out\n"));
    assert!(log.contains("\te\towned-err\n"));
    assert!(!fixture.daemon.paths.config_dir.join("prod").exists());
    assert!(
        !fixture
            .daemon
            .home
            .path()
            .join(".local/share/devbox/prod")
            .exists()
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn restart_no_service_is_not_restarted_after_exit() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            service("once", "exit 1", RestartPolicy::No),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "once").await;
    let failed = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(5),
        )
        .await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let run = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(1),
        )
        .await;
    assert_eq!(run.restarts, 0);
    assert_eq!(run.attempt_id, failed.attempt_id);
    let properties = fixture.show(&run.unit);
    assert_eq!(properties.get("NRestarts").map(String::as_str), Some("0"));
    assert_eq!(
        properties.get("ActiveState").map(String::as_str),
        Some("failed")
    );
    assert_eq!(run.exit.as_ref().and_then(|exit| exit.code), Some(1));
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn service_restarts_on_failure_until_start_limit_then_fails_once() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    let mut alerts = bounded(client.subscribe("notify.raised"))
        .await
        .expect("subscribe notifications");
    fixture
        .define(
            &client,
            &project,
            service("crash", "exit 1", RestartPolicy::OnFailure),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "crash").await;
    fixture.start_limit(&started).await;
    // Unit failure can precede the daemon's required ten-second status observation.
    let failed = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(12),
        )
        .await;
    assert!(failed.restarts >= 2);
    let properties = fixture.show(&failed.unit);
    assert!(matches!(
        properties.get("Result").map(String::as_str),
        Some("start-limit-hit" | "exit-code")
    ));
    assert_eq!(
        properties
            .get("NRestarts")
            .and_then(|value| value.parse::<u32>().ok()),
        Some(failed.restarts)
    );
    let event = bounded(alerts.next_event())
        .await
        .expect("start-limit event");
    let notification: Notification =
        serde_json::from_str(&event.payload).expect("typed notification");
    assert_eq!(notification.run_id, failed.id);
    assert!(matches!(
        notification.reason,
        NotificationReason::StartLimit
    ));
    for _ in 0..2 {
        bounded(client.call::<RuntimeExited>(&exit_record::Exited {
            run_id: failed.id.clone(),
            attempt_id: failed.attempt_id.clone().expect("attempt"),
        }))
        .await
        .expect("duplicate exit event");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(400), alerts.next_event())
            .await
            .is_err(),
        "duplicate start-limit alert"
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn manual_restart_resets_the_start_limit() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            service("recover", "exit 1", RestartPolicy::OnFailure),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "recover").await;
    fixture.start_limit(&started).await;
    let failed = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(12),
        )
        .await;
    fixture
        .define(
            &client,
            &project,
            service("recover", "sleep 60", RestartPolicy::OnFailure),
            SaveTo::ThisPc,
        )
        .await;
    let restarted = bounded(client.call::<RuntimeRestart>(&RunId {
        run_id: failed.id.clone(),
    }))
    .await
    .expect("manual restart");
    let run = fixture
        .wait(
            &client,
            &project,
            &restarted.id,
            RunState::Running,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(run.id, failed.id);
    assert_ne!(run.attempt_id, failed.attempt_id);
    assert_eq!(run.restarts, 0);
    assert!(run.pid.is_some());
    assert!(run.exit.is_none());
    let properties = fixture.show(&run.unit);
    assert_eq!(
        properties.get("Result").map(String::as_str),
        Some("success")
    );
    assert_eq!(properties.get("Slice"), Some(&fixture.slice));
    assert_eq!(properties.get("Nice").map(String::as_str), Some("10"));
    let stopped = bounded(client.call::<RuntimeStop>(&RunId { run_id: run.id }))
        .await
        .expect("stop restarted run");
    assert_eq!(stopped.state, RunState::Stopped);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn daemon_restart_reconciles_finished_and_running_runs() {
    let mut fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(&client, &project, task("live", "sleep 60"), SaveTo::ThisPc)
        .await;
    fixture
        .define(
            &client,
            &project,
            task(
                "finish",
                "while [ ! -f release ]; do sleep 0.05; done; printf 'finished\\n'",
            ),
            SaveTo::ThisPc,
        )
        .await;
    let live = fixture.start(&client, &project, "live").await;
    let finish = fixture.start(&client, &project, "finish").await;
    let live = fixture
        .wait(
            &client,
            &project,
            &live.id,
            RunState::Running,
            Duration::from_secs(5),
        )
        .await;
    fixture.daemon.stop();
    drop(client);
    std::fs::write(fixture.daemon.home.path().join("project/release"), "owned")
        .expect("release owned task while daemon absent");
    file_exists(
        &fixture
            .daemon
            .paths
            .data_dir
            .join("runs")
            .join(format!("{}.exit", finish.id)),
    )
    .await;
    fixture.daemon.launch();
    let client = fixture.daemon.client().await;
    let recovered_live = fixture
        .wait(
            &client,
            &project,
            &live.id,
            RunState::Running,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(recovered_live.attempt_id, live.attempt_id);
    assert_eq!(recovered_live.pid, live.pid);
    let recovered_exit = fixture
        .wait(
            &client,
            &project,
            &finish.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        recovered_exit.exit.as_ref().and_then(|exit| exit.code),
        Some(0)
    );
    assert_eq!(fixture.runs(&client, &project).await.len(), 2);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn depends_on_runs_in_order() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            task("first", "printf 'first\\n' >> order"),
            SaveTo::ThisPc,
        )
        .await;
    let mut second = task("second", "printf 'second\\n' >> order");
    let RunDef::Task(def) = &mut second else {
        unreachable!()
    };
    def.depends_on.push("first".into());
    fixture
        .define(&client, &project, second, SaveTo::ThisPc)
        .await;
    let started = fixture.start(&client, &project, "second").await;
    fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(
        std::fs::read_to_string(fixture.daemon.home.path().join("project/order"))
            .expect("execution order"),
        "first\nsecond\n"
    );
    let runs = fixture.runs(&client, &project).await;
    let first = runs
        .iter()
        .find(|run| run.def_id == "first")
        .expect("first run");
    let second = runs
        .iter()
        .find(|run| run.def_id == "second")
        .expect("second run");
    assert!(first.ended_ms.expect("first ended") <= second.started_ms);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn depends_on_stops_after_failure() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(&client, &project, task("broken", "exit 7"), SaveTo::ThisPc)
        .await;
    let mut after = task("after", "touch must-not-exist");
    let RunDef::Task(def) = &mut after else {
        unreachable!()
    };
    def.depends_on.push("broken".into());
    fixture
        .define(&client, &project, after, SaveTo::ThisPc)
        .await;
    let error = bounded(client.call::<RuntimeStart>(&StartRun {
        project_id: project.clone(),
        id: "after".into(),
        worktree: None,
    }))
    .await
    .expect_err("failed dependency");
    assert!(matches!(error, ClientError::Rpc { code, .. } if code == "runtime.dependency_failed"));
    let runs = fixture.runs(&client, &project).await;
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].def_id, "broken");
    assert_eq!(runs[0].exit.as_ref().and_then(|exit| exit.code), Some(7));
    assert!(
        !fixture
            .daemon
            .home
            .path()
            .join("project/must-not-exist")
            .exists()
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn start_requires_current_trust_and_creates_no_unit_when_stale() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            task("trusted", "touch approved"),
            SaveTo::Project,
        )
        .await;
    let config = fixture
        .daemon
        .home
        .path()
        .join("project/.devbox/devbox.toml");
    std::fs::write(
        &config,
        "[[task]]\nid='trusted'\ncommand='touch newly-approved'\n",
    )
    .expect("external edit stales trust");
    let error = bounded(client.call::<RuntimeStart>(&StartRun {
        project_id: project.clone(),
        id: "trusted".into(),
        worktree: None,
    }))
    .await
    .expect_err("stale trust");
    assert!(matches!(error, ClientError::Rpc { code, .. } if code == "projects.trust_required"));
    assert!(fixture.units().is_empty());
    assert!(fixture.runs(&client, &project).await.is_empty());
    for file in ["approved", "newly-approved"] {
        assert!(
            !fixture
                .daemon
                .home
                .path()
                .join(Path::new("project").join(file))
                .exists()
        );
    }
    fixture.approve(&client, &project).await;
    let started = fixture.start(&client, &project, "trusted").await;
    fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    assert!(
        fixture
            .daemon
            .home
            .path()
            .join("project/newly-approved")
            .is_file()
    );
    assert!(!fixture.daemon.home.path().join("project/approved").exists());
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn killed_wrapper_uses_stop_post_current_attempt_and_reconciles_failed() {
    use exit_record::{ExitRecord, ExitSource};
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            service(
                "kill-wrapper",
                "printf 'ready\\n'; sleep 60",
                RestartPolicy::No,
            ),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "kill-wrapper").await;
    let running = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Running,
            Duration::from_secs(5),
        )
        .await;
    let run_dir = fixture.daemon.paths.data_dir.join("runs");
    let attempt_file = run_dir.join(format!("{}.attempt", running.id));
    file_exists(&attempt_file).await;
    let attempt = std::fs::read_to_string(&attempt_file).expect("current wrapper attempt");
    assert_eq!(running.attempt_id.as_deref(), Some(attempt.as_str()));
    fixture.kill_main(&running);
    let failed = fixture
        .wait(
            &client,
            &project,
            &running.id,
            RunState::Failed,
            Duration::from_secs(5),
        )
        .await;
    let record: ExitRecord = serde_json::from_slice(
        &std::fs::read(run_dir.join(format!("{}.exit", running.id))).expect("fallback exit file"),
    )
    .expect("typed fallback exit");
    assert_eq!(record.by, ExitSource::Systemd);
    assert_eq!(record.signal.as_deref(), Some("KILL"));
    assert_eq!(record.code, None);
    assert_eq!(record.attempt_id, attempt);
    assert_eq!(failed.attempt_id.as_deref(), Some(attempt.as_str()));
    assert_eq!(failed.exit, Some(record));
    assert_eq!(failed.restarts, 0);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn definition_environment_overrides_captured_environment_in_the_child() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    let def = RunDef::Task(TaskDef {
        id: "env".into(),
        command: "printf '%s|%s|%s|%s' \"$LANG\" \"$PATH\" \"$HOME\" \"$XDG_RUNTIME_DIR\"".into(),
        env: [
            ("LANG".into(), "C".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]
        .into(),
        ..Default::default()
    });
    fixture.define(&client, &project, def, SaveTo::ThisPc).await;
    let started = fixture.start(&client, &project, "env").await;
    let run = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    let log = std::fs::read_to_string(
        fixture
            .daemon
            .paths
            .data_dir
            .join("runs")
            .join(format!("{}.log", run.id)),
    )
    .expect("owned log");
    assert!(log.contains(&format!(
            "C|/usr/bin:/bin|{}|{}",
            fixture.daemon.paths.home.display(),
            fixture
                .daemon
                .paths
                .socket
                .parent()
                .expect("runtime")
                .display()
        )));
}
