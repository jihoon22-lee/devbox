mod support;
use devbox_runtime::*;
use std::time::Duration;
use support::{
    bounded,
    runtime::{RuntimeFixture, service},
};

#[tokio::test]
#[ignore = "needs systemd user"]
async fn daemon_restart_recovers_start_limit_and_persists_notification_deduplication() {
    let mut fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    fixture
        .define(
            &client,
            &project,
            service(
                "offline-crash",
                "while [ ! -f crash ]; do sleep 0.05; done; exit 1",
                RestartPolicy::OnFailure,
            ),
            SaveTo::ThisPc,
        )
        .await;
    let started = fixture.start(&client, &project, "offline-crash").await;
    fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Running,
            Duration::from_secs(5),
        )
        .await;
    fixture.daemon.stop();
    drop(client);
    std::fs::write(fixture.daemon.home.path().join("project/crash"), "owned")
        .expect("release owned crash while daemon absent");
    fixture.start_limit(&started).await;
    let properties = fixture.show(&started.unit);
    let restarts: u32 = properties
        .get("NRestarts")
        .expect("restart count")
        .parse()
        .expect("numeric restarts");
    assert!(restarts >= 2);
    assert!(!fixture.start_limit_notified(&started.id).await);
    fixture.daemon.launch();
    let client = fixture.daemon.client().await;
    let failed = fixture
        .wait(
            &client,
            &project,
            &started.id,
            RunState::Failed,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(failed.restarts, restarts);
    assert!(fixture.start_limit_notified(&failed.id).await);
    assert_eq!(failed.exit.as_ref().and_then(|exit| exit.code), Some(1));
    // Startup emits before a client can subscribe; the durable marker proves it
    // was recorded, while duplicate exit events must not emit a second alert.
    let mut alerts = bounded(client.subscribe("notify.raised"))
        .await
        .expect("subscribe after recovery");
    for _ in 0..2 {
        bounded(client.call::<RuntimeExited>(&exit_record::Exited {
            run_id: failed.id.clone(),
            attempt_id: failed.attempt_id.clone().expect("current attempt"),
        }))
        .await
        .expect("repeat durable exit notification");
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(400), alerts.next_event())
            .await
            .is_err(),
        "duplicate recovered start-limit notification"
    );
    assert!(fixture.start_limit_notified(&failed.id).await);
}
