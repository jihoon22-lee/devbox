mod support;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::runtime::RuntimeFixture;

async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(&c.raw_call(method, params, None).await.expect("owned RPC")).expect("JSON")
}
fn fixture() -> RuntimeFixture {
    let d = support::TestDaemon::fixture();
    d.paths.ensure_dirs().unwrap();
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .unwrap();
    d.write_config("[[agents.profiles]]\nid='fake'\nlabel='Fixture'\ncommand='bash -c \"while read -r line; do :; done\"'\nhooks='none'\n");
    RuntimeFixture::with_daemon(d)
}
#[tokio::test]
async fn agents_test_runs_in_the_agent_worktree() {
    let f = fixture();
    let c = f.daemon.client().await;
    let repo = f.daemon.git_repo("projects/agent-test");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).unwrap();
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\ntest='pwd > test-cwd.txt; printf agent-test-output'\n",
    )
    .unwrap();
    let p = f.daemon.add_project(&c, &repo).await;
    let agent = f.daemon.create_agent(&c, &p, "fake", true).await;
    let task = f
        .daemon
        .wait_for(&c, &agent, "running", Duration::from_secs(5))
        .await;
    let result = call(&c, "agents.test", json!({"id":agent})).await;
    assert!(
        result["runId"].is_string(),
        "agents.test must return a runtime runId, got {result}"
    );
    let run_id = result["runId"].as_str().unwrap();
    let run = f
        .wait(
            &c,
            &p,
            run_id,
            devbox_runtime::RunState::Exited,
            Duration::from_secs(5),
        )
        .await;
    assert_eq!(run.exit.unwrap().code, Some(0));
    let wt = task["worktreePath"].as_str().unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(wt).join("test-cwd.txt"))
            .unwrap()
            .trim(),
        wt
    );
    assert!(!Path::new(&repo).join("test-cwd.txt").exists());
    assert_eq!(run.def_id, "[agent] test");
    assert_eq!(serde_json::to_value(run.kind).unwrap(), "agentTest");
    let detail = call(&c, "runtime.get", json!({"runId":run_id})).await;
    assert_eq!(detail["run"]["id"], run_id);
}

#[tokio::test]
async fn agent_test_does_not_shadow_or_restart_a_same_named_task() {
    use devbox_runtime::{ListRuns, RunState, RuntimeList, SaveTo};
    let f = fixture();
    let c = f.daemon.client().await;
    let repo = f.daemon.git_repo("projects/identity");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).unwrap();
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\ntest='printf agent > result'\n",
    )
    .unwrap();
    let p = f.daemon.add_project(&c, &repo).await;
    f.define(
        &c,
        &p,
        support::runtime::task("[agent] test", "printf ordinary > ordinary-result"),
        SaveTo::ThisPc,
    )
    .await;
    let ordinary = f.start(&c, &p, "[agent] test").await;
    f.wait(
        &c,
        &p,
        &ordinary.id,
        RunState::Exited,
        Duration::from_secs(5),
    )
    .await;
    let agent = f.daemon.create_agent(&c, &p, "fake", true).await;
    let task = f
        .daemon
        .wait_for(&c, &agent, "running", Duration::from_secs(5))
        .await;
    let first = call(&c, "agents.test", json!({"id":agent})).await;
    f.wait(
        &c,
        &p,
        first["runId"].as_str().unwrap(),
        RunState::Exited,
        Duration::from_secs(5),
    )
    .await;
    let list = c
        .call::<RuntimeList>(&ListRuns {
            project_id: p.clone(),
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(list.runs.len(), 1);
    assert_eq!(
        list.runs[0].id, ordinary.id,
        "agent tests cannot become ordinary task representatives"
    );
    let before = call(&c, "runtime.runs", json!({"projectId":p,"limit":100})).await;
    let rejected = c
        .raw_call("runtime.restart", json!({"runId":first["runId"]}), None)
        .await
        .unwrap_err();
    assert!(
        matches!(rejected, devbox_cli::client::ClientError::Rpc { code, .. } if code == "runtime.def_invalid")
    );
    assert_eq!(
        call(&c, "runtime.runs", json!({"projectId":p,"limit":100})).await["items"],
        before["items"]
    );
    let second = call(&c, "agents.test", json!({"id":agent})).await;
    assert_ne!(first["runId"], second["runId"]);
    f.wait(
        &c,
        &p,
        second["runId"].as_str().unwrap(),
        RunState::Exited,
        Duration::from_secs(5),
    )
    .await;
    let wt = Path::new(task["worktreePath"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(wt.join("result")).unwrap(), "agent");
    assert!(!wt.join("ordinary-result").exists());
    let location = call(&c, "observe.resolve_location", json!({"projectId":p,"source":{"kind":"run","runId":second["runId"]},"path":"result","line":1,"column":null})).await;
    assert_eq!(
        location["path"],
        wt.join("result").to_str().unwrap(),
        "run source cwd resolves the owned worktree file"
    );
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\ntest='touch stale-command'\n",
    )
    .unwrap();
    let rejected = c
        .raw_call("agents.test", json!({"id":agent}), None)
        .await
        .unwrap_err();
    assert!(
        matches!(rejected, devbox_cli::client::ClientError::Rpc { code, .. } if code == "agents.project_trust_required")
    );
    assert!(!wt.join("stale-command").exists());
}
