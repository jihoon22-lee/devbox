use serde::Deserialize;
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Exit {
    code: Option<i32>,
    signal: Option<String>,
    by: String,
    attempt_id: String,
}
struct Fixture {
    temp: tempfile::TempDir,
    instance: String,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().expect("fixture"),
            instance: format!("test-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]),
        }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_devbox"));
        cmd.env_clear()
            .env(
                "PATH",
                std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()),
            )
            .args(["--instance", &self.instance])
            .env("HOME", self.temp.path())
            .env("XDG_DATA_HOME", self.temp.path().join("data"))
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("XDG_CONFIG_HOME", self.temp.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.temp.path())
            .env_remove("INVOCATION_ID")
            .env_remove("TOKEN")
            .stdin(Stdio::null());
        cmd
    }
    fn runs(&self) -> PathBuf {
        self.temp
            .path()
            .join("data/devbox")
            .join(&self.instance)
            .join("runs")
    }
    fn exit(&self) -> Exit {
        serde_json::from_slice(&std::fs::read(self.runs().join("run-1.exit")).expect("exit file"))
            .expect("valid exit")
    }
    fn log(&self) -> String {
        std::fs::read_to_string(self.runs().join("run-1.log")).expect("log file")
    }
}
#[test]
fn wrapper_records_lines_with_stream_markers_and_exit_code() {
    let f = Fixture::new();
    let status = f
        .command()
        .args([
            "exec",
            "--run",
            "run-1",
            "--",
            "sh",
            "-c",
            "echo out; echo err >&2; exit 3",
        ])
        .status()
        .expect("wrapper");
    assert_eq!(status.code(), Some(3));
    assert_eq!(f.exit().code, Some(3));
    let log = f.log();
    let mut lines: Vec<_> = log
        .lines()
        .map(|line| line.split_once('\t').expect("timestamp").1)
        .collect();
    assert_eq!(
        lines.remove(0),
        format!(
            "s\t{}",
            devbox_core::run_log::attempt_marker(&f.exit().attempt_id)
        )
    );
    lines.sort();
    assert_eq!(lines, ["e\terr", "o\tout"]);
}

