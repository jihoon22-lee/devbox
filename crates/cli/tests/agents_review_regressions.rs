mod support;
use devbox_cli::client::Client;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::TestDaemon;

fn fixture(cleanup: Option<bool>) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        r#"#!/usr/bin/env bash
printf change > agent.txt
git add agent.txt && git commit -qm 'agent change'
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do :; done
"#,
    )
    .expect("owned fixture");
    let setting = cleanup
        .map(|value| format!("cleanup_after_close={value}\n"))
        .unwrap_or_default();
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[agents]\n{setting}[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).expect("command")
    ));
    d
}

async fn call(client: &Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &client
            .raw_call(method, params, None)
            .await
            .expect("fixture RPC"),
    )
    .expect("fixture JSON")
}

async fn start(d: &mut TestDaemon) -> (String, Client, String, String, Value) {
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fixture", true).await;
    let idle = d
        .wait_for(&client, &id, "idle", Duration::from_secs(10))
        .await;
    (repo, client, project, id, idle)
}

async fn merge(client: &Client, id: &str) -> Value {
    let changes = call(client, "agents.changes", json!({"id":id})).await;
    call(
        client,
        "agents.merge",
        json!({
            "id":id,"strategy":"squash",
            "expect":{"headOid":changes["headOid"],"baseOid":changes["baseOid"]}
        }),
    )
    .await
}

fn discard_expect(check: &Value) -> Value {
    let mut expected = json!({
        "headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"]
    });
    if let Some(fingerprint) = check.get("fingerprint") {
        expected["discardFingerprint"] = fingerprint.clone();
    }
    expected
}

#[tokio::test]
async fn generic_terminal_close_rejects_owned_agent_and_retains_session() {
    let mut d = fixture(Some(false));
    let (_, client, _, id, idle) = start(&mut d).await;
    let result = client
        .raw_call("terminal.close", json!({"id":idle["terminalId"]}), None)
        .await;
    assert!(
        result.is_err(),
        "generic close must reject an agent session"
    );
    assert!(format!("{result:?}").contains("terminal.agent_owned"));
    assert_eq!(
        call(&client, "agents.get", json!({"id":id})).await["state"],
        "idle"
    );
    let terminals = call(&client, "terminal.list", json!({})).await;
    assert!(
        terminals["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|row| row["id"] == idle["terminalId"])
    );
}

#[tokio::test]
async fn omitted_cleanup_setting_removes_clean_closed_worktree() {
    let mut d = fixture(None);
    let (_, client, _, id, idle) = start(&mut d).await;
    let closed = merge(&client, &id).await;
    assert_eq!(closed["closedHow"], "merged");
    assert!(closed["cleanupError"].is_null());
    assert!(!Path::new(idle["worktreePath"].as_str().expect("path")).exists());
}

#[tokio::test]
async fn closed_review_keeps_both_oids_and_commits_after_cleanup_and_base_moves() {
    let mut d = fixture(Some(true));
    let (repo, client, _, id, _) = start(&mut d).await;
    let original_changes = call(&client, "agents.changes", json!({"id":id})).await;
    let original_commits = call(&client, "agents.commits", json!({"id":id})).await;
    let original_patch = call(&client, "agents.patch", json!({"id":id,"path":"agent.txt"})).await;
    merge(&client, &id).await;
    std::fs::write(
        Path::new(&repo).join("agent.txt"),
        "unrelated newer content",
    )
    .expect("file");
    d.git(Path::new(&repo), &["add", "agent.txt"]);
    d.git(Path::new(&repo), &["commit", "-qm", "later base commit"]);
    assert_eq!(
        call(&client, "agents.changes", json!({"id":id})).await,
        original_changes
    );
    assert_eq!(
        call(&client, "agents.commits", json!({"id":id})).await,
        original_commits
    );
    assert_eq!(
        call(&client, "agents.patch", json!({"id":id,"path":"agent.txt"})).await,
        original_patch
    );
}

#[tokio::test]
async fn discard_refuses_same_head_dirty_content_and_ignored_changes() {
    let mut d = fixture(Some(false));
    let (_, client, _, id, idle) = start(&mut d).await;
    let wt = Path::new(idle["worktreePath"].as_str().expect("path"));
    std::fs::write(wt.join("agent.txt"), "first dirty contents").expect("file");
    std::fs::write(wt.join(".gitignore"), "ignored/\n").expect("ignore");
    std::fs::create_dir(wt.join("ignored")).expect("ignored directory");
    std::fs::write(wt.join("ignored/cache"), "first ignored contents").expect("ignored file");
    let check = call(&client, "agents.discard_check", json!({"id":id})).await;
    std::fs::write(wt.join("agent.txt"), "changed dirty contents").expect("file");
    let result = client
        .raw_call(
            "agents.discard",
            json!({"id":id,"expect":discard_expect(&check)}),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "changed dirty bytes must preserve the worktree"
    );
    assert!(format!("{result:?}").contains("agents.state_changed"));
    assert!(wt.exists());
    let check = call(&client, "agents.discard_check", json!({"id":id})).await;
    std::fs::write(wt.join("ignored/cache"), "changed ignored contents").expect("ignored file");
    let result = client
        .raw_call(
            "agents.discard",
            json!({"id":id,"expect":discard_expect(&check)}),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "ignored bytes removed by force must be confirmed"
    );
    assert!(format!("{result:?}").contains("agents.state_changed"));
    assert!(wt.join("ignored/cache").exists());
}

#[tokio::test]
async fn discard_refuses_teardown_writing_unconfirmed_contents() {
    let mut d = fixture(Some(false));
    let repo = d.git_repo("projects/app");
    std::fs::create_dir(Path::new(&repo).join(".devbox")).expect("config directory");
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\nteardown=['printf changed > agent.txt']\n",
    )
    .expect("config");
    d.git(Path::new(&repo), &["add", ".devbox/devbox.toml"]);
    d.git(Path::new(&repo), &["commit", "-qm", "teardown"]);
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fixture", true).await;
    let idle = d
        .wait_for(&client, &id, "idle", Duration::from_secs(10))
        .await;
    let check = call(&client, "agents.discard_check", json!({"id":id})).await;
    let result = client
        .raw_call(
            "agents.discard",
            json!({"id":id,"expect":discard_expect(&check)}),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "teardown changes are outside the confirmed snapshot"
    );
    assert!(format!("{result:?}").contains("agents.state_changed"));
    assert_eq!(
        std::fs::read_to_string(
            Path::new(idle["worktreePath"].as_str().expect("path")).join("agent.txt")
        )
        .expect("file"),
        "changed"
    );
}

