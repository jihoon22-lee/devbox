//! Real CLI output boundary: version command line endings must not fail startup doctor.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write owned command");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("set owned permissions");
}

#[test]
fn doctor_accepts_version_line_endings_but_rejects_old_and_malformed_versions() {
    let fixture = tempfile::tempdir().expect("owned doctor home");
    let home = fixture.path();
    let tools = home.join("tools");
    fs::create_dir(&tools).expect("owned command directory");
    // Never invoke host services, optional tools, credential probes, or shell profiles.
    for tool in [
        "systemctl",
        "loginctl",
        "git",
        "claude",
        "codex",
        "gh",
        "node",
        "pnpm",
        "cargo",
        "rustc",
        "docker",
        "powershell.exe",
    ] {
        executable(&tools.join(tool), "exit 0");
    }
    let config = home.join(".config/devbox-accept/accept");
    fs::create_dir_all(&config).expect("owned config directory");
    fs::write(config.join("env"), "PATH=\"/owned/tools\"\n").expect("owned saved environment");
    let bin = home.join(".local/share/devbox-accept/bin");
    fs::create_dir_all(&bin).expect("owned binary directory");
    executable(&bin.join("devbox"), "exit 0");

    for (version, expected) in [
        ("tmux 3.6\n", "ok"),
        ("tmux 3.6\r\n", "ok"),
        ("tmux next-3.6\n", "ok"),
        ("tmux 2.9\n", "fail"),
        ("tmux 3.6?\n", "fail"),
        ("tmux 3.6\nunexpected", "fail"),
    ] {
        executable(&tools.join("tmux"), &format!("printf '%s' '{version}'"));
        let output = Command::new(env!("CARGO_BIN_EXE_devbox"))
            .args(["doctor", "--json"])
            .env_clear()
            .env("HOME", home)
            .env("USER", "owned-doctor-fixture")
            .env("PATH", &tools)
            .env("DEVBOX_INSTANCE", "accept")
            .env("CLAUDE_CONFIG_DIR", home.join("claude"))
            .env("CODEX_HOME", home.join("codex"))
            .env("GH_CONFIG_DIR", home.join("gh"))
            .output()
            .expect("run owned CLI fixture");
        let report: devbox_protocol::system::DoctorReport =
            serde_json::from_slice(&output.stdout).expect("doctor JSON report");
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "tool.tmux")
            .expect("tmux check");
        let status = serde_json::to_value(check.status).expect("status JSON");
        assert_eq!(status, expected, "owned version output {version:?}");
        // PID 1 need not be systemd on CI. The CLI exit must still reflect all Fail checks.
        assert_eq!(
            output.status.success(),
            !report
                .checks
                .iter()
                .any(|check| check.status == devbox_protocol::system::Status::Fail)
        );
    }
}
