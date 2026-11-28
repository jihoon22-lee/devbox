//! A hand-run `devbox doctor` looks tools up in the instance's saved environment (what the
//! daemon and its tasks get), says which environment it used, and `--startup` keeps the caller's.
#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).expect("write owned command");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("set owned permissions");
}

struct Fixture {
    home: tempfile::TempDir,
}
impl Fixture {
    /// `saved` holds the only tools; the caller's `PATH` is an empty owned folder, so no host
    /// tool, service or shell profile is ever run.
    fn new() -> Self {
        let home = tempfile::tempdir().expect("owned doctor home");
        let saved = home.path().join("saved");
        fs::create_dir(&saved).expect("owned saved tool folder");
        fs::create_dir(home.path().join("caller")).expect("owned caller tool folder");
        executable(&saved.join("git"), "exit 0");
        executable(&saved.join("tmux"), "printf '%s\\n' 'tmux 3.6'");
        executable(
            &saved.join("gh"),
            "case \"$*\" in 'auth status --help') echo '      --no-refresh   fixture';; esac\nexit 0",
        );
        executable(
            &saved.join("powershell.exe"),
            "printf '[general]\\ninstanceIdleTimeout=-1\\n'",
        );
        let bin = home.path().join(".local/share/devbox-accept/bin");
        fs::create_dir_all(&bin).expect("owned binary directory");
        executable(&bin.join("devbox"), "exit 0");
        fs::create_dir_all(home.path().join(".config/devbox-accept/accept"))
            .expect("owned config directory");
        Self { home }
    }
    fn env_file(&self) -> std::path::PathBuf {
        self.home.path().join(".config/devbox-accept/accept/env")
    }
    fn save_environment(&self) {
        let saved = self.home.path().join("saved");
        fs::write(self.env_file(), format!("PATH=\"{}\"\n", saved.display()))
            .expect("owned saved environment");
    }
    fn command(&self) -> Command {
        let home = self.home.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_devbox"));
        command
            .arg("doctor")
            .env_clear()
            .env("HOME", home)
            .env("USER", "owned-doctor-fixture")
            .env("PATH", home.join("caller"))
            .env("DEVBOX_INSTANCE", "accept")
            .env("CLAUDE_CONFIG_DIR", home.join("claude"))
            .env("CODEX_HOME", home.join("codex"))
            .env("GH_CONFIG_DIR", home.join("gh"));
        command
    }
    fn doctor(&self, args: &[&str]) -> String {
        let output = self
            .command()
            .args(args)
            .output()
            .expect("run owned CLI fixture");
        String::from_utf8(output.stdout).expect("UTF-8 doctor output")
    }
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend(args);
        serde_json::from_str(&self.doctor(&all)).expect("doctor JSON")
    }
}

fn row<'a>(report: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == id)
        .unwrap_or_else(|| panic!("{id} row"))
}

#[test]
fn a_hand_run_doctor_finds_tools_that_are_only_in_the_saved_environment() {
    let fixture = Fixture::new();
    fixture.save_environment();
    let file = fixture.env_file().display().to_string();

    let report = fixture.json(&[]);
    assert_eq!(
        report["environment"],
        serde_json::json!({ "source": "instance", "file": file })
    );
    // A `PATH` lookup, two commands started by name, and the Windows reader.
    for id in ["tool.git", "tool.gh", "tool.tmux", "auth.gh", "wsl.idle"] {
        assert_eq!(row(&report, id)["status"], "ok", "{id}: {report}");
    }
    // The report still reads as the protocol's report.
    let typed: devbox_protocol::system::DoctorReport =
        serde_json::from_value(report).expect("protocol report");
    assert!(typed.checks.iter().any(|check| check.id == "env"));

    let text = fixture.doctor(&[]);
    assert_eq!(
        text.lines().next(),
        Some(
            format!(
                "점검 환경: 데몬 환경 파일 {file}의 PATH로 도구를 찾았습니다(에이전트 CLI의 셸 조회는 이 셸에서 합니다)"
            )
            .as_str()
        )
    );
    assert!(
        text.lines().any(|line| line.starts_with("Ok tool.gh ")),
        "{text}"
    );
}

