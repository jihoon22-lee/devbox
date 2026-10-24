mod source_support;
use source_support::*;
#[tokio::test]
async fn config_warning_lists_keys_without_values_or_synthetic_override() {
    let Ok(case) = std::env::var("DEVBOX_SAFETY_CONFIG_CASE") else {
        let configs = tempfile::tempdir().expect("owned Git configs");
        for (case, contents) in [
            ("empty", ""),
            (
                "global-lfs",
                "[filter \"lfs\"]\nclean = owned-lfs-clean\nsmudge = owned-lfs-smudge\nprocess = owned-lfs-process\nrequired = true\n",
            ),
        ] {
            let config = configs.path().join(case);
            std::fs::write(&config, contents).expect("write owned global config");
            let result = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "config_warning_lists_keys_without_values_or_synthetic_override",
                    "--nocapture",
                ])
                .env("DEVBOX_SAFETY_CONFIG_CASE", case)
                .env("GIT_CONFIG_GLOBAL", &config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env_remove("GIT_CONFIG_PARAMETERS")
                .env_remove("GIT_CONFIG_COUNT")
                .output()
                .expect("run isolated config-warning case");
            assert!(
                result.status.success()
                    && String::from_utf8_lossy(&result.stdout).contains("1 passed;"),
                "{case}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        return;
    };
    let mut expected: Vec<String> = match case.as_str() {
        "empty" => vec![],
        "global-lfs" => [
            "filter.lfs.clean",
            "filter.lfs.smudge",
            "filter.lfs.process",
            "filter.lfs.required",
        ]
        .map(str::to_owned)
        .to_vec(),
        _ => panic!("unknown owned config case"),
    };
    let r = repo();
    let d = r.path();
    let keys = |warnings: Vec<devbox_git::safety::GitConfigWarning>| {
        let mut keys: Vec<_> = warnings.into_iter().map(|warning| warning.key).collect();
        keys.sort();
        keys
    };
    expected.sort();
    assert_eq!(
        keys(
            devbox_git::safety::warnings(d)
                .await
                .expect("read initial warning keys")
        ),
        expected
    );
    for key in [
        "core.fsmonitor",
        "core.hooksPath",
        "filter.owned.clean",
        "diff.external",
    ] {
        git(d, &["config", key, "SECRET-SENTINEL-NEVER-RETURN"]);
        expected.push(key.to_ascii_lowercase());
    }
    expected.sort();
    let warnings = devbox_git::safety::warnings(d)
        .await
        .expect("read local and global warning keys");
    assert!(warnings.iter().all(|w| !w.key.contains("SECRET")));
    assert_eq!(keys(warnings), expected);
}

#[tokio::test]
async fn cleanup_preserves_user_main_locked_dirty_unmerged_and_unowned() {
    let r = repo();
    let d = r.path();
    git(d, &["branch", "user"]);
    git(d, &["branch", "agent/clean"]);
    assert!(
        devbox_git::cleanup::candidates(d, &[], &[])
            .await
            .expect("owned source result")
            .is_empty()
    );
    let names = vec!["main".into(), "user".into(), "agent/clean".into()];
    let result = devbox_git::cleanup::candidates(d, &[], &names)
        .await
        .expect("owned source result");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].branch, "agent/clean");
    let parent = tempfile::tempdir().expect("create owned temporary directory");
    let tree = parent.path().join("tree");
    git(
        d,
        &[
            "worktree",
            "add",
            "-q",
            tree.to_str().expect("owned UTF-8 path"),
            "agent/clean",
        ],
    );
    let paths = vec![tree.to_string_lossy().into_owned()];
    git(
        d,
        &["worktree", "lock", tree.to_str().expect("owned UTF-8 path")],
    );
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
    git(
        d,
        &[
            "worktree",
            "unlock",
            tree.to_str().expect("owned UTF-8 path"),
        ],
    );
    std::fs::write(tree.join("dirty"), "dirty").expect("write owned fixture");
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
    git(&tree, &["add", "."]);
    git(&tree, &["commit", "-qm", "unmerged"]);
    assert!(
        devbox_git::cleanup::candidates(d, &paths, &names)
            .await
            .expect("owned source result")
            .is_empty()
    );
}
