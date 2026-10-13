mod support;
use devbox_cli::client::{Client, ClientStream};
use serde_json::{Value, json};
use std::time::Duration;
use support::{TestDaemon, bounded};

async fn attach(client: &Client, id: &str) -> ClientStream {
    bounded(client.open_stream(
        "terminal.attach",
        json!({"id":id,"cols":100,"rows":30,"readOnly":false}),
    ))
    .await
    .expect("owned fixture")
}
fn contains(bytes: &[u8], marker: &[u8]) -> bool {
    bytes.windows(marker.len()).any(|w| w == marker)
}

#[tokio::test]
async fn terminal_and_agent_reconnect_with_history_and_live_input_after_restart() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        "#!/bin/bash\nwhile read -r answer; do printf 'agent-%s\\n' \"$answer\"; done\n",
    )
    .expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("owned fixture"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='live'\nlabel='Live'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).expect("owned fixture")
    ));
    let repo = d.git_repo("projects/keep");
    d.launch();
    let client = d.client().await;
    let terminal: Value = serde_json::from_str(
        &bounded(client.raw_call(
            "terminal.create",
            json!({"cwd":d.home.path(),"command":"bash --norc --noprofile"}),
            None,
        ))
        .await
        .expect("owned fixture"),
    )
    .expect("owned fixture");
    let terminal_id = terminal["id"].as_str().expect("owned fixture");
    let project = d.add_project(&client, &repo).await;
    let agent_id = d.create_agent(&client, &project, "live", true).await;
    let before = d
        .wait_for(&client, &agent_id, "running", Duration::from_secs(5))
        .await;
    let agent_terminal = before["terminalId"].as_str().expect("owned fixture");
    let terminal_stream = attach(&client, terminal_id).await;
    let agent_stream = attach(&client, agent_terminal).await;
    terminal_stream
        .write(b"printf 'terminal-%s\\n' before\r")
        .await
        .expect("owned fixture");
    agent_stream
        .write(b"before\r")
        .await
        .expect("owned fixture");
    assert!(contains(
        &terminal_stream
            .read_until(b"terminal-before", Duration::from_secs(2))
            .await,
        b"terminal-before"
    ));
    assert!(contains(
        &agent_stream
            .read_until(b"agent-before", Duration::from_secs(2))
            .await,
        b"agent-before"
    ));
    drop((terminal_stream, agent_stream, client));
    d.restart();
    let client = d.client().await;
    let list: Value = serde_json::from_str(
        &bounded(client.raw_call("terminal.list", json!({}), None))
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture");
    for id in [terminal_id, agent_terminal] {
        assert!(
            list["items"]
                .as_array()
                .expect("owned fixture")
                .iter()
                .any(|t| t["id"] == id && t["alive"] == true)
        );
    }
    let after = d
        .wait_for(&client, &agent_id, "running", Duration::from_secs(3))
        .await;
    assert_eq!(before["worktreePath"], after["worktreePath"]);
    assert_eq!(before["terminalId"], after["terminalId"]);
    let terminal_stream = attach(&client, terminal_id).await;
    let agent_stream = attach(&client, agent_terminal).await;
    assert!(contains(
        &terminal_stream
            .read_until(b"terminal-before", Duration::from_secs(2))
            .await,
        b"terminal-before"
    ));
    assert!(contains(
        &agent_stream
            .read_until(b"agent-before", Duration::from_secs(2))
            .await,
        b"agent-before"
    ));
    terminal_stream
        .write(b"printf 'terminal-%s\\n' after\r")
        .await
        .expect("owned fixture");
    agent_stream.write(b"after\r").await.expect("owned fixture");
    assert!(contains(
        &terminal_stream
            .read_until(b"terminal-after", Duration::from_secs(2))
            .await,
        b"terminal-after"
    ));
    assert!(contains(
        &agent_stream
            .read_until(b"agent-after", Duration::from_secs(2))
            .await,
        b"agent-after"
    ));
    eprintln!(
        "SC4 fixture daemon restart: agent+terminal alive, history and live input restored (scope bypass explicit; managed/Windows acceptance separate)"
    );
}
