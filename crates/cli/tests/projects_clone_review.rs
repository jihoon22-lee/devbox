mod support;
use serde_json::json;
use std::{path::Path, time::Duration};
use support::TestDaemon;

#[test]
fn launch_config_rejects_positional_arrays_instead_of_tables() {
    let mut accepted = Vec::new();
    for text in [
        "launch=[[],[[\"shell\",\"echo positional\"]],[]]",
        "[launch]\nterminals=[[\"shell\",\"echo positional\"]]",
        "[launch]\nagents=[[\"fake\",false,\"positional prompt\"]]",
    ] {
        if devbox_projects::config::parse(text).is_ok() {
            accepted.push(text);
        }
    }
    assert!(
        accepted.is_empty(),
        "malformed launch tables accepted: {accepted:?}"
    );
}

#[tokio::test]
async fn clone_rejects_password_url_before_persisting_credentials() {
    let mut d = TestDaemon::fixture();
    let source = d.git_repo("source");
    d.launch();
    let c = d.client().await;
    let target = d.home.path().join("credential-target");
    let result = c
        .raw_call(
            "projects.clone",
            json!({"url":format!("file://fixture-user:fixture-secret@localhost{source}"),"dir":target}),
            None,
        )
        .await;
    let persisted = std::fs::read_to_string(target.join(".git/config"))
        .is_ok_and(|config| config.contains("fixture-secret"));
    assert!(
        result.is_err(),
        "credential-bearing URL was accepted; plaintext persisted: {persisted}"
    );
    assert!(!persisted, "URL password must not be written to Git config");
}

#[tokio::test]
async fn clone_does_not_follow_target_symlink_swapped_after_reservation() {
    use std::os::unix::fs::PermissionsExt;
    let mut d = TestDaemon::fixture();
    let source = d.git_repo("source");
    let target = d.home.path().join("target");
    let external = d.home.path().join("unselected-external");
    std::fs::create_dir(&external).unwrap();
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let ready = d.home.path().join("git-ready");
    let release = d.home.path().join("git-release");
    let real_git = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git = String::from_utf8(real_git.stdout).unwrap();
    let quote = |p: &Path| devbox_core::sh_quote(p.to_str().unwrap());
    let script = format!(
        "#!/bin/bash\nfor arg in \"$@\"; do\nif [[ $arg == clone ]]; then\n touch {ready}\n for ((n=0;n<500;n++)); do\n [[ -f {release} ]] && break\n sleep 0.01\n done\n [[ -f {release} ]] || exit 99\nfi\ndone\nexec {real_git} \"$@\"\n",
        ready = quote(&ready),
        release = quote(&release),
        real_git = devbox_core::sh_quote(real_git.trim())
    );
    let git = bin.join("git");
    std::fs::write(&git, script).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
    d.extra_env.push((
        "PATH".into(),
        format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
    ));
    d.launch();
    let c = d.client().await;
    let clone = c.raw_call("projects.clone", json!({"url":source,"dir":target}), None);
    let swap = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        std::fs::remove_dir(&target).unwrap();
        std::os::unix::fs::symlink(&external, &target).unwrap();
        std::fs::write(&release, "release").unwrap();
    };
    let (result, ()) = tokio::join!(clone, swap);
    assert!(
        !external.join(".git").exists(),
        "clone wrote outside selected destination; RPC accepted: {}",
        result.is_ok()
    );
}
