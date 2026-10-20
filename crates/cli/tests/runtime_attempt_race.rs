mod support;
use devbox_runtime::{exit_record::ExitStore, *};
use std::time::{Duration, Instant};
use support::{
    bounded,
    runtime::{RuntimeFixture, service},
};

#[tokio::test]
#[ignore = "needs systemd user"]
async fn automatic_restart_ignores_old_exit_and_stop_retires_current_unit() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    // The first invocation fails; the next invocation stays alive in the same unit.
    fixture
        .define(
            &client,
            &project,
            service(
                "attempt-race",
                "if [ ! -f first-attempt ]; then touch first-attempt; exit 1; fi; sleep 60",
                RestartPolicy::OnFailure,
            ),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "attempt-race").await;
    let store = ExitStore::new(&fixture.daemon.paths.data_dir.join("runs"), &started.id)
        .expect("owned exit store");
    let deadline = Instant::now() + Duration::from_secs(5);
    let old_attempt = loop {
        if let Some(record) = store.read().expect("first exit record") {
            assert_eq!(record.code, Some(1));
            assert_eq!(
                store
                    .current_attempt()
                    .expect("first current attempt")
                    .as_deref(),
                Some(record.attempt_id.as_str())
            );
            break record.attempt_id;
        }
        assert!(
            Instant::now() < deadline,
            "first failed attempt was not observable"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let restarted = loop {
        let run = fixture
            .runs(&client, &project)
            .await
            .into_iter()
            .find(|run| run.id == started.id)
            .expect("tracked run");
        if run.state == RunState::Running
            && run.restarts == 1
            && run
                .attempt_id
                .as_ref()
                .is_some_and(|attempt| attempt != &old_attempt)
        {
            break run;
        }
        assert!(
            Instant::now() < deadline,
            "second attempt deadline: {run:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(restarted.pid.is_some());
    assert!(restarted.exit.is_none());
    assert_eq!(
        store.current_attempt().expect("second current attempt"),
        restarted.attempt_id
    );
    assert!(store.read().expect("retired first exit").is_none());
    let before = fixture.show(&restarted.unit);
    assert_eq!(
        before
            .get("MainPID")
            .and_then(|pid| pid.parse::<u32>().ok()),
        restarted.pid
    );
    assert_eq!(before.get("InvocationID"), restarted.attempt_id.as_ref());

    bounded(client.call::<RuntimeExited>(&exit_record::Exited {
        run_id: restarted.id.clone(),
        attempt_id: old_attempt,
    }))
    .await
    .expect("late old-attempt event");
    let after = fixture
        .runs(&client, &project)
        .await
        .into_iter()
        .find(|run| run.id == restarted.id)
        .expect("current run after stale event");
    assert_eq!(after.state, RunState::Running);
    assert_eq!(after.attempt_id, restarted.attempt_id);
    assert_eq!(after.pid, restarted.pid);
    assert_eq!(after.restarts, 1);
    assert!(after.exit.is_none());
    let properties = fixture.show(&after.unit);
    assert_eq!(properties.get("MainPID"), before.get("MainPID"));
    assert_eq!(properties.get("InvocationID"), before.get("InvocationID"));
    assert_eq!(
        properties.get("ActiveState").map(String::as_str),
        Some("active")
    );

    let stopped = bounded(client.call::<RuntimeStop>(&RunId {
        run_id: after.id.clone(),
    }))
    .await
    .expect("stop current attempt");
    assert_eq!(stopped.state, RunState::Stopped);
    assert!(!stopped.ready);
    assert!(stopped.pid.is_none());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let properties = fixture.show(&stopped.unit);
        if properties
            .get("LoadState")
            .is_some_and(|state| state == "not-found")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stopped owned unit was not collected"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let persisted = fixture
        .runs(&client, &project)
        .await
        .into_iter()
        .find(|run| run.id == stopped.id)
        .expect("persisted stopped run");
    assert_eq!(persisted.state, RunState::Stopped);
    assert!(!persisted.ready);
    assert!(persisted.pid.is_none());
}