#[tokio::test]
async fn missing_worktree_does_not_hide_healthy_agent_git_stats() {
    let mut d = fixture(Some(false));
    let (repo, client, project, id, _) = start(&mut d).await;
    let broken = d.create_agent(&client, &project, "fixture", true).await;
    let idle = d
        .wait_for(&client, &broken, "idle", Duration::from_secs(10))
        .await;
    d.git(
        Path::new(&repo),
        &[
            "worktree",
            "remove",
            "--force",
            idle["worktreePath"].as_str().expect("path"),
        ],
    );
    let result = client.raw_call("agents.git_stats", json!({}), None).await;
    assert!(
        result.is_ok(),
        "per-task failures must preserve healthy stats: {result:?}"
    );
    let stats: Value = serde_json::from_str(&result.expect("stats")).expect("JSON");
    assert!(
        stats["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|row| row["id"] == id && row["ahead"] == 1)
    );
    assert!(
        !stats["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|row| row["id"] == broken)
    );
    assert!(
        stats["failures"]
            .as_array()
            .expect("independent diagnostic")
            .iter()
            .any(|row| row["id"] == broken)
    );
}

#[tokio::test]
async fn oversized_prompt_is_rejected_before_task_reservation() {
    let mut d = fixture(Some(false));
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let result = client
        .raw_call(
            "agents.create",
            json!({
                "projectId":project,"title":"Too large","profile":"fixture","worktree":true,
                "prompt":"가".repeat(22_000)
            }),
            None,
        )
        .await;
    assert!(
        result.is_err(),
        "UTF-8 prompt bytes must be checked before commit"
    );
    assert!(format!("{result:?}").contains("agents.input_too_large"));
    assert_eq!(
        call(&client, "agents.list", json!({})).await["items"],
        json!([])
    );
}

fn tmux_failure_wrapper(d: &mut TestDaemon, operation: &str) {
    use std::os::unix::fs::PermissionsExt;
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned wrapper directory");
    let wrapper = bin.join("tmux");
    let marker = d.home.path().join("fail-tmux");
    let count = d.home.path().join("paste-count");
    // Only this fixture daemon sees the wrapper; the real owned tmux server remains unchanged.
    std::fs::write(&wrapper, format!(
        "#!/bin/sh\nfor arg in \"$@\"; do if [ \"$arg\" = paste-buffer ]; then printf x >>{}; fi; done\nif [ -f {} ]; then\n for arg in \"$@\"; do\n  if [ \"$arg\" = '{}' ]; then cat >/dev/null; exit 1; fi\n done\nfi\nexec /usr/bin/tmux \"$@\"\n",
        devbox_core::sh_quote(count.to_str().expect("counter")), devbox_core::sh_quote(marker.to_str().expect("marker")), operation,
    )).expect("owned wrapper");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
        .expect("wrapper permissions");
    let mut path = vec![bin];
    path.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    ));
    d.extra_env.push((
        "PATH".into(),
        std::env::join_paths(path)
            .expect("fixture PATH")
            .to_str()
            .expect("UTF-8 PATH")
            .into(),
    ));
}

