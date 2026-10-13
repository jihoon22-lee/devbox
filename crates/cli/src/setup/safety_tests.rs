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
    assert_eq!(units.len(), 6);
    assert_eq!(units[3].0, "devbox@test-custom.service.d/paths.conf");
    assert!(units[3].1.contains("ExecStart=:\""));
    assert!(units[3].1.contains("space%%\\\"${HOME}"));
    for key in [
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        assert!(units[3].1.contains(key));
    }
    assert!(units[4].1.contains("ListenStream=\nListenStream="));
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
    let body = &units[4].1;
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
#[test]
fn environment_file_path_is_loaded_by_systemd_parser() {
    for suffix in ["ordinary", "space%\"\\${HOME}#literal"] {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let mut paths = devbox_core::paths::Paths::resolve(
            devbox_core::paths::Instance::parse("test-env-parser").expect("parse fixture instance"),
            &FakeEnv(temp.path().into()),
        )
        .expect("resolve fixture paths");
        paths.config_dir = temp.path().join(suffix).join("devbox");
        paths.instance_config_dir = paths.config_dir.join(paths.instance.as_str());
        let env_file = paths.instance_config_dir.join("env");
        publish(&env_file, "PATH=\"/usr/bin\"\n", false).expect("publish fixture environment");
        std::fs::create_dir_all(&paths.bin_dir).expect("create fixture binary directory");
        let binary = paths.bin_dir.join("devbox");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").expect("write fixture executable");
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))
            .expect("make fixture executable");
        let units = instance_units(&paths).expect("render fixture units");
        let service = temp.path().join("test-env-parser.service");
        std::fs::write(&service, &units[3].1).expect("write fixture unit");
        // Offline manager parsing only: never load/start a unit in the real user manager.
        let out = std::process::Command::new("/usr/bin/systemd-analyze")
            .args(["--user", "--generators=no", "verify"])
            .arg(&service)
            .env_clear()
            .env("HOME", temp.path())
            .env("XDG_RUNTIME_DIR", temp.path())
            .env("PATH", "/usr/bin:/bin")
            .env("SYSTEMD_UNIT_PATH", format!("{}:", temp.path().display()))
            .env("SYSTEMD_LOG_LEVEL", "debug")
            .output()
            .expect("systemd-analyze is required for environment file parser regression");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{stderr}");
        // verify may exit successfully even when it warns and ignores EnvironmentFile.
        let warning = stderr
            .lines()
            .find(|line| line.contains("EnvironmentFile= path"));
        assert!(warning.is_none(), "{warning:?}");
        let stdout = String::from_utf8_lossy(&out.stdout);
        let expected = env_file.to_str().expect("fixture path is UTF-8");
        assert!(
            stdout
                .lines()
                .any(|line| line.trim_start().strip_prefix("EnvironmentFile: ") == Some(expected)),
            "environment file was not loaded literally: {stdout}\n{stderr}"
        );
    }
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
            assert!(calls[2].contains("devbox-tmux@test-setup.service"));
            assert!(
                !calls
                    .iter()
                    .any(|call| call.contains("restart devbox-tmux@"))
            );
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

#[test]
fn tmux_unit_is_managed_foreground_and_independent_of_daemon_lifetime() {
    let units = render_units();
    let (_, body) = units
        .iter()
        .find(|(name, _)| *name == "devbox-tmux@.service")
        .expect("tmux unit");
    assert!(body.starts_with(MARKER));
    assert!(body.contains("Type=exec"));
    assert!(body.contains("tmux -D -L devbox-%i"));
    assert!(!body.contains("PartOf=devbox@"));
    assert!(!body.contains("KillMode=none"));
}

#[tokio::test]
async fn tmux_config_refuses_foreign_files_and_symlinks_before_manager_changes() {
    for linked in [false, true] {
        let temp = tempfile::tempdir().expect("fixture home");
        let paths = devbox_core::Paths::resolve(
            devbox_core::Instance::parse("test-tmux").expect("instance"),
            &FakeEnv(temp.path().into()),
        )
        .expect("paths");
        std::fs::create_dir_all(&paths.instance_config_dir).expect("fixture config directory");
        let target = paths.instance_config_dir.join("tmux.conf");
        if linked {
            let foreign = temp.path().join("foreign");
            std::fs::write(&foreign, "foreign").expect("foreign fixture");
            symlink(foreign, &target).expect("fixture symlink");
        } else {
            std::fs::write(&target, "foreign").expect("foreign fixture");
        }
        let runner = FakeRunner {
            env: b"PATH=/usr/bin\0".to_vec(),
            calls: Default::default(),
            fail: None,
        };
        let report = install(&paths, &runner).await;
        assert!(!report.steps.last().expect("report").ok);
        assert_eq!(runner.calls.lock().expect("calls").len(), 1);
        assert_eq!(
            std::fs::read_to_string(target).expect("foreign remains"),
            "foreign"
        );
    }
}

#[tokio::test]
async fn refresh_captures_only_environment_and_preserves_previous_on_failure() {
    let temp = tempfile::tempdir().expect("fixture home");
    let paths = devbox_core::Paths::resolve(
        devbox_core::Instance::parse("test-refresh").unwrap(),
        &FakeEnv(temp.path().into()),
    )
    .unwrap();
    let runner = FakeRunner {
        env: b"PATH=/usr/bin\0TOKEN=secret\0".to_vec(),
        calls: Default::default(),
        fail: None,
    };
    refresh_with(&paths, &runner).await.unwrap();
    let file = paths.instance_config_dir.join("env");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text, "PATH=\"/usr/bin\"\n");
    assert_eq!(*runner.calls.lock().unwrap(), vec!["bash -lc env -0"]);
    assert!(!paths.home.join(".config/systemd/user").exists());
    let invalid = FakeRunner {
        env: b"PATH=relative\0".to_vec(),
        calls: Default::default(),
        fail: None,
    };
    assert!(refresh_with(&paths, &invalid).await.is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let foreign = temp.path().join("foreign");
    std::fs::write(&foreign, "preserve").unwrap();
    std::fs::remove_file(&file).unwrap();
    symlink(&foreign, &file).unwrap();
    assert!(refresh_with(&paths, &runner).await.is_err());
    assert_eq!(std::fs::read_to_string(foreign).unwrap(), "preserve");
}
