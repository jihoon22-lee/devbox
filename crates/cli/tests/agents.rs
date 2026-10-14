mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

const FAKE: &str = r#"#!/usr/bin/env bash
send() { printf '%s' "$1" | "$DEVBOX_BIN" agent event --from claude; }
send '{"hook_event_name":"SessionStart","session_id":"sess-1"}'
send '{"hook_event_name":"UserPromptSubmit","session_id":"sess-1"}'
touch "$HOME/tool-ready"
while [ ! -e "$HOME/send-waiting" ]; do sleep 0.01; done
date +%s%N > "$HOME/waiting-hook.ns"
send '{"hook_event_name":"PermissionRequest","tool_name":"Bash"}'
read -r answer
while [ ! -e "$HOME/send-idle" ]; do sleep 0.01; done
date +%s%N > "$HOME/idle-hook.ns"
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
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    support::bounded(async {
        while !d.home.path().join("tool-ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let mut events = client.subscribe("agents.changed").await.expect("subscribe");
    std::fs::write(d.home.path().join("send-waiting"), "go").expect("owned fixture");
    measure_hook(&d, &mut events, &id, "waiting").await;
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
    client.raw_call("agents.event", json!({"agentId":id,"launchId":d.agent_launch(&id).await,"source":"claude","payload":"{\"hook_event_name\":\"Notification\",\"notification_type\":\"permission_prompt\",\"message\":\"needs Bash\"}"}), None).await.expect("same-state hook");
    support::bounded(events.next_event())
        .await
        .expect("same-state event");
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
    // Drain the submitted instruction's revision before measuring the next hook.
    // Delivery metadata can commit independently of the running transition.
    let list: Value = serde_json::from_str(
        &client
            .raw_call("agents.list", json!({}), None)
            .await
            .expect("send revision"),
    )
    .expect("list");
    let submitted = list["rev"]
        .as_str()
        .expect("revision")
        .parse::<u64>()
        .expect("revision number");
    loop {
        let event = support::bounded(events.next_event())
            .await
            .expect("instruction event");
        if event.rev.parse::<u64>().expect("event revision") >= submitted {
            break;
        }
    }
    std::fs::write(d.home.path().join("send-idle"), "go").expect("owned fixture");
    measure_hook(&d, &mut events, &id, "idle").await;
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

async fn measure_hook(
    d: &TestDaemon,
    events: &mut devbox_cli::client::Subscription,
    id: &str,
    state: &str,
) {
    let event = support::bounded(events.next_event())
        .await
        .expect("hook event");
    let received = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("owned fixture")
        .as_nanos();
    let payload: Value = serde_json::from_str(&event.payload).expect("owned fixture");
    assert_eq!(payload["id"], id);
    let sent: u128 = std::fs::read_to_string(d.home.path().join(format!("{state}-hook.ns")))
        .expect("owned fixture")
        .trim()
        .parse()
        .expect("owned fixture");
    let elapsed = Duration::from_nanos(
        u64::try_from(received.checked_sub(sent).expect("hook before event"))
            .expect("owned fixture"),
    );
    eprintln!(
        "SC3 fixture CLI hook→agents.changed {state} {:.3}ms",
        elapsed.as_secs_f64() * 1000.0
    );
    assert!(elapsed < Duration::from_secs(2), "SC3 hook event deadline");
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
