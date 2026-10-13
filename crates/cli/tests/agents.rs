mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

const FAKE: &str = r#"#!/usr/bin/env bash
send() { printf '%s' "$1" | "$DEVBOX_BIN" agent event --from claude; }
send '{"hook_event_name":"SessionStart","session_id":"sess-1"}'
send '{"hook_event_name":"UserPromptSubmit","session_id":"sess-1"}'
send '{"hook_event_name":"PermissionRequest","tool_name":"Bash"}'
read -r answer
send '{"hook_event_name":"Stop","session_id":"sess-1","last_assistant_message":"done"}'
while read -r answer; do :; done
"#;

fn fixture() -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    let tool = d.home.path().join("fake-tool.sh");
    std::fs::write(&tool, FAKE).expect("owned fake tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).expect("TOML quoted string")
    ));
    d
}

#[tokio::test]
async fn hooks_update_task_metadata_and_survive_restart_until_session_is_lost() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", true).await;
    let task = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(10))
        .await;
    assert_eq!(task["sessionId"], "sess-1");
    assert!(
        task["worktreePath"]
            .as_str()
            .expect("worktree")
            .ends_with("app-agents/fix-login")
    );
    let started = std::time::Instant::now();
    client.raw_call("agents.event", json!({"agentId":id,"source":"claude","payload":"{\"hook_event_name\":\"Notification\",\"notification_type\":\"permission_prompt\",\"message\":\"needs Bash\"}"}), None).await.expect("same-state hook");
    let task = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    assert_eq!(task["attention"]["message"], "needs Bash");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "SC3 hook-to-query deadline"
    );
    client
        .raw_call("agents.send", json!({"id":id,"text":"yes"}), None)
        .await
        .expect("instruction through agent action");
    d.wait_for(&client, &id, "idle", Duration::from_secs(10))
        .await;
    drop(client);
    d.restart();
    let client = d.client().await;
    d.wait_for(&client, &id, "idle", Duration::from_secs(3))
        .await;
    d.kill_tmux_server();
    drop(client);
    d.restart();
    let client = d.client().await;
    let task = d
        .wait_for(&client, &id, "failed", Duration::from_secs(3))
        .await;
    assert_eq!(task["reason"], "session_lost");
}

#[tokio::test]
async fn failed_setup_is_visible_and_keeps_owned_shell() {
    let mut d = fixture();
    let repo = d.git_repo("projects/setup");
    std::fs::create_dir_all(std::path::Path::new(&repo).join(".devbox"))
        .expect("owned config directory");
    std::fs::write(
        std::path::Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\nsetup=['exit 3']\n",
    )
    .expect("owned setup config");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    let task = d
        .wait_for(&client, &id, "failed", Duration::from_secs(10))
        .await;
    assert_eq!(task["reason"], "setup:3");
    let terminals: Value = serde_json::from_str(
        &client
            .raw_call("terminal.list", json!({}), None)
            .await
            .expect("terminals"),
    )
    .expect("list");
    assert!(
        terminals["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|t| t["id"] == task["terminalId"] && t["alive"] == true)
    );
}
