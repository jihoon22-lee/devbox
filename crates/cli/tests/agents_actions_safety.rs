mod support;
use devbox_cli::client::Client;
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use support::TestDaemon;

fn fixture() -> TestDaemon {
    let d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("commit.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf change > agent.txt
git add agent.txt && git commit -qm 'agent change'
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do :; done
"#).expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("owned fixture"))
    );
    d.write_config(&format!("[agents]\ncleanup_after_close=false\n[[agents.profiles]]\nid='commit'\nlabel='C'\ncommand={}\nhooks='none'\n", serde_json::to_string(&command).expect("owned fixture")));
    d
}
async fn call(c: &Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &c.raw_call(method, params, None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture")
}
async fn start(d: &mut TestDaemon) -> (String, Client, String, String, Value) {
    let repo = d.git_repo("projects/app");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "commit", true).await;
    let idle = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    (repo, c, project, id, idle)
}
fn rev(d: &TestDaemon, repo: &str, reference: &str) -> String {
    let out = d
        .environment("git")
        .args(["-C", repo, "rev-parse", reference])
        .stdout(Stdio::piped())
        .output()
        .expect("owned fixture");
    assert!(out.status.success());
    String::from_utf8(out.stdout)
        .expect("owned fixture")
        .trim()
        .into()
}
async fn reviewed(c: &Client, id: &str) -> Value {
    let ch = call(c, "agents.changes", json!({"id":id})).await;
    json!({"headOid":ch["headOid"],"baseOid":ch["baseOid"]})
}
async fn refused(c: &Client, method: &str, params: Value, code: &str) {
    let err = c
        .raw_call(method, params, None)
        .await
        .expect_err("unsafe mutation rejected");
    assert!(format!("{err:?}").contains(code), "{err:?}");
}
#[tokio::test]
async fn late_head_and_stale_discard_count_preserve_the_task() {
    let mut d = fixture();
    let (repo, c, _, id, idle) = start(&mut d).await;
    let expect = reviewed(&c, &id).await;
    let wt = idle["worktreePath"].as_str().expect("owned fixture");
    let base = rev(&d, &repo, "main");
    d.git(Path::new(wt), &["commit", "--allow-empty", "-qm", "late"]);
    refused(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"squash","expect":expect}),
        "agents.state_changed",
    )
    .await;
    assert_eq!(rev(&d, &repo, "main"), base);
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    refused(
        &c,
        "agents.discard",
        json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":0}}),
        "agents.state_changed",
    )
    .await;
    assert!(Path::new(wt).exists());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "idle"
    );
    let fresh = reviewed(&c, &id).await;
    let merged = call(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"squash","expect":fresh}),
    )
    .await;
    assert_eq!(merged["closedHow"], "merged");
}
#[tokio::test]
async fn base_checkout_change_cannot_merge_into_another_branch() {
    let mut d = fixture();
    let (repo, c, _, id, _) = start(&mut d).await;
    let expect = reviewed(&c, &id).await;
    let base = rev(&d, &repo, "main");
    d.git(Path::new(&repo), &["checkout", "-qb", "external"]);
    refused(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"merge","expect":expect}),
        "agents.base_not_checked_out",
    )
    .await;
    assert_eq!(rev(&d, &repo, "main"), base);
    assert_eq!(rev(&d, &repo, "external"), base);
}
#[tokio::test]
async fn changed_worktree_branch_cannot_be_discarded() {
    let mut d = fixture();
    let (_, c, _, id, idle) = start(&mut d).await;
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let wt = idle["worktreePath"].as_str().expect("owned fixture");
    d.git(Path::new(wt), &["checkout", "-qb", "external-agent"]);
    refused(&c,"agents.discard",json!({"id":id,"expect":{"headOid":check["headOid"],"unmergedCommits":check["unmergedCommits"]}}),"agents.state_changed").await;
    assert!(Path::new(wt).exists());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "idle"
    );
}
#[tokio::test]
async fn cleanup_preserves_foreign_worktrees_and_symlinks() {
    let mut d = fixture();
    let (repo, c, project, _, _) = start(&mut d).await;
    let foreign = d.home.path().join("foreign");
    d.git(
        Path::new(&repo),
        &[
            "worktree",
            "add",
            "-qb",
            "agent/foreign",
            foreign.to_str().expect("owned fixture"),
        ],
    );
    let link = d.home.path().join("foreign-link");
    std::os::unix::fs::symlink(&foreign, &link).expect("owned fixture");
    for path in [&foreign, &link] {
        refused(
            &c,
            "agents.cleanup",
            json!({"projectId":project,"worktrees":[path],"branches":["agent/foreign"]}),
            "agents.unsafe_worktree",
        )
        .await;
        assert!(foreign.join("initial.txt").exists());
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("owned fixture")
                .file_type()
                .is_symlink()
        );
    }
    let candidates = call(
        &c,
        "agents.cleanup_candidates",
        json!({"projectId":project}),
    )
    .await;
    assert!(
        !candidates["worktrees"]
            .as_array()
            .expect("owned fixture")
            .contains(&json!(foreign))
    );
    assert!(
        !candidates["branches"]
            .as_array()
            .expect("owned fixture")
            .contains(&json!("agent/foreign"))
    );
}

