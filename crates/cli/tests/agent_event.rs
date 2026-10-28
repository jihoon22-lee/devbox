mod support;
use devbox_protocol::frame::{Frame, FrameCodec};
use devbox_protocol::message::{ServerMessage, json_frame, parse_json};
use futures::{SinkExt, StreamExt};
use serde_json::value::RawValue;
use std::process::Stdio;
use std::time::Duration;
use support::{ChildGuard, TestDaemon, exits};
use tokio::net::UnixStream;
use tokio_util::codec::Framed;

#[test]
fn absent_agent_is_a_noop_without_reading_stdin() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "claude"])
        .env_remove("DEVBOX_AGENT_ID")
        .stdin(Stdio::piped());
    let child = ChildGuard(command.spawn().expect("spawn hook"));
    assert!(exits(child).success());
}

#[test]
fn unavailable_daemon_does_not_fail_tool_hook() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "setup", "--ok"])
        .env("DEVBOX_AGENT_ID", "owned-agent");
    assert!(exits(ChildGuard(command.spawn().expect("spawn hook"))).success());
}

#[test]
fn blocked_stdin_is_bounded_even_when_agent_is_set() {
    let d = TestDaemon::fixture();
    let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
    command
        .args(["agent", "event", "--from", "claude"])
        .env("DEVBOX_AGENT_ID", "owned-agent")
        .stdin(Stdio::piped());
    assert!(exits(ChildGuard(command.spawn().expect("spawn hook"))).success());
}

/// Any version but this build's: the binary a hook of a pre-update session still runs.
const OLDER_VERSION: &str = "1.0.0-rc.0";

#[tokio::test]
async fn rc1_shaped_agent_event_applies_in_the_new_daemon() {
    assert_ne!(OLDER_VERSION, devbox_cli::VERSION);
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        "#!/usr/bin/env bash\nwhile read -r line; do :; done\n",
    )
    .expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\nhooks='none'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/older-hook");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fixture", false).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    let launch = d.agent_launch(&id).await;

    let socket = UnixStream::connect(&d.socket).await.expect("connect hook");
    let mut wire = Framed::new(socket, FrameCodec::default());
    let hello = format!(
        r#"{{"type":"hello","version":"{OLDER_VERSION}","protocol":1,"client":"cli","instance":"{}"}}"#,
        d.instance
    );
    wire.send(raw(&hello)).await.expect("send older Hello");
    // The older client checks Welcome against its own version and protocol before calling.
    match receive(&mut wire).await {
        ServerMessage::Welcome {
            version,
            protocol,
            daemon_id,
        } => {
            assert_eq!(version, OLDER_VERSION);
            assert_eq!(protocol, 1);
            assert!(!daemon_id.is_empty());
        }
        other => panic!("expected Welcome, got {other:?}"),
    }
    // The rc.1 (28eb2a91) `AgentEventParams` field set, as that binary serializes it.
    let params = format!(
        r#"{{"agentId":"{id}","launchId":"{launch}","source":"claude","payload":"{{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}}","code":null}}"#
    );
    let request =
        format!(r#"{{"type":"request","id":1,"method":"agents.event","params":{params}}}"#);
    wire.send(raw(&request)).await.expect("send older event");
    let response = receive(&mut wire).await;
    assert!(
        matches!(
            response,
            ServerMessage::Response {
                id: 1,
                result: Some(_),
                error: None,
                ..
            }
        ),
        "{response:?}"
    );
    let task = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(5))
        .await;
    assert_eq!(task["attention"]["message"], "Bash 사용 승인이 필요합니다.");
}

#[tokio::test]
async fn cli_accepts_opencode_and_pi_sources() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned paths");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux config");
    let tool = d.home.path().join("tool.sh");
    std::fs::write(
        &tool,
        "#!/usr/bin/env bash\nwhile read -r line; do :; done\n",
    )
    .expect("owned tool");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("path"))
    );
    d.write_config(&format!(
        "[[agents.profiles]]\nid='fixture'\nlabel='Fixture'\nhooks='none'\ncommand={}\n",
        serde_json::to_string(&command).expect("command")
    ));
    let repo = d.git_repo("projects/plugin-sources");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "fixture", false).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(10))
        .await;
    // What the opencode plugin and the pi extension run: the report is the last argument.
    let report = |source: &str, payload: &str| {
        let mut command = d.environment(env!("CARGO_BIN_EXE_devbox"));
        command
            .args(["agent", "event", "--from", source, payload])
            .env("DEVBOX_AGENT_ID", &id);
        exits(ChildGuard(command.spawn().expect("spawn plugin report")))
    };
    let opencode = r#"{"v":1,"event":"needsInput","message":"bash"}"#;
    assert!(report("opencode", opencode).success());
    let task = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(5))
        .await;
    assert_eq!(task["attention"]["message"], "opencode 승인 요청: bash");
    assert!(report("opencode", r#"{"v":1,"event":"inputResolved"}"#).success());
    d.wait_for(&client, &id, "running", Duration::from_secs(5))
        .await;
    let trust = r#"{"v":1,"event":"needsInput","message":"project_trust"}"#;
    assert!(report("pi", trust).success());
    let task = d
        .wait_for(&client, &id, "waiting", Duration::from_secs(5))
        .await;
    assert_eq!(
        task["attention"]["message"],
        "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다."
    );
    assert!(
        report(
            "pi",
            r#"{"v":1,"event":"working","sessionId":"pi-session-1"}"#
        )
        .success()
    );
    let task = d
        .wait_for(&client, &id, "running", Duration::from_secs(5))
        .await;
    assert_eq!(task["sessionId"], "pi-session-1");
    // Any other source is a usage error, and reports nothing.
    assert!(!report("paseo", r#"{"v":1,"event":"idle"}"#).success());
    let task = d
        .wait_for(&client, &id, "running", Duration::from_secs(5))
        .await;
    assert_eq!(task["sessionId"], "pi-session-1");
}

fn raw(json: &str) -> Frame {
    json_frame(
        0,
        &RawValue::from_string(json.into()).expect("owned JSON fixture"),
    )
}

async fn receive(wire: &mut Framed<UnixStream, FrameCodec>) -> ServerMessage {
    let frame = support::bounded(wire.next())
        .await
        .expect("daemon frame")
        .expect("decode daemon frame");
    parse_json(&frame).expect("server message")
}
