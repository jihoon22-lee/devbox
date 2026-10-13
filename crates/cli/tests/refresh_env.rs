mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_core::{Env, Instance, Paths};
use devbox_protocol::message::ClientKind;
use devbox_protocol::system::{Ping, PingParams, RefreshEnv};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;
use support::{ChildGuard, TestDaemon};

struct FixtureEnv(PathBuf, PathBuf);
impl Env for FixtureEnv {
    fn var(&self, key: &str) -> Option<String> {
        match key {
            "HOME" => Some(self.0.display().to_string()),
            "XDG_RUNTIME_DIR" => Some(self.1.display().to_string()),
            _ => None,
        }
    }
    fn uid(&self) -> u32 {
        1000
    }
}

#[tokio::test]
async fn refresh_rpc_keeps_foreground_alive_and_restarts_managed_test_instance() {
    for foreground in [true, false] {
        let mut daemon = TestDaemon::fixture();
        if foreground {
            daemon.instance = format!(
                "dev-refresh-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            );
            daemon.paths = Paths::resolve(
                Instance::parse(&daemon.instance).expect("owned fixture"),
                &FixtureEnv(daemon.home.path().into(), daemon.home.path().join("run")),
            )
            .expect("owned fixture");
            daemon.socket = daemon.paths.socket.clone();
        }
        let bin = daemon.home.path().join("fake-bin");
        std::fs::create_dir(&bin).expect("owned fixture");
        let bash = bin.join("bash");
        std::fs::write(&bash, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/capture-calls\"\nprintf 'PATH=/new/tool/bin\\0LANG=C\\0GITHUB_TOKEN=fixture-secret\\0'\n").expect("owned fixture");
        std::fs::set_permissions(&bash, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture");
        // Any accidental setup/manager action is trapped inside this fixture.
        let manager = bin.join("systemctl");
        std::fs::write(
            &manager,
            "#!/bin/sh\nprintf invoked > \"$HOME/manager-invoked\"\nexit 1\n",
        )
        .expect("owned fixture");
        std::fs::set_permissions(manager, std::fs::Permissions::from_mode(0o700))
            .expect("owned fixture");
        daemon
            .extra_env
            .push(("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())));
        let mut command = daemon.environment(env!("CARGO_BIN_EXE_devbox"));
        command.arg("daemon");
        if foreground {
            command.arg("--foreground");
        }
        daemon.child = Some(ChildGuard(command.spawn().expect("owned fixture")));
        let client = support::bounded(async {
            loop {
                if let Ok(client) =
                    Client::connect(&daemon.socket, ClientKind::Test, &daemon.instance).await
                {
                    break client;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        let first = client
            .raw_call(
                "system.refresh_env",
                serde_json::json!({}),
                Some("refresh-owned".into()),
            )
            .await
            .expect("owned fixture");
        let result: devbox_protocol::system::RefreshEnvResult =
            serde_json::from_str(&first).expect("owned fixture");
        assert_eq!(result.restarting, !foreground);
        let repeated = client
            .raw_call(
                "system.refresh_env",
                serde_json::json!({}),
                Some("refresh-owned".into()),
            )
            .await
            .expect("owned fixture");
        assert_eq!(first, repeated);
        assert_eq!(
            std::fs::read_to_string(daemon.home.path().join("capture-calls"))
                .expect("owned fixture"),
            "-lc env -0\n"
        );
        let env = daemon.paths.instance_config_dir.join("env");
        let saved = std::fs::read_to_string(&env).expect("owned fixture");
        assert!(saved.contains("PATH=\"/new/tool/bin\""));
        assert!(!saved.contains("fixture-secret"));
        assert_eq!(
            std::fs::metadata(&env)
                .expect("owned fixture")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        if !foreground {
            let child = daemon.child.take().expect("owned fixture");
            assert!(!support::exits(child).success(), "managed restart exit");
            assert!(!daemon.home.path().join("manager-invoked").exists());
            continue;
        }
        assert!(client.call::<Ping>(&PingParams {}).await.is_ok());
        assert!(
            daemon
                .child
                .as_mut()
                .expect("owned fixture")
                .0
                .try_wait()
                .expect("owned fixture")
                .is_none()
        );
        assert!(!daemon.home.path().join("manager-invoked").exists());
        assert!(!daemon.home.path().join(".config/systemd/user").exists());
        std::fs::write(&bash, "#!/bin/sh\nexit 1\n").expect("owned fixture");
        let error = client
            .call::<RefreshEnv>(&PingParams {})
            .await
            .expect_err("fixture operation must fail");
        assert!(
            matches!(error, ClientError::Rpc { code, .. } if code == "system.env_refresh_failed")
        );
        assert_eq!(std::fs::read_to_string(env).expect("owned fixture"), saved);
        assert!(client.call::<Ping>(&PingParams {}).await.is_ok());
    }
}
