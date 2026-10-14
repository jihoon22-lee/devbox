mod support;
use std::{os::unix::fs::PermissionsExt, time::Duration};
use support::TestDaemon;

#[tokio::test]
async fn test_instance_uses_scope_unless_explicitly_disabled() {
    let mut d = TestDaemon::fixture();
    d.extra_env.retain(|(key, _)| key != "DEVBOX_NO_SCOPE");
    d.paths.ensure_dirs().expect("owned fixture");
    std::fs::write(
        d.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned fixture");
    let bin = d.home.path().join("bin");
    std::fs::create_dir(&bin).expect("owned fixture");
    let scope = bin.join("systemd-run");
    std::fs::write(&scope, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/scope-args\"\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n").expect("owned fixture");
    std::fs::set_permissions(scope, std::fs::Permissions::from_mode(0o700)).expect("owned fixture");
    d.extra_env
        .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
    d.write_config(
        "[[agents.profiles]]\nid='sleepy'\nlabel='Sleepy'\ncommand='sleep 60'\nhooks='none'\n",
    );
    let repo = d.git_repo("projects/scope");
    d.launch();
    let client = d.client().await;
    let project = d.add_project(&client, &repo).await;
    let id = d.create_agent(&client, &project, "sleepy", false).await;
    d.wait_for(&client, &id, "running", Duration::from_secs(5))
        .await;
    support::bounded(async {
        while !d.home.path().join("scope-args").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let args = std::fs::read_to_string(d.home.path().join("scope-args")).expect("owned fixture");
    assert!(args.contains(&format!("devbox-{}-agent-{id}", d.instance)));
    assert!(args.contains("--user\n--scope\n"));
    // This validates selection only: the owned shim is not real systemd acceptance.
}
