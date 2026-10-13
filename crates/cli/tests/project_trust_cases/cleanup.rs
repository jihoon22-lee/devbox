use super::*;

fn commit_change(d: &TestDaemon, task: &Value, name: &str) {
    let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
    std::fs::write(wt.join(name), "owned change").expect("owned file");
    d.git(wt, &["add", name]);
    d.git(wt, &["commit", "-qm", "owned change"]);
}
async fn merge(c: &devbox_cli::client::Client, id: &str) -> Value {
    let ch = call(c, "agents.changes", json!({"id":id})).await;
    call(c,"agents.merge",json!({"id":id,"strategy":"merge","expect":{"headOid":ch["headOid"],"baseOid":ch["baseOid"]}})).await
}

#[tokio::test]
async fn merge_and_pr_preflight_unapproved_teardown_before_irreversible_git() {
    let (mut d, repo) = fixture(true);
    write(&repo, "[agent]\nteardown=['true']\n");
    d.git(Path::new(&repo), &["add", ".devbox/devbox.toml"]);
    d.git(Path::new(&repo), &["commit", "-qm", "owned configuration"]);
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = create(&c, &project, "Preflight", true).await;
    let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    commit_change(&d, &task, "change.txt");
    let ch = call(&c, "agents.changes", json!({"id":id})).await;
    let expect = json!({"headOid":ch["headOid"],"baseOid":ch["baseOid"]});
    refused(
        &c,
        "agents.merge",
        json!({"id":id,"strategy":"squash","expect":expect}),
        "agents.project_trust_required",
    )
    .await;
    refused(
        &c,
        "agents.pr",
        json!({"id":id,"expect":expect}),
        "agents.project_trust_required",
    )
    .await;
    let after = call(&c, "agents.changes", json!({"id":id})).await;
    assert_eq!(ch["baseOid"], after["baseOid"]);
    assert_eq!(ch["headOid"], after["headOid"]);
    assert!(Path::new(task["worktreePath"].as_str().expect("worktree")).exists());
    assert_eq!(
        call(&c, "agents.get", json!({"id":id})).await["state"],
        "review"
    );
}

#[tokio::test]
async fn bulk_preflight_preserves_every_resource_and_branch_only_cleanup_needs_no_consent() {
    let (mut d, repo) = fixture(false);
    let marker = d.home.path().join("teardown");
    write(
        &repo,
        &format!(
            "[agent]\nteardown=[{}]\n",
            serde_json::to_string(&format!(
                "touch {}",
                devbox_core::sh_quote(marker.to_str().expect("owned path"))
            ))
            .expect("TOML string")
        ),
    );
    d.git(Path::new(&repo), &["add", ".devbox/devbox.toml"]);
    d.git(Path::new(&repo), &["commit", "-qm", "owned configuration"]);
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let mut closed = Vec::new();
    for index in 0..3 {
        let id = create(&c, &project, &format!("Cleanup {index}"), true).await;
        let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
        commit_change(&d, &task, &format!("change{index}.txt"));
        let view = call(
            &c,
            "projects.trust_preview",
            json!({"id":project,"operation":"merge","agentId":id}),
        )
        .await;
        assert_eq!(
            view["requiresApproval"], false,
            "cleanup disabled is a command-free merge"
        );
        let result = merge(&c, &id).await;
        assert!(result["cleanupError"].is_null());
        closed.push(result);
    }
    for task in &closed[1..] {
        d.git(
            Path::new(&repo),
            &[
                "worktree",
                "remove",
                task["worktreePath"].as_str().expect("owned path"),
            ],
        );
    }
    // End the newest branch-only task without consenting to unrelated teardown.
    call(
        &c,
        "agents.cleanup",
        json!({"projectId":project,"worktrees":[],"branches":[closed[2]["branch"]]}),
    )
    .await;
    assert!(!marker.exists());
    let request = json!({"projectId":project,"worktrees":[closed[0]["worktreePath"]],"branches":[closed[1]["branch"]]});
    refused(
        &c,
        "agents.cleanup",
        request.clone(),
        "agents.project_trust_required",
    )
    .await;
    assert!(Path::new(closed[0]["worktreePath"].as_str().expect("worktree")).exists());
    d.git(
        Path::new(&repo),
        &[
            "show-ref",
            "--verify",
            &format!(
                "refs/heads/{}",
                closed[1]["branch"].as_str().expect("branch")
            ),
        ],
    );
    assert!(!marker.exists());
    d.approve_project(&c, &project).await;
    call(&c, "agents.cleanup", request).await;
    assert!(marker.exists());
    assert!(!Path::new(closed[0]["worktreePath"].as_str().expect("worktree")).exists());
}

