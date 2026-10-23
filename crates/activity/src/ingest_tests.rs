//! `activity.ingest` batch contract: overlap and resends, server-side rules,
//! run changes and the cumulative `dropped` count.
use crate::fixture::*;
use crate::model::privacy::PrivacyRules;
use crate::service_tests::fail;
use crate::*;

/// seq 1 A, 2 B, 3 A, 4 C, 5 pause — the same five observations each time.
fn five(a: &Ax) -> Vec<model::event::Observation> {
    vec![
        a.fg("a.exe", "A", 1_000, 1_000),
        a.fg("b.exe", "B", 2_000, 2_000),
        a.fg("a.exe", "A", 3_000, 3_000),
        a.fg("c.exe", "C", 4_000, 4_000),
        a.pause(5_000),
    ]
}

/// An overlapping batch (seq 2–5 after 1–3) applies only seq 4–5: the stored
/// sessions equal one clean ingest of 1–5.
#[tokio::test]
async fn overlapping_batch_applies_only_new_observations() -> T {
    let clean = fixture();
    let events = five(&clean);
    assert_eq!(clean.send(events.clone()).await.map_err(fail)?.accepted, 5);
    clean.flush().await;
    let a = fixture();
    let events = five(&a);
    assert_eq!(
        a.send(events[..3].to_vec()).await.map_err(fail)?.accepted,
        3
    );
    assert_eq!(
        a.send(events[1..].to_vec()).await.map_err(fail)?.accepted,
        2
    );
    a.flush().await;
    let (got, want) = (a.sessions().await, clean.sessions().await);
    assert_eq!(want.len(), 4);
    assert_eq!(got, want);
    Ok(())
}

/// A stale batch (upto below the run's) is ignored and does not lower the run's
/// `upto`, so a later resend of what was already applied stays ignored.
#[tokio::test]
async fn stale_batch_does_not_reopen_applied_observations() -> T {
    let a = fixture();
    let events = five(&a);
    assert_eq!(a.send(events.clone()).await.map_err(fail)?.accepted, 5);
    assert_eq!(
        a.send(events[..3].to_vec()).await.map_err(fail)?.accepted,
        0
    );
    assert_eq!(
        a.send(events[3..].to_vec()).await.map_err(fail)?.accepted,
        0
    );
    a.flush().await;
    assert_eq!(a.sessions().await.len(), 4);
    Ok(())
}

#[tokio::test]
async fn excluded_process_is_dropped_server_side() -> T {
    let a = fixture();
    a.svc
        .set_rules(SetRules {
            rules: PrivacyRules {
                excluded_processes: vec!["Secret.exe".into()],
                ..PrivacyRules::default()
            },
            idle_threshold_ms: 300_000,
        })
        .await
        .map_err(fail)?;
    a.send(vec![
        a.fg("secret.exe", "vault", 1_000, 1_000),
        a.fg("code.exe", "main.rs", 2_000, 2_000),
        a.fg("secret.exe", "vault", 3_000, 3_000),
        a.pause(4_000),
    ])
    .await
    .map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert!(rows.iter().all(|s| s.app != "secret.exe"), "{rows:?}");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].app.as_str(), rows[0].start_ms, rows[0].end_ms),
        ("code.exe", 2_000, 3_000)
    );
    Ok(())
}

/// A new `runId` (the app restarted) ends the previous run at its last input.
#[tokio::test]
async fn new_run_closes_the_previous_run_at_its_last_input() -> T {
    let a = fixture();
    a.send_as(
        "run-1",
        vec![
            a.fg("code.exe", "main.rs", 1_000, 1_000),
            a.fg("code.exe", "main.rs", 6_000, 5_000),
        ],
        0,
    )
    .await
    .map_err(fail)?;
    a.flush().await;
    assert!(a.sessions().await.is_empty(), "the session is still open");
    let next = a.fg("chrome.exe", "docs", 9_000, 9_000);
    a.send_as("run-2", vec![next], 0).await.map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].app.as_str(), rows[0].start_ms, rows[0].end_ms),
        ("code.exe", 1_000, 5_000)
    );
    Ok(())
}

/// `dropped` is the run's cumulative count: resends and overlaps add nothing,
/// a new run starts from zero.
#[tokio::test]
async fn dropped_is_cumulative_per_run() -> T {
    let a = fixture();
    let events = five(&a);
    a.send_as("run-1", events[..2].to_vec(), 3)
        .await
        .map_err(fail)?;
    a.send_as("run-1", events[..2].to_vec(), 3)
        .await
        .map_err(fail)?;
    a.send_as("run-1", events[1..3].to_vec(), 3)
        .await
        .map_err(fail)?;
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.dropped, 3);
    a.send_as("run-1", events[3..].to_vec(), 5)
        .await
        .map_err(fail)?;
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.dropped, 5);
    let next = a.fg("x.exe", "x", 9_000, 9_000);
    a.send_as("run-2", vec![next], 2).await.map_err(fail)?;
    assert_eq!(a.svc.status(NoParams {}).await.map_err(fail)?.dropped, 7);
    Ok(())
}

/// Rules see the whole title; only the stored result is cut to 4 KiB.
#[tokio::test]
async fn rules_apply_before_titles_are_cut() -> T {
    let a = fixture();
    a.svc
        .set_rules(SetRules {
            rules: PrivacyRules {
                excluded_title_patterns: vec!["private-tail".into()],
                ..PrivacyRules::default()
            },
            idle_threshold_ms: 300_000,
        })
        .await
        .map_err(fail)?;
    let long = format!("{} private-tail", "a".repeat(MAX_TITLE_BYTES));
    let plain = "b".repeat(MAX_TITLE_BYTES + 10);
    a.send(vec![
        a.fg("code.exe", &long, 1_000, 1_000),
        a.fg("code.exe", &plain, 2_000, 2_000),
        a.pause(3_000),
    ])
    .await
    .map_err(fail)?;
    a.flush().await;
    let rows = a.sessions().await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].title, "", "the excluded title is not stored at all");
    assert_eq!(rows[1].title.len(), MAX_TITLE_BYTES);
    Ok(())
}