#[test]
fn without_a_usable_saved_environment_the_doctor_says_it_used_the_callers() {
    let fixture = Fixture::new();
    let file = fixture.env_file().display().to_string();
    let oversized = format!("PATH=\"/usr/bin\"\n{}", "#".repeat(70_000));
    for (content, reason, said) in [
        (None, "missing", "이 없습니다"),
        (
            Some("PATH=\"relative\"\n"),
            "invalid",
            "쓸 수 있는 PATH가 없거나",
        ),
        (
            Some(oversized.as_str()),
            "invalid",
            "쓸 수 있는 PATH가 없거나",
        ),
    ] {
        match content {
            Some(content) => fs::write(fixture.env_file(), content).expect("owned environment"),
            None => assert!(!fixture.env_file().exists()),
        }
        let report = fixture.json(&[]);
        assert_eq!(
            report["environment"],
            serde_json::json!({ "source": "caller", "file": file, "reason": reason })
        );
        // The caller's `PATH` has no tools: nothing from the unusable file was used.
        assert_eq!(row(&report, "tool.gh")["status"], "warn", "{report}");
        assert!(
            row(&report, "wsl.idle")["message"]
                .as_str()
                .expect("message")
                .contains("powershell.exe"),
            "{report}"
        );
        let text = fixture.doctor(&[]);
        let first = text.lines().next().expect("environment line");
        assert!(
            first.starts_with("점검 환경: 이 셸의 PATH로 도구를 찾았습니다("),
            "{first}"
        );
        assert!(
            first.contains(said) && first.contains("devbox setup"),
            "{first}"
        );
    }
}

#[test]
fn the_startup_doctor_keeps_the_callers_environment_and_says_so() {
    let fixture = Fixture::new();
    fixture.save_environment();
    let report = fixture.json(&["--startup"]);
    assert_eq!(
        report["environment"],
        serde_json::json!({
            "source": "caller",
            "file": fixture.env_file().display().to_string(),
            "reason": "startup"
        })
    );
    // The saved environment has these tools; the start-up check does not look there.
    assert_eq!(row(&report, "tool.git")["status"], "fail", "{report}");
    assert_eq!(row(&report, "tool.tmux")["status"], "fail", "{report}");
    assert_eq!(row(&report, "tool.gh")["status"], "warn", "{report}");
    let text = fixture.doctor(&["--startup"]);
    assert!(
        text.lines().next().is_some_and(|line| line
            .starts_with("점검 환경: 이 셸의 PATH로 도구를 찾았습니다(앱이 시작할 때 하는 점검")),
        "{text}"
    );
}

#[test]
fn a_linked_saved_environment_is_not_followed_and_the_doctor_says_why() {
    let fixture = Fixture::new();
    let real = fixture.home.path().join("real-env");
    let saved = fixture.home.path().join("saved");
    fs::write(&real, format!("PATH=\"{}\"\n", saved.display())).expect("owned environment");
    std::os::unix::fs::symlink(&real, fixture.env_file()).expect("owned link");
    let report = fixture.json(&[]);
    assert_eq!(
        report["environment"],
        serde_json::json!({
            "source": "caller",
            "file": fixture.env_file().display().to_string(),
            "reason": "notRegular"
        })
    );
    assert_eq!(row(&report, "tool.gh")["status"], "warn", "{report}");
    let text = fixture.doctor(&[]);
    let first = text.lines().next().expect("environment line");
    assert!(first.contains("일반 파일이 아닙니다"), "{first}");
    assert!(first.contains("devbox setup"), "{first}");
    assert!(!first.contains("권한"), "{first}");
}

#[test]
#[cfg(target_os = "linux")]
fn terminating_startup_doctor_also_terminates_its_probe() {
    use std::{
        io::{ErrorKind, Read},
        os::unix::{net::UnixListener, process::ExitStatusExt},
        process::Stdio,
        time::{Duration, Instant},
    };

    let fixture = Fixture::new();
    let listener =
        UnixListener::bind(fixture.home.path().join("ready.sock")).expect("owned socket");
    listener
        .set_nonblocking(true)
        .expect("nonblocking fixture listener");
    // The probe's open socket identifies its lifetime without looking up a possibly reused PID.
    // The fixture also exits on EOF, even when the behavior under test regresses.
    executable(
        &fixture.home.path().join("caller/powershell.exe"),
        r#"exec /usr/bin/python3 -c 'import os, socket; s=socket.socket(socket.AF_UNIX); s.connect(os.path.join(os.environ["HOME"],"ready.sock")); s.recv(1)'"#,
    );
    let mut doctor = fixture
        .command()
        .arg("--startup")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start fixture doctor");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut connection = loop {
        match listener.accept() {
            Ok((connection, _)) => break connection,
            Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                let _ = doctor.kill();
                let _ = doctor.wait();
                panic!("owned probe did not connect: {e}");
            }
        }
    };
    doctor.kill().expect("terminate doctor during the probe");
    assert_eq!(
        doctor.wait().expect("reap doctor").signal(),
        Some(libc::SIGKILL)
    );
    connection
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("limit probe disconnect wait");
    assert_eq!(
        connection
            .read(&mut [0])
            .expect("probe must exit with its doctor"),
        0
    );
}
