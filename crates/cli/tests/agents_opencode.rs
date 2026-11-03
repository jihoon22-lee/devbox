mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::{ChildGuard, TestDaemon, exits};

/// Plays opencode: keeps what the start script handed it and reads instructions line by line.
const FAKE_OPENCODE: &str = r#"#!/usr/bin/env bash
printf '%s' "$OPENCODE_CONFIG_CONTENT" > "$HOME/opencode-config"
printf '%s\n' "$@" > "$HOME/opencode-args"
while IFS= read -r line; do printf '%s\n' "$line" >> "$HOME/received"; done
"#;

fn fixture() -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    let tool = d.home.path().join("fake-opencode.sh");
    std::fs::write(&tool, FAKE_OPENCODE).expect("owned fake tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='oc'\nlabel='opencode fixture'\ncommand={}\nhooks='opencode'\n",
        serde_json::to_string(&command).expect("TOML quoted string")
    ));
    d
}

/// What the devbox opencode plugin runs for one report.
fn report(d: &TestDaemon, id: &str, payload: &str) {
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "opencode", payload])
        .env("DEVBOX_AGENT_ID", id);
    assert!(exits(ChildGuard(command.spawn().expect("spawn plugin report"))).success());
}

#[tokio::test]
async fn an_opencode_permission_wait_refuses_sends_until_the_plugin_resolves_it() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "oc", false).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let home = d.home.path().to_owned();
    support::bounded(async {
        while !home.join("opencode-args").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    // The session loads devbox's plugin from the private hooks folder, and gets the prompt.
    let config: Value = serde_json::from_str(
        &std::fs::read_to_string(home.join("opencode-config")).expect("plugin config"),
    )
    .expect("config JSON");
    let url = config["plugin"][0].as_str().expect("plugin URL");
    let plugin = std::path::PathBuf::from(url.strip_prefix("file://").expect("file URL"));
    assert_eq!(
        plugin.parent(),
        Some(&*d.paths.data_dir.join("agent-hooks"))
    );
    let name = plugin.file_name().and_then(|n| n.to_str()).expect("name");
    assert!(
        name.starts_with("opencode-plugin-") && name.ends_with(".ts"),
        "{name}"
    );
    assert_eq!(
        std::fs::read_to_string(&plugin).expect("plugin file"),
        include_str!("../../agents/assets/opencode-plugin.ts")
    );
    assert_eq!(
        std::fs::read_to_string(home.join("opencode-args")).expect("tool arguments"),
        "--prompt=fix it\n"
    );
    let pane = format!("={}:", running["terminalId"].as_str().expect("terminal"));
    let screen = |d: &TestDaemon| d.tmux(&["capture-pane", "-p", "-J", "-S", "-", "-t", &pane]);
    report(
        &d,
        &id,
        r#"{"v":1,"event":"working","sessionId":"ses_root"}"#,
    );
    report(&d, &id, r#"{"v":1,"event":"needsInput","message":"bash"}"#);
    let waiting = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    assert_eq!(waiting["sessionId"], "ses_root");
    assert_eq!(waiting["attention"]["message"], "opencode 승인 요청: bash");
    let text = "owned-s7-7-instruction";
    let refused = client
        .raw_call("agents.send", json!({"id":id,"text":text}), None)
        .await
        .expect_err("send while opencode asks for a permission");
    assert!(
        matches!(&refused, devbox_cli::client::ClientError::Rpc { code, .. } if code == "agents.awaiting_input"),
        "{refused:?}"
    );
    let after = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    for field in ["attention", "followUps", "activitySeq"] {
        assert_eq!(after[field], waiting[field], "{field}");
    }
    assert!(!screen(&d).contains(text));
    assert!(
        !d.tmux(&["list-buffers", "-F", "#{buffer_name} #{buffer_sample}"])
            .contains(text)
    );
    report(&d, &id, r#"{"v":1,"event":"inputResolved"}"#);
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    let sent: Value = serde_json::from_str(
        &client
            .raw_call("agents.send", json!({"id":id,"text":text}), None)
            .await
            .expect("the same send after the reply"),
    )
    .expect("task");
    assert_eq!(sent["followUps"].as_array().expect("history").len(), 1);
    // The tool reads its input in order: one line proves the refused send never arrived.
    let received = home.join("received");
    support::bounded(async {
        while !std::fs::read_to_string(&received).is_ok_and(|lines| lines.ends_with('\n'))
            || !screen(&d).contains(text)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&received).expect("received input"),
        format!("{text}\n")
    );
    assert_eq!(screen(&d).matches(text).count(), 1);
}
