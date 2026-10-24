mod support;
use serde_json::{Value, json};
use std::io::Write;
use std::time::Duration;
use support::TestDaemon;
#[path = "mcp_cases/support.rs"]
mod mcp_support;
use mcp_support::{Mcp, body, names};
#[test]
fn offline_initialize_notifications_and_eof_keep_stdout_protocol_only() {
    let d = TestDaemon::fixture();
    let mut m = Mcp::start(&d);
    let init = m.call(json!({"jsonrpc":"2.0","id":"client-id","method":"initialize","params":{"protocolVersion":"custom-client-version","capabilities":{},"clientInfo":{"name":"owned-test","version":"1"}}}));
    assert_eq!(init["id"], "client-id");
    assert_eq!(init["result"]["protocolVersion"], "custom-client-version");
    assert_eq!(init["result"]["capabilities"]["tools"], json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "devbox");
    assert_eq!(init["result"]["serverInfo"]["version"], devbox_cli::VERSION);
    m.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    m.send(json!({"jsonrpc":"2.0","method":"unknown-notification"}));
    m.send(json!({"jsonrpc":"2.0","method":"tools/call","params":false}));
    let rejected_null = m.call(json!({"jsonrpc":"2.0","id":null,"method":"tools/list"}));
    assert_eq!(rejected_null["error"]["code"], -32600);
    assert!(rejected_null["id"].is_null());
    let listed = m.call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    assert_eq!(
        names(&listed),
        [
            "devbox_projects",
            "devbox_agents",
            "devbox_runs",
            "devbox_logs",
            "devbox_problems"
        ]
    );
    let unavailable = m.tool("devbox_projects", json!({}));
    assert_eq!(unavailable["result"]["isError"], true);
    assert!(
        unavailable["result"]["content"][0]["text"]
            .as_str()
            .expect("owned fixture")
            .contains("데몬")
    );
    m.finish();
}
#[test]
fn malformed_requests_recover_with_typed_jsonrpc_errors_and_preserve_ids() {
    let d = TestDaemon::fixture();
    let mut m = Mcp::start(&d);
    for (message, code) in [
        (
            r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{"protocolVersion":"test","capabilities":{},"clientInfo":["name","version"]}}"#,
            -32602,
        ),
        ("{", -32700),
        ("[]", -32600),
        ("true", -32600),
        (r#"{"jsonrpc":"1.0","id":1,"method":"tools/list"}"#, -32600),
        (r#"{"jsonrpc":"2.0","id":{},"method":"tools/list"}"#, -32600),
        (
            r#"{"jsonrpc":"2.0","id":1.5,"method":"tools/list"}"#,
            -32600,
        ),
        (r#"{"jsonrpc":"2.0","id":2,"method":"missing"}"#, -32601),
        (
            r#"{"jsonrpc":"2.0","id":3,"method":"initialize","params":{}}"#,
            -32602,
        ),
        (
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/list","params":[]}"#,
            -32602,
        ),
        (
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"devbox_projects","arguments":{"extra":true}}}"#,
            -32602,
        ),
        (
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"devbox_agent_send","arguments":{"id":"owned","text":1}}}"#,
            -32602,
        ),
    ] {
        m.raw(message);
        let reply = m.read();
        assert_eq!(reply["error"]["code"], code, "{message}: {reply}");
        assert_eq!(reply["jsonrpc"], "2.0");
        assert!(reply.get("result").is_none());
    }
    m.raw(r#"{"jsonrpc":"2.0","id":18446744073709551615,"method":"tools/list"}"#);
    assert_eq!(m.read()["id"].as_u64(), Some(u64::MAX));
    m.raw(r#"{"jsonrpc":"2.0","id":-9223372036854775808,"method":"tools/list"}"#);
    assert_eq!(m.read()["id"].as_i64(), Some(i64::MIN));
    let unknown = m.tool("no-such-tool", json!({}));
    assert_eq!(unknown["error"]["code"], -32602);
    assert_eq!(unknown["id"], 7);
    assert!(unknown.get("result").is_none());
    m.finish();
}
#[test]
fn oversized_and_invalid_utf8_lines_are_bounded_and_recover() {
    let d = TestDaemon::fixture();
    let mut m = Mcp::start(&d);
    m.raw(&"x".repeat(1024 * 1024 + 1));
    assert_eq!(m.read()["error"]["code"], -32700);
    m.input
        .as_mut()
        .expect("owned fixture")
        .write_all(&[0xff, b'\n'])
        .expect("owned fixture");
    assert_eq!(m.read()["error"]["code"], -32700);
    assert_eq!(
        names(&m.call(json!({"jsonrpc":"2.0","id":8,"method":"tools/list"}))),
        [
            "devbox_projects",
            "devbox_agents",
            "devbox_runs",
            "devbox_logs",
            "devbox_problems"
        ]
    );
    m.finish();
}
#[test]
fn local_false_overrides_shared_write_true() {
    let d = TestDaemon::fixture();
    d.write_config("[mcp]\nwrite=true\n");
    std::fs::write(
        d.paths.config_dir.join("config.local.toml"),
        "[mcp]\nwrite=false\n",
    )
    .expect("owned fixture");
    let mut m = Mcp::start(&d);
    assert_eq!(
        names(&m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))),
        [
            "devbox_projects",
            "devbox_agents",
            "devbox_runs",
            "devbox_logs",
            "devbox_problems"
        ]
    );
    assert_eq!(
        m.tool("devbox_agent_send", json!({"id":"missing","text":"deny"}))["result"]["isError"],
        true
    );
    m.finish();
}
fn agent_fixture(write: bool) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("owned-fake.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf '%s' '{"hook_event_name":"PermissionRequest","tool_name":"Bash"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do :; done
"#).expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("owned fixture"))
    );
    d.write_config(&format!("[mcp]\nwrite={write}\n[[agents.profiles]]\nid='owned-fake'\nlabel='테스트'\ncommand={}\nhooks='none'\n", serde_json::to_string(&command).expect("owned fixture")));
    d
}
#[tokio::test]
async fn read_tools_return_projects_and_only_open_tasks_and_denied_send_has_no_effect() {
    let mut d = agent_fixture(false);
    let repo = d.git_repo("projects/app");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "owned-fake", false).await;
    let task = d
        .wait_for(&c, &id, "waiting", Duration::from_secs(10))
        .await;
    // Future closure guarantees agents.list(includeClosedHours=0) still returns
    // this owned row, so MCP must explicitly exclude Closed rather than rely on time.
    let mut closed: devbox_agents::AgentTask =
        serde_json::from_value(task.clone()).expect("owned fixture");
    closed.id = "owned-closed".into();
    closed.state = devbox_agents::state::AgentState::Closed;
    closed.closed_ms = Some(i64::MAX);
    let migrations: Vec<_> = [
        devbox_projects::MIGRATIONS,
        devbox_terminal::MIGRATIONS,
        devbox_agents::MIGRATIONS,
        devbox_runtime::MIGRATIONS,
        devbox_secrets::MIGRATIONS,
        devbox_api::MIGRATIONS,
        devbox_webhooks::MIGRATIONS,
    ]
    .concat();
    let db = devbox_core::Db::open(
        &d.paths.db_path,
        &d.paths.backups_dir,
        devbox_cli::VERSION,
        &migrations,
    )
    .expect("owned fixture");
    db.write(move |tx| devbox_agents::store::put(tx, &closed))
        .await
        .expect("owned fixture");
    let all: Value = serde_json::from_str(
        &c.raw_call("agents.list", json!({"includeClosedHours":0}), None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture");
    assert!(
        all["items"]
            .as_array()
            .expect("owned fixture")
            .iter()
            .any(|t| t["id"] == "owned-closed")
    );
    let mut m = Mcp::start(&d);
    let projects = body(&m.tool("devbox_projects", json!({})));
    assert_eq!(projects["items"][0]["id"], project);
    let agents = body(&m.tool("devbox_agents", json!({})));
    assert_eq!(agents["items"].as_array().expect("owned fixture").len(), 1);
    assert_eq!(agents["items"][0]["id"], id);
    assert_eq!(agents["items"][0]["state"], "waiting");
    assert_eq!(agents["items"][0]["attention"], task["attention"]);
    let denied = m.tool("devbox_agent_send", json!({"id":id,"text":"must not send"}));
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .expect("owned fixture")
            .contains("쓰기")
    );
    let after = d.wait_for(&c, &id, "waiting", Duration::from_secs(2)).await;
    assert_eq!(after["followUps"], json!([]));
    m.finish();
}
#[tokio::test]
async fn enabled_send_reaches_only_owned_fake_agent_once_and_maps_rpc_errors() {
    let mut d = agent_fixture(true);
    let repo = d.git_repo("projects/app");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "owned-fake", false).await;
    d.wait_for(&c, &id, "waiting", Duration::from_secs(10))
        .await;
    let mut m = Mcp::start(&d);
    let tools = m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert_eq!(
        names(&tools),
        [
            "devbox_projects",
            "devbox_agents",
            "devbox_runs",
            "devbox_logs",
            "devbox_problems",
            "devbox_agent_send",
            "devbox_run_task"
        ]
    );
    assert_eq!(
        tools["result"]["tools"][5]["inputSchema"]["required"],
        json!(["id", "text"])
    );
    m.send(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"devbox_agent_send","arguments":{"id":id,"text":"notification must not send"}}}));
    let barrier = m.call(
        json!({"jsonrpc":"2.0","id":12,"method":"tools/list","params":{"_meta":{"owned":"test"}}}),
    );
    assert_eq!(barrier["id"], 12);
    let invalid = m.tool(
        "devbox_agent_send",
        json!({"id":id,"text":"extra must not send","extra":true}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    let sent = body(&m.tool(
        "devbox_agent_send",
        json!({"id":id,"text":"owned instruction"}),
    ));
    assert_eq!(sent["id"], id);
    assert_eq!(
        sent["followUps"].as_array().expect("owned fixture").len(),
        1
    );
    assert_eq!(sent["followUps"][0]["text"], "owned instruction");
    let error = m.tool(
        "devbox_agent_send",
        json!({"id":"missing","text":"owned instruction"}),
    );
    assert_eq!(error["result"]["isError"], true);
    assert_eq!(
        error["result"]["content"][0]["text"],
        "에이전트 작업을 찾을 수 없습니다."
    );
    m.finish();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stalled_daemon_rpc_is_bounded_and_connection_is_closed_at_eof() {
    use devbox_protocol::frame::FrameCodec;
    use devbox_protocol::message::{
        ClientKind, ClientMessage, ServerMessage, json_frame, parse_json,
    };
    use futures::{SinkExt, StreamExt};
    use tokio_util::codec::Framed;
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("owned fixture");
    let peer = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("owned fixture");
        let mut framed = Framed::new(stream, FrameCodec::default());
        let hello = framed
            .next()
            .await
            .expect("owned fixture")
            .expect("owned fixture");
        assert!(matches!(
            parse_json::<ClientMessage>(&hello).expect("owned fixture"),
            ClientMessage::Hello {
                client: ClientKind::Mcp,
                ..
            }
        ));
        framed
            .send(json_frame(
                0,
                &ServerMessage::Welcome {
                    version: devbox_cli::VERSION.into(),
                    protocol: devbox_protocol::PROTOCOL_VERSION,
                    daemon_id: "owned-stalled-fixture".into(),
                },
            ))
            .await
            .expect("owned fixture");
        let request = framed
            .next()
            .await
            .expect("owned fixture")
            .expect("owned fixture");
        assert!(
            matches!(parse_json::<ClientMessage>(&request).expect("owned fixture"), ClientMessage::Request { method, .. } if method == "projects.list")
        );
        // Deliberately withhold the response; MCP must close the cancelled client.
        assert!(
            tokio::time::timeout(Duration::from_secs(7), framed.next())
                .await
                .expect("owned fixture")
                .is_none()
        );
    });
    let mut m = Mcp::start(&d);
    let started = std::time::Instant::now();
    let result = m.tool("devbox_projects", json!({}));
    assert!(started.elapsed() < Duration::from_secs(7));
    assert_eq!(result["result"]["isError"], true);
    assert!(
        result["result"]["content"][0]["text"]
            .as_str()
            .expect("owned fixture")
            .contains("시간이 초과")
    );
    assert_eq!(
        names(&m.call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))),
        [
            "devbox_projects",
            "devbox_agents",
            "devbox_runs",
            "devbox_logs",
            "devbox_problems"
        ]
    );
    m.finish();
    peer.await.expect("owned fixture");
}
