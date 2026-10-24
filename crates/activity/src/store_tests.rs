use crate::fixture::*;
use crate::service_tests::fail;
use crate::writer::{MAX_PENDING, Writer};
use crate::*;
use devbox_core::HubMessage;
use std::sync::Arc;
use std::time::Duration;

async fn next_status(rx: &mut devbox_core::HubReceiver) -> ActivityStatus {
    let message = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("a status event")
        .expect("hub open");
    let HubMessage::Event { payload, .. } = message else {
        panic!("unexpected resync");
    };
    serde_json::from_str(&payload).expect("status payload")
}

/// CV-F08 · AR-F05: a write failure keeps every session (also later ones), reports
/// it, and the retry after a lost reply stores each session once.
#[tokio::test]
async fn storage_failure_keeps_later_sessions_and_reports_status() -> T {
    let a = fixture();
    let mut status = a.hub.subscribe("activity.status_changed");
    a.ingest(a.fg("a.exe", "A", 1_000, 1_000))
        .await
        .map_err(fail)?;
    a.ingest(a.fg("b.exe", "B", 2_000, 2_000))
        .await
        .map_err(fail)?;
    a.flush().await;
    assert_eq!(a.sessions().await.len(), 1, "first insert stored A");
    a.set_mode(Mode::CommitThenFail);
    for (at, app) in [(3_000, "c.exe"), (4_000, "d.exe"), (5_000, "e.exe")] {
        a.ingest(a.fg(app, "x", at, at)).await.map_err(fail)?;
    }
    a.flush().await;
    let failed = next_status(&mut status).await;
    assert_eq!(failed.state, StorageState::StorageFailed);
    assert_eq!(failed.pending, 3);
    assert_eq!(failed.last_error, Some(StorageProblem::Database));
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?, failed);
    // A session closing while storage is down waits too.
    a.ingest(a.fg("f.exe", "x", 6_000, 6_000))
        .await
        .map_err(fail)?;
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.pending, 4);
    a.set_mode(Mode::Store);
    a.flush().await;
    let rows = a.sessions().await;
    let starts: Vec<i64> = rows.iter().map(|s| s.start_ms).collect();
    assert_eq!(
        starts,
        [1_000, 2_000, 3_000, 4_000, 5_000],
        "no loss, no duplicate"
    );
    let mut ids: Vec<&str> = rows.iter().map(|s| s.id.as_str()).collect();
    ids.dedup();
    assert_eq!(ids.len(), 5);
    let mut last = next_status(&mut status).await;
    while last.state != StorageState::Ok {
        last = next_status(&mut status).await;
    }
    assert_eq!((last.pending, last.last_error), (0, None));
    Ok(())
}

#[tokio::test]
async fn retry_queue_caps_at_10000_and_counts_drops() -> T {
    let a = fixture();
    let writer = Arc::new(Writer::new(a.sink.clone(), a.hub.clone()));
    let total = MAX_PENDING + 5;
    let sessions: Vec<Session> = (0..total)
        .map(|n| session(&format!("s{n:05}"), n as i64))
        .collect();
    a.set_mode(Mode::Fail);
    writer.push(sessions[..total - 55].to_vec());
    writer.flush().await; // the failed batch goes back to the front
    writer.push(sessions[total - 55..].to_vec());
    let status = writer.status();
    assert_eq!(status.pending as usize, MAX_PENDING);
    assert_eq!(status.dropped, 5);
    assert_eq!(status.state, StorageState::StorageFailed);
    a.set_mode(Mode::Store);
    writer.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), MAX_PENDING);
    assert_eq!(rows[0].id, "s00005", "the oldest five were dropped");
    assert_eq!(rows[MAX_PENDING - 1].id, format!("s{:05}", total - 1));
    let after = writer.status();
    assert_eq!(
        (after.state, after.pending, after.dropped),
        (StorageState::Ok, 0, 5)
    );
    Ok(())
}

#[tokio::test]
async fn flush_writes_in_batches_of_100_and_app_drops_are_counted() -> T {
    let a = fixture();
    let writer = Writer::new(a.sink.clone(), a.hub.clone());
    writer.push((0..250).map(|n| session(&format!("b{n}"), n)).collect());
    writer.add_dropped(7);
    writer.flush().await;
    assert_eq!(a.sink.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert_eq!(a.sessions().await.len(), 250);
    assert_eq!(writer.status().dropped, 7);
    Ok(())
}
