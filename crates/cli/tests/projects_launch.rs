mod support;
use devbox_cli::client::Client;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::TestDaemon;

async fn call(c: &Client, method: &str, params: Value) -> Value {
    serde_json::from_str(&c.raw_call(method, params, None).await.unwrap()).unwrap()
}

fn fixture(config: &str) -> (TestDaemon, String) {
    let d = TestDaemon::fixture();
    d.write_config(
        "[[agents.profiles]]\nid='fake'\nlabel='Fake'\ncommand='sleep 600'\nhooks='none'\n",
    );
    let repo = d.git_repo("projects/web");
    std::fs::create_dir_all(Path::new(&repo).join(".devbox")).unwrap();
    std::fs::write(Path::new(&repo).join(".devbox/devbox.toml"), config).unwrap();
    (d, repo)
}

#[tokio::test]
async fn launch_creates_configured_terminals_and_inline_agent_and_summary_counts() {
    let (mut d, repo) = fixture(
        "[launch]\nservices=['future-service']\nterminals=[{name='a'},{name='b',command='sleep 600'}]\nagents=[{tool='fake',prompt='initial instruction'}]\n",
    );
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let result = call(&c, "projects.launch", json!({"id":p})).await;
    assert_eq!(result, json!({"terminals":2,"agents":1}));
    let agents = call(&c, "agents.list", json!({})).await;
    let task = &agents["items"][0];
    assert!(task["worktreePath"].is_null());
    assert_eq!(task["prompt"], "initial instruction");
    let id = task["id"].as_str().unwrap();
    d.wait_for(&c, id, "running", Duration::from_secs(10)).await;
    let terms = call(&c, "terminal.list", json!({"projectId":p})).await;
    assert_eq!(terms["items"].as_array().unwrap().len(), 3);
    assert!(
        terms["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"] == "b")
    );
    let summary = call(&c, "projects.summary", json!({})).await;
    let item = &summary["items"][0];
    assert_eq!(item["id"], p);
    assert_eq!(item["branch"], "main");
    assert_eq!(item["dirty"], true);
    assert_eq!(item["terminals"], 3);
    assert_eq!(item["agents"], json!({"open":1,"attention":0}));
    call(&c, "agents.event", json!({"agentId":id,"source":"claude","payload": "{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}"})).await;
    let summary = call(&c, "projects.summary", json!({})).await;
    assert_eq!(
        summary["items"][0]["agents"],
        json!({"open":1,"attention":1})
    );
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    call(&c, "agents.discard", json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"]}})).await;
    let summary = call(&c, "projects.summary", json!({})).await;
    assert_eq!(
        summary["items"][0]["agents"],
        json!({"open":0,"attention":0})
    );
    assert_eq!(summary["items"][0]["terminals"], 2);
}

#[tokio::test]
async fn partial_launch_failure_reports_counts_and_preserves_created_terminal() {
    use std::os::unix::fs::PermissionsExt;
    let (mut d, repo) = fixture(
        "[launch]\nterminals=[{name='retained',command='sleep 600'}]\nagents=[{tool='fake'}]\n",
    );
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let counter = d.home.path().join("branch-queries");
    let git = bin.join("git");
    let real_git = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    // Preflight succeeds; agents.create's later branch query fails after a terminal was created.
    let script = format!(
        "#!/bin/bash\nfor arg in \"$@\"; do\nif [[ $arg == for-each-ref ]]; then\n n=0\n [[ -f {counter} ]] && read -r n < {counter}\n n=$((n+1))\n echo \"$n\" > {counter}\n if [[ $n -gt 1 ]]; then echo private-fixture-marker >&2; exit 1; fi\nfi\ndone\nexec {git} \"$@\"\n",
        counter = devbox_core::sh_quote(counter.to_str().unwrap()),
        git = devbox_core::sh_quote(real_git.trim())
    );
    std::fs::write(&git, script).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
    d.extra_env.push((
        "PATH".into(),
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
    ));
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let error = c
        .raw_call("projects.launch", json!({"id":p}), None)
        .await
        .unwrap_err();
    let debug = format!("{error:?}");
    let devbox_cli::client::ClientError::Rpc {
        code,
        detail: Some(detail),
        ..
    } = error
    else {
        panic!("{debug}")
    };
    assert_eq!(code, "projects.launch_failed");
    let detail: Value = serde_json::from_str(detail.get()).unwrap();
    assert_eq!(detail["terminals"], 1);
    assert_eq!(detail["agents"], 0);
    assert!(!debug.contains("private-fixture-marker"));
    let terms = call(&c, "terminal.list", json!({"projectId":p})).await;
    assert_eq!(terms["items"].as_array().unwrap().len(), 1);
    assert_eq!(terms["items"][0]["title"], "retained");
    assert_eq!(terms["items"][0]["alive"], true);
    assert_eq!(call(&c, "agents.list", json!({})).await["items"], json!([]));
}

