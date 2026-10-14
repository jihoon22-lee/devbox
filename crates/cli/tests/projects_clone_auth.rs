mod support;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path};
use support::TestDaemon;

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).expect("owned fixture");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
}

fn real_git() -> String {
    let output = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("owned fixture");
    String::from_utf8(output.stdout)
        .expect("owned fixture")
        .trim()
        .into()
}

fn configure_git(d: &TestDaemon, key: &str, value: &str) {
    let status = d
        .environment("git")
        .args(["config", "--file"])
        .arg(d.home.path().join(".gitconfig"))
        .args([key, value])
        .status()
        .expect("owned fixture");
    assert!(status.success());
}

fn intercept_clone(d: &mut TestDaemon, before: &str) {
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned fixture");
    let script = format!(
        "#!/bin/bash\nprefix=()\nfor arg in \"$@\"; do\nif [[ $arg == clone ]]; then\n{before}\nbreak\nfi\nprefix+=(\"$arg\")\ndone\nexec {} \"$@\"\n",
        devbox_core::sh_quote(&real_git())
    );
    executable(&bin.join("git"), &script);
    d.extra_env.push((
        "PATH".into(),
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").expect("owned fixture")
        ),
    ));
}

#[tokio::test]
async fn clone_preserves_configured_https_helper_without_interactive_prompts() {
    let mut d = TestDaemon::fixture();
    let source = d.git_repo("source");
    let helper = d.home.path().join("credential-helper");
    let marker = d.home.path().join("credential-called");
    executable(
        &helper,
        &format!(
            "#!/bin/bash\n[[ $GIT_TERMINAL_PROMPT == 0 && $GCM_INTERACTIVE == Never && $GCM_GUI_PROMPT == false ]] || exit 91\nif [[ $1 == get ]]; then\n touch {}\n printf 'username=fixture-user\\npassword=fixture-password\\n'\nfi\n",
            devbox_core::sh_quote(marker.to_str().expect("owned fixture"))
        ),
    );
    configure_git(
        &d,
        "credential.helper",
        helper.to_str().expect("owned fixture"),
    );
    let before = format!(
        "printf 'protocol=https\\nhost=fixture.invalid\\n\\n' | {} \"${{prefix[@]}}\" credential fill >/dev/null || exit 92",
        devbox_core::sh_quote(&real_git())
    );
    intercept_clone(&mut d, &before);
    d.launch();
    let c = d.client().await;
    let target = d.home.path().join("clone");
    let result = c
        .raw_call("projects.clone", json!({"url":source,"dir":target}), None)
        .await
        .expect("owned fixture");
    let project: Value = serde_json::from_str(&result).expect("owned fixture");
    assert_eq!(project["path"], target.to_str().expect("owned fixture"));
    assert!(
        marker.exists(),
        "configured HTTPS helper must remain available"
    );
    assert!(
        !std::fs::read_to_string(target.join(".git/config"))
            .expect("owned fixture")
            .contains("fixture-password")
    );
}

#[tokio::test]
async fn clone_preserves_configured_and_environment_ssh_identity_with_batch_mode() {
    for (prefix, environment_override) in [
        ("", false),
        ("", true),
        ("SSH_AUTH_SOCK='fixture socket' ", false),
        ("SSH_AUTH_SOCK='fixture socket' ", true),
        ("env SSH_AUTH_SOCK='fixture socket' ", false),
        ("env SSH_AUTH_SOCK='fixture socket' ", true),
    ] {
        let mut d = TestDaemon::fixture();
        let source = d.git_repo("source");
        let ssh = d.home.path().join("fixture ssh");
        let arguments = d.home.path().join("ssh-arguments");
        executable(
            &ssh,
            &format!(
                "#!/bin/bash\n[[ $SSH_ASKPASS_REQUIRE == never ]] || exit 91\n{}\nfor arg in \"$@\"; do [[ $arg == -G ]] && exit 0; done\nprintf '%s\\n' \"$@\" > {}\nexec {} upload-pack {}\n",
                if prefix.is_empty() {
                    ""
                } else {
                    "[[ $SSH_AUTH_SOCK == 'fixture socket' ]] || exit 94"
                },
                devbox_core::sh_quote(arguments.to_str().expect("owned fixture")),
                devbox_core::sh_quote(&real_git()),
                devbox_core::sh_quote(&source),
            ),
        );
        let command = format!(
            "{prefix}{} -i 'fixture-config-identity with space'",
            devbox_core::sh_quote(ssh.to_str().expect("owned fixture"))
        );
        configure_git(&d, "core.sshCommand", &command);
        if environment_override {
            d.extra_env.push((
                "GIT_SSH_COMMAND".into(),
                format!(
                    "{prefix}{} -i 'fixture-env-identity with space'",
                    devbox_core::sh_quote(ssh.to_str().expect("owned fixture"))
                ),
            ));
        }
        d.launch();
        let c = d.client().await;
        c.raw_call(
            "projects.clone",
            json!({"url":format!("ssh://git@fixture.invalid{source}"),"dir":d.home.path().join("clone")}),
            None,
        )
        .await
        .expect("owned fixture");
        let arguments = std::fs::read_to_string(arguments).expect("owned fixture");
        assert!(arguments.lines().any(|argument| argument
            == if environment_override {
                "fixture-env-identity with space"
            } else {
                "fixture-config-identity with space"
            }));
        assert!(arguments.contains("-oBatchMode=yes"));
        assert!(arguments.contains("-oStrictHostKeyChecking=yes"));
        assert!(d.home.path().join("clone/initial.txt").exists());
    }
}

#[tokio::test]
async fn clone_rejects_http_token_usernames_and_uri_passwords_before_reservation() {
    let mut d = TestDaemon::fixture();
    d.launch();
    let c = d.client().await;
    for (index, url) in [
        "https://fixture-token@fixture.invalid/repo.git",
        "http://fixture-user:fixture-password@fixture.invalid/repo.git",
        "ssh://git:fixture-password@fixture.invalid/repo.git",
        "file://fixture-token@localhost/tmp/repo.git",
    ]
    .iter()
    .enumerate()
    {
        let target = d.home.path().join(format!("rejected-{index}"));
        let result = c
            .raw_call("projects.clone", json!({"url":url,"dir":target}), None)
            .await
            .expect_err("fixture operation must fail");
        assert!(format!("{result:?}").contains("projects.clone_invalid_url"));
        assert!(!format!("{result:?}").contains("fixture-token"));
        assert!(!format!("{result:?}").contains("fixture-password"));
        assert!(!target.exists());
    }
}
