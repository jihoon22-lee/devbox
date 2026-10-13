mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

async fn notify(
    client: &devbox_cli::client::Client,
    id: &str,
    cwd: &str,
    thread: &str,
    turn: &str,
) {
    client.raw_call("agents.event", json!({"agentId":id,"source":"codex","payload":json!({"type":"agent-turn-complete","thread-id":thread,"turn-id":turn,"cwd":cwd,"last-assistant-message":"done"}).to_string()}), None).await.expect("completion candidate");
}
fn timestamp(d: &TestDaemon) -> String {
    let output = d
        .environment("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%S.%3NZ"])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("fixture clock");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("timestamp")
        .trim()
        .into()
}
fn record(time: &str, kind: &str, payload: Value) -> String {
    json!({"timestamp":time,"type":kind,"payload":payload}).to_string() + "\n"
}
#[tokio::test]
async fn exact_primary_thread_and_current_turn_gate_completion_over_rpc() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux conf");
    let tool = d.home.path().join("hold.sh");
    std::fs::write(&tool, "while read -r line; do :; done\n").expect("owned fake");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fake-codex'\nlabel='Fake'\ncommand={}\nhooks='codex'\n",
        serde_json::to_string(&command).expect("quote")
    ));
    let repo = d.git_repo("projects/codex");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake-codex", true).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(5))
        .await;
    let cwd = running["workdir"].as_str().expect("workdir");
    let missing = client.raw_call("agents.event", json!({"agentId":id,"source":"codex","payload":json!({"type":"agent-turn-complete","thread-id":"primary","turn-id":"one","cwd":cwd}).to_string()}), None).await.expect_err("missing metadata rejected");
    assert!(format!("{missing:?}").contains("agents.session_metadata"));
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    let time = timestamp(&d);
    let dir = d
        .home
        .path()
        .join(".codex/sessions")
        .join(time[..10].replace('-', "/"));
    std::fs::create_dir_all(&dir).expect("owned rollout dir");
    let file = dir.join("rollout-owned-primary.jsonl");
    let mut body = record(
        &time,
        "session_meta",
        json!({"id":"primary","timestamp":time,"cwd":cwd,"source":"cli","thread_source":"user"}),
    );
    body += &record(
        &time,
        "event_msg",
        json!({"type":"task_started","turn_id":"one","started_at":time}),
    );
    body += &record(&time, "turn_context", json!({"turn_id":"one","cwd":cwd}));
    std::fs::write(&file, &body).expect("owned rollout");
    notify(&client, &id, cwd, "automatic-title", "one").await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    assert_eq!(running["sessionId"], "primary");
    notify(&client, &id, cwd, "primary", "one").await;
    d.wait_for(&client, &id, "idle", Duration::from_secs(2))
        .await;
    let time = timestamp(&d);
    body += &record(
        &time,
        "event_msg",
        json!({"type":"task_started","turn_id":"two","started_at":time}),
    );
    body += &record(&time, "turn_context", json!({"turn_id":"two","cwd":cwd}));
    std::fs::write(&file, &body).expect("new turn");
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    client.raw_call("agents.event",json!({"agentId":id,"source":"claude","payload":"{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}"}),None).await.expect("new turn waiting");
    notify(&client, &id, cwd, "primary", "one").await;
    d.wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    notify(&client, &id, cwd, "primary", "two").await;
    d.wait_for(&client, &id, "idle", Duration::from_secs(2))
        .await;
}
