use devbox_core::Instance;
use devbox_terminal::tmux::Tmux;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[tokio::test]
async fn channel_tmux_activation_uses_only_its_own_service() {
    const CHILD: &str = "DEVBOX_CHANNEL_TMUX_FIXTURE";
    if let Ok(root) = std::env::var(CHILD) {
        let root = std::path::PathBuf::from(root);
        let instance = std::env::var("DEVBOX_INSTANCE").unwrap();
        let tmux = Tmux::with(format!("devbox-{instance}"), root.join("tmux.conf"));
        tmux.ensure_server(&Instance::parse(&instance).unwrap())
            .await
            .unwrap();
        return;
    }
    for (instance, unit) in [
        ("prod", "devbox-tmux@prod.service"),
        ("accept", "devbox-accept-tmux@accept.service"),
        ("dev", "devbox-dev-tmux@dev.service"),
    ] {
        let fixture = tempfile::tempdir().unwrap();
        for (name, script) in [
            (
                "tmux",
                "#!/bin/sh\nif [ -f \"$DEVBOX_CHANNEL_TMUX_FIXTURE/ready\" ]; then exit 0; fi\necho 'no server running' >&2\nexit 1\n",
            ),
            (
                "systemctl",
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$DEVBOX_CHANNEL_TMUX_FIXTURE/calls\"\n: > \"$DEVBOX_CHANNEL_TMUX_FIXTURE/ready\"\n",
            ),
        ] {
            let path = fixture.path().join(name);
            std::fs::write(&path, script).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "channel_tmux_activation_uses_only_its_own_service",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", fixture.path())
            .env("PATH", fixture.path())
            .env(CHILD, fixture.path())
            .env("DEVBOX_INSTANCE", instance)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(10), child.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.path().join("calls")).unwrap(),
            format!("--user\nstart\n{unit}\n")
        );
    }
}
