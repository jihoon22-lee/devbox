use crate::*;
use devbox_core::{Db, Hub};
use std::sync::Arc;

#[tokio::test]
async fn project_file_live_run_reservation_blocks_removal_but_closed_history_does_not() {
    for state in [
        "starting",
        "running",
        "restarting",
        "exited",
        "failed",
        "stopped",
    ] {
        let home = tempfile::tempdir().expect("owned project");
        let project = home.path().join("project");
        std::fs::create_dir_all(project.join(".devbox")).expect("fixture result");
        std::fs::write(
            project.join(".devbox/devbox.toml"),
            "[[service]]\nid='web'\ncommand='sleep 60'\n",
        )
        .expect("fixture result");
        let db = Db::open(
            &home.path().join("db"),
            &home.path().join("backups"),
            "1.0.0-dev",
            MIGRATIONS,
        )
        .expect("fixture result");
        let service =
            ProjectsService::new(db.clone(), Arc::new(Hub::new()), home.path().into(), vec![]);
        let registered = service
            .add(AddProject {
                path: project.display().to_string(),
            })
            .await
            .expect("fixture result");
        let id = registered.id.clone();
        // Optional-domain fixture mirrors the runtime reservation's FK/JSON contract,
        // without a projects -> runtime dependency or a host service.
        db.write(move |tx| {
            tx.execute_batch("CREATE TABLE runtime_runs (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects_projects(id) ON DELETE CASCADE, started_ms INTEGER NOT NULL, body TEXT NOT NULL);")?;
            tx.execute("INSERT INTO runtime_runs VALUES ('owned-run', ?1, 0, ?2)", rusqlite::params![id, serde_json::json!({"run":{"state":state}}).to_string()])?;
            Ok(())
        }).await.expect("fixture result");
        let result = service
            .remove(RemoveProject {
                id: registered.id.clone(),
            })
            .await;
        let live = matches!(state, "starting" | "running" | "restarting");
        if live {
            assert!(
                matches!(result, Err(Fail::Domain(ProjectsError::InUse { .. }))),
                "live {state} reservation must retain project: {result:?}"
            );
        } else {
            result.expect("completed history permits project removal");
        }
        let retained: i64 = db
            .read(|tx| tx.query_row("SELECT count(*) FROM runtime_runs", [], |r| r.get(0)))
            .await
            .expect("fixture result");
        assert_eq!(
            retained,
            i64::from(live),
            "reservation retention for {state}"
        );
        assert!(project.join(".devbox/devbox.toml").exists());
    }
}
