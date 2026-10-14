mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

fn fixture() -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    d.write_config("[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand='cat'\nhooks='none'\n");
    d
}
async fn value(client: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &client
            .raw_call(method, params, None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture")
}
#[tokio::test]
async fn dependent_project_removal_is_rejected_and_agent_cleanup_remains_possible() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", true).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let error = client
        .raw_call("projects.remove", json!({"id":project}), None)
        .await
        .expect_err("open agent protects its project ownership");
    assert!(format!("{error:?}").contains("projects.in_use"));
    let check = value(&client, "agents.discard_check", json!({"id":id})).await;
    value(&client, "agents.discard", json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]}})).await;
    client
        .raw_call("projects.remove", json!({"id":project}), None)
        .await
        .expect("fully cleaned closed history permits removal");
}
#[tokio::test]
async fn closed_retained_resources_protect_project_until_explicit_cleanup() {
    let mut d = fixture();
    d.write_config("[agents]\ncleanup_after_close=false\n[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand='cat'\nhooks='none'\n");
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", true).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let changes = value(&client, "agents.changes", json!({"id":id})).await;
    value(&client, "agents.merge", json!({"id":id,"strategy":"merge","expect":{"headOid":changes["headOid"],"baseOid":changes["baseOid"]}})).await;
    assert!(
        client
            .raw_call("projects.remove", json!({"id":project}), None)
            .await
            .is_err()
    );
    let candidates = value(
        &client,
        "agents.cleanup_candidates",
        json!({"projectId":project}),
    )
    .await;
    assert_eq!(
        candidates["worktrees"]
            .as_array()
            .expect("owned fixture")
            .len(),
        1
    );
    value(
        &client,
        "agents.cleanup",
        json!({"projectId":project,"worktrees":candidates["worktrees"],"branches":[]}),
    )
    .await;
    assert!(
        client
            .raw_call("projects.remove", json!({"id":project}), None)
            .await
            .is_err(),
        "retained branch still needs stable project ownership"
    );
    let candidates = value(
        &client,
        "agents.cleanup_candidates",
        json!({"projectId":project}),
    )
    .await;
    assert_eq!(
        candidates["branches"]
            .as_array()
            .expect("owned fixture")
            .len(),
        1
    );
    value(
        &client,
        "agents.cleanup",
        json!({"projectId":project,"worktrees":[],"branches":candidates["branches"]}),
    )
    .await;
    client
        .raw_call("projects.remove", json!({"id":project}), None)
        .await
        .expect("cleaned historical task is not a live dependency");
}
#[tokio::test]
async fn concurrent_create_and_remove_cannot_commit_an_orphan() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let remover = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let (created, removed) = tokio::join!(
        client.raw_call("agents.create", json!({"projectId":project,"title":"race","profile":"fake","worktree":true,"prompt":""}), None),
        remover.raw_call("projects.remove", json!({"id":project}), None)
    );
    assert!(
        !(created.is_ok() && removed.is_ok()),
        "creation and deletion cannot both commit: created={created:?}, removed={removed:?}"
    );
}

#[tokio::test]
async fn exited_terminal_can_be_closed_before_removing_its_project() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let session = value(
        &client,
        "terminal.create",
        json!({"projectId":project,"command":"exit 0"}),
    )
    .await;
    support::bounded(async {
        loop {
            let list = value(&client, "terminal.list", json!({"projectId":project})).await;
            if list["items"][0]["alive"] == false {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        client
            .raw_call("projects.remove", json!({"id":project}), None)
            .await
            .is_err()
    );
    let closed = client
        .raw_call("terminal.close", json!({"id":session["id"]}), None)
        .await;
    assert!(
        closed.is_ok(),
        "close dead record: {closed:?}; {}",
        std::fs::read_to_string(d.home.path().join("stderr.log")).unwrap_or_default()
    );
    client
        .raw_call("projects.remove", json!({"id":project}), None)
        .await
        .expect("no remaining dependencies");
}
