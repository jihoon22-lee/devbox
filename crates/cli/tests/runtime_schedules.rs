mod support;
use devbox_core::notify::{Notification, NotificationReason};
use devbox_runtime::*;
use std::time::{Duration, Instant};
use support::{bounded, runtime::RuntimeFixture};

async fn schedule(
    f: &RuntimeFixture,
    client: &devbox_cli::client::Client,
    project: &str,
    cron: &str,
) {
    f.define(
        client,
        project,
        RunDef::Schedule(ScheduleDef {
            id: "scheduled".into(),
            task: "task".into(),
            cron: cron.into(),
            tz: Some("UTC".into()),
            missed: MissedPolicy::Skip,
        }),
        SaveTo::ThisPc,
    )
    .await;
}
async fn await_scheduled(
    f: &RuntimeFixture,
    client: &devbox_cli::client::Client,
    project: &str,
    except: Option<&str>,
) -> Run {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(run) = f
            .runs(client, project)
            .await
            .into_iter()
            .find(|r| r.kind == RunKind::Scheduled && Some(r.id.as_str()) != except)
        {
            return f
                .wait(
                    client,
                    project,
                    &run.id,
                    RunState::Running,
                    Duration::from_secs(5),
                )
                .await;
        }
        assert!(
            Instant::now() < deadline,
            "production scheduler did not start task"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[tokio::test]
#[ignore = "needs systemd user"]
async fn scheduled_rpc_async_failure_and_graceful_shutdown_preserve_owned_run() {
    let mut f = RuntimeFixture::new();
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    f.define(
        &client,
        &project,
        RunDef::Task(TaskDef {
            id: "task".into(),
            command: "while [ ! -f release ]; do sleep 0.05; done; exit 7".into(),
            ..Default::default()
        }),
        SaveTo::ThisPc,
    )
    .await;
    let mut alerts = bounded(client.subscribe("notify.raised")).await.unwrap();
    schedule(&f, &client, &project, "* * * * * *").await;
    let started = await_scheduled(&f, &client, &project, None).await;
    schedule(&f, &client, &project, "0 0 0 1 1 *").await;
    let status = bounded(client.call::<RuntimeSchedules>(&SchedulesQuery {
        project_id: project.clone(),
    }))
    .await
    .unwrap();
    assert_eq!(status.items.len(), 1);
    assert!(status.items[0].next.is_some());
    assert_eq!(
        status.items[0].last.as_ref().unwrap().result,
        ScheduleOutcome::Running
    );
    std::fs::write(f.daemon.home.path().join("project/release"), "owned").unwrap();
    let event = bounded(alerts.next_event()).await.unwrap();
    let alert: Notification = serde_json::from_str(&event.payload).unwrap();
    assert!(matches!(alert.reason, NotificationReason::ScheduleFailed));
    assert_eq!(alert.project_id, project);
    assert_eq!(alert.task.as_deref(), Some("task"));
    assert_eq!(alert.run_id.as_deref(), Some(started.id.as_str()));
    let failed = f
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(5),
        )
        .await;
    for _ in 0..2 {
        bounded(client.call::<RuntimeExited>(&exit_record::Exited {
            run_id: failed.id.clone(),
            attempt_id: failed.attempt_id.clone().unwrap(),
        }))
        .await
        .unwrap();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), alerts.next_event())
            .await
            .is_err()
    );
    let status = bounded(client.call::<RuntimeSchedules>(&SchedulesQuery {
        project_id: project.clone(),
    }))
    .await
    .unwrap();
    assert_eq!(
        status.items[0].last.as_ref().unwrap().result,
        ScheduleOutcome::Failed
    );

    // A second already-started unit survives the daemon's real graceful scheduler shutdown.
    std::fs::remove_file(f.daemon.home.path().join("project/release")).unwrap();
    schedule(&f, &client, &project, "* * * * * *").await;
    let survivor = await_scheduled(&f, &client, &project, Some(&failed.id)).await;
    schedule(&f, &client, &project, "0 0 0 1 1 *").await;
    let mut child = f.daemon.child.take().unwrap();
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "graceful daemon shutdown blocked on scheduled execution"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        f.show(&survivor.unit)
            .get("ActiveState")
            .map(String::as_str),
        Some("active")
    );
    drop(client);
    drop(child);
    f.daemon.launch();
    let client = f.daemon.client().await;
    let recovered = f
        .runs(&client, &project)
        .await
        .into_iter()
        .find(|r| r.id == survivor.id)
        .unwrap();
    assert_eq!(recovered.kind, RunKind::Scheduled);
    assert_eq!(recovered.state, RunState::Running);
    let mut stopped_alerts = bounded(client.subscribe("notify.raised")).await.unwrap();
    let stopped = bounded(client.call::<RuntimeStop>(&RunId {
        run_id: survivor.id.clone(),
    }))
    .await
    .unwrap();
    assert_eq!(stopped.state, RunState::Stopped);
    for _ in 0..2 {
        bounded(client.call::<RuntimeExited>(&exit_record::Exited {
            run_id: stopped.id.clone(),
            attempt_id: stopped.attempt_id.clone().unwrap(),
        }))
        .await
        .unwrap();
    }
    let status = bounded(client.call::<RuntimeSchedules>(&SchedulesQuery {
        project_id: project,
    }))
    .await
    .unwrap();
    assert_eq!(
        status.items[0].last.as_ref().unwrap().result,
        ScheduleOutcome::Stopped
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stopped_alerts.next_event())
            .await
            .is_err(),
        "manual stop must not raise schedule_failed"
    );
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn unchanged_approval_schedule_edit_does_not_renotify_trust() {
    let f = RuntimeFixture::new();
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let config = f.daemon.home.path().join("project/.devbox/devbox.toml");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(config, "[[task]]\nid='task'\ncommand='true'\n").unwrap();
    let mut alerts = bounded(client.subscribe("notify.raised")).await.unwrap();
    for cron in ["* * * * * *", "*/1 * * * * *"] {
        f.define(
            &client,
            &project,
            RunDef::Schedule(ScheduleDef {
                id: "scheduled".into(),
                task: "task".into(),
                cron: cron.into(),
                tz: Some("UTC".into()),
                missed: MissedPolicy::Skip,
            }),
            SaveTo::ThisPc,
        )
        .await;
        let result = tokio::time::timeout(Duration::from_millis(2200), alerts.next_event()).await;
        if cron.starts_with("*/") {
            assert!(
                result.is_err(),
                "same approval identity must not emit another trust alert after cron-only edit: {result:?}"
            );
        } else {
            let event = result.unwrap().unwrap();
            let alert: Notification = serde_json::from_str(&event.payload).unwrap();
            assert!(matches!(
                alert.reason,
                NotificationReason::ScheduleTrustRequired
            ));
        }
    }
}
