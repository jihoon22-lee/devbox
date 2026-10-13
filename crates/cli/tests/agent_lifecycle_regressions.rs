mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

fn fixture(tool: &str) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().unwrap();
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .unwrap();
    let script = d.home.path().join("tool.sh");
    std::fs::write(&script, format!("#!/usr/bin/env bash\n{tool}\n")).unwrap();
    let command = format!("bash {}", devbox_core::sh_quote(script.to_str().unwrap()));
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).unwrap()
    ));
    d
}

async fn terminal_alive(d: &TestDaemon, client: &devbox_cli::client::Client, id: &str) -> String {
    let task: Value = support::bounded(async {
        loop {
            let task: Value = serde_json::from_str(
                &client
                    .raw_call("agents.get", json!({"id":id}), None)
                    .await
                    .unwrap(),
            )
            .unwrap();
            if task["terminalId"].is_string() {
                break task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let terminal = task["terminalId"].as_str().unwrap().to_owned();
    let list: Value = serde_json::from_str(
        &client
            .raw_call("terminal.list", json!({}), None)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == terminal && s["alive"] == true)
    );
    assert!(d.home.path().exists());
    terminal
}

#[tokio::test]
async fn killed_tool_resumes_with_the_owned_terminal_still_alive() {
    let mut d = fixture(
        "printf '%s\\n' \"$DEVBOX_AGENT_LAUNCH\" >> \"$HOME/launches\"\nif [ ! -e \"$HOME/started\" ]; then touch \"$HOME/started\"; exit 137; fi\nwhile read -r line; do printf '%s\\n' \"$line\" >> \"$HOME/instructions\"; done",
    );
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", true).await;
    let failed = d
        .wait_for(&client, &id, "failed", Duration::from_secs(10))
        .await;
    assert_eq!(failed["reason"], "killed:KILL");
    let old_terminal = terminal_alive(&d, &client, &id).await;
    client
        .raw_call("agents.resume", json!({"id":id}), None)
        .await
        .expect("resume replaces only the ended tool holder");
    let resumed = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    assert_ne!(resumed["terminalId"], old_terminal);
    support::bounded(async {
        while std::fs::read_to_string(d.home.path().join("launches"))
            .unwrap()
            .lines()
            .count()
            != 2
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let launches = std::fs::read_to_string(d.home.path().join("launches")).unwrap();
    let stale = launches.lines().next().unwrap();
    client
        .raw_call(
            "agents.event",
            json!({"agentId":id,"source":"exit","code":137,"payload":stale}),
            None,
        )
        .await
        .unwrap();
    client
        .raw_call(
            "agents.event",
            json!({"agentId":id,"source":"exit","code":137,"payload":""}),
            None,
        )
        .await
        .unwrap();
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    client
        .raw_call("agents.review", json!({"id":id}), None)
        .await
        .unwrap();
    client
        .raw_call(
            "agents.send",
            json!({"id":id,"text":"live review instruction"}),
            None,
        )
        .await
        .expect("a reviewed live tool still accepts instructions");
}

#[tokio::test]
async fn ended_agent_rejects_prose_and_its_holder_never_executes_queued_input() {
    let mut d = fixture("exit 137");
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    d.wait_for(&client, &id, "failed", Duration::from_secs(10))
        .await;
    let terminal = terminal_alive(&d, &client, &id).await;
    let sentinel = d.home.path().join("must-not-execute");
    let instruction = format!(
        "touch {}",
        devbox_core::sh_quote(sentinel.to_str().unwrap())
    );
    let rejected = client
        .raw_call("agents.send", json!({"id":id,"text":instruction}), None)
        .await;
    // Model input already queued when the tool exits, including input outside agents.send.
    let status = d
        .environment("tmux")
        .args([
            "-L",
            &format!("devbox-{}", d.instance),
            "send-keys",
            "-t",
            &format!("={terminal}:"),
            &instruction,
            "Enter",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !sentinel.exists(),
        "ended holder must never execute prose as a shell command"
    );
    assert!(rejected.is_err(), "agents.send must reject an ended tool");
}

#[tokio::test]
async fn live_tool_cannot_be_replaced_even_after_user_review() {
    let mut d = fixture("while read -r line; do :; done");
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let terminal = terminal_alive(&d, &client, &id).await;
    client
        .raw_call("agents.review", json!({"id":id}), None)
        .await
        .unwrap();
    assert!(
        client
            .raw_call("agents.resume", json!({"id":id}), None)
            .await
            .is_err()
    );
    assert_eq!(terminal_alive(&d, &client, &id).await, terminal);
}

#[tokio::test]
async fn marker_recovers_an_exit_hook_lost_while_the_daemon_is_stopped() {
    let mut d = fixture("while [ ! -e \"$HOME/exit-now\" ]; do sleep 0.01; done\nexit 137");
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    terminal_alive(&d, &client, &id).await;
    drop(client);
    d.stop();
    std::fs::write(d.home.path().join("exit-now"), "go").unwrap();
    support::bounded(async {
        loop {
            let phases = std::fs::read_dir(d.paths.data_dir.join("agents").join(&id)).unwrap();
            if phases.filter_map(Result::ok).any(|file| {
                file.path().extension().is_some_and(|v| v == "phase")
                    && std::fs::read_to_string(file.path())
                        .is_ok_and(|text| text.trim() == "exit:137")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    d.launch();
    let client = d.client().await;
    let ended = d
        .wait_for(&client, &id, "failed", Duration::from_secs(3))
        .await;
    assert_eq!(ended["reason"], "killed:KILL");
    assert_eq!(ended["toolExited"], true);
    terminal_alive(&d, &client, &id).await;
    client
        .raw_call("agents.resume", json!({"id":id}), None)
        .await
        .expect("owned marker permits replacing the holder after a lost hook");
}

#[tokio::test]
async fn failed_holder_close_keeps_old_launch_evidence_so_resume_can_retry() {
    use std::os::unix::fs::PermissionsExt;
    let mut d = fixture(
        "if [ ! -e \"$HOME/started\" ]; then touch \"$HOME/started\"; exit 137; fi\nwhile read -r line; do :; done",
    );
    let repo = d.git_repo("projects/app");
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let tmux = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("tmux"))
        .find(|path| path.is_file())
        .expect("installed tmux");
    let wrapper = bin.join("tmux");
    std::fs::write(&wrapper, format!(
        "#!/usr/bin/env bash\nfor arg in \"$@\"; do\n if [ \"$arg\" = kill-session ] && [ -e \"$HOME/fail-kill\" ]; then\n  rm \"$HOME/fail-kill\"; echo 'fixture requested kill failure' >&2; exit 1\n fi\ndone\nexec {} \"$@\"\n",
        devbox_core::sh_quote(tmux.to_str().unwrap())
    )).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    d.extra_env.push((
        "PATH".into(),
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
    ));
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    let failed = d
        .wait_for(&client, &id, "failed", Duration::from_secs(10))
        .await;
    let old = terminal_alive(&d, &client, &id).await;
    std::fs::write(d.home.path().join("fail-kill"), "one failure").unwrap();
    let rejected = client
        .raw_call("agents.resume", json!({"id":id}), None)
        .await;
    assert!(rejected.is_err(), "the first owned holder stop must fail");
    assert!(
        !d.home.path().join("fail-kill").exists(),
        "failure was injected at kill-session"
    );
    assert_eq!(terminal_alive(&d, &client, &id).await, old);
    let current: Value = serde_json::from_str(
        &client
            .raw_call("agents.get", json!({"id":id}), None)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(current["toolExited"], failed["toolExited"]);
    assert_eq!(current["state"], failed["state"]);
    client
        .raw_call("agents.resume", json!({"id":id}), None)
        .await
        .expect("retry uses preserved ended-launch evidence");
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    assert_ne!(terminal_alive(&d, &client, &id).await, old);
}
