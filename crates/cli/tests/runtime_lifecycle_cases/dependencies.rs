//! A dependency service that is already running is reused, never launched twice (X1-02).
use crate::support::{
    bounded,
    runtime::{RuntimeFixture, service},
};
use devbox_cli::client::Client;
use devbox_core::sh_quote;
use devbox_runtime::*;

/// `db` listens on a free port picked now, so a second copy cannot bind it.
async fn define_db_and_dependents(fixture: &RuntimeFixture, client: &Client, project: &str) {
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("owned free port")
        .local_addr()
        .expect("free port")
        .port();
    let listen = format!(
        "import socket,time; s=socket.socket(); s.bind(('127.0.0.1',{port})); s.listen(); time.sleep(60)"
    );
    let mut db = service(
        "db",
        &format!("exec /usr/bin/python3 -c {}", sh_quote(&listen)),
        RestartPolicy::No,
    );
    if let RunDef::Service(def) = &mut db {
        def.ports_hint = vec![port];
    }
    fixture.define(client, project, db, SaveTo::ThisPc).await;
    for id in ["api", "worker"] {
        let mut dependent = service(id, "sleep 60", RestartPolicy::No);
        if let RunDef::Service(def) = &mut dependent {
            def.depends_on.push("db".into());
        }
        fixture
            .define(client, project, dependent, SaveTo::ThisPc)
            .await;
    }
}

/// Every recorded run of one definition, in any state.
async fn runs_of(fixture: &RuntimeFixture, client: &Client, project: &str, def: &str) -> Vec<Run> {
    fixture
        .runs(client, project)
        .await
        .into_iter()
        .filter(|run| run.def_id == def)
        .collect()
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn restarting_a_service_keeps_its_running_dependency() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    define_db_and_dependents(&fixture, &client, &project).await;
    let api = fixture.start(&client, &project, "api").await;
    let runs = fixture.runs(&client, &project).await;
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(runs.iter().all(|run| run.state.is_live()), "{runs:?}");
    let db = runs_of(&fixture, &client, &project, "db").await;
    assert_eq!(db.len(), 1, "{db:?}");
    let restarted = bounded(client.call::<RuntimeRestart>(&RunId {
        run_id: api.id.clone(),
    }))
    .await
    .expect("restart with the dependency running");
    assert_eq!(restarted.id, api.id);
    assert!(restarted.state.is_live(), "{restarted:?}");
    // [재시작] replaces the final run with a new attempt; it never reuses it.
    assert!(api.attempt_id.is_some() && restarted.attempt_id.is_some());
    assert_ne!(restarted.attempt_id, api.attempt_id);
    let after = runs_of(&fixture, &client, &project, "db").await;
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].state.is_live(), "{after:?}");
    assert_eq!(after[0].id, db[0].id);
    assert_eq!(after[0].unit, db[0].unit);
    assert_eq!(after[0].attempt_id, db[0].attempt_id);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn starting_a_service_reuses_a_dependency_started_on_its_own() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    define_db_and_dependents(&fixture, &client, &project).await;
    let db = fixture.start(&client, &project, "db").await;
    let api = fixture.start(&client, &project, "api").await;
    assert!(api.state.is_live(), "{api:?}");
    let after = runs_of(&fixture, &client, &project, "db").await;
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].state.is_live(), "{after:?}");
    assert_eq!(after[0].id, db.id);
}

#[tokio::test]
#[ignore = "needs systemd user"]
async fn two_services_share_one_running_dependency() {
    let fixture = RuntimeFixture::new();
    let client = fixture.daemon.client().await;
    let project = fixture.project(&client).await;
    define_db_and_dependents(&fixture, &client, &project).await;
    let api = fixture.start(&client, &project, "api").await;
    let worker = fixture.start(&client, &project, "worker").await;
    let runs = fixture.runs(&client, &project).await;
    let db: Vec<_> = runs.iter().filter(|run| run.def_id == "db").collect();
    assert_eq!(db.len(), 1, "{runs:?}");
    assert!(db[0].state.is_live(), "{runs:?}");
    for dependent in [&api, &worker] {
        let run = runs
            .iter()
            .find(|run| run.id == dependent.id)
            .expect("dependent run");
        assert!(run.state.is_live(), "{run:?}");
    }
}