fn fake_gh(d: &mut TestDaemon, success: bool) {
    use std::os::unix::fs::PermissionsExt;
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned fixture");
    let gh = bin.join("gh");
    let script = if success {
        "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then exit 0; fi\nprintf '%s\\n' 'https://github.com/fixture/app/pull/1'\n"
    } else {
        "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then exit 0; fi\nprintf '%s\\n' 'PRIVATE_GH_FAILURE' >&2\nexit 1\n"
    };
    std::fs::write(&gh, script).expect("owned fixture");
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).expect("owned fixture");
    let mut search_path = vec![bin];
    search_path.extend(std::env::split_paths(
        &std::env::var_os("PATH").expect("owned fixture"),
    ));
    d.extra_env.push((
        "PATH".into(),
        std::env::join_paths(search_path)
            .expect("owned fixture")
            .to_str()
            .expect("owned fixture")
            .into(),
    ));
}
fn remote(d: &TestDaemon, repo: &str) -> String {
    let remote = d.home.path().join("remote.git");
    std::fs::create_dir(&remote).expect("owned fixture");
    d.git(&remote, &["init", "--bare", "-q"]);
    d.git(
        Path::new(repo),
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("owned fixture"),
        ],
    );
    remote.to_str().expect("owned fixture").into()
}
#[tokio::test]
async fn pull_request_pushes_confirmed_commit_and_closes_with_url() {
    let mut d = fixture();
    fake_gh(&mut d, true);
    let (repo, c, _, id, idle) = start(&mut d).await;
    let remote = remote(&d, &repo);
    let expect = reviewed(&c, &id).await;
    let closed = call(&c, "agents.pr", json!({"id":id,"expect":expect})).await;
    assert_eq!(closed["closedHow"], "pullRequest");
    assert!(closed["tokens"].is_null());
    assert_eq!(closed["tokensTruncated"], false);
    assert_eq!(
        call(&c, "agents.usage", json!({"id":id})).await,
        json!({"tokens":null,"truncated":false})
    );
    assert_eq!(closed["prUrl"], "https://github.com/fixture/app/pull/1");
    assert_eq!(closed["closedCommits"], 1);
    assert_eq!(
        rev(&d, &remote, idle["branch"].as_str().expect("owned fixture")),
        expect["headOid"].as_str().expect("owned fixture")
    );
    assert!(Path::new(idle["worktreePath"].as_str().expect("owned fixture")).exists());
}
#[tokio::test]
async fn failed_pull_request_keeps_task_open_and_reports_no_private_output() {
    let mut d = fixture();
    fake_gh(&mut d, false);
    let (repo, c, _, id, idle) = start(&mut d).await;
    let remote = remote(&d, &repo);
    let expect = reviewed(&c, &id).await;
    let error = c
        .raw_call("agents.pr", json!({"id":id,"expect":expect}), None)
        .await
        .expect_err("gh creation fails");
    assert!(format!("{error:?}").contains("internal"));
    assert!(!format!("{error:?}").contains("PRIVATE_GH_FAILURE"));
    let task = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(task["state"], "idle");
    assert!(task["prUrl"].is_null());
    assert!(task["closedMs"].is_null());
    assert!(Path::new(idle["worktreePath"].as_str().expect("owned fixture")).exists());
    // Push already succeeded before gh failed; the reviewed commit remains actionable.
    assert_eq!(
        rev(&d, &remote, idle["branch"].as_str().expect("owned fixture")),
        expect["headOid"].as_str().expect("owned fixture")
    );
}

#[tokio::test]
async fn incomplete_confirmation_returns_invalid_params_without_side_effects() {
    let mut d = fixture();
    let (repo, c, _, id, idle) = start(&mut d).await;
    let initial = rev(&d, &repo, "main");
    let expect = reviewed(&c, &id).await;
    for (method, params) in [
        ("agents.merge", json!({"id":id,"strategy":"merge"})),
        (
            "agents.merge",
            json!({"id":id,"strategy":"merge","expect":{"headOid":expect["headOid"]}}),
        ),
        ("agents.pr", json!({"id":id})),
        (
            "agents.discard",
            json!({"id":id,"expect":{"headOid":expect["headOid"]}}),
        ),
    ] {
        refused(&c, method, params, "invalid_params").await;
    }
    assert_eq!(rev(&d, &repo, "main"), initial);
    assert!(Path::new(idle["worktreePath"].as_str().expect("owned fixture")).exists());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "idle"
    );
}

#[tokio::test]
async fn review_regression_missing_profile_does_not_leave_successful_pr_open() {
    let mut d = fixture();
    fake_gh(&mut d, true);
    let (repo, c, _, id, _) = start(&mut d).await;
    remote(&d, &repo);
    let expect = reviewed(&c, &id).await;
    drop(c);
    d.stop();
    d.write_config("[agents]\ncleanup_after_close=false\n");
    d.launch();
    let c = d.client().await;
    let response = c
        .raw_call("agents.pr", json!({"id":id,"expect":expect}), None)
        .await;
    let closed = call(&c, "agents.get", json!({"id":id})).await;
    assert_eq!(
        closed["state"], "closed",
        "successful PR must close: {response:?}"
    );
    assert!(response.is_ok());
    assert_eq!(closed["prUrl"], "https://github.com/fixture/app/pull/1");
    assert!(closed["tokens"].is_null());
    assert_eq!(closed["tokensTruncated"], true);
    assert_eq!(
        call(&c, "agents.usage", json!({"id":id})).await,
        json!({"tokens":null,"truncated":true})
    );
}
