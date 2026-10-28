mod support;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::{ChildGuard, TestDaemon, exits};

/// Plays pi: keeps its arguments, NUL-separated, reads instructions line by line, and ends on
/// `exit`. It never reports anything, like a pi whose extension did not load.
const FAKE_PI: &str = r#"#!/usr/bin/env bash
printf '%s\0' "$@" > "$HOME/pi-args.part"
mv "$HOME/pi-args.part" "$HOME/pi-args"
while IFS= read -r line; do
  [ "$line" = exit ] && exit 0
  printf '%s\n' "$line" >> "$HOME/received"
done
"#;

/// A profile with pi's hooks whose command is the fake above, never a real `pi`.
fn fixture() -> TestDaemon {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned dirs");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("tmux config");
    let tool = d.home.path().join("fake-pi.sh");
    std::fs::write(&tool, FAKE_PI).expect("owned fake tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='pi-fixture'\nlabel='pi fixture'\ncommand={}\nhooks='pi'\n",
        serde_json::to_string(&command).expect("TOML quoted string")
    ));
    // pi's folder is the one under this fixture's home, whatever the test run has set.
    for key in ["PI_CODING_AGENT_DIR", "PI_CODING_AGENT_SESSION_DIR"] {
        d.extra_env.push((key.into(), String::new()));
    }
    d
}

/// The arguments the fake pi was started with.
async fn pi_args(home: &Path) -> Vec<String> {
    let file = home.join("pi-args");
    support::bounded(async {
        while !file.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let args = std::fs::read_to_string(&file).expect("pi arguments");
    let args = args.strip_suffix('\0').expect("NUL-terminated arguments");
    args.split('\0').map(str::to_owned).collect()
}

/// What the devbox pi extension runs for one report.
fn report(d: &TestDaemon, id: &str, payload: &str) {
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "pi", payload])
        .env("DEVBOX_AGENT_ID", id);
    assert!(exits(ChildGuard(command.spawn().expect("spawn extension report"))).success());
}

#[tokio::test]
async fn pi_session_id_is_assigned_before_the_first_launch() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    d.approve_project(&client, &project).await;
    // pi would attach `@notes.md` as a file; the prompt must reach it as the message.
    let body = json!({"projectId":project,"title":"Fix login","profile":"pi-fixture","worktree":false,"prompt":"@notes.md fix it"});
    let created: Value = serde_json::from_str(
        &client
            .raw_call("agents.create", body, None)
            .await
            .expect("create pi task"),
    )
    .expect("task");
    // The task has its session when it is created, before anything is launched.
    let session = created["sessionId"].as_str().expect("assigned session");
    let uuid = uuid::Uuid::parse_str(session).expect("UUID");
    assert_eq!(uuid.get_version_num(), 4, "{session}");
    assert_eq!(uuid.hyphenated().to_string(), session);
    let id = created["id"].as_str().expect("id");
    let running = d
        .wait_for(&client, id, "running", Duration::from_secs(10))
        .await;
    assert_eq!(running["sessionId"], session);
    // pi loads devbox's extension from the private hooks folder and starts that session.
    let home = d.home.path().to_owned();
    let args = pi_args(&home).await;
    assert_eq!(args.len(), 6, "{args:?}");
    assert_eq!(args[0], "-e");
    let extension = Path::new(&args[1]);
    assert_eq!(
        extension.parent(),
        Some(&*d.paths.data_dir.join("agent-hooks"))
    );
    let name = extension
        .file_name()
        .and_then(|n| n.to_str())
        .expect("name");
    assert!(
        name.starts_with("pi-extension-") && name.ends_with(".ts"),
        "{name}"
    );
    assert_eq!(
        std::fs::read_to_string(extension).expect("extension file"),
        include_str!("../../agents/assets/pi-extension.ts")
    );
    assert_eq!(
        args[2..],
        ["--session-id", session, "--", " @notes.md fix it"]
    );
    // Usage is read from that session's own file, in pi's folder for the task's folder.
    let workdir = running["workdir"].as_str().expect("workdir");
    let project_folder = workdir
        .trim_start_matches('/')
        .replace(['/', '\\', ':'], "-");
    let folder = home
        .join(".pi/agent/sessions")
        .join(format!("--{project_folder}--"));
    std::fs::create_dir_all(&folder).expect("owned pi sessions");
    let recorded = include_str!("../../agents/tests/fixtures/pi-1.3.0/session.jsonl")
        .replace("00000000-0000-4000-8000-000000000001", session)
        .replace("/tmp/devbox-pi-fixture/repo", workdir);
    std::fs::write(
        folder.join(format!("2026-10-28T05-08-56-172Z_{session}.jsonl")),
        recorded,
    )
    .expect("owned session");
    let usage: devbox_agents::AgentUsage = serde_json::from_str(
        &client
            .raw_call("agents.usage", json!({"id":id}), None)
            .await
            .expect("pi usage"),
    )
    .expect("usage");
    let tokens = usage.tokens.expect("tokens");
    assert_eq!(
        (
            tokens.input,
            tokens.output,
            tokens.cache_read,
            tokens.cache_write
        ),
        (1870, 17, 430, 0)
    );
    assert!(!usage.truncated);
    // With no report from pi at all, a resume still goes back to that session, without the prompt.
    std::fs::remove_file(home.join("pi-args")).expect("first arguments");
    client
        .raw_call("agents.send", json!({"id":id,"text":"exit"}), None)
        .await
        .expect("end the fake pi");
    d.wait_for(&client, id, "review", Duration::from_secs(10))
        .await;
    client
        .raw_call("agents.resume", json!({"id":id}), None)
        .await
        .expect("resume the pi task");
    let resumed = pi_args(&home).await;
    assert_eq!(resumed[0], "-e");
    assert_eq!(resumed[1], args[1]);
    assert_eq!(resumed[2..], ["--session-id", session]);
}

#[tokio::test]
async fn a_pi_trust_question_refuses_sends_until_the_user_decides() {
    let mut d = fixture();
    let repo = d.git_repo("projects/app");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "pi-fixture", false).await;
    let running = d
        .wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let home = d.home.path().to_owned();
    assert_eq!(
        pi_args(&home).await.last().map(String::as_str),
        Some("fix it")
    );
    let pane = format!("={}:", running["terminalId"].as_str().expect("terminal"));
    let screen = |d: &TestDaemon| d.tmux(&["capture-pane", "-p", "-J", "-S", "-", "-t", &pane]);
    report(
        &d,
        &id,
        r#"{"v":1,"event":"needsInput","message":"project_trust"}"#,
    );
    let waiting = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(2))
        .await;
    assert_eq!(
        waiting["attention"]["message"],
        "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다."
    );
    // Enter would answer pi's trust question, whose first choice trusts the folder.
    let text = "owned-s7-8-instruction";
    let refused = client
        .raw_call("agents.send", json!({"id":id,"text":text}), None)
        .await
        .expect_err("send while pi asks about trust");
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
    // pi starts its session once the user decides, and the extension reports that.
    report(&d, &id, r#"{"v":1,"event":"inputResolved"}"#);
    d.wait_for(&client, &id, "running", Duration::from_secs(2))
        .await;
    let sent: Value = serde_json::from_str(
        &client
            .raw_call("agents.send", json!({"id":id,"text":text}), None)
            .await
            .expect("the same send after the decision"),
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
