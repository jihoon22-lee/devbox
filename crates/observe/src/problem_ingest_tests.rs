use crate::*;
use devbox_core::{Hub, run_log::attempt_marker};
use std::sync::Arc;
fn problem(n: u32) -> Problem {
    Problem {
        file: "x.ts".into(),
        line: n,
        col: Some(1),
        severity: ProblemSeverity::Error,
        message: "TS1005: expected".into(),
        source: String::new(),
    }
}
fn run(root: &std::path::Path, id: &str, attempt: &str) -> ProblemRun {
    ProblemRun {
        run_id: id.into(),
        project_id: "project".into(),
        root: root.into(),
        source_cwd: Some(root.into()),
        windows_directory: None,
        path: root.join(format!("{id}.log")),
        attempt_id: Some(attempt.into()),
        started_ms: 1,
    }
}
fn record(stream: &str, text: &str) -> String {
    format!("2026-10-20T01:02:03.004Z\t{stream}\t{text}\n")
}
#[tokio::test]
async fn problems_dedup_restart_only_own_source_and_rev_pages() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let a = run(temp.path(), "a", "one");
    let b = run(temp.path(), "b", "one");
    service
        .apply_problem_batch(a.clone(), vec![problem(1), problem(1)], false, &hub)
        .unwrap();
    service
        .apply_problem_batch(b, vec![problem(1)], false, &hub)
        .unwrap();
    let rev = service.problem_rev();
    service
        .apply_problem_batch(a.clone(), vec![problem(1)], false, &hub)
        .unwrap();
    assert_eq!(service.problem_rev(), rev);
    let page = service
        .problem_result(ProblemsQuery {
            project_id: "project".into(),
            limit: Some(1),
            cursor: None,
        })
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.next_cursor.is_some());
    let mut restarted = a;
    restarted.attempt_id = Some("two".into());
    service
        .apply_problem_batch(restarted, vec![], false, &hub)
        .unwrap();
    let page2 = service
        .problem_result(ProblemsQuery {
            project_id: "project".into(),
            limit: Some(100),
            cursor: None,
        })
        .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert_eq!(page2.items[0].source, "run:b");
    assert_eq!(page2.rev, (rev + 1).to_string());
    assert!(
        service
            .problem_result(ProblemsQuery {
                project_id: "project".into(),
                limit: Some(1),
                cursor: page.next_cursor
            })
            .is_err()
    );
}
#[tokio::test]
async fn problems_ingest_markers_stream_context_and_same_millisecond_attempts() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let r = run(temp.path(), "a", "new");
    let lines = [
        ("s", attempt_marker("old")),
        ("o", "x.ts(1,1): error old".into()),
        ("o", attempt_marker("new")),
        ("o", "x.ts(2,1): error forged".into()),
        ("s", attempt_marker("new")),
        ("e", "error[E0308]: wrong type".into()),
        ("o", "--> x.rs:3:1".into()),
        ("e", "--> x.rs:4:2".into()),
        ("s", attempt_marker("old")),
        ("e", "x.ts(5,1): error stale".into()),
    ];
    std::fs::write(
        &r.path,
        lines.iter().map(|(s, t)| record(s, t)).collect::<String>(),
    )
    .unwrap();
    let batch = service.read_problem_batch(&r).await.unwrap();
    let rows = batch.problems;
    let overflow = batch.overflow;
    assert!(!overflow);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].line, 4);
    assert_eq!(rows[0].message, "E0308: wrong type");
    service.shutdown().await;
}
#[tokio::test]
async fn problems_duplicate_backlog_does_not_report_overflow() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let r = run(temp.path(), "a", "one");
    std::fs::write(
        &r.path,
        record("s", &attempt_marker("one"))
            + &record("o", "x.ts(1,1): error repeated").repeat(2100),
    )
    .unwrap();
    let batch = service.read_problem_batch(&r).await.unwrap();
    let rows = batch.problems;
    let overflow = batch.overflow;
    assert!(
        !overflow,
        "duplicates must not change observable truncation/revision"
    );
    assert_eq!(rows.len(), 1);
}
#[tokio::test]
async fn problems_result_bound_is_explicit_and_does_not_churn() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Arc::new(Hub::new());
    let r = run(temp.path(), "a", "one");
    service
        .apply_problem_batch(r.clone(), (1..=2050).map(problem).collect(), false, &hub)
        .unwrap();
    let rev = service.problem_rev();
    let p = service
        .problem_result(ProblemsQuery {
            project_id: "project".into(),
            limit: Some(100),
            cursor: None,
        })
        .unwrap();
    assert!(p.truncated);
    assert_eq!(p.items.len(), 100);
    assert!(serde_json::to_vec(&p).unwrap().len() < 768 * 1024);
    service
        .apply_problem_batch(r, vec![problem(2050)], false, &hub)
        .unwrap();
    assert_eq!(service.problem_rev(), rev);
    assert!(
        service
            .problem_result(ProblemsQuery {
                project_id: "project".into(),
                limit: Some(101),
                cursor: None
            })
            .is_err()
    );
}
#[tokio::test]
async fn problems_markerless_logs_are_not_attributed_to_current_attempt() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let r = run(temp.path(), "a", "one");
    std::fs::write(&r.path, record("o", "x.ts(1,1): error old unknown attempt")).unwrap();
    assert!(
        service
            .read_problem_batch(&r)
            .await
            .unwrap()
            .problems
            .is_empty()
    );
}

