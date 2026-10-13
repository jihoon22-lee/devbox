mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;
#[tokio::test]
async fn quiet_output_waits_and_resumes_while_hook_attention_survives_redraw() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().unwrap();
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .unwrap();
    let tool = d.home.path().join("output-tool.sh");
    std::fs::write(
        &tool,
        r#"#!/usr/bin/env bash
printf '\nApprove this edit? (y/n)\n'
while [ ! -e "$HOME/go-$DEVBOX_AGENT_ID" ]; do sleep 0.05; done
printf '\033[2J\033[Hworking\n1\n2\n3\n4\n5\n6\n7\n'
while read -r answer; do :; done
"#,
    )
    .unwrap();
    let command = format!("bash {}", devbox_core::sh_quote(tool.to_str().unwrap()));
    d.write_config(&format!("[[agents.profiles]]\nid='output'\nlabel='Output'\ncommand={}\nhooks='none'\nwaitingPatterns=['(?i)approve this edit']\n", serde_json::to_string(&command).unwrap()));
    let repo = d.git_repo("projects/output");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let auto = d.create_agent(&c, &project, "output", false).await;
    let hooked = d.create_agent(&c, &project, "output", false).await;
    for id in [&auto, &hooked] {
        let task = d.wait_for(&c, id, "waiting", Duration::from_secs(12)).await;
        assert_eq!(task["attention"]["kind"], "output");
    }
    c.raw_call("agents.event", json!({"agentId":hooked,"source":"claude","payload":"{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}"}), None).await.unwrap();
    for id in [&auto, &hooked] {
        std::fs::write(d.home.path().join(format!("go-{id}")), "go").unwrap();
    }
    d.wait_for(&c, &auto, "running", Duration::from_secs(12))
        .await;
    let hooked: Value = serde_json::from_str(
        &c.raw_call("agents.get", json!({"id":hooked}), None)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(hooked["state"], "waiting");
    assert_eq!(hooked["attention"]["kind"], "input");
}
