use crate::fixture::*;
use crate::service_tests::fail;
use crate::*;

#[tokio::test]
async fn graceful_shutdown_persists_two_hours_of_acknowledged_input_across_restart() -> T {
    let mut a = fixture();
    let start = 1_000;
    assert_eq!(
        a.ingest(a.fg("code.exe", "owned project", start, start))
            .await
            .map_err(fail)?
            .accepted,
        1
    );
    for n in 1..=240 {
        assert_eq!(
            a.ingest(a.input("code.exe", start + n * 30_000))
                .await
                .map_err(fail)?
                .accepted,
            1
        );
        a.flush().await;
    }
    assert!(
        a.sessions().await.is_empty(),
        "ordinary flush does not split an open session"
    );
    a.svc.shutdown().await;
    a.restart();
    let rows = a.sessions().await;
    assert_eq!(
        rows.len(),
        1,
        "all acknowledged input through the last input is durable"
    );
    assert_eq!(
        (rows[0].start_ms, rows[0].end_ms),
        (start, start + 7_200_000)
    );
    a.ingest(a.input("code.exe", start + 7_230_000))
        .await
        .map_err(fail)?;
    a.ingest(a.pause(start + 7_240_000)).await.map_err(fail)?;
    a.flush().await;
    assert_eq!(
        a.sessions().await.len(),
        2,
        "restart retains the old interval alongside the new one"
    );
    Ok(())
}

#[tokio::test]
async fn shutdown_trims_pending_idle_tail_and_is_idempotent_with_or_without_idle_event() -> T {
    for with_idle in [false, true] {
        let a = fixture();
        a.ingest(a.fg("code.exe", "active", 900_000, 900_000))
            .await
            .map_err(fail)?;
        a.ingest(a.input("code.exe", 1_000_000))
            .await
            .map_err(fail)?;
        a.ingest(a.fg("code.exe", "idle tail1", 1_100_000, 1_000_000))
            .await
            .map_err(fail)?;
        a.ingest(a.fg("code.exe", "idle tail2", 1_200_000, 1_000_000))
            .await
            .map_err(fail)?;
        if with_idle {
            a.ingest(a.idle(1_300_000, 1_000_000)).await.map_err(fail)?;
        }
        a.svc.shutdown().await;
        let first = a.sessions().await;
        assert_eq!(
            first.len(),
            1,
            "pending intervals after the last input are excluded"
        );
        assert_eq!((first[0].start_ms, first[0].end_ms), (900_000, 1_000_000));
        for _ in 0..3 {
            a.svc.shutdown().await;
        }
        assert_eq!(
            a.sessions().await,
            first,
            "a drained run is never emitted twice"
        );
    }
    Ok(())
}

#[tokio::test]
async fn shutdown_uses_current_privacy_rules_then_project_attribution() -> T {
    use crate::model::attribution::ProjectMatch;
    use crate::model::privacy::PrivacyRules;
    let a = fixture();
    *a.projects.lock().expect("fixture projects") = vec![
        ProjectMatch {
            project_id: "raw-secret-project".into(),
            basenames: vec!["secret project".into()],
        },
        ProjectMatch {
            project_id: "visible-project".into(),
            basenames: vec!["project".into()],
        },
    ];
    a.ingest(a.fg("code.exe", "secret project", 1_000, 1_000))
        .await
        .map_err(fail)?;
    a.ingest(a.input("code.exe", 6_000)).await.map_err(fail)?;
    a.svc
        .set_rules(SetRules {
            rules: PrivacyRules {
                redact_title_patterns: vec!["secret".into()],
                ..PrivacyRules::default()
            },
            idle_threshold_ms: 300_000,
        })
        .await
        .map_err(fail)?;
    a.svc.shutdown().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "[redacted] project");
    assert_eq!(rows[0].project_id.as_deref(), Some("visible-project"));
    let excluded = fixture_with("excluded_processes = ['secret.exe']");
    excluded
        .ingest(excluded.fg("secret.exe", "private project", 1_000, 1_000))
        .await
        .map_err(fail)?;
    excluded
        .ingest(excluded.input("secret.exe", 6_000))
        .await
        .map_err(fail)?;
    excluded.svc.shutdown().await;
    assert!(excluded.sessions().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn shutdown_keeps_failed_finalized_session_for_retry_without_duplicate_ids() -> T {
    let mut a = fixture();
    *a.sink.mode.lock().expect("sink") = Mode::Fail;
    a.ingest(a.fg("owned.exe", "owned activity", 1_000, 1_000))
        .await
        .map_err(fail)?;
    a.ingest(a.input("owned.exe", 5_000)).await.map_err(fail)?;
    for _ in 0..2 {
        a.svc.shutdown().await;
        let status = a.svc.status(NoParams {}).await.map_err(fail)?;
        assert_eq!(
            (status.state, status.pending),
            (StorageState::StorageFailed, 1)
        );
        assert!(a.sessions().await.is_empty());
    }
    *a.sink.mode.lock().expect("sink") = Mode::CommitThenFail;
    a.svc.shutdown().await;
    let stored = a.sessions().await;
    assert_eq!(stored.len(), 1);
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.pending, 1);
    *a.sink.mode.lock().expect("sink") = Mode::Store;
    a.svc.shutdown().await;
    assert_eq!(
        a.sessions().await,
        stored,
        "retry uses the original finalized identity"
    );
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.pending, 0);
    a.restart();
    assert_eq!(a.sessions().await, stored);
    Ok(())
}