#[tokio::test]
async fn problems_dropped_uncommitted_batch_replays_and_stale_attempt_cannot_resurrect() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let mut r = run(temp.path(), "a", "one");
    std::fs::write(
        &r.path,
        record("s", &attempt_marker("one")) + &record("o", "x.ts(1,1): error old"),
    )
    .unwrap();
    let old = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(old.problems.len(), 1);
    drop(old);
    let replay = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(replay.problems.len(), 1);
    service
        .commit_problem_batch(r.clone(), replay, true, &hub)
        .await
        .unwrap();
    r.attempt_id = Some("two".into());
    let next = service.read_problem_batch(&r).await.unwrap();
    assert!(next.problems.is_empty());
    service
        .commit_problem_batch(r, next, true, &hub)
        .await
        .unwrap();
    assert!(
        service
            .problem_result(ProblemsQuery {
                project_id: "project".into(),
                limit: Some(100),
                cursor: None
            })
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn problems_json_escaping_still_fits_one_protocol_page() {
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let r = run(temp.path(), "a", "one");
    let rows = (1..=100)
        .map(|line| Problem {
            file: "\"".repeat(4096),
            line,
            col: None,
            severity: ProblemSeverity::Error,
            message: "\"".repeat(4096),
            source: String::new(),
        })
        .collect();
    service.apply_problem_batch(r, rows, false, &hub).unwrap();
    let page = service
        .problem_result(ProblemsQuery {
            project_id: "project".into(),
            limit: Some(100),
            cursor: None,
        })
        .unwrap();
    assert!(serde_json::to_vec(&page).unwrap().len() < 768 * 1024);
    assert!(!page.items.is_empty());
    assert!(page.next_cursor.is_some());
}

#[tokio::test]
async fn problems_cancelled_scan_replays_all_uncommitted_lines() {
    let temp = tempfile::tempdir().unwrap();
    let service = Arc::new(ObserveService::default());
    let hub = Hub::new();
    let r = run(temp.path(), "a", "one");
    std::fs::write(
        &r.path,
        record("s", &attempt_marker("one")) + &record("o", "x.ts(1,1): error initial"),
    )
    .unwrap();
    let first = service.read_problem_batch(&r).await.unwrap();
    service
        .commit_problem_batch(r.clone(), first, true, &hub)
        .await
        .unwrap();
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&r.path)
        .unwrap();
    for n in 2..=1001 {
        file.write_all(record("o", &format!("x.ts({n},1): error cancelled")).as_bytes())
            .unwrap();
    }
    let worker = {
        let service = service.clone();
        let r = r.clone();
        tokio::spawn(async move { service.read_problem_batch(&r).await })
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while service.problems.readers.lock().await.contains_key("a") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    assert!(matches!(worker.await,Err(error) if error.is_cancelled()));
    let replay = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(replay.problems.len(), 1001);
    service
        .commit_problem_batch(r, replay, true, &hub)
        .await
        .unwrap();
    service.shutdown().await;
}

#[tokio::test]
async fn problems_partial_physical_record_waits_for_lf_and_replays_complete_diagnostic() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let r = run(temp.path(), "partial", "one");
    std::fs::write(&r.path, record("s", &attempt_marker("one"))).unwrap();
    let initial = service.read_problem_batch(&r).await.unwrap();
    service
        .commit_problem_batch(r.clone(), initial, true, &hub)
        .await
        .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&r.path)
        .unwrap();
    let complete = record("e", "x.ts(3,1): error expected semicolon");
    let cut = complete.find("expected").unwrap() + "expected".len();
    file.write_all(&complete.as_bytes()[..cut]).unwrap();
    let partial = service.read_problem_batch(&r).await.unwrap();
    assert!(
        partial.problems.is_empty(),
        "incomplete physical record must not emit a partial diagnostic: {:?}",
        partial.problems
    );
    service
        .commit_problem_batch(r.clone(), partial, true, &hub)
        .await
        .unwrap();
    file.write_all(&complete.as_bytes()[cut..]).unwrap();
    let completed = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(completed.problems.len(), 1);
    assert_eq!(completed.problems[0].message, "expected semicolon");
    service
        .commit_problem_batch(r, completed, true, &hub)
        .await
        .unwrap();
    service.shutdown().await;
}

