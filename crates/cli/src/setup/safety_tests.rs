use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
#[test]
fn publication_is_private_and_refuses_foreign_files_and_symlinks() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let env = temp.path().join("env");
    publish(&env, "PATH=\"/usr/bin\"\n", false).expect("publish fixture config");
    assert_eq!(
        std::fs::metadata(&env)
            .expect("inspect fixture metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(publish(&env, "replacement", true).is_err());
    assert_eq!(
        std::fs::read_to_string(&env).expect("read fixture text"),
        "PATH=\"/usr/bin\"\n"
    );
    let link = temp.path().join("link");
    symlink(&env, &link).expect("create fixture symlink");
    assert!(publish(&link, "evil", false).is_err());
    let parent_link = temp.path().join("parent");
    symlink(temp.path(), &parent_link).expect("create fixture symlink");
    assert!(publish(&parent_link.join("env"), "evil", false).is_err());
}
#[test]
fn resolved_xdg_paths_are_bound_to_only_the_selected_instance() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let mut paths = devbox_core::paths::Paths::resolve(
        devbox_core::paths::Instance::parse("test-custom").expect("parse fixture instance"),
        &FakeEnv(temp.path().into()),
    )
    .expect("parse fixture instance");
    let root = temp.path().join("custom space%\"${HOME}");
    paths.config_dir = root.join("config/devbox");
    paths.instance_config_dir = paths.config_dir.join("test-custom");
    paths.data_dir = root.join("data/devbox/test-custom");
    paths.bin_dir = root.join("data/devbox/bin");
    paths.state_dir = root.join("state/devbox/test-custom");
    paths.socket = root.join("runtime/devbox-test-custom.sock");
    let units = instance_units(&paths).expect("render fixture units");
    assert_eq!(units.len(), 4);
    assert_eq!(units[2].0, "devbox@test-custom.service.d/paths.conf");
    assert!(units[2].1.contains("ExecStart=:\""));
    assert!(units[2].1.contains("space%%\\\"${HOME}"));
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        assert!(units[2].1.contains(key));
    }
    assert!(units[3].1.contains("ListenStream=\nListenStream="));
    let other = temp
        .path()
        .join(".config/systemd/user/devbox@other.service.d/paths.conf");
    std::fs::create_dir_all(other.parent().expect("fixture path has parent"))
        .expect("fixture path has parent");
    std::fs::write(&other, "other instance").expect("write fixture file");
    for (name, body) in units {
        publish(
            &temp.path().join(".config/systemd/user").join(name),
            &body,
            true,
        )
        .expect("publish fixture config");
    }
    assert_eq!(
        std::fs::read_to_string(other).expect("read fixture text"),
        "other instance"
    );
    paths.socket = root.join("bad\npath");
    assert!(instance_units(&paths).is_err());
}
#[test]
fn unit_paths_without_xdg_parents_return_contextual_errors() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let paths = devbox_core::paths::Paths::resolve(
        devbox_core::paths::Instance::parse("test-parent").expect("parse fixture instance"),
        &FakeEnv(temp.path().into()),
    )
    .expect("resolve fixture paths");
    for (field, value, message) in [
        ("config", "/", "config directory has no XDG parent"),
        ("data", "/devbox", "data directory has no XDG ancestor"),
        ("state", "/devbox", "state directory has no XDG ancestor"),
        ("socket", "/", "socket has no runtime parent"),
    ] {
        let mut invalid = paths.clone();
        match field {
            "config" => invalid.config_dir = value.into(),
            "data" => invalid.data_dir = value.into(),
            "state" => invalid.state_dir = value.into(),
            _ => invalid.socket = value.into(),
        }
        assert_eq!(
            instance_units(&invalid).expect_err("reject missing parent"),
            message
        );
    }
}
#[test]
fn socket_path_is_accepted_by_systemd_parser_without_shell_quoting() {
    let temp = tempfile::tempdir().expect("create fixture directory");
    let mut paths = devbox_core::paths::Paths::resolve(
        devbox_core::paths::Instance::parse("test-parser").expect("parse fixture instance"),
        &FakeEnv(temp.path().into()),
    )
    .expect("parse fixture instance");
    // Spaces, quotes, backslashes and dollar signs are literal socket path bytes.
    paths.socket = temp.path().join("socket space%\"\\$literal.sock");
    let units = instance_units(&paths).expect("render fixture units");
    let body = &units[3].1;
    let socket = temp.path().join("fixture.socket");
    let service = temp.path().join("fixture.service");
    std::fs::write(&socket, body).expect("write fixture file");
    std::fs::write(&service, "[Service]\nExecStart=/bin/true\n").expect("write fixture file");
    let out = std::process::Command::new("/usr/bin/systemd-analyze")
        .args(["--user", "verify"])
        .arg(&socket)
        .arg(&service)
        .env_clear()
        .env("HOME", temp.path())
        .env("XDG_RUNTIME_DIR", temp.path())
        .env("PATH", "/usr/bin:/bin")
        .env("SYSTEMD_UNIT_PATH", format!("{}:", temp.path().display()))
        .output()
        .expect("systemd-analyze is required for socket parser regression");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = paths
        .socket
        .to_str()
        .expect("fixture path is UTF-8")
        .replace('%', "%%");
    assert!(body.contains(&format!("ListenStream={raw}\n")));
}
struct FakeEnv(std::path::PathBuf);
impl devbox_core::paths::Env for FakeEnv {
    fn var(&self, key: &str) -> Option<String> {
        (key == "HOME").then(|| self.0.display().to_string())
    }
    fn uid(&self) -> u32 {
        12345
    }
}
struct FakeRunner {
    env: Vec<u8>,
    calls: std::sync::Mutex<Vec<String>>,
    fail: Option<&'static str>,
}
impl Runner for FakeRunner {
    async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
        let call = format!("{cmd} {}", args.join(" "));
        self.calls
            .lock()
            .expect("lock fixture state")
            .push(call.clone());
        if self.fail.is_some_and(|s| call.contains(s)) {
            Err("fixture failure".into())
        } else {
            Ok(if cmd == "bash" {
                self.env.clone()
            } else {
                vec![]
            })
        }
    }
}
#[tokio::test]
async fn setup_orders_commands_and_stops_after_each_failure() {
    for fail in [None, Some("daemon-reload"), Some("enable"), Some("restart")] {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let paths = devbox_core::paths::Paths::resolve(
            devbox_core::paths::Instance::parse("test-setup").expect("parse fixture instance"),
            &FakeEnv(temp.path().into()),
        )
        .expect("parse fixture instance");
        let runner = FakeRunner {
            env: b"PATH=/usr/bin\0TOKEN=secret\0".to_vec(),
            calls: Default::default(),
            fail,
        };
        let report = install(&paths, &runner).await;
        let expected = match fail {
            None => 5,
            Some("daemon-reload") => 3,
            Some("enable") => 4,
            _ => 5,
        };
        assert_eq!(report.steps.len(), expected);
        assert_eq!(
            report.steps.last().expect("report contains a step").ok,
            fail.is_none()
        );
        let calls = runner.calls.lock().expect("lock fixture state");
        assert_eq!(calls.len(), expected - 1);
        if expected >= 4 {
            assert!(
                calls[2]
                    .contains("enable --now devbox@test-setup.socket devbox@test-setup.service")
            );
        }
    }
}
#[tokio::test]
async fn invalid_env_or_foreign_unit_prevents_reload() {
    for invalid_env in [true, false] {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let paths = devbox_core::paths::Paths::resolve(
            devbox_core::paths::Instance::parse("test-setup").expect("parse fixture instance"),
            &FakeEnv(temp.path().into()),
        )
        .expect("parse fixture instance");
        let unit_dir = paths.home.join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).expect("create fixture directory");
        std::fs::write(unit_dir.join("devbox@.socket"), "foreign").expect("write fixture file");
        let runner = FakeRunner {
            env: if invalid_env {
                b"PATH=relative\0".to_vec()
            } else {
                b"PATH=/usr/bin\0".to_vec()
            },
            calls: Default::default(),
            fail: None,
        };
        let report = install(&paths, &runner).await;
        assert_eq!(report.steps.len(), if invalid_env { 1 } else { 2 });
        assert!(!report.steps.last().expect("report contains a step").ok);
        assert_eq!(runner.calls.lock().expect("lock fixture state").len(), 1);
    }
}
