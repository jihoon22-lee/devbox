mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

fn fixture(tool: &str) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let script = d.home.path().join("tool.sh");
    std::fs::write(&script, format!("#!/usr/bin/env bash\n{tool}\n")).expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(script.to_str().expect("owned fixture"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand={}\nhooks='none'\n",
        serde_json::to_string(&command).expect("owned fixture")
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
                    .expect("owned fixture"),
            )
            .expect("owned fixture");
            if task["terminalId"].is_string() {
                break task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let terminal = task["terminalId"]
        .as_str()
        .expect("owned fixture")
        .to_owned();
    let list: Value = serde_json::from_str(
        &client
            .raw_call("terminal.list", json!({}), None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture");
    assert!(
        list["items"]
            .as_array()
            .expect("owned fixture")
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
            .expect("owned fixture")
            .lines()
            .count()
            != 2
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let launches = std::fs::read_to_string(d.home.path().join("launches")).expect("owned fixture");
    let stale = launches.lines().next().expect("owned fixture");
    client
        .raw_call(
            "agents.event",
            json!({"agentId":id,"source":"exit","code":137,"payload":stale}),
            None,
        )
        .await
        .expect("owned fixture");
    client
        .raw_call(
            "agents.event",
            json!({"agentId":id,"source":"exit","code":137,"payload":""}),
            None,
        )
        .await
        .expect("owned fixture");
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    client
        .raw_call("agents.review", json!({"id":id}), None)
        .await
        .expect("owned fixture");
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
        devbox_core::sh_quote(sentinel.to_str().expect("owned fixture"))
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
        .expect("owned fixture");
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
        .expect("owned fixture");
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
    std::fs::write(d.home.path().join("exit-now"), "go").expect("owned fixture");
    support::bounded(async {
        loop {
            let phases = std::fs::read_dir(d.paths.data_dir.join("agents").join(&id))
                .expect("owned fixture");
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
    std::fs::create_dir(&bin).expect("owned fixture");
    let tmux = std::env::split_paths(&std::env::var_os("PATH").expect("owned fixture"))
        .map(|dir| dir.join("tmux"))
        .find(|path| path.is_file())
        .expect("installed tmux");
    let wrapper = bin.join("tmux");
    std::fs::write(&wrapper, format!(
        "#!/usr/bin/env bash\nfor arg in \"$@\"; do\n if [ \"$arg\" = kill-session ] && [ -e \"$HOME/fail-kill\" ]; then\n  rm \"$HOME/fail-kill\"; echo 'fixture requested kill failure' >&2; exit 1\n fi\ndone\nexec {} \"$@\"\n",
        devbox_core::sh_quote(tmux.to_str().expect("owned fixture"))
    )).expect("owned fixture");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("owned fixture");
    d.extra_env.push((
        "PATH".into(),
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").expect("owned fixture")
        ),
    ));
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    let failed = d
        .wait_for(&client, &id, "failed", Duration::from_secs(10))
        .await;
    let old = terminal_alive(&d, &client, &id).await;
    std::fs::write(d.home.path().join("fail-kill"), "one failure").expect("owned fixture");
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
            .expect("owned fixture"),
    )
    .expect("owned fixture");
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