#[tokio::test]
async fn problems_partial_system_marker_is_retained_until_its_lf() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let r = run(temp.path(), "partial-marker", "one");
    let marker = record("s", &attempt_marker("one"));
    let cut = marker.len() - 3;
    std::fs::write(&r.path, &marker.as_bytes()[..cut]).unwrap();
    let partial = service.read_problem_batch(&r).await.unwrap();
    assert!(partial.problems.is_empty());
    service
        .commit_problem_batch(r.clone(), partial, true, &hub)
        .await
        .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&r.path)
        .unwrap();
    file.write_all(&marker.as_bytes()[cut..]).unwrap();
    file.write_all(record("e", "x.ts(3,1): error complete marker").as_bytes())
        .unwrap();
    let completed = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(completed.problems.len(), 1);
    assert_eq!(completed.problems[0].message, "complete marker");
    service
        .commit_problem_batch(r, completed, true, &hub)
        .await
        .unwrap();
    service.shutdown().await;
}

#[tokio::test]
async fn problems_completed_run_does_not_cache_a_pending_fragment_as_complete() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let service = ObserveService::default();
    let hub = Hub::new();
    let r = run(temp.path(), "completed-partial", "one");
    let diagnostic = record("e", "x.ts(3,1): error expected semicolon");
    let cut = diagnostic.find("expected").unwrap() + "expected".len();
    std::fs::write(
        &r.path,
        record("s", &attempt_marker("one")) + &diagnostic[..cut],
    )
    .unwrap();
    let partial = service.read_problem_batch(&r).await.unwrap();
    assert!(partial.problems.is_empty());
    service
        .commit_problem_batch(r.clone(), partial, false, &hub)
        .await
        .unwrap();
    assert!(
        !service
            .problems
            .completed
            .lock()
            .await
            .contains_key(&r.run_id)
    );
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&r.path)
        .unwrap();
    file.write_all(&diagnostic.as_bytes()[cut..]).unwrap();
    let completed = service.read_problem_batch(&r).await.unwrap();
    assert_eq!(completed.problems.len(), 1);
    assert_eq!(completed.problems[0].message, "expected semicolon");
    service
        .commit_problem_batch(r.clone(), completed, false, &hub)
        .await
        .unwrap();
    assert!(
        service
            .problems
            .completed
            .lock()
            .await
            .contains_key(&r.run_id)
    );
    service.shutdown().await;
}

#[tokio::test]
async fn problems_complete_reader_preserves_raw_log_eof_fragment_display() {
    let temp = tempfile::tempdir().unwrap();
    let r = run(temp.path(), "raw-fragment", "one");
    let line = record("e", "x.ts(3,1): error fragment");
    std::fs::write(&r.path, line.trim_end_matches('\n')).unwrap();
    let mut reader = crate::sources::FileSource::open(
        &ResolvedSource {
            source: LogSource::Run { run_id: r.run_id },
            path: Some(r.path),
        },
        &[],
        &LogFrom::Offset { offset: 0 },
    )
    .await
    .unwrap();
    let fragment = reader.next().await.unwrap().unwrap();
    assert_eq!(fragment.text, "x.ts(3,1): error fragment");
}