#[tokio::test]
async fn launch_validates_every_profile_and_config_before_creating_resources() {
    let (mut d, repo) = fixture(
        "[launch]\nterminals=[{name='must not start'}]\nagents=[{tool='fake'},{tool='missing'}]\n",
    );
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let error = c
        .raw_call("projects.launch", json!({"id":p}), None)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("projects.launch_profile_not_found"));
    assert_eq!(
        call(&c, "terminal.list", json!({})).await["items"],
        json!([])
    );
    assert_eq!(call(&c, "agents.list", json!({})).await["items"], json!([]));
    std::fs::write(
        Path::new(&repo).join(".devbox/devbox.toml"),
        "[agnet]\nsetup=['touch should-not-run']\n",
    )
    .unwrap();
    let cfg = call(&c, "projects.config", json!({"id":p})).await;
    assert!(cfg["error"]["message"].as_str().unwrap().contains("agnet"));
    assert_eq!(cfg["config"]["launch"]["terminals"], json!([]));
    let error = c
        .raw_call("projects.launch", json!({"id":p}), None)
        .await
        .unwrap_err();
    assert!(format!("{error:?}").contains("projects.configuration_invalid"));
    assert!(!Path::new(&repo).join("should-not-run").exists());
}

#[tokio::test]
async fn launch_honors_worktree_and_repeated_launch_uses_distinct_branches() {
    let (mut d, repo) = fixture("[launch]\nagents=[{tool='fake',worktree=true}]\n");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    for _ in 0..2 {
        assert_eq!(
            call(&c, "projects.launch", json!({"id":p})).await,
            json!({"terminals":0,"agents":1})
        );
    }
    let tasks = call(&c, "agents.list", json!({})).await;
    let tasks = tasks["items"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert_ne!(tasks[0]["branch"], tasks[1]["branch"]);
    for task in tasks {
        assert!(task["worktreePath"].is_string());
        d.wait_for(
            &c,
            task["id"].as_str().unwrap(),
            "running",
            Duration::from_secs(10),
        )
        .await;
    }
}

#[tokio::test]
async fn summary_handles_plain_and_unavailable_projects_independently() {
    let mut d = TestDaemon::fixture();
    let repo = d.git_repo("projects/git");
    let plain = d.home.path().join("projects/plain");
    std::fs::create_dir_all(&plain).unwrap();
    d.launch();
    let c = d.client().await;
    let git = d.add_project(&c, &repo).await;
    let nongit = d.add_project(&c, plain.to_str().unwrap()).await;
    std::fs::remove_dir(&plain).unwrap();
    let summary = call(&c, "projects.summary", json!({})).await;
    let items = summary["items"].as_array().unwrap();
    let git_item = items.iter().find(|i| i["id"] == git).unwrap();
    assert_eq!(git_item["branch"], "main");
    assert_eq!(git_item["dirty"], false);
    let missing = items.iter().find(|i| i["id"] == nongit).unwrap();
    assert!(missing["dirty"].is_null());
    assert!(missing["error"].is_object());
    std::fs::create_dir(&plain).unwrap();
    let summary = call(&c, "projects.summary", json!({})).await;
    let plain = summary["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == nongit)
        .unwrap();
    assert!(plain["branch"].is_null());
    assert_eq!(plain["dirty"], false);
    assert_eq!(plain["ahead"], 0);
    assert!(plain["error"].is_null());
}