#[tokio::test]
async fn inline_end_does_not_require_unrelated_unapproved_teardown() {
    let (mut d, repo) = fixture(true);
    write(&repo, "[agent]\nteardown=['rm initial.txt']\n");
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let id = create(&c, &project, "Inline", false).await;
    d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    let view = call(
        &c,
        "projects.trust_preview",
        json!({"id":project,"operation":"discard","agentId":id}),
    )
    .await;
    assert_eq!(view["requiresApproval"], false);
    assert_eq!(view["approved"], false);
    let check = call(&c, "agents.discard_check", json!({"id":id})).await;
    let closed = call(
        &c,
        "agents.discard",
        json!({"id":id,"expect":discard_expect(&check)}),
    )
    .await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(Path::new(&repo).join("initial.txt").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn pr_keeps_pre_git_teardown_and_preserves_success_when_cleanup_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (mut d, repo) = fixture(true);
    let marker = d.home.path().join("approved-teardown");
    let switched = d.home.path().join("switched-teardown");
    write(
        &repo,
        &format!(
            "[agent]\nteardown=[{},'exit 7']\n",
            serde_json::to_string(&format!(
                "touch {}",
                devbox_core::sh_quote(marker.to_str().expect("owned path"))
            ))
            .expect("TOML string")
        ),
    );
    d.git(Path::new(&repo), &["add", ".devbox/devbox.toml"]);
    d.git(Path::new(&repo), &["commit", "-qm", "owned configuration"]);
    let remote = d.home.path().join("remote.git");
    std::fs::create_dir(&remote).expect("owned remote");
    d.git(&remote, &["init", "--bare", "-q"]);
    d.git(
        Path::new(&repo),
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("owned remote"),
        ],
    );
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned bin");
    let gh = bin.join("gh");
    let changed = format!(
        "[agent]\nteardown=[{}]\n",
        serde_json::to_string(&format!(
            "touch {}",
            devbox_core::sh_quote(switched.to_str().expect("owned path"))
        ))
        .expect("TOML string")
    );
    std::fs::write(&gh,format!("#!/bin/bash\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then exit 0; fi\nprintf %s {} > {}\nprintf '%s\\n' 'https://github.com/fixture/trust/pull/1'\n",devbox_core::sh_quote(&changed),devbox_core::sh_quote(Path::new(&repo).join(".devbox/devbox.toml").to_str().expect("owned config")))).expect("owned gh");
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).expect("owned gh");
    d.extra_env.push((
        "PATH".into(),
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").expect("fixture PATH")
        ),
    ));
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    d.approve_project(&c, &project).await;
    let id = create(&c, &project, "PR snapshot", true).await;
    let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    commit_change(&d, &task, "change.txt");
    let ch = call(&c, "agents.changes", json!({"id":id})).await;
    let closed = call(
        &c,
        "agents.pr",
        json!({"id":id,"expect":{"headOid":ch["headOid"],"baseOid":ch["baseOid"]}}),
    )
    .await;
    assert_eq!(closed["closedHow"], "pullRequest");
    assert_eq!(closed["prUrl"], "https://github.com/fixture/trust/pull/1");
    assert!(closed["cleanupError"].as_str().is_some());
    assert!(marker.exists());
    assert!(!switched.exists());
    assert!(Path::new(task["worktreePath"].as_str().expect("worktree")).exists());
    d.git(
        &remote,
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/{}", task["branch"].as_str().expect("branch")),
        ],
    );
}

#[tokio::test]
async fn malformed_config_does_not_block_preflight_for_command_free_branches() {
    let (mut d, repo) = fixture(false);
    d.launch();
    let c = d.client().await;
    let project = d.add_project(&c, &repo).await;
    let inline = create(&c, &project, "Inline", false).await;
    d.wait_for(&c, &inline, "review", Duration::from_secs(10))
        .await;
    let worktree = create(&c, &project, "No automatic cleanup", true).await;
    d.wait_for(&c, &worktree, "review", Duration::from_secs(10))
        .await;
    write(&repo, "[invalid config");
    for (operation, id) in [
        ("discard", &inline),
        ("merge", &worktree),
        ("pr", &worktree),
    ] {
        let view = call(
            &c,
            "projects.trust_preview",
            json!({"id":project,"operation":operation,"agentId":id}),
        )
        .await;
        assert_eq!(view["requiresApproval"], false);
        assert_eq!(view["status"], "not_required");
        assert_eq!(view["approved"], false);
        assert!(view["config"].is_null());
        assert!(view["digest"].is_null());
    }
    let check = call(&c, "agents.discard_check", json!({"id":inline})).await;
    let closed = call(
        &c,
        "agents.discard",
        json!({"id":inline,"expect":discard_expect(&check)}),
    )
    .await;
    assert_eq!(closed["closedHow"], "discarded");
    assert!(Path::new(&repo).join("initial.txt").exists());
}
