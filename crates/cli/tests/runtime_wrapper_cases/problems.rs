use super::Fixture;
use devbox_observe::{ObserveService, ProblemRun};

#[tokio::test]
async fn wrapper_new_attempt_starts_after_the_preserved_partial_previous_record() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.runs()).unwrap();
    let log = f.runs().join("run-1.log");
    let partial = "2026-10-20T01:02:03.004Z\to\tx.ts(1,1): error truncated";
    std::fs::write(&log, partial).unwrap();
    let status = f
        .command()
        .args([
            "exec",
            "--run",
            "run-1",
            "--attempt",
            "new",
            "--",
            "sh",
            "-c",
            "printf 'x.ts(3,1): error current\\n'",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let contents = f.log();
    assert!(
        contents.starts_with(&format!("{partial}\n")),
        "new marker must start on its own physical line without changing previous bytes: {contents}"
    );
    let marker = contents
        .lines()
        .nth(1)
        .unwrap()
        .splitn(3, '\t')
        .collect::<Vec<_>>();
    assert_eq!(marker[1], "s");
    assert_eq!(
        devbox_core::run_log::parse_attempt_marker(marker[2]),
        Some("new")
    );
    let service = ObserveService::default();
    let batch = service
        .read_problem_batch(&ProblemRun {
            run_id: "run-1".into(),
            project_id: "fixture".into(),
            root: f.temp.path().into(),
            path: log,
            attempt_id: Some("new".into()),
            started_ms: 1,
        })
        .await
        .unwrap();
    assert_eq!(batch.problems.len(), 1);
    assert_eq!(batch.problems[0].line, 3);
    assert_eq!(batch.problems[0].message, "current");
    service.shutdown().await;
}
