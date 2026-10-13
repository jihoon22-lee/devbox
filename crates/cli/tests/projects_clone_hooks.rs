mod support;

use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path};
use support::TestDaemon;

#[tokio::test]
async fn clone_checkout_does_not_execute_tracked_hooks_from_global_relative_hooks_path() {
    let mut daemon = TestDaemon::fixture();
    let global_config = daemon.home.path().join(".gitconfig");
    daemon.extra_env.extend([
        (
            "GIT_CONFIG_GLOBAL".into(),
            global_config.to_str().expect("owned fixture").to_owned(),
        ),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ]);
    let source = daemon.git_repo("untrusted-source");
    let source = Path::new(&source);
    std::fs::create_dir(source.join(".githooks")).expect("owned fixture");
    let hook = source.join(".githooks/post-checkout");
    std::fs::write(&hook, "#!/bin/sh\nprintf hook-ran > clone-hook-marker\n")
        .expect("owned fixture");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
    daemon.git(source, &["add", ".githooks/post-checkout"]);
    daemon.git(
        source,
        &["update-index", "--chmod=+x", ".githooks/post-checkout"],
    );
    daemon.git(source, &["commit", "-qm", "track checkout hook"]);

    // Only this daemon and its owned Git processes read the simulated user config.
    let mut config = std::fs::read_to_string(&global_config).expect("owned fixture");
    config.push_str("\n[core]\nhooksPath = .githooks\n");
    std::fs::write(&global_config, &config).expect("owned fixture");
    daemon.launch();
    let client = daemon.client().await;
    let target = daemon.home.path().join("cloned");
    let response = client
        .raw_call("projects.clone", json!({"url":source,"dir":target}), None)
        .await
        .expect("clone checks out and registers the owned repository");
    let project: Value = serde_json::from_str(&response).expect("owned fixture");
    assert_eq!(project["path"], target.to_str().expect("owned fixture"));
    assert!(target.join("initial.txt").is_file());
    assert!(target.join(".githooks/post-checkout").is_file());
    assert!(
        !target.join("clone-hook-marker").exists(),
        "clone executed tracked repository content before project trust"
    );
    assert_eq!(
        std::fs::read_to_string(global_config).expect("owned fixture"),
        config,
        "the caller's Git configuration must be preserved"
    );
    daemon.git(&target, &["checkout", "-q", "main"]);
    assert_eq!(
        std::fs::read_to_string(target.join("clone-hook-marker")).expect("owned fixture"),
        "hook-ran",
        "the clone override must not persist into later Git mutations"
    );
}