#[tokio::test]
async fn failed_load_buffer_preserves_idle_attention_and_instruction_history() {
    let mut d = fixture(Some(false));
    tmux_failure_wrapper(&mut d, "load-buffer");
    let (_, client, _, id, idle) = start(&mut d).await;
    std::fs::write(d.home.path().join("fail-tmux"), "owned failure").expect("marker");
    let result = client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"never delivered"}),
            None,
        )
        .await;
    assert!(result.is_err(), "fixture load failure must be reported");
    let task = call(&client, "agents.get", json!({"id":id})).await;
    assert_eq!(task["state"], idle["state"]);
    assert_eq!(task["attention"], idle["attention"]);
    assert_eq!(task["followUps"], idle["followUps"]);
}

#[tokio::test]
async fn failed_enter_records_unknown_delivery_without_claiming_success_or_retry() {
    let mut d = fixture(Some(false));
    tmux_failure_wrapper(&mut d, "send-keys");
    let (_, client, _, id, _) = start(&mut d).await;
    std::fs::write(d.home.path().join("fail-tmux"), "owned failure").expect("marker");
    let result = client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"ambiguous instruction"}),
            None,
        )
        .await;
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("agents.delivery_unknown"));
    let task = call(&client, "agents.get", json!({"id":id})).await;
    assert_eq!(task["followUps"].as_array().expect("history").len(), 1);
    assert_eq!(task["followUps"][0]["delivery"], "unknown");
}

#[tokio::test]
async fn discarded_review_keeps_database_commits_after_owned_git_objects_are_pruned() {
    let mut d = fixture(Some(false));
    let (repo, client, _, id, _) = start(&mut d).await;
    let commits = call(&client, "agents.commits", json!({"id":id})).await;
    let changes = call(&client, "agents.changes", json!({"id":id})).await;
    let check = call(&client, "agents.discard_check", json!({"id":id})).await;
    call(
        &client,
        "agents.discard",
        json!({"id":id,"expect":discard_expect(&check)}),
    )
    .await;
    d.git(
        Path::new(&repo),
        &["reflog", "expire", "--expire=now", "--all"],
    );
    d.git(Path::new(&repo), &["gc", "--prune=now"]);
    assert_eq!(
        call(&client, "agents.commits", json!({"id":id})).await,
        commits
    );
    assert_eq!(
        call(&client, "agents.changes", json!({"id":id})).await,
        changes
    );
    let patch = client
        .raw_call("agents.patch", json!({"id":id,"path":"agent.txt"}), None)
        .await;
    assert!(
        patch.is_err(),
        "missing frozen objects must be explicit rather than a current-checkout patch"
    );
    assert!(format!("{patch:?}").contains("agents.review_unavailable"));
}

#[tokio::test]
async fn accumulated_history_limit_rejects_before_paste_and_preserves_the_full_history() {
    let mut d = fixture(Some(false));
    let (_, client, _, id, idle) = start(&mut d).await;
    let mut task: devbox_agents::AgentTask = serde_json::from_value(idle).expect("task");
    task.follow_ups = (0..8)
        .map(|index| devbox_agents::FollowUp {
            at_ms: index,
            text: "x".repeat(60 * 1024),
            instruction_id: None,
            delivery: None,
        })
        .collect();
    let expected = serde_json::to_value(&task.follow_ups).expect("history");
    let migrations = [
        devbox_projects::MIGRATIONS,
        devbox_terminal::MIGRATIONS,
        devbox_agents::MIGRATIONS,
        devbox_runtime::MIGRATIONS,
        devbox_secrets::MIGRATIONS,
        devbox_search::MIGRATIONS,
        devbox_activity::MIGRATIONS,
    ]
    .concat();
    let db = devbox_core::Db::open(
        &d.paths.db_path,
        &d.paths.backups_dir,
        devbox_cli::VERSION,
        &migrations,
    )
    .expect("owned DB");
    db.write(move |tx| devbox_agents::store::put(tx, &task))
        .await
        .expect("seed owned history");
    let result = client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"y".repeat(60*1024)}),
            None,
        )
        .await;
    assert!(result.is_err());
    assert!(format!("{result:?}").contains("agents.input_too_large"));
    let after = call(&client, "agents.get", json!({"id":id})).await;
    assert_eq!(
        after["followUps"], expected,
        "rejected input cannot truncate previously accepted history"
    );
    assert_eq!(after["state"], "idle");
}

