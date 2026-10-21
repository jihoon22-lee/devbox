#[path = "mcp_cases/support.rs"]
mod mcp_support;
mod support;
use mcp_support::{Mcp, body, names};
use serde_json::json;
use support::TestDaemon;

#[test]
fn run_tools_publish_bounded_schemas_and_gate_direct_writes() {
    let d = TestDaemon::fixture();
    let mut m = Mcp::start(&d);
    let list = m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert!(names(&list).contains(&"devbox_runs".to_owned()));
    assert!(names(&list).contains(&"devbox_logs".to_owned()));
    assert!(names(&list).contains(&"devbox_problems".to_owned()));
    assert!(!names(&list).contains(&"devbox_run_task".to_owned()));
    assert!(
        !names(&list)
            .iter()
            .any(|name| name.contains("trust") || name.contains("approve"))
    );
    let logs = list["result"]["tools"]
        .as_array()
        .expect("fixture value")
        .iter()
        .find(|tool| tool["name"] == "devbox_logs")
        .expect("fixture value");
    assert_eq!(
        logs["inputSchema"]["properties"]["lines"]["type"],
        "integer"
    );
    assert_eq!(logs["inputSchema"]["properties"]["lines"]["maximum"], 500);
    for lines in [json!(0), json!(501), json!("10"), json!(1.5), json!(-1)] {
        let result = m.tool("devbox_logs", json!({"runId":"owned", "lines":lines}));
        assert_eq!(result["error"]["code"], -32602);
    }
    let denied = m.tool("devbox_run_task", json!({"projectId":"owned", "id":"task"}));
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .expect("fixture value")
            .contains("쓰기")
    );
    m.finish();
}

