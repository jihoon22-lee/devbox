mod support;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use support::TestDaemon;

async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(&c.raw_call(method, params, None).await.expect("owned RPC"))
        .expect("response")
}
async fn refused(c: &devbox_cli::client::Client, method: &str, params: Value, code: &str) {
    let error = c
        .raw_call(method, params, None)
        .await
        .expect_err("authorization must fail");
    assert!(format!("{error:?}").contains(code), "{error:?}");
}
fn write(repo: &str, config: &str) {
    std::fs::create_dir_all(Path::new(repo).join(".devbox")).expect("owned config");
    std::fs::write(Path::new(repo).join(".devbox/devbox.toml"), config).expect("owned config");
}
fn fixture(cleanup: bool) -> (TestDaemon, String) {
    let d = TestDaemon::fixture();
    d.write_config(&format!("[agents]\ncleanup_after_close={cleanup}\n[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\ncommand='true'\nhooks='none'\n"));
    let repo = d.git_repo("projects/trust");
    (d, repo)
}
async fn create(
    c: &devbox_cli::client::Client,
    project: &str,
    title: &str,
    worktree: bool,
) -> String {
    call(c,"agents.create",json!({"projectId":project,"title":title,"profile":"fixture","worktree":worktree,"prompt":""})).await["id"].as_str().expect("task ID").into()
}
async fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < deadline, "missing {}", path.display());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
fn discard_expect(check: &Value) -> Value {
    json!({"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]})
}

#[tokio::test]
async fn direct_create_denies_unapproved_setup_before_reserving_resources() {
    let mut d = TestDaemon::fixture();
    d.write_config(
        "[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\ncommand='sleep 600'\nhooks='none'\n",
    );
    let repo = d.git_repo("projects/trust");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).expect("owned config");
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\nsetup=['touch unapproved']\n",
    )
    .expect("owned config");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let result = c.raw_call("agents.create", json!({"projectId":project,"title":"Trust","profile":"fixture","worktree":true,"prompt":""}), None).await;
    assert!(result.is_err(), "unapproved setup was reserved: {result:?}");
    assert!(format!("{:?}", result.expect_err("denied")).contains("agents.project_trust_required"));
    let rows = c
        .raw_call("agents.list", json!({}), None)
        .await
        .expect("list");
    assert!(
        serde_json::from_str::<serde_json::Value>(&rows).expect("rows")["items"]
            .as_array()
            .expect("items")
            .is_empty()
    );
    assert!(!Path::new(&repo).join("unapproved").exists());
    assert!(!Path::new(&repo).with_file_name("trust-agents").exists());
    refused(&c, "agents.create", json!({"projectId":"missing-project","title":"Missing","profile":"fixture","worktree":true,"prompt":""}), "agents.project_not_found").await;
}

#[tokio::test]
async fn direct_launch_denies_unapproved_terminal_before_creating_resources() {
    let mut d = TestDaemon::fixture();
    let repo = d.git_repo("projects/trust-launch");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).expect("owned config");
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[launch]\nterminals=[{name='unsafe',command='touch unapproved'}]\n",
    )
    .expect("owned config");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let result = c
        .raw_call("projects.launch", json!({"id":project}), None)
        .await;
    assert!(result.is_err(), "unapproved launch ran: {result:?}");
    assert!(format!("{:?}", result.expect_err("denied")).contains("projects.trust_required"));
    let rows = c
        .raw_call("terminal.list", json!({}), None)
        .await
        .expect("list");
    assert!(
        serde_json::from_str::<serde_json::Value>(&rows).expect("rows")["items"]
            .as_array()
            .expect("items")
            .is_empty()
    );
    assert!(!Path::new(&repo).join("unapproved").exists());
}

