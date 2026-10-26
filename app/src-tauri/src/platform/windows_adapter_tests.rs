use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[tokio::test]
async fn windows_native_probe_accepts_ipv6_only_loopback() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, 0))
        .await
        .expect("fixture result");
    assert!(loopback_port_open(listener.local_addr().expect("fixture result").port()).await);
}

#[tokio::test]
async fn windows_native_probe_accepts_ipv4_and_rejects_closed_loopback() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("fixture result");
    let port = listener.local_addr().expect("fixture result").port();
    assert!(loopback_port_open(port).await);
    drop(listener);
    assert!(!loopback_port_open(port).await);
}

struct HeldReader(Arc<AtomicBool>);
impl Drop for HeldReader {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct AbortTask<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn windows_native_heartbeat_has_cadence_and_stops_with_direct_child() {
    let (output, mut events) = tokio::sync::mpsc::channel(16);
    let done = CancellationToken::new();
    let token = done.clone();
    let start = tokio::time::Instant::now();
    let mut task = AbortTask(tokio::spawn(async move {
        heartbeat(
            &output,
            &RunIdentity {
                run_id: "quiet".into(),
                attempt_id: "attempt".into(),
            },
            &token,
            Duration::from_millis(20),
        )
        .await
    }));
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("fixture result")
            .expect("fixture value");
        assert_eq!(event.run_id, "quiet");
        assert_eq!(event.attempt_id, "attempt");
        assert!(matches!(event.event, WindowsOutput::Heartbeat));
    }
    assert!(start.elapsed() >= Duration::from_millis(40));
    done.cancel();
    tokio::time::timeout(Duration::from_secs(1), &mut task.0)
        .await
        .expect("fixture result")
        .expect("fixture result")
        .expect("fixture result");
    assert!(events.recv().await.is_none());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn windows_native_quiet_fixture_heartbeat_allows_cancelling_owned_process() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().expect("fixture result");
    let executable = fixture.path().join("quiet.py");
    std::fs::write(
        &executable,
        b"#!/usr/bin/python3\nimport os,time\nfrom pathlib import Path\nPath(__file__).with_suffix('.pid').write_text(str(os.getpid()))\ntime.sleep(60)\n",
    ).expect("fixture result");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("fixture result");
    let pid_file = executable.with_extension("pid");
    let (output, mut events) = tokio::sync::mpsc::channel(16);
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let mut task = AbortTask(tokio::spawn(async move {
        run_with_executable(
            RunWindows {
                run_id: "quiet".into(),
                attempt_id: "attempt".into(),
                cwd: "C:\\fixture".into(),
                command: "fixture".into(),
                env: Default::default(),
                ports: vec![18432],
                observation: devbox_protocol::windows::ObservationControl {
                    schema: 1,
                    launch_id: "owned-launch".into(),
                    nonce: "0123456789abcdef0123456789abcdef".into(),
                    job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
                    assigned_path: "C:\\owned\\control\\assigned.json".into(),
                    grant_path: "C:\\owned\\control\\grant.json".into(),
                    closed_path: "C:\\owned\\control\\closed.json".into(),
                },
            },
            output,
            token,
            executable.as_os_str(),
        )
        .await;
    }));
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("quiet process must send a heartbeat")
        .expect("fixture value");
    assert!(matches!(event.event, WindowsOutput::Heartbeat));
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("fixture result")
        .parse()
        .expect("fixture result");
    let process = std::path::PathBuf::from(format!("/proc/{pid}"));
    assert!(process.exists());
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(3), &mut task.0)
        .await
        .expect("cancel must stop the direct fixture process")
        .expect("fixture result");
    assert!(!process.exists(), "owned direct child must be reaped");
    while let Some(event) = events.recv().await {
        assert!(matches!(event.event, WindowsOutput::Heartbeat));
    }
}

#[tokio::test]
async fn windows_native_exit_code_survives_descendant_pipe_drain_deadline() {
    let dropped = Arc::new(AtomicBool::new(false));
    let reader = HeldReader(dropped.clone());
    let start = tokio::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        child_and_output(
            async { Ok(42) },
            async {
                let _reader = reader;
                std::future::pending::<Result<(), ()>>().await
            },
            Duration::from_millis(20),
        ),
    )
    .await
    .expect("direct child completion must not wait forever for descendants");
    assert_eq!(result, Ok(42));
    assert!(start.elapsed() >= Duration::from_millis(20));
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn windows_native_output_eof_still_waits_for_direct_child_status() {
    let result = child_and_output(
        async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok(17)
        },
        async { Ok(()) },
        Duration::from_millis(5),
    )
    .await;
    assert_eq!(result, Ok(17));
}

#[tokio::test]
async fn windows_native_output_error_fails_without_waiting_for_child_exit() {
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        child_and_output(
            std::future::pending::<Result<i32, ()>>(),
            async { Err(()) },
            Duration::from_millis(20),
        ),
    )
    .await
    .expect("read failure must be returned immediately");
    assert_eq!(result, Err(()));
}

#[tokio::test]
async fn windows_native_stdin_timeout_fails_and_closes_owned_pipe() {
    let (writer, mut reader) = tokio::io::duplex(1);
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        write_stdin(writer, b"ab", Duration::from_millis(20)),
    )
    .await
    .expect("unresponsive stdin must time out");
    assert_eq!(result, Err(()));
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), reader.read_to_end(&mut bytes))
        .await
        .expect("timed-out writer must be dropped")
        .expect("fixture result");
    assert_eq!(bytes, b"a");
}

#[tokio::test]
async fn windows_native_readiness_checks_every_port_at_cadence_and_emits_once() {
    let count = Arc::new(AtomicUsize::new(0));
    let visits = count.clone();
    let (output, mut events) = tokio::sync::mpsc::channel(16);
    let identity = RunIdentity {
        run_id: "r".into(),
        attempt_id: "a".into(),
    };
    let start = tokio::time::Instant::now();
    readiness(
        &[18432, 18433],
        &output,
        &identity,
        &CancellationToken::new(),
        Duration::from_millis(20),
        move |port| {
            let index = visits.fetch_add(1, Ordering::SeqCst);
            async move {
                assert_eq!(
                    port,
                    if index.is_multiple_of(2) {
                        18432
                    } else {
                        18433
                    }
                );
                index != 1
            }
        },
    )
    .await
    .expect("fixture result");
    assert!(start.elapsed() >= Duration::from_millis(20));
    assert_eq!(count.load(Ordering::SeqCst), 4);
    let ready = events.try_recv().expect("fixture result");
    assert_eq!(ready.run_id, "r");
    assert_eq!(ready.attempt_id, "a");
    assert!(matches!(ready.event, WindowsOutput::Ready));
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn windows_native_readiness_stops_when_process_exits_without_emitting_ready() {
    let (output, mut events) = tokio::sync::mpsc::channel(16);
    let identity = RunIdentity {
        run_id: "r".into(),
        attempt_id: "a".into(),
    };
    let done = CancellationToken::new();
    done.cancel();
    readiness(
        &[18432],
        &output,
        &identity,
        &done,
        Duration::from_millis(20),
        |_| async { panic!("exited process must not probe") },
    )
    .await
    .expect("fixture result");
    assert!(events.try_recv().is_err());
}