#[path = "mcp_cases/server.rs"]
mod server;
use devbox_runtime::*;
use server::{Server, record};
use std::time::Duration;
use support::runtime::{RuntimeFixture, task};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_tools_return_recent_runs_exact_tails_and_problems() {
    let d = TestDaemon::fixture();
    let server = Server::start(&d, None).await;
    let client = d.client().await;
    let project = d.add_project(&client, &d.git_repo("project")).await;
    for i in 0..105 {
        server.seed(&d, &project, &format!("r{i:03}"), i, "").await;
    }
    let text: String = (0..600).map(|i| record(&format!("line-{i:03}"))).collect();
    server.seed(&d, &project, "logs", 106, &text).await;
    server
        .seed(
            &d,
            &project,
            "diagnostics",
            107,
            &record("src/x.ts(10,5): error TS1005: expected"),
        )
        .await;
    let mut m = Mcp::start(&d);
    let runs = body(&m.tool("devbox_runs", json!({})));
    assert_eq!(runs["items"].as_array().expect("fixture value").len(), 100);
    assert_eq!(runs["items"][0]["id"], "diagnostics");
    assert_eq!(runs["items"][99]["id"], "r007");
    for count in [1, 200, 256, 500] {
        let logs = body(&m.tool("devbox_logs", json!({"runId":"logs","lines":count})));
        let items = logs["items"].as_array().expect("fixture value");
        assert_eq!(items.len(), count);
        assert_eq!(items[0]["text"], format!("line-{:03}", 600 - count));
        assert_eq!(items[count - 1]["text"], "line-599");
        assert_eq!(logs["truncated"], true);
    }
    let problems = body(&m.tool("devbox_problems", json!({"projectId":project})));
    assert_eq!(problems["items"][0]["line"], 10);
    assert_eq!(problems["items"][0]["message"], "TS1005: expected");
    assert_eq!(
        body(&m.tool("devbox_runs", json!({"projectId":"missing"})))["items"],
        json!([])
    );
    m.finish();
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_cross_byte_limited_pages_and_bound_the_combined_response() {
    let d = TestDaemon::fixture();
    let server = Server::start(&d, None).await;
    let client = d.client().await;
    let project = d.add_project(&client, &d.git_repo("project")).await;
    let text: String = (0..80)
        .map(|i| record(&format!("line-{i:03} {}", "x".repeat(16000))))
        .collect();
    server.seed(&d, &project, "large", 1, &text).await;
    let mut m = Mcp::start(&d);
    let result = body(&m.tool("devbox_logs", json!({"runId":"large","lines":80})));
    let items = result["items"].as_array().expect("fixture value");
    assert!(items.len() > 1 && items.len() < 80);
    assert!(
        items.last().expect("fixture value")["text"]
            .as_str()
            .expect("fixture value")
            .starts_with("line-079")
    );
    assert!(serde_json::to_vec(&result).expect("fixture result").len() <= 768 * 1024);
    assert_eq!(result["truncated"], true);
    m.finish();
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs owned systemd user fixture"]
async fn mcp_logs_never_include_secret_values() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f.daemon, Some(f.slice.clone())).await;
    let secret = "owned-mcp-fixture-secret-42";
    server
        .services
        .secrets
        .set(devbox_secrets::SetSecret {
            name: "TOKEN".into(),
            value: secret.into(),
        })
        .await
        .expect("fixture result");
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let mut def = task(
        "secret",
        "printf '%s\\n' \"$TOKEN\"; printf 'owned-executed\\n'",
    );
    if let RunDef::Task(def) = &mut def {
        def.secrets = vec!["TOKEN".into()];
    }
    f.define(&client, &project, def, SaveTo::ThisPc).await;
    let run = f.start(&client, &project, "secret").await;
    f.wait(
        &client,
        &project,
        &run.id,
        RunState::Exited,
        Duration::from_secs(10),
    )
    .await;
    let disk = std::fs::read_to_string(
        f.daemon
            .paths
            .data_dir
            .join("runs")
            .join(format!("{}.log", run.id)),
    )
    .expect("fixture result");
    assert!(disk.contains("owned-executed"));
    assert!(disk.contains("[REDACTED]"));
    assert!(!disk.contains(secret));
    let mut m = Mcp::start(&f.daemon);
    let response = m.tool("devbox_logs", json!({"runId":run.id,"lines":500}));
    assert!(!response.to_string().contains(secret));
    let logs = body(&response).to_string();
    assert!(logs.contains("[REDACTED]"));
    assert!(logs.contains("owned-executed"));
    m.finish();
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs owned systemd user fixture"]
async fn mcp_run_task_requires_trust_and_cannot_approve() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    f.daemon.write_config("[mcp]\nwrite=true\n");
    f.daemon.launch();
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let path = f.daemon.home.path().join("project/.devbox/devbox.toml");
    std::fs::create_dir_all(path.parent().expect("fixture value")).expect("fixture result");
    std::fs::write(&path, "[[task]]\nid='owned'\ncommand = 'printf owned'\n")
        .expect("fixture result");
    let mut m = Mcp::start(&f.daemon);
    let list = m.call(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}));
    assert!(names(&list).contains(&"devbox_run_task".to_owned()));
    assert!(
        !names(&list)
            .iter()
            .any(|name| name.contains("trust") || name.contains("approve"))
    );
    for approve in ["projects.trust_approve", "devbox_trust_approve"] {
        assert_eq!(
            m.tool(approve, json!({"id":project}))["error"]["code"],
            -32602
        );
    }
    let denied = m.tool("devbox_run_task", json!({"projectId":project,"id":"owned"}));
    assert_eq!(denied["result"]["isError"], true);
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .expect("fixture value")
            .contains("projects.trust_required")
    );
    assert!(f.runs(&client, &project).await.is_empty());
    assert!(f.units().is_empty());
    f.approve(&client, &project).await;
    m.send(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"devbox_run_task","arguments":{"projectId":project,"id":"owned"}}}));
    m.call(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    assert!(f.runs(&client, &project).await.is_empty());
    assert!(f.units().is_empty());
    std::fs::write(&path, "[[task]]\nid='owned'\ncommand = 'printf changed'\n")
        .expect("fixture result");
    let denied = m.tool("devbox_run_task", json!({"projectId":project,"id":"owned"}));
    assert!(
        denied["result"]["content"][0]["text"]
            .as_str()
            .expect("fixture value")
            .contains("projects.trust_required")
    );
    assert!(f.runs(&client, &project).await.is_empty());
    assert!(f.units().is_empty());
    f.approve(&client, &project).await;
    let started = body(&m.tool("devbox_run_task", json!({"projectId":project,"id":"owned"})));
    assert_eq!(started["defId"], "owned");
    assert_eq!(f.runs(&client, &project).await.len(), 1);
    m.finish();
}

#[path = "mcp_cases/paging.rs"]
mod paging;
