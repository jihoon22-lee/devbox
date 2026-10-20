use crate::{
    run_store::{self, StoredRun},
    service_tests::Fixture,
    *,
};
#[tokio::test]
async fn live_unit_lookup_keeps_runs_older_than_the_history_page() {
    let f = Fixture::new().await;
    let project = f.project.clone();
    f.db.write(move |tx| {
        for i in 0..300 {
            run_store::put(
                tx,
                &StoredRun {
                    secret_definition: None,
                    run: Run {
                        id: format!("run-{i}"),
                        project_id: project.clone(),
                        def_id: "task".into(),
                        kind: RunKind::Task,
                        state: if i == 0 {
                            RunState::Running
                        } else {
                            RunState::Exited
                        },
                        exit: None,
                        started_ms: i,
                        ended_ms: None,
                        restarts: 0,
                        unit: format!("devbox-run-{i}.service"),
                        ready: true,
                        pid: None,
                        port: None,
                        attempt_id: None,
                    },
                    schedule: None,
                    restart: RestartPolicy::No,
                    start_limit: None,
                    worktree: None,
                    start_limit_notified: false,
                },
            )?;
        }
        Ok(())
    })
    .await
    .unwrap();
    let rows = f.runs.live_unit_runs().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].run_id, "run-0");
    assert_eq!(rows[0].project_id, f.project);
}
