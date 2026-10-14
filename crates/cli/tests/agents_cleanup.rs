mod support;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use support::TestDaemon;

async fn call(c: &devbox_cli::client::Client, method: &str, params: Value) -> Value {
    serde_json::from_str(
        &c.raw_call(method, params, None)
            .await
            .expect("owned fixture"),
    )
    .expect("owned fixture")
}
async fn candidates(c: &devbox_cli::client::Client, project: &str) -> Value {
    call(c, "agents.cleanup_candidates", json!({"projectId":project})).await
}

#[tokio::test]
async fn cleanup_preserves_foreign_active_dirty_locked_and_unmerged_worktrees() {
    let mut d = TestDaemon::fixture();
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let tool = d.home.path().join("idle.sh");
    std::fs::write(&tool, r#"#!/usr/bin/env bash
printf '%s' '{"hook_event_name":"Stop","last_assistant_message":"done"}' | "$DEVBOX_BIN" agent event --from claude
while read -r text; do :; done
"#).expect("owned fixture");
    let command = format!(
        "bash {}",
        devbox_core::sh_quote(tool.to_str().expect("owned fixture"))
    );
    d.write_config(&format!("[agents]\ncleanup_after_close=false\n[[agents.profiles]]\nid='idle'\nlabel='I'\ncommand={}\nhooks='none'\n", serde_json::to_string(&command).expect("owned fixture")));
    let repo = d.git_repo("projects/app");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = d.create_agent(&c, &project, "idle", true).await;
    let task = d.wait_for(&c, &id, "idle", Duration::from_secs(10)).await;
    let worktree = task["worktreePath"].as_str().expect("owned fixture");
    let branch = task["branch"].as_str().expect("owned fixture");
    assert_eq!(candidates(&c, &project).await["worktrees"], json!([]));
    let ch = call(&c, "agents.changes", json!({"id":id})).await;
    call(&c, "agents.merge", json!({"id":id,"strategy":"merge","expect":{"headOid":ch["headOid"],"baseOid":ch["baseOid"]}})).await;
    let eligible = candidates(&c, &project).await;
    assert_eq!(eligible["worktrees"], json!([worktree]));
    assert_eq!(eligible["branches"], json!([branch]));

    let foreign = Path::new(worktree)
        .parent()
        .expect("owned fixture")
        .join("foreign");
    d.git(
        Path::new(&repo),
        &[
            "worktree",
            "add",
            "-b",
            "agent/foreign",
            foreign.to_str().expect("owned fixture"),
            "main",
        ],
    );
    let foreign_file = foreign.join("keep.txt");
    std::fs::write(&foreign_file, "foreign data").expect("owned fixture");
    assert_eq!(candidates(&c, &project).await, eligible);
    let error = c
        .raw_call(
            "agents.cleanup",
            json!({"projectId":project,"worktrees":[foreign],"branches":["agent/foreign"]}),
            None,
        )
        .await
        .expect_err("fixture operation must fail");
    assert!(format!("{error:?}").contains("agents.unsafe_worktree"));
    assert!(foreign_file.exists());

    let dirty = Path::new(worktree).join("keep.txt");
    std::fs::write(&dirty, "owned dirty data").expect("owned fixture");
    assert_eq!(candidates(&c, &project).await["worktrees"], json!([]));
    std::fs::remove_file(&dirty).expect("owned fixture");
    d.git(Path::new(&repo), &["worktree", "lock", worktree]);
    assert_eq!(candidates(&c, &project).await["worktrees"], json!([]));
    d.git(Path::new(&repo), &["worktree", "unlock", worktree]);

    d.git(
        Path::new(worktree),
        &["commit", "--allow-empty", "-m", "unmerged late commit"],
    );
    assert_eq!(candidates(&c, &project).await["worktrees"], json!([]));
    d.git(
        Path::new(worktree),
        &[
            "reset",
            "--hard",
            ch["headOid"].as_str().expect("owned fixture"),
        ],
    );
    let result = call(
        &c,
        "agents.cleanup",
        json!({"projectId":project,"worktrees":[worktree],"branches":[]}),
    )
    .await;
    assert!(result["rev"].is_string());
    assert!(!Path::new(worktree).exists());
    // A ref reused before cleanup is no longer the closed task's approved source.
    d.git(
        Path::new(&repo),
        &["commit", "--allow-empty", "-qm", "foreign main advancement"],
    );
    d.git(Path::new(&repo), &["branch", "-f", branch, "main"]);
    assert_eq!(
        candidates(&c, &project).await,
        json!({"worktrees":[],"branches":[]})
    );
    d.git(
        Path::new(&repo),
        &[
            "branch",
            "-f",
            branch,
            ch["headOid"].as_str().expect("owned fixture"),
        ],
    );
    assert_eq!(
        candidates(&c, &project).await,
        json!({"worktrees":[],"branches":[branch]})
    );
    call(
        &c,
        "agents.cleanup",
        json!({"projectId":project,"worktrees":[],"branches":[branch]}),
    )
    .await;

    let refs = d
        .environment("git")
        .args([
            "-C",
            &repo,
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads/",
        ])
        .stdout(std::process::Stdio::piped())
        .output()
        .expect("owned fixture");
    assert!(refs.status.success());
    assert!(
        !String::from_utf8(refs.stdout)
            .expect("owned fixture")
            .lines()
            .any(|b| b == branch)
    );
    assert!(foreign_file.exists());
    // Released ownership cannot authorize a new foreign ref with the old name.
    d.git(Path::new(&repo), &["branch", branch, "main"]);
    assert_eq!(
        candidates(&c, &project).await,
        json!({"worktrees":[],"branches":[]})
    );
}
