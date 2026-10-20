mod support;
use serde_json::{Value, json};
use std::time::Duration;
use support::TestDaemon;

fn fixture(cleanup: bool) -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("commit.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf '{"hook_event_name":"SessionStart","session_id":"fake-%s"}' "$$" | "$DEVBOX_BIN" agent event --from claude
if [ ! -f agent.txt ]; then
 echo change > agent.txt
 git add agent.txt
 git commit -qm 'agent change'
fi
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude; done
"#).expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("owned fixture"))
    );
    d.write_config(&format!("[agents]\ncleanup_after_close={cleanup}\n[[agents.profiles]]\nid='commit'\nlabel='C'\ncommand={}\nhooks='none'\n",serde_json::to_string(&command).expect("owned fixture")));
    d
}
async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &c.raw_call(method, params, None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture")
}
async fn expect(c: &devbox_cli::client::Client, id: &str) -> Value {
    let ch = call(c, "agents.changes", json!({"id":id})).await;
    json!({"headOid":ch["headOid"],"baseOid":ch["baseOid"]})
}
#[tokio::test]
async fn squash_merge_freezes_record_and_cleans_owned_worktree() {
    let mut d = fixture(true);
    let repo = d.git_repo("projects/app");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let reviewed = expect(&c, &id).await;
    let closed = call(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"squash","message":"change","expect":reviewed}),
    )
    .await;
    assert_eq!(closed["closedHow"], "merged");
    assert_eq!(closed["closedCommits"], 1);
    assert!(closed["closedMs"].as_i64().is_some());
    assert!(std::path::Path::new(&repo).join("agent.txt").exists());
    assert!(!std::path::Path::new(idle["worktreePath"].as_str().expect("owned fixture")).exists());
}

#[tokio::test]
async fn instructions_review_resume_and_test_use_owned_sessions() {
    let mut runtime = support::runtime::RuntimeFixture::with_daemon(fixture(false));
    let d = &mut runtime.daemon;
    let repo = d.git_repo("projects/lifecycle");
    std::fs::create_dir_all(std::path::Path::new(&repo).join(".devbox")).expect("owned fixture");
    std::fs::write(
        std::path::Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\ntest='printf tested > test-result.txt'\n",
    )
    .expect("owned fixture");
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    call(&c, "agents.send", json!({"id":id,"text":"one\ntwo"})).await;
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    assert_eq!(task["followUps"][0]["text"], "one\ntwo");
    let review = call(&c, "agents.review", json!({"id":id})).await;
    assert_eq!(review["state"], "review");
    let run = call(&c, "agents.test", json!({"id":id})).await;
    assert!(run["runId"].is_string());
    assert!(run.get("terminalId").is_none());
    let wt = std::path::Path::new(idle["worktreePath"].as_str().expect("owned fixture"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !wt.join("test-result.txt").exists() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    d.kill_tmux_server();
    let error = c
        .raw_call("agents.send", json!({"id":id,"text":"lost"}), None)
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.session_missing"));
    drop(c);
    d.restart();
    let c = d.client().await;
    let resumed = call(&c, "agents.resume", json!({"id":id})).await;
    assert_ne!(resumed["terminalId"], idle["terminalId"]);
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    assert_eq!(task["followUps"][0]["text"], "one\ntwo");
    assert_eq!(task["prompt"], idle["prompt"]);
    assert_ne!(
        task["sessionId"], idle["sessionId"],
        "fresh hook session metadata survives resume launch persistence"
    );
    let data = d.paths.data_dir.join("agents").join(&id);
    assert!(data.join("start.sh").exists());
    assert!(std::fs::read_dir(data).expect("owned fixture").any(|e| {
        e.expect("owned fixture")
            .file_name()
            .to_string_lossy()
            .starts_with("resume-")
    }));
}

#[tokio::test]
async fn dirty_base_is_preserved_and_confirmed_discard_removes_owned_resources() {
    let mut d = fixture(false);
    let repo = d.git_repo("projects/discard");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    std::fs::write(
        std::path::Path::new(&repo).join("dirty.txt"),
        "owned user edit",
    )
    .expect("owned fixture");
    let reviewed = expect(&c, &id).await;
    let error = c
        .raw_call(
            "agents.merge",
            json!({"id":id,"strategy":"merge","expect":reviewed}),
            None,
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.base_dirty"));
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let closed=call(&c,"agents.discard",json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]}})).await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(!std::path::Path::new(idle["worktreePath"].as_str().expect("owned fixture")).exists());
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&repo).join("dirty.txt"))
            .expect("owned fixture"),
        "owned user edit"
    );
    let error = c
        .raw_call("agents.send", json!({"id":id,"text":"closed"}), None)
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.bad_state"));
}

