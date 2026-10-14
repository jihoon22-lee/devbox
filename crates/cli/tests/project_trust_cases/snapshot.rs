use super::*;

#[cfg(unix)]
#[tokio::test]
async fn preparation_uses_approved_snapshot_when_checkout_changes_the_config() {
    use std::os::unix::fs::PermissionsExt;
    let (mut d, repo) = fixture(false);
    write(&repo, "[agent]\nsetup=['touch approved-setup']\n");
    let bin = d.home.path().join("checkout-bin");
    std::fs::create_dir(&bin).expect("owned bin");
    let real_git = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("Git path");
    assert!(real_git.status.success());
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!(
        "#!/bin/bash\ncheckout=; previous=; current_repo=\nfor argument do\n if [[ $previous:$argument == worktree:add ]]; then checkout=1; fi\n if [[ $previous == -C ]]; then current_repo=$argument; fi\n previous=$argument\ndone\n{} \"$@\" || exit $?\nif [[ $checkout == 1 && $current_repo == {} ]]; then printf %s {} > {}; fi\n",
        devbox_core::sh_quote(String::from_utf8(real_git.stdout).expect("Git path").trim()),
        devbox_core::sh_quote(&repo),
        devbox_core::sh_quote("[agent]\nsetup=['touch switched-setup']\n"),
        devbox_core::sh_quote(Path::new(&repo).join(".devbox/devbox.toml").to_str().expect("owned config"))
    )).expect("owned checkout wrapper");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("owned wrapper");
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
    let id = create(&c, &project, "Snapshot", true).await;
    let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
    let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
    assert!(
        std::fs::read_to_string(Path::new(&repo).join(".devbox/devbox.toml"))
            .expect("changed config")
            .contains("switched-setup")
    );
    assert!(wt.join("approved-setup").exists());
    assert!(!wt.join("switched-setup").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn composite_launch_keeps_terminals_prompt_and_setup_on_the_same_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    let (mut d, repo) = fixture(false);
    write(
        &repo,
        "[agent]\nsetup=['touch approved-setup']\n[launch]\nterminals=[{name='approved',command='touch approved-terminal'}]\nagents=[{tool='fixture',worktree=true,prompt='approved prompt'}]\n",
    );
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned bin");
    let real_git = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("git path");
    let new = "[agent]\nsetup=['touch switched-setup']\n[launch]\nterminals=[{name='switched',command='touch switched-terminal'}]\nagents=[{tool='fixture',worktree=true,prompt='switched prompt'}]\n";
    let git = bin.join("git");
    std::fs::write(&git,format!("#!/bin/bash\nfor arg in \"$@\"; do if [[ $arg == for-each-ref ]]; then printf %s {} > {}; fi; done\nexec {} \"$@\"\n",devbox_core::sh_quote(new),devbox_core::sh_quote(Path::new(&repo).join(".devbox/devbox.toml").to_str().expect("owned path")),devbox_core::sh_quote(String::from_utf8(real_git.stdout).expect("git path").trim()))).expect("owned wrapper");
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).expect("owned wrapper");
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
    let launch = call(&c, "projects.launch", json!({"id":project})).await;
    assert_eq!(launch["terminals"], 1);
    assert_eq!(launch["agents"], 1);
    let tasks = call(&c, "agents.list", json!({})).await;
    let id = tasks["items"][0]["id"].as_str().expect("agent");
    let task = d.wait_for(&c, id, "review", Duration::from_secs(10)).await;
    assert_eq!(task["prompt"], "approved prompt");
    wait_file(&Path::new(&repo).join("approved-terminal")).await;
    assert!(!Path::new(&repo).join("switched-terminal").exists());
    let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
    assert!(wt.join("approved-setup").exists());
    assert!(!wt.join("switched-setup").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn merge_cleanup_keeps_the_pre_git_snapshot_including_no_teardown() {
    use std::os::unix::fs::PermissionsExt;
    for has_teardown in [true, false] {
        let (mut d, repo) = fixture(true);
        let approved = d.home.path().join("approved-teardown");
        let switched = d.home.path().join("switched-teardown");
        let old = if has_teardown {
            format!(
                "[agent]\nteardown=[{}]\n",
                serde_json::to_string(&format!(
                    "touch {}",
                    devbox_core::sh_quote(approved.to_str().expect("owned path"))
                ))
                .expect("TOML string")
            )
        } else {
            "[agent]\nsetup=[]\n".into()
        };
        write(&repo, &old);
        d.git(Path::new(&repo), &["add", ".devbox/devbox.toml"]);
        d.git(Path::new(&repo), &["commit", "-qm", "owned configuration"]);
        d.launch();
        let c = d.client().await;
        let project = d.add_project(&c, &repo).await;
        if has_teardown {
            d.approve_project(&c, &project).await;
        }
        let id = create(&c, &project, "Merge snapshot", true).await;
        let task = d.wait_for(&c, &id, "review", Duration::from_secs(10)).await;
        let wt = Path::new(task["worktreePath"].as_str().expect("worktree"));
        std::fs::write(wt.join("change.txt"), "owned change").expect("owned change");
        d.git(wt, &["add", "change.txt"]);
        d.git(wt, &["commit", "-qm", "owned change"]);
        let hook = Path::new(&repo).join(".git/hooks/post-commit");
        let new = format!(
            "[agent]\nteardown=[{}]\n",
            serde_json::to_string(&format!(
                "touch {}",
                devbox_core::sh_quote(switched.to_str().expect("owned path"))
            ))
            .expect("TOML string")
        );
        std::fs::write(
            &hook,
            format!(
                "#!/bin/bash\nprintf %s {} > {}\n",
                devbox_core::sh_quote(&new),
                devbox_core::sh_quote(
                    Path::new(&repo)
                        .join(".devbox/devbox.toml")
                        .to_str()
                        .expect("owned config")
                )
            ),
        )
        .expect("owned hook");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
            .expect("owned hook");
        let ch = call(&c, "agents.changes", json!({"id":id})).await;
        let closed=call(&c,"agents.merge",json!({"id":id,"strategy":"squash","expect":{"headOid":ch["headOid"],"baseOid":ch["baseOid"]}})).await;
        assert_eq!(closed["closedHow"], "merged");
        assert_eq!(
            approved.exists(),
            has_teardown,
            "cleanup must consume the exact pre-Git snapshot"
        );
        assert!(
            !switched.exists(),
            "a Git hook cannot switch teardown commands"
        );
        assert!(
            !wt.exists(),
            "the pre-Git command-free cleanup must stay command-free"
        );
        assert!(closed["cleanupError"].is_null());
    }
}
