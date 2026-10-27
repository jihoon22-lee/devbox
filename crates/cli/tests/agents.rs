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
    fixture_with(FAKE)
}

fn fixture_with(script: &str) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    let tool = d.home.path().join("fake-tool.sh");
    std::fs::write(&tool, script).expect("owned fake tool");
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
    // Sending is refused during the approval; the approved tool's PostToolUse ends it.
    let approved = json!({"hook_event_name":"PostToolUse","tool_name":"Bash"});
    claude_hook(&d, &client, &id, approved).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
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

/// Deliver a Claude hook the way the hook CLI does, for the agent's current launch.
async fn claude_hook(d: &TestDaemon, client: &devbox_cli::client::Client, id: &str, hook: Value) {
    client
        .raw_call(
            "agents.event",
            json!({"agentId":id,"launchId":d.agent_launch(id).await,"source":"claude","payload":hook.to_string()}),
            None,
        )
        .await
        .expect("owned Claude hook");
}

const RECORDER: &str = r#"#!/usr/bin/env bash
while IFS= read -r line; do printf '%s\n' "$line" >> "$HOME/received"; done
"#;

#[tokio::test]
async fn send_is_refused_while_waiting_and_the_pane_receives_nothing() {
    let mut d = fixture_with(RECORDER);
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let pane = format!("={}:", running["terminalId"].as_str().expect("terminal"));
    let screen = |d: &TestDaemon| d.tmux(&["capture-pane", "-p", "-J", "-S", "-", "-t", &pane]);
    let call = |event: &str, tool: &str, input: Value| json!({"hook_event_name":event,"tool_name":tool,"tool_input":input});
    let make = json!({"command":"make"});
    claude_hook(
        &d,
        &client,
        &id,
        call("PermissionRequest", "Bash", make.clone()),
    )
    .await;
    let waiting = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    // A parallel tool finishing does not answer the approval.
    let other = call("PostToolUse", "Read", json!({"file_path":"/owned/a"}));
    claude_hook(&d, &client, &id, other).await;
    let text = "owned-s7-4-instruction";
    let refused = client
        .raw_call("agents.send", json!({"id":id,"text":text}), None)
        .await
        .expect_err("send while the agent waits for approval");
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
    claude_hook(&d, &client, &id, call("PostToolUse", "Bash", make)).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    let sent: Value = serde_json::from_str(
        &client
            .raw_call("agents.send", json!({"id":id,"text":text}), None)
            .await
            .expect("the same send after the approval"),
    )
    .expect("task");
    assert_eq!(sent["followUps"].as_array().expect("history").len(), 1);
    // The tool reads its input in order: one line proves the refused send never arrived.
    let received = d.home.path().join("received");
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

#[tokio::test]
async fn a_request_that_cannot_be_paired_keeps_sends_refused_until_the_turn_ends() {
    let mut d = fixture_with(RECORDER);
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fake", false).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let pane = format!("={}:", running["terminalId"].as_str().expect("terminal"));
    let screen = |d: &TestDaemon| d.tmux(&["capture-pane", "-p", "-J", "-S", "-", "-t", &pane]);
    let make = json!({"command":"make"});
    let request =
        json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":make});
    claude_hook(&d, &client, &id, request).await;
    // A second dialog whose input is deeper than serde_json's recursion limit: no key.
    let mut deep = json!([]);
    for _ in 0..200 {
        deep = json!([deep]);
    }
    let second =
        json!({"hook_event_name":"PermissionRequest","tool_name":"Write","tool_input":deep});
    claude_hook(&d, &client, &id, second).await;
    let ended = json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":make});
    claude_hook(&d, &client, &id, ended).await;
    let waiting = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    assert_eq!(
        waiting["attention"]["message"],
        "Write 사용 승인이 필요합니다."
    );
    let text = "owned-s7-4-unpaired";
    let refused = client
        .raw_call("agents.send", json!({"id":id,"text":text}), None)
        .await
        .expect_err("send while the second dialog is open");
    assert!(
        matches!(&refused, devbox_cli::client::ClientError::Rpc { code, .. } if code == "agents.awaiting_input"),
        "{refused:?}"
    );
    assert!(!screen(&d).contains(text));
    assert!(
        !d.tmux(&["list-buffers", "-F", "#{buffer_name} #{buffer_sample}"])
            .contains(text)
    );
    // The end of the turn still ends the wait.
    claude_hook(&d, &client, &id, json!({"hook_event_name":"Stop"})).await;
    d.wait_for(&client, &id, "idle", Duration::from_secs(2))
        .await;
    client
        .raw_call("agents.send", json!({"id":id,"text":text}), None)
        .await
        .expect("send after the turn ends");
    let received = d.home.path().join("received");
    support::bounded(async {
        while !std::fs::read_to_string(&received).is_ok_and(|lines| lines.ends_with('\n')) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert_eq!(
        std::fs::read_to_string(&received).expect("received input"),
        format!("{text}\n")
    );
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

#[tokio::test]
async fn claude_task_starts_after_the_devbox_path_changes() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    let tool = d.home.path().join("fake-claude.sh");
    std::fs::write(
        &tool,
        "#!/usr/bin/env bash\nwhile read -r line; do :; done\n",
    )
    .expect("owned fake tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fake-claude'\nlabel='Fake Claude'\ncommand={}\nhooks='claude'\n",
        serde_json::to_string(&command).expect("TOML quoted string")
    ));
    let repo = d.git_repo("projects/update");
    let mut project = None;
    let mut started = Vec::new();
    // Same data directory, a different devbox path per daemon run, as across an update.
    // Each path links to the real binary like the installed `bin/devbox` does.
    for name in ["a", "b"] {
        let devbox = d.home.path().join(name).join("devbox");
        std::fs::create_dir_all(devbox.parent().expect("parent")).expect("owned bin directory");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_devbox"), &devbox)
            .expect("owned devbox link");
        let devbox = devbox.to_str().expect("fixture UTF-8").to_owned();
        d.extra_env.retain(|(key, _)| key != "DEVBOX_TEST_BIN");
        d.extra_env.push(("DEVBOX_TEST_BIN".into(), devbox.clone()));
        d.restart();
        let client = d.client().await;
        if project.is_none() {
            project = Some(d.add_project(&client, &repo).await);
        }
        let project = project.as_deref().expect("project");
        let id = d.create_agent(&client, project, "fake-claude", true).await;
        d.wait_for(&client, &id, "running", Duration::from_secs(10))
            .await;
        started.push((devbox, claude_settings_file(&d, &id)));
    }
    assert_ne!(started[0].1, started[1].1);
    for (devbox, settings) in &started {
        let body = std::fs::read_to_string(settings).expect("owned settings");
        assert!(
            body.contains(&format!("'{devbox}' agent event --from claude")),
            "{settings}: {body}"
        );
    }
}

fn claude_settings_file(d: &TestDaemon, id: &str) -> String {
    let script = d.paths.data_dir.join("agents").join(id).join("start.sh");
    let script = std::fs::read_to_string(script).expect("owned start script");
    let (_, rest) = script
        .split_once("--settings '")
        .expect("Claude settings argument");
    rest.split_once('\'')
        .expect("quoted settings path")
        .0
        .into()
}

#[tokio::test]
async fn start_with_a_missing_tool_fails_before_creating_a_worktree() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    // Neither the fixture's interactive shell (an empty home) nor the daemon PATH has it.
    let missing = "devbox-owned-missing-tool";
    d.write_config(&format!(
        "[[agents.profiles]]\nid='missing'\nlabel='Missing'\ncommand='{missing} --flag'\nhooks='none'\n\
         [[agents.profiles]]\nid='shell-syntax'\nlabel='Shell syntax'\ncommand='MODE=1 {missing}'\nhooks='none'\n"
    ));
    let repo = d.git_repo("projects/missing");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    d.approve_project(&client, &project).await;
    let refused = client
        .raw_call(
            "agents.create",
            json!({"projectId":project,"title":"Missing tool","profile":"missing","worktree":true,"prompt":"fix it"}),
            None,
        )
        .await
        .expect_err("a missing CLI ends the start");
    let devbox_cli::client::ClientError::Rpc { code, detail, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert_eq!(code, "agents.tool_missing");
    let detail: Value =
        serde_json::from_str(detail.as_deref().expect("detail").get()).expect("detail JSON");
    assert_eq!(detail["command"], missing);
    // Nothing was reserved or created: no task, no branch, no worktree.
    let list: Value = serde_json::from_str(
        &client
            .raw_call("agents.list", json!({}), None)
            .await
            .expect("list"),
    )
    .expect("list JSON");
    assert_eq!(list["items"], json!([]), "{list}");
    let git = |args: &[&str]| {
        let output = d
            .environment("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .output()
            .expect("fixture git");
        assert!(output.status.success());
        String::from_utf8(output.stdout).expect("git output")
    };
    assert_eq!(git(&["branch", "--list", "agent/*"]), "");
    assert_eq!(
        git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
    assert!(!std::path::Path::new(&format!("{repo}-agents")).exists());
    // Shell syntax before the program: it runs as written, unresolved, as before.
    let started = client
        .raw_call(
            "agents.create",
            json!({"projectId":project,"title":"Shell syntax","profile":"shell-syntax","worktree":false,"prompt":"fix it"}),
            None,
        )
        .await
        .expect("an unresolved command still starts");
    let task: Value = serde_json::from_str(&started).expect("task JSON");
    assert_eq!(task["profile"], "shell-syntax");
}