#[tokio::test]
async fn queries_return_review_files_commits_and_literal_patch() {
    let mut d = fixture(false);
    let repo = d.git_repo("projects/queries");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let changes = call(&c, "agents.changes", json!({"id":id})).await;
    assert_eq!(changes["ahead"], 1);
    assert_eq!(changes["files"][0]["path"], "agent.txt");
    let patch = call(&c, "agents.patch", json!({"id":id,"path":"agent.txt"})).await;
    assert!(
        patch["patch"]
            .as_str()
            .expect("owned fixture")
            .contains("+change")
    );
    assert_eq!(patch["truncated"], false);
    let commits = call(&c, "agents.commits", json!({"id":id})).await;
    assert_eq!(commits["items"][0]["subject"], "agent change");
    let error = c
        .raw_call(
            "agents.patch",
            json!({"id":id,"path":"../initial.txt"}),
            None,
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.invalid_input"));
}

#[tokio::test]
async fn rebase_conflicts_are_owned_and_abort_restores_the_agent_branch() {
    let mut d = fixture(false);
    let repo = d.git_repo("projects/rebase");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let wt = std::path::Path::new(idle["worktreePath"].as_str().expect("owned fixture"));
    std::fs::write(
        std::path::Path::new(&repo).join("agent.txt"),
        "base change\n",
    )
    .expect("owned fixture");
    d.git(std::path::Path::new(&repo), &["add", "agent.txt"]);
    d.git(
        std::path::Path::new(&repo),
        &["commit", "-qm", "base change"],
    );
    let conflict = call(&c, "agents.rebase", json!({"id":id})).await;
    assert_eq!(conflict["result"], "conflict");
    assert_eq!(conflict["files"][0], "agent.txt");
    drop(c);
    d.restart();
    let c = d.client().await;
    let recovered = call(&c, "agents.rebase_status", json!({"id":id})).await;
    assert_eq!(recovered, json!({"owned":true,"files":["agent.txt"]}));
    let marker = d
        .environment("git")
        .arg("-C")
        .arg(wt)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "rebase-merge",
        ])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("owned marker");
    assert!(marker.status.success());
    let head_name = std::path::Path::new(std::str::from_utf8(&marker.stdout).expect("path").trim())
        .join("head-name");
    let original = std::fs::read_to_string(&head_name).expect("owned branch marker");
    std::fs::write(&head_name, "refs/heads/foreign\n").expect("mismatched fixture marker");
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await["owned"],
        false
    );
    let rejected = c
        .raw_call("agents.rebase_abort", json!({"id":id}), None)
        .await
        .expect_err("same ownership rule rejects foreign branch");
    assert!(format!("{rejected:?}").contains("agents.operation_in_progress"));
    std::fs::write(&head_name, original).expect("restore owned marker");
    call(&c, "agents.rebase_abort", json!({"id":id})).await;
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await["owned"],
        false
    );
    assert_eq!(
        std::fs::read_to_string(wt.join("agent.txt")).expect("owned fixture"),
        "change\n"
    );
    let changes = call(&c, "agents.changes", json!({"id":id})).await;
    let head = changes["headOid"].clone();
    let base = changes["baseOid"].clone();
    let error = c
        .raw_call(
            "agents.merge",
            json!({"id":id,"strategy":"merge","expect":{"headOid":head,"baseOid":base}}),
            None,
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.conflict"));
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&repo).join("agent.txt"))
            .expect("owned fixture"),
        "base change\n"
    );
    let status = d
        .environment("git")
        .args(["-C", &repo, "status", "--porcelain"])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("owned fixture");
    assert!(
        status.stdout.is_empty(),
        "failed merge restores a clean base"
    );
    let external = d
        .environment("git")
        .arg("-C")
        .arg(wt)
        .args(["rebase", "main"])
        .output()
        .expect("owned fixture");
    assert!(!external.status.success());
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await["owned"],
        false
    );
    let error = c
        .raw_call("agents.rebase_abort", json!({"id":id}), None)
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.operation_in_progress"));
    d.git(wt, &["rebase", "--abort"]);
    let conflict = call(&c, "agents.rebase", json!({"id":id})).await;
    assert_eq!(conflict["result"], "conflict");
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await["owned"],
        true
    );
    std::fs::write(wt.join("agent.txt"), "resolved change\n").expect("resolve owned conflict");
    d.git(wt, &["add", "agent.txt"]);
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await,
        json!({"owned":true,"files":[]})
    );
    d.git(wt, &["-c", "core.editor=true", "rebase", "--continue"]);
    // Git completed externally, while the previous owned-rebase DB flag remains.
    assert_eq!(
        call(&c, "agents.rebase_status", json!({"id":id})).await,
        json!({"owned":false,"files":[]})
    );
}

