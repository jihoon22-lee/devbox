use super::*;
use std::{sync::atomic::AtomicUsize, time::Duration};

#[tokio::test]
async fn scheduler_retries_transient_claim_failure_without_external_event() {
    retry_after_claim_failure(false).await;
}

#[tokio::test]
async fn scheduler_retry_rechecks_trust_after_transient_claim_failure() {
    retry_after_claim_failure(true).await;
}

async fn retry_after_claim_failure(untrusted: bool) {
    let h = Harness::new(MissedPolicy::Skip).await;
    if untrusted {
        h.f.write("[[task]]\nid='task'\ncommand='echo unapproved'\n");
    }
    let mut alerts = h.f.hub.subscribe("notify.raised");
    h.f.db
        .write(|tx| {
            tx.execute_batch(
                "CREATE TRIGGER fail_claim BEFORE INSERT ON runtime_schedule_claims
         BEGIN SELECT RAISE(ABORT, 'owned transient claim failure'); END;",
            )
        })
        .await
        .expect("install owned failure");
    let calls = Arc::new(AtomicUsize::new(0));
    let clock_calls = calls.clone();
    let started = h.started.clone();
    let completed = Arc::new(tokio::sync::Notify::new());
    let notify = completed.clone();
    let scheduler = test_scheduler(
        h.f.runs.clone(),
        Arc::new(move || {
            if clock_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                BASE
            } else {
                BASE + 60000
            }
        }),
        Arc::new(move |graph| {
            started
                .lock()
                .expect("fixture result")
                .push(graph.nodes.clone());
            let notify = notify.clone();
            Box::pin(async move {
                notify.notify_one();
                Ok(fake_run(&graph, RunState::Running))
            })
        }),
    );
    let handle = scheduler.start().await.expect("start scheduler");
    // Startup tick, first deadline, failed due tick, then next wait: the owned
    // failure remains installed until the event loop has finished its error path.
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) < 4 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("failed tick returned to wait");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 4, "errors must back off");
    assert_eq!(h.count(), 0);
    h.f.db
        .write(|tx| tx.execute_batch("DROP TRIGGER fail_claim;"))
        .await
        .expect("remove owned failure without publishing a hub event");
    let resumed = tokio::time::timeout(Duration::from_secs(3), async {
        if untrusted {
            let HubMessage::Event { payload, .. } = alerts.recv().await.expect("trust alert")
            else {
                panic!("unexpected notification resync")
            };
            let alert: Notification = serde_json::from_str(&payload).expect("notification payload");
            assert!(matches!(
                alert.reason,
                NotificationReason::ScheduleTrustRequired
            ));
        } else {
            completed.notified().await;
        }
    })
    .await;
    handle.cancel.cancel();
    handle.join.await.expect("stop scheduler");
    assert!(
        resumed.is_ok(),
        "scheduler must retry without an external wake"
    );
    let expected_runs = usize::from(!untrusted);
    assert_eq!(h.count(), expected_runs, "only authorized occurrences run");
    h.advance(1);
    h.scheduler
        .tick(false)
        .await
        .expect("subsequent recomputation");
    h.scheduler.drain().await;
    assert_eq!(
        h.count(),
        expected_runs,
        "recomputation must not replay the claimed tick"
    );
}

#[tokio::test]
async fn scheduler_without_schedules_stays_idle() {
    let f = Fixture::new().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let clock_calls = calls.clone();
    let scheduler = test_scheduler(
        f.runs.clone(),
        Arc::new(move || {
            clock_calls.fetch_add(1, Ordering::SeqCst);
            BASE
        }),
        Arc::new(|_| panic!("no schedules can run")),
    );
    let handle = scheduler.start().await.expect("start empty scheduler");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    handle.cancel.cancel();
    handle.join.await.expect("stop scheduler");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "empty scans do not retry");
}