#[tokio::test]
async fn unchanged_approval_executes_setup_test_teardown_and_survives_daemon_restart() {
    let (mut d, repo) = fixture(false);
    let teardown = d.home.path().join("teardown-ran");
    write(
        &repo,
        &format!(
            "[agent]\nsetup=['touch setup-approved']\ntest='touch test-approved'\nteardown=[{}]\n",
            serde_json::to_string(&format!(
                "touch {}",
                devbox_core::sh_quote(teardown.to_str().expect("owned path"))
            ))
            .expect("TOML string")
        ),
    );
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    d.approve_project(&c, &project).await;
    let id = create(&c, &project, "Approved", true).await;
    let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
    assert!(wt.join("setup-approved").exists());
    d.restart();
    let c = d.client().await;
    call(&c, "agents.test", json!({"id":id})).await;
    wait_file(&wt.join("test-approved")).await;
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let closed = call(
        &c,
        "agents.discard",
        json!({"id":id,"expect":discard_expect(&check)}),
    )
    .await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(teardown.exists());
    assert!(!wt.exists());
}

#[tokio::test]
async fn direct_test_and_changed_teardown_are_denied_before_terminal_quiesce_or_deletion() {
    let (mut d, repo) = fixture(false);
    write(
        &repo,
        "[agent]\ntest='touch unapproved-test'\nteardown=['touch unapproved-teardown']\n",
    );
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    // No setup command: creating a user-selected tool does not consent to test/teardown.
    let id = create(&c, &project, "Unapproved", true).await;
    let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
    let before = call(&c, "terminal.list", json!({"projectId":project})).await;
    refused(
        &c,
        "agents.test",
        json!({"id":id}),
        "agents.project_trust_required",
    )
    .await;
    d.approve_project(&c, &project).await;
    write(
        &repo,
        "# changed after consent\n[agent]\ntest='touch unapproved-test'\nteardown=['touch unapproved-teardown']\n",
    );
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    refused(
        &c,
        "agents.discard",
        json!({"id":id,"expect":discard_expect(&check)}),
        "agents.project_trust_required",
    )
    .await;
    let after = call(&c, "terminal.list", json!({"projectId":project})).await;
    assert_eq!(
        before["items"], after["items"],
        "trust refusal must not quiesce the live terminal"
    );
    assert!(wt.exists());
    assert!(!wt.join("unapproved-test").exists());
    assert!(!wt.join("unapproved-teardown").exists());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "review"
    );
    d.git(
        Path::new(&repo),
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/{}", task["branch"].as_str().expect("branch")),
        ],
    );
}

#[tokio::test]
async fn preview_approval_rejects_changed_bytes_path_and_scope_without_approving() {
    let (mut d, repo) = fixture(false);
    write(&repo, "[agent]\nsetup=['true']\n");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let view = call(
        &c,
        "projects.trust_preview",
        json!({"id":project,"operation":"create","worktree":false}),
    )
    .await;
    assert_eq!(view["executionDirectories"], json!([repo]));
    for (path, digest, scope) in [
        (
            Value::String("/different".into()),
            view["digest"].clone(),
            view["scopeVersion"].clone(),
        ),
        (
            view["canonicalPath"].clone(),
            Value::String("different".into()),
            view["scopeVersion"].clone(),
        ),
        (
            view["canonicalPath"].clone(),
            view["digest"].clone(),
            json!(devbox_projects::trust::SCOPE_VERSION + 1),
        ),
    ] {
        refused(&c,"projects.trust_approve",json!({"id":project,"expectedCanonicalPath":path,"expectedDigest":digest,"expectedScopeVersion":scope}),"projects.trust_preview_stale").await;
    }
    write(&repo, "# exact byte difference\n[agent]\nsetup=['true']\n");
    refused(&c,"projects.trust_approve",json!({"id":project,"expectedCanonicalPath":view["canonicalPath"],"expectedDigest":view["digest"],"expectedScopeVersion":view["scopeVersion"]}),"projects.trust_preview_stale").await;
    refused(&c,"agents.create",json!({"projectId":project,"title":"Stale","profile":"fixture","worktree":true,"prompt":""}),"agents.project_trust_required").await;
}

#[path = "project_trust_cases/cleanup.rs"]
mod cleanup;
#[path = "project_trust_cases/snapshot.rs"]
mod snapshot;