use nix::{
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use std::{
    io::Write,
    os::unix::process::ExitStatusExt,
    time::{Duration, Instant},
};
struct OwnedChild {
    child: std::process::Child,
    pid_file: PathBuf,
}
impl OwnedChild {
    fn wait(&mut self) -> std::process::ExitStatus {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().expect("wait") {
                return status;
            }
            assert!(Instant::now() < end, "owned wrapper timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Ok(text) = std::fs::read_to_string(&self.pid_file) {
            if let Ok(pid) = text.parse::<i32>() {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }
    }
}
impl Fixture {
    fn spawn(&self, script: &str, attempt: &str, input: Option<&[u8]>) -> OwnedChild {
        let pid_file = self.temp.path().join("child-pid");
        let mut cmd = self.command();
        cmd.args(["exec", "--run", "run-1", "--attempt", attempt])
            .env("READY", &pid_file)
            .env("FINISH", self.temp.path().join("finish"));
        if input.is_some() {
            cmd.arg("--secrets-stdin").stdin(Stdio::piped());
        }
        cmd.args(["--", "python3", "-c", script]);
        let mut child = cmd.spawn().expect("spawn wrapper");
        if let Some(input) = input {
            child
                .stdin
                .take()
                .expect("secret stdin")
                .write_all(input)
                .expect("secret input");
        }
        OwnedChild { child, pid_file }
    }
    fn ready(&self) -> i32 {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(text) = std::fs::read_to_string(self.temp.path().join("child-pid")) {
                if let Ok(pid) = text.parse() {
                    return pid;
                }
            }
            assert!(Instant::now() < end, "fixture child did not become ready");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn fallback(
        &self,
        attempt: Option<&str>,
        invocation: Option<&str>,
        kind: &str,
        status: &str,
    ) -> std::process::Output {
        let mut cmd = self.command();
        cmd.args(["run-exited", "--run", "run-1"])
            .env("SERVICE_RESULT", "signal")
            .env("EXIT_CODE", kind)
            .env("EXIT_STATUS", status);
        if let Some(attempt) = attempt {
            cmd.args(["--attempt", attempt]);
        }
        if let Some(invocation) = invocation {
            cmd.env("INVOCATION_ID", invocation);
        }
        cmd.output().expect("fallback")
    }
}
#[test]
fn secrets_from_stdin_reach_the_child_but_not_argv() {
    let f = Fixture::new();
    let script = r#"import hashlib,os,pathlib,time
assert hashlib.sha256(os.environ['TOKEN'].encode()).hexdigest() == '6627521dfffd5769d0608ed28e49d0865f7fbe74c360d9e721422d8df7f94903'
print('ACK',flush=True)
pathlib.Path(os.environ['READY']).write_text(str(os.getpid()))
while not pathlib.Path(os.environ['FINISH']).exists(): time.sleep(.01)
"#;
    let mut owned = f.spawn(
        script,
        "secret-attempt",
        Some(b"TOKEN=owned-token-for-task3\0"),
    );
    let child = f.ready();
    for pid in [owned.child.id(), child as u32] {
        let argv = std::fs::read(format!("/proc/{pid}/cmdline")).expect("owned argv");
        assert!(!argv.windows(20).any(|w| w == b"owned-token-for-task3"));
    }
    let parent_env =
        std::fs::read(format!("/proc/{}/environ", owned.child.id())).expect("wrapper environment");
    assert!(
        !parent_env
            .split(|b| *b == 0)
            .any(|entry| entry.starts_with(b"TOKEN="))
    );
    std::fs::write(f.temp.path().join("finish"), "").expect("release");
    assert!(owned.wait().success());
    assert!(f.log().contains("\to\tACK\n"));
    assert!(!f.log().contains("owned-token-for-task3"));
}
#[test]
fn secrets_are_masked_before_chunk_newline_and_truncation_boundaries() {
    let f = Fixture::new();
    let script = r#"import os
v=os.environ['TOKEN'].encode()
os.write(1,v[:5]);os.write(1,v[5:]+b'\n')
os.write(2,os.environ['MULTI'].encode()+b'\n')
os.write(1,b'x'*(1024*1024-4)+v+b'END\n')
os.write(1,os.environb[b'RAW']+b'\nACK\n')
"#;
    let mut owned = f.spawn(script, "mask-attempt", Some(b"TOKEN=owned-token-for-task3\0MULTI=multi-secret\nsecond-secret\0RAW=raw-\xff-secret\0"));
    assert!(owned.wait().success());
    let log = f.log();
    for forbidden in ["owned", "multi-secret", "second-secret", "raw-", "END"] {
        assert!(
            !log.contains(forbidden),
            "leaked fixture segment {forbidden}"
        );
    }
    assert!(log.contains("[REDACTED]"));
    assert!(log.contains("…(잘림)"));
    assert!(log.contains("\to\tACK\n"));
    assert!(log.len() < 1024 * 1024 + 1024);
}
#[test]
fn wrapper_drains_concurrent_streams_and_bounds_invalid_utf8_long_lines() {
    let f = Fixture::new();
    let script = r#"import os,threading
def emit(fd,prefix):
 for i in range(500): os.write(fd,prefix+str(i).encode()+b'\n')
a=threading.Thread(target=emit,args=(1,b'out-')); b=threading.Thread(target=emit,args=(2,b'err-'))
a.start(); b.start(); a.join(); b.join()
os.write(1,b'\xff'+b'x'*(1024*1024+100)+b'\nlast')
"#;
    let mut owned = f.spawn(script, "concurrent", None);
    assert!(owned.wait().success());
    let log = f.log();
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 1003);
    for i in 0..500 {
        assert!(log.contains(&format!("\to\tout-{i}\n")));
        assert!(log.contains(&format!("\te\terr-{i}\n")));
    }
    let stdout: Vec<_> = lines.iter().filter(|line| line.contains("\to\t")).collect();
    assert!(stdout[500].contains('�'));
    assert!(stdout[500].ends_with("…(잘림)"));
    assert!(stdout[501].ends_with("\to\tlast"));
}
#[test]
fn run_exited_does_not_overwrite_wrapper_result() {
    let f = Fixture::new();
    assert_eq!(
        f.command()
            .args([
                "exec",
                "--run",
                "run-1",
                "--attempt",
                "first",
                "--",
                "sh",
                "-c",
                "exit 3"
            ])
            .status()
            .expect("wrapper")
            .code(),
        Some(3)
    );
    let before = std::fs::read(f.runs().join("run-1.exit")).expect("result");
    assert!(
        f.fallback(Some("first"), None, "killed", "KILL")
            .status
            .success()
    );
    assert!(f.fallback(None, None, "killed", "9").status.success());
    assert_eq!(
        std::fs::read(f.runs().join("run-1.exit")).expect("result"),
        before
    );
    assert_eq!(f.exit().by, "wrapper");
}
#[test]
fn run_exited_records_killed_wrapper() {
    let f = Fixture::new();
    let script = "import os,pathlib,time\nprint('before kill',flush=True)\npathlib.Path(os.environ['READY']).write_text(str(os.getpid()))\ntime.sleep(30)";
    let mut owned = f.spawn(script, "killed", None);
    f.ready();
    let end = Instant::now() + Duration::from_secs(5);
    while !f.log().contains("before kill") {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    owned.child.kill().expect("kill owned wrapper");
    assert_eq!(owned.wait().signal(), Some(9));
    assert!(
        f.fallback(Some("killed"), Some("killed"), "killed", "KILL")
            .status
            .success()
    );
    let exit = f.exit();
    assert_eq!(exit.by, "systemd");
    assert_eq!(exit.signal.as_deref(), Some("KILL"));
    assert_eq!(exit.code, None);
    assert!(f.log().contains("before kill"));
}
#[test]
fn term_reaches_child_group_and_wrapper_preserves_signal_status() {
    let f = Fixture::new();
    let script = r#"import os,pathlib,signal,subprocess,time
p=subprocess.Popen(['python3','-c',"import os,pathlib,signal,time; signal.signal(signal.SIGTERM,lambda *a:(pathlib.Path(os.environ['READY']+'.term').write_text('TERM'),exit(0))); pathlib.Path(os.environ['READY']+'.grand').write_text(str(os.getpid())); time.sleep(30)"])
while not pathlib.Path(os.environ['READY']+'.grand').exists(): time.sleep(.01)
pathlib.Path(os.environ['READY']).write_text(str(os.getpid()))
time.sleep(30)
"#;
    let mut owned = f.spawn(script, "term", None);
    f.ready();
    kill(Pid::from_raw(owned.child.id() as i32), Signal::SIGTERM).expect("term wrapper");
    assert_eq!(owned.wait().signal(), Some(15));
    assert_eq!(f.exit().signal.as_deref(), Some("TERM"));
    assert_eq!(
        std::fs::read_to_string(f.temp.path().join("child-pid.term")).expect("grandchild term"),
        "TERM"
    );
}
#[test]
fn new_attempt_retires_exit_and_old_or_duplicate_fallback_cannot_finish_it() {
    let f = Fixture::new();
    assert!(
        f.command()
            .args(["exec", "--run", "run-1", "--attempt", "first", "--", "true"])
            .status()
            .expect("first")
            .success()
    );
    let script = "import os,pathlib,time\npathlib.Path(os.environ['READY']).write_text(str(os.getpid()))\nwhile not pathlib.Path(os.environ['FINISH']).exists(): time.sleep(.01)";
    let mut owned = f.spawn(script, "second", None);
    f.ready();
    assert!(!f.runs().join("run-1.exit").exists());
    assert!(f.runs().join("run-1.first.exit").is_file());
    for invocation in [None, Some("first")] {
        assert!(
            f.fallback(Some("first"), invocation, "killed", "9")
                .status
                .success()
        );
    }
    assert!(!f.runs().join("run-1.exit").exists());
    std::fs::write(f.temp.path().join("finish"), "").expect("release");
    assert!(owned.wait().success());
    assert_eq!(f.exit().attempt_id, "second");
}
#[test]
fn systemd_fallback_covers_failure_before_begin_and_rejects_retired_invocations() {
    let f = Fixture::new();
    // No exec at all: systemd failed before launching the wrapper.
    assert!(
        f.fallback(None, Some("early"), "killed", "9")
            .status
            .success()
    );
    assert_eq!(f.exit().attempt_id, "early");
    assert!(
        f.fallback(None, Some("next-early"), "killed", "9")
            .status
            .success()
    );
    assert_eq!(f.exit().attempt_id, "next-early");
    assert!(
        f.fallback(None, Some("early"), "killed", "9")
            .status
            .success()
    );
    assert_eq!(f.exit().attempt_id, "next-early");
    assert!(
        f.command()
            .args(["exec", "--run", "run-1", "--attempt", "newer", "--", "true"])
            .status()
            .expect("newer")
            .success()
    );
    assert!(
        f.fallback(None, Some("next-early"), "killed", "9")
            .status
            .success()
    );
    assert_eq!(f.exit().attempt_id, "newer");
}
#[test]
fn malformed_inputs_have_no_run_files_or_child_side_effects() {
    for args in [
        vec!["exec", "--run", "../bad", "--", "true"],
        vec![
            "exec",
            "--run",
            "run-1",
            "--attempt",
            "../bad",
            "--",
            "true",
        ],
        vec![
            "exec",
            "--run",
            "run-1",
            "--",
            "no-such-owned-task3-executable",
        ],
    ] {
        let f = Fixture::new();
        assert!(
            !f.command()
                .args(args)
                .output()
                .expect("invalid")
                .status
                .success()
        );
        assert!(!f.runs().exists());
    }
    for input in [
        b"TOKEN=owned-token".as_slice(),
        b"BAD NAME=owned-token\0",
        b"TOKEN=first\0TOKEN=second\0",
    ] {
        let f = Fixture::new();
        let mut owned = f.spawn(
            "raise AssertionError('child must not execute')",
            "invalid",
            Some(input),
        );
        assert!(!owned.wait().success());
        assert!(!f.runs().exists());
    }
}

#[tokio::test]
async fn wrapper_notifies_owned_socket_with_only_current_attempt_identity() {
    use devbox_protocol::{
        frame::FrameCodec,
        message::{ClientMessage, ServerMessage, json_frame, parse_json},
    };
    use futures::{SinkExt, StreamExt};
    use tokio_util::codec::Framed;
    let f = Fixture::new();
    let listener =
        tokio::net::UnixListener::bind(f.temp.path().join(format!("devbox-{}.sock", f.instance)))
            .expect("owned socket");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept wrapper");
        let mut framed = Framed::new(socket, FrameCodec::default());
        let hello = framed
            .next()
            .await
            .expect("hello frame")
            .expect("hello read");
        assert!(matches!(
            parse_json::<ClientMessage>(&hello).expect("hello"),
            ClientMessage::Hello { .. }
        ));
        framed
            .send(json_frame(
                0,
                &ServerMessage::Welcome {
                    version: devbox_cli::VERSION.into(),
                    protocol: devbox_protocol::PROTOCOL_VERSION,
                    daemon_id: "owned-task3".into(),
                },
            ))
            .await
            .expect("welcome");
        let frame = framed.next().await.expect("request").expect("frame");
        let ClientMessage::Request { method, params, .. } =
            parse_json::<ClientMessage>(&frame).expect("request")
        else {
            panic!("expected request")
        };
        assert_eq!(method, "runtime.exited");
        let event: devbox_runtime::exit_record::Exited =
            serde_json::from_str(params.get()).expect("typed exited event");
        // Dropping without an RPC response must not alter the durable child exit.
        event
    });
    let mut cmd = f.command();
    cmd.args([
        "exec",
        "--run",
        "run-1",
        "--attempt",
        "socket-attempt",
        "--",
        "sh",
        "-c",
        "exit 3",
    ]);
    let output = tokio::process::Command::from(cmd)
        .output()
        .await
        .expect("wrapper");
    assert_eq!(output.status.code(), Some(3));
    let event = tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .expect("server timeout")
        .expect("server");
    assert_eq!(event.run_id, "run-1");
    assert_eq!(event.attempt_id, "socket-attempt");
    assert_eq!(f.exit().code, Some(3));
}
#[tokio::test]
async fn stalled_notification_is_bounded_and_preserves_the_durable_result() {
    let f = Fixture::new();
    let listener =
        tokio::net::UnixListener::bind(f.temp.path().join(format!("devbox-{}.sock", f.instance)))
            .expect("owned socket");
    let mut cmd = f.command();
    cmd.args(["exec", "--run", "run-1", "--", "sh", "-c", "exit 3"]);
    let child = tokio::spawn(async move {
        tokio::process::Command::from(cmd)
            .output()
            .await
            .expect("wrapper")
    });
    let (_stalled, _) = listener.accept().await.expect("socket");
    let output = tokio::time::timeout(Duration::from_secs(2), child)
        .await
        .expect("bounded notification")
        .expect("child task");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(f.exit().code, Some(3));
}
#[test]
fn explicit_attempt_precedes_systemd_invocation_and_standalone_ids_are_fresh() {
    let f = Fixture::new();
    let run = |attempt: Option<&str>, invocation: Option<&str>| {
        let mut cmd = f.command();
        cmd.args(["exec", "--run", "run-1"]);
        if let Some(id) = attempt {
            cmd.args(["--attempt", id]);
        }
        if let Some(id) = invocation {
            cmd.env("INVOCATION_ID", id);
        }
        assert!(
            cmd.args(["--", "true"])
                .status()
                .expect("wrapper")
                .success()
        );
        f.exit().attempt_id
    };
    assert_eq!(run(Some("explicit"), Some("systemd")), "explicit");
    assert_eq!(run(None, Some("systemd")), "systemd");
    let first = run(None, None);
    let second = run(None, None);
    assert_ne!(first, second);
    assert!(uuid::Uuid::parse_str(&first).is_ok());
}

#[path = "runtime_wrapper_cases/background.rs"]
mod background;

#[path = "runtime_wrapper_cases/problems.rs"]
mod problems;