fn send_persistence_db(d: &TestDaemon) -> devbox_core::Db {
    let migrations = [
        devbox_projects::MIGRATIONS,
        devbox_terminal::MIGRATIONS,
        devbox_agents::MIGRATIONS,
        devbox_runtime::MIGRATIONS,
        devbox_secrets::MIGRATIONS,
        devbox_search::MIGRATIONS,
        devbox_activity::MIGRATIONS,
    ]
    .concat();
    devbox_core::Db::open(
        &d.paths.db_path,
        &d.paths.backups_dir,
        devbox_cli::VERSION,
        &migrations,
    )
    .expect("owned DB")
}
async fn send_finish_failure(ambiguous_paste: bool) {
    let mut d = fixture(Some(false));
    tmux_failure_wrapper(&mut d, "send-keys");
    let (_, client, _, id, _) = start(&mut d).await;
    let db = send_persistence_db(&d);
    db.write(|tx| tx.execute_batch("CREATE TRIGGER fail_send_finish BEFORE UPDATE ON agents_tasks WHEN json_extract(NEW.body,'$.followUps[#-1].delivery') IN ('delivered','unknown') BEGIN SELECT RAISE(FAIL,'fixture finish failure'); END"))
        .await.expect("owned DB trigger");
    if ambiguous_paste {
        std::fs::write(d.home.path().join("fail-tmux"), "owned failure").expect("marker");
    }
    let result = client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"private-body-sentinel"}),
            None,
        )
        .await;
    assert_eq!(
        std::fs::read_to_string(d.home.path().join("paste-count")).expect("actual paste count"),
        "x"
    );
    let task = call(&client, "agents.get", json!({"id":id})).await;
    assert_eq!(task["followUps"][0]["delivery"], "pending");
    assert_eq!(task["followUps"][0]["text"], "private-body-sentinel");
    assert!(
        format!("{result:?}").contains("agents.delivery_unknown"),
        "paste happened before finish DB failure: {result:?}"
    );
}
#[tokio::test]
async fn successful_paste_then_finish_db_failure_reports_delivery_unknown() {
    send_finish_failure(false).await;
}
#[tokio::test]
async fn ambiguous_paste_then_finish_db_failure_still_reports_delivery_unknown() {
    send_finish_failure(true).await;
}
#[tokio::test]
async fn successful_paste_then_final_get_failure_reports_delivery_unknown() {
    let mut d = fixture(Some(false));
    tmux_failure_wrapper(&mut d, "send-keys");
    let (_, client, _, id, _) = start(&mut d).await;
    let db = send_persistence_db(&d);
    db.write(|tx| tx.execute_batch("CREATE TRIGGER fail_send_result AFTER UPDATE ON agents_tasks WHEN json_extract(NEW.body,'$.followUps[#-1].delivery')='delivered' AND json_extract(NEW.body,'$.state')!='fixture-final-get-failure' BEGIN UPDATE agents_tasks SET body=json_set(NEW.body,'$.state','fixture-final-get-failure') WHERE id=NEW.id; END"))
        .await.expect("owned post-commit read failure");
    let result = client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"private-body-sentinel"}),
            None,
        )
        .await;
    assert_eq!(
        std::fs::read_to_string(d.home.path().join("paste-count")).expect("actual paste count"),
        "x"
    );
    let row = db.read(move |tx| tx.query_row("SELECT json_extract(body,'$.followUps[0].delivery'),json_extract(body,'$.followUps[0].text') FROM agents_tasks WHERE id=?1",[id],|row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))).await.expect("durable sent instruction");
    assert_eq!(row, ("delivered".into(), "private-body-sentinel".into()));
    assert!(
        format!("{result:?}").contains("agents.delivery_unknown"),
        "read failure cannot mean failed delivery: {result:?}"
    );
}
