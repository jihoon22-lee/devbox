mod support;
use serde_json::{Value, json};
use std::{io::Write, process::Stdio, time::Duration};
use support::{ChildGuard, TestDaemon, exits};

fn hook(d: &TestDaemon, id: &str, launch: Option<&str>, payload: Value) {
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "claude"])
        .env("DEVBOX_AGENT_ID", id)
        .env_remove("DEVBOX_AGENT_LAUNCH")
        .stdin(Stdio::piped());
    if let Some(launch) = launch {
        command.env("DEVBOX_AGENT_LAUNCH", launch);
    }
    let mut child = ChildGuard(command.spawn().expect("owned hook"));
    child
        .0
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("hook payload");
    assert!(exits(child).success());
}
async fn task(c: &devbox_cli::client::Client, id: &str) -> Value {
    serde_json::from_str(
        &c.raw_call("agents.get", json!({"id":id}), None)
            .await
            .expect("task"),
    )
    .expect("task JSON")
}
#[tokio::test]
async fn cli_claude_hooks_bind_to_the_resumed_launch_and_reject_missing_generation() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf '%s\n' "$DEVBOX_AGENT_LAUNCH" >> "$HOME/launches"
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"ready"}' | "$DEVBOX_BIN" agent event --from claude
while read -r line; do :; done
"#).expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\nhooks='none'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/launch");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "fixture", false).await;
    d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let old = std::fs::read_to_string(d.home.path().join("launches"))
        .expect("old launch")
        .trim()
        .to_owned();
    d.kill_tmux_server();
    drop(c);
    d.restart();
    let c = d.client().await;
    c.raw_call("agents.resume", json!({"id":id}), None)
        .await
        .expect("actual resume");
    d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let launches = std::fs::read_to_string(d.home.path().join("launches")).expect("launches");
    let current = launches.lines().last().expect("new launch");
    assert_ne!(old, current);
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"replacement"}),
        json!({"hook_event_name":"Stop","last_assistant_message":"finished"}),
        json!({"hook_event_name":"PermissionRequest","tool_name":"Bash"}),
    ] {
        hook(
            &d,
            &id,
            Some(current),
            json!({"hook_event_name":"UserPromptSubmit"}),
        );
        let before = task(&c, &id).await;
        assert_eq!(before["state"], "running");
        for generation in [Some(old.as_str()), None, Some("")] {
            hook(&d, &id, generation, payload.clone());
            assert_eq!(
                task(&c, &id).await,
                before,
                "stale/missing CLI launch must not apply {payload}"
            );
        }
        hook(&d, &id, Some(current), payload.clone());
        let after = task(&c, &id).await;
        match payload["hook_event_name"].as_str().expect("event") {
            "SessionStart" => assert_eq!(after["sessionId"], "replacement"),
            "Stop" => assert_eq!(after["state"], "idle"),
            _ => assert_eq!(after["state"], "waiting"),
        }
    }
}