#[tokio::test]
async fn automatic_cleanup_preserves_teardown_changes_and_closed_result() {
    let mut d = fixture(true);
    let repo = d.git_repo("projects/retain");
    std::fs::create_dir_all(std::path::Path::new(&repo).join(".devbox")).expect("owned fixture");
    std::fs::write(
        std::path::Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\nteardown=['printf retained > retained.txt']\n",
    )
    .expect("owned fixture");
    d.git(std::path::Path::new(&repo), &["add", ".devbox/devbox.toml"]);
    d.git(
        std::path::Path::new(&repo),
        &["commit", "-qm", "teardown config"],
    );
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let reviewed = expect(&c, &id).await;
    let closed = call(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"squash","expect":reviewed}),
    )
    .await;
    assert_eq!(closed["closedHow"], "merged");
    assert!(
        closed["cleanupError"]
            .as_str()
            .expect("owned fixture")
            .contains("agents.worktree_dirty")
    );
    assert_eq!(
        std::fs::read_to_string(
            std::path::Path::new(idle["worktreePath"].as_str().expect("owned fixture"))
                .join("retained.txt")
        )
        .expect("owned fixture"),
        "retained"
    );
}

#[tokio::test]
async fn inline_discard_closes_only_its_session_and_preserves_the_project() {
    let mut d = fixture(true);
    let repo = d.git_repo("projects/inline");
    std::fs::create_dir_all(std::path::Path::new(&repo).join(".devbox")).expect("owned fixture");
    std::fs::write(
        std::path::Path::new(&repo).join(".devbox/devbox.toml"),
        "[agent]\nteardown=['rm initial.txt']\n",
    )
    .expect("owned fixture");
    d.launch();
    let c = d.client().await;
    let p = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &p, "commit", false).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    std::fs::write(
        std::path::Path::new(&repo).join("initial.txt"),
        "dirty tracked data",
    )
    .expect("owned fixture");
    std::fs::write(
        std::path::Path::new(&repo).join("untracked.txt"),
        "user data",
    )
    .expect("owned fixture");
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let closed=call(&c,"agents.discard",json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]}})).await;
    assert_eq!(closed["closedHow"], "discarded");
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&repo).join("initial.txt"))
            .expect("owned fixture"),
        "dirty tracked data"
    );
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&repo).join("untracked.txt"))
            .expect("owned fixture"),
        "user data"
    );
    let branch = d
        .environment("git")
        .args(["-C", &repo, "branch", "--show-current"])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("owned fixture");
    assert_eq!(
        String::from_utf8(branch.stdout)
            .expect("owned fixture")
            .trim(),
        "main"
    );
    let sessions = call(&c, "terminal.list", json!({})).await;
    assert!(
        !sessions["items"]
            .as_array()
            .expect("owned fixture")
            .iter()
            .any(|t| t["id"] == idle["terminalId"])
    );
}

#[tokio::test]
async fn review_inline_session_can_end_after_checkout_with_fresh_confirmation() {
    let mut d = fixture(false);
    let repo = d.git_repo("projects/inline-switched");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "commit", false).await;
    d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    d.git(
        std::path::Path::new(&repo),
        &["checkout", "-qb", "user-work"],
    );
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    d.git(
        std::path::Path::new(&repo),
        &["commit", "--allow-empty", "-qm", "late inline commit"],
    );
    let stale = c
        .raw_call(
            "agents.discard",
            json!({"id":id,"expect":{
                "headOid":check["headOid"], "unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]
            }}),
            None,
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{stale:?}").contains("agents.state_changed"));
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let result = c
        .raw_call(
            "agents.discard",
            json!({"id":id,"expect":{
                "headOid":check["headOid"], "unmergedCommits":check["unmergedCommits"],"discardFingerprint":check["fingerprint"]
            }}),
            None,
        )
        .await;
    assert!(
        result.is_ok(),
        "freshly confirmed inline session end failed: {result:?}"
    );
    assert!(std::path::Path::new(&repo).join("agent.txt").exists());
    let branch = d
        .environment("git")
        .args(["-C", &repo, "branch", "--show-current"])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("owned fixture");
    assert_eq!(
        String::from_utf8(branch.stdout)
            .expect("owned fixture")
            .trim(),
        "user-work"
    );
}
