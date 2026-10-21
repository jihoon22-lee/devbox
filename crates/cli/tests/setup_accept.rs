//! Every setup subprocess and file belongs to this temporary HOME.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("fixture value")).expect("fixture result");
    std::fs::write(path, bytes).expect("fixture result");
}
fn fingerprint(path: &Path) -> (Vec<u8>, SystemTime) {
    (
        std::fs::read(path).expect("fixture result"),
        std::fs::metadata(path)
            .expect("fixture result")
            .modified()
            .expect("fixture result"),
    )
}
fn executable(path: &Path, script: &str) {
    write(path, script.as_bytes());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("fixture result");
}

fn command(home: &Path) -> Command {
    let stub_dir = home.join("stubs");
    executable(
        &stub_dir.join("bash"),
        "#!/bin/sh\nprintf 'PATH=/usr/bin\\0LANG=C\\0'\n",
    );
    executable(
        &stub_dir.join("systemctl"),
        "#!/bin/sh\nprintf '%s ' \"$@\" >> \"$HOME/systemctl-calls\"\nprintf '\\n' >> \"$HOME/systemctl-calls\"\n",
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_devbox"));
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", &stub_dir)
        .env("XDG_RUNTIME_DIR", home.join("run"))
        .env("DEVBOX_INSTANCE", "accept");
    command
}

#[test]
fn accept_setup_leaves_prod_binary_and_template_untouched() {
    let fixture = tempfile::tempdir().expect("fixture result");
    let home = fixture.path();
    let files = [
        ".local/share/devbox/bin/devbox",
        ".config/systemd/user/devbox@.service",
        ".config/systemd/user/devbox@.socket",
        ".config/systemd/user/devbox-tmux@.service",
        ".config/devbox/config.toml",
        ".config/devbox/config.local.toml",
        ".config/devbox/prod/env",
        ".local/share/devbox/prod/secret-names.json",
    ];
    let before: Vec<_> = files
        .iter()
        .map(|relative| {
            let path = home.join(relative);
            write(
                &path,
                format!("# Managed by devbox setup\nowned prod sentinel {relative}\n").as_bytes(),
            );
            let before = fingerprint(&path);
            (path, before)
        })
        .collect();
    let output = command(home)
        .args(["setup", "--json"])
        .output()
        .expect("fixture result");
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for (path, before) in before {
        assert_eq!(
            fingerprint(&path),
            before,
            "prod sentinel changed: {}",
            path.display()
        );
    }
    let units = home.join(".config/systemd/user");
    for name in [
        "devbox-accept@.socket",
        "devbox-accept@.service",
        "devbox-accept-tmux@.service",
    ] {
        assert!(units.join(name).is_file(), "missing {name}");
    }
    let service =
        std::fs::read_to_string(units.join("devbox-accept@.service")).expect("fixture result");
    assert!(service.contains(".local/share/devbox-accept/bin/devbox daemon"));
    assert!(service.contains(".config/devbox-accept/%i/env"));
    let calls = std::fs::read_to_string(home.join("systemctl-calls")).expect("fixture result");
    assert_eq!(
        calls,
        "--user daemon-reload \n--user enable --now devbox-accept@accept.socket devbox-accept@accept.service devbox-accept-tmux@accept.service \n--user restart devbox-accept@accept.service \n"
    );
}

#[test]
fn accept_setup_preserves_custom_xdg_roots_across_systemd() {
    let fixture = tempfile::tempdir().expect("fixture result");
    let home = fixture.path();
    let config = home.join("custom config");
    let data = home.join("custom data");
    let prod_template = config.join("systemd/user/devbox@.service");
    write(&prod_template, b"owned foreign prod template");
    let before = fingerprint(&prod_template);
    let output = command(home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .args(["setup", "--json"])
        .output()
        .expect("fixture result");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fingerprint(&prod_template), before);
    assert!(!home.join(".config").exists());
    assert!(config.join("devbox-accept/accept/env").is_file());
    let dropin = std::fs::read_to_string(
        config.join("systemd/user/devbox-accept@accept.service.d/paths.conf"),
    )
    .expect("fixture result");
    assert!(dropin.contains(&format!(
        "Environment=\"XDG_CONFIG_HOME={}\"",
        config.display()
    )));
    assert!(dropin.contains(&format!("Environment=\"XDG_DATA_HOME={}\"", data.display())));
    assert!(dropin.contains(&format!(
        "ExecStart=:\"{}/devbox-accept/bin/devbox\" daemon",
        data.display()
    )));
}

#[test]
fn accept_doctor_requires_accept_hook_binary_even_when_prod_exists() {
    let fixture = tempfile::tempdir().expect("fixture result");
    let home = fixture.path();
    executable(
        &home.join(".local/share/devbox/bin/devbox"),
        "#!/bin/sh\nexit 0\n",
    );
    for available in [false, true] {
        if available {
            executable(
                &home.join(".local/share/devbox-accept/bin/devbox"),
                "#!/bin/sh\nexit 0\n",
            );
        }
        let output = command(home)
            .args(["doctor", "--json"])
            .output()
            .expect("fixture result");
        let report: devbox_protocol::system::DoctorReport =
            serde_json::from_slice(&output.stdout).expect("fixture result");
        let hook = report
            .checks
            .iter()
            .find(|check| check.id == "hook.binary")
            .expect("fixture value");
        assert_eq!(
            hook.status,
            if available {
                devbox_protocol::system::Status::Ok
            } else {
                devbox_protocol::system::Status::Fail
            }
        );
    }
}
