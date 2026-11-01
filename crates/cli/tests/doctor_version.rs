//! Real CLI output boundary: version command line endings must not fail startup doctor, and the
//! app's start-up doctor starts no interactive shell and no agent CLI.
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

#[test]
fn the_startup_doctor_asks_no_shell_and_starts_no_agent_cli() {
    let fixture = tempfile::tempdir().expect("owned doctor home");
    let home = fixture.path();
    let tools = home.join("tools");
    fs::create_dir(&tools).expect("owned command directory");
    // Never invoke host services, optional tools, credential probes, or shell profiles.
    for tool in [
        "systemctl",
        "loginctl",
        "git",
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
    executable(&tools.join("tmux"), "printf '%s\\n' 'tmux 3.6'");
    // The interactive shell and the agent CLIs only record that they were started.
    let started = home.join("started");
    for tool in ["bash", "claude", "codex", "opencode", "pi"] {
        executable(
            &tools.join(tool),
            &format!("echo {tool} >> '{}'", started.display()),
        );
    }
    let config = home.join(".config/devbox-accept/accept");
    fs::create_dir_all(&config).expect("owned config directory");
    fs::write(config.join("env"), "PATH=\"/owned/tools\"\n").expect("owned saved environment");
    let bin = home.join(".local/share/devbox-accept/bin");
    fs::create_dir_all(&bin).expect("owned binary directory");
    executable(&bin.join("devbox"), "exit 0");
    let doctor = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_devbox"))
            .args(args)
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
        let ids: Vec<_> = report.checks.into_iter().map(|check| check.id).collect();
        (output.status.success(), ids)
    };
    let agent_rows = [
        "tool.claude",
        "tool.codex",
        "tool.opencode",
        "tool.pi",
        "auth.claude",
        "auth.codex",
    ];
    let (startup_ok, startup) = doctor(&["doctor", "--json", "--startup"]);
    assert!(
        !started.exists(),
        "the start-up doctor started a shell or an agent CLI"
    );
    for id in agent_rows {
        assert!(!startup.iter().any(|row| row == id), "{id}");
    }
    for id in ["systemd", "tool.tmux", "env", "tool.git", "hook.binary"] {
        assert!(startup.iter().any(|row| row == id), "{id}");
    }
    // The environment check still looks each agent CLI up and asks it for its version.
    let (full_ok, full) = doctor(&["doctor", "--json"]);
    for id in agent_rows {
        assert!(full.iter().any(|row| row == id), "{id}");
    }
    let mut seen: Vec<_> = fs::read_to_string(&started)
        .expect("the full doctor's starts")
        .lines()
        .map(str::to_owned)
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            "bash", "bash", "bash", "bash", "claude", "codex", "opencode", "pi"
        ]
    );
    // Agent rows never fail, so leaving them out cannot change whether a launch goes on.
    assert_eq!(startup_ok, full_ok);
}
