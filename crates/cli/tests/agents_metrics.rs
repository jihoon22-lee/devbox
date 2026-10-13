mod support;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::TestDaemon;
async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(&c.raw_call(method, params, None).await.unwrap()).unwrap()
}
fn fixture() -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().unwrap();
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .unwrap();
    let tool = d.home.path().join("usage-tool.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf '{"hook_event_name":"SessionStart","session_id":"s-%s"}' "$DEVBOX_AGENT_ID" | "$DEVBOX_BIN" agent event --from claude
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"ready"}' | "$DEVBOX_BIN" agent event --from claude
while read -r answer; do :; done
"#).unwrap();
    let command = format!("bash {}", devbox_core::sh_quote(tool.to_str().unwrap()));
    d.write_config(&format!("[agents]\ncleanup_after_close=true\n[[agents.profiles]]\nid='usage'\nlabel='Usage'\ncommand={}\nhooks='claude'\n", serde_json::to_string(&command).unwrap()));
    d
}
fn usage_file(d: &TestDaemon, task: &Value, input: u64, truncated: bool) -> std::path::PathBuf {
    let cwd = task["workdir"].as_str().unwrap();
    let encoded: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = d.home.path().join(".claude/projects").join(encoded);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}.jsonl", task["sessionId"].as_str().unwrap()));
    let line = json!({"type":"assistant","sessionId":task["sessionId"],"cwd":cwd,"message":{"id":"owned-synthetic-message","usage":{"input_tokens":input,"output_tokens":3,"cache_creation_input_tokens":2,"cache_read_input_tokens":4}}});
    std::fs::write(
        &path,
        format!("{line}\n{}", if truncated { "{partial" } else { "" }),
    )
    .unwrap();
    path
}
#[tokio::test]
async fn discard_freezes_tokens_truncation_and_unavailable_snapshot_across_restart() {
    let mut d = fixture();
    let repo = d.git_repo("projects/metrics");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let mut closed = Vec::new();
    for worktree in [false, true] {
        let id = d.create_agent(&c, &project, "usage", worktree).await;
        let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
        let path = usage_file(&d, &task, 20, true);
        let expected = call(&c, "agents.usage", json!({"id":id})).await;
        assert_eq!(expected["tokens"]["input"], 20);
        assert_eq!(expected["truncated"], true);
        let check = call(&c, "agents.discard_check", json!({"id":id})).await;
        let task = call(&c, "agents.discard", json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"]}})).await;
        assert_eq!(task["tokens"], expected["tokens"]);
        assert_eq!(task["tokensTruncated"], true);
        std::fs::write(path, "invalid replacement\n").unwrap();
        assert_eq!(call(&c, "agents.usage", json!({"id":id})).await, expected);
        closed.push((id, expected));
    }
    let id = d.create_agent(&c, &project, "usage", false).await;
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    call(&c, "agents.discard", json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"]}})).await;
    usage_file(&d, &task, 999, false);
    let missing = json!({"tokens":null,"truncated":false});
    assert_eq!(call(&c, "agents.usage", json!({"id":id})).await, missing);
    closed.push((id, missing));
    drop(c);
    d.restart();
    let c = d.client().await;
    for (id, usage) in closed {
        assert_eq!(call(&c, "agents.usage", json!({"id":id})).await, usage);
    }
}
#[tokio::test]
async fn git_stats_resources_and_merge_usage_follow_owned_task_lifecycle() {
    let mut d = fixture();
    let repo = d.git_repo("projects/merge-metrics");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "usage", true).await;
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let wt = Path::new(task["workdir"].as_str().unwrap());
    std::fs::write(wt.join("change.txt"), "owned change").unwrap();
    d.git(wt, &["add", "change.txt"]);
    d.git(wt, &["commit", "-qm", "owned fixture change"]);
    let stats = call(&c, "agents.git_stats", json!({})).await;
    assert_eq!(stats["items"].as_array().unwrap().len(), 1);
    assert_eq!(stats["items"][0]["id"], id);
    assert_eq!(stats["items"][0]["changedFiles"], 1);
    assert_eq!(stats["items"][0]["ahead"], 1);
    assert_eq!(stats["items"][0]["behind"], 0);
    assert_eq!(
        call(&c, "agents.resources", json!({})).await,
        json!({"items":[]})
    );
    let path = usage_file(&d, &task, 40, false);
    let changes = call(&c, "agents.changes", json!({"id":id})).await;
    let closed = call(&c, "agents.merge", json!({"id":id,"strategy":"squash","expect":{"headOid":changes["headOid"],"baseOid":changes["baseOid"]}})).await;
    assert_eq!(closed["tokens"]["input"], 40);
    assert!(!wt.exists());
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        call(&c, "agents.usage", json!({"id":id})).await["tokens"]["input"],
        40
    );
    assert_eq!(
        call(&c, "agents.git_stats", json!({})).await,
        json!({"items":[]})
    );
}

#[tokio::test]
async fn review_regression_missing_profile_does_not_leave_successful_merge_open() {
    let mut d = fixture();
    let repo = d.git_repo("projects/removed-profile");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "usage", true).await;
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let wt = Path::new(task["workdir"].as_str().unwrap());
    std::fs::write(wt.join("change.txt"), "owned change").unwrap();
    d.git(wt, &["add", "change.txt"]);
    d.git(wt, &["commit", "-qm", "owned fixture change"]);
    drop(c);
    d.stop();
    d.write_config("[agents]\ncleanup_after_close=false\n");
    d.launch();
    let c = d.client().await;
    let changes = call(&c, "agents.changes", json!({"id":id})).await;
    assert_eq!(
        call(&c, "agents.usage", json!({"id":id})).await,
        json!({"tokens":null,"truncated":true})
    );
    let response = c.raw_call("agents.merge", json!({"id":id,"strategy":"merge","expect":{"headOid":changes["headOid"],"baseOid":changes["baseOid"]}}), None).await;
    assert!(
        Path::new(&repo).join("change.txt").exists(),
        "merge already changed the base checkout"
    );
    let task = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(
        task["state"], "closed",
        "completed merge must be recorded closed even if profile was removed; RPC: {response:?}"
    );
    assert!(response.is_ok());
    assert!(task["tokens"].is_null());
    assert_eq!(task["tokensTruncated"], true);
    assert_eq!(
        call(&c, "agents.usage", json!({"id":id})).await,
        json!({"tokens":null,"truncated":true})
    );
}
