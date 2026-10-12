mod support;
use std::process::Stdio;
use support::{TestDaemon, bounded};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(unsafe_code)]
fn denied_stdio(d: &TestDaemon) -> tokio::process::Command {
    let mut command = tokio::process::Command::from(d.environment(env!("CARGO_BIN_EXE_devbox")));
    command
        .arg("bridge")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    // SAFETY: only this fixture's inherited pipe inodes are changed before exec;
    // fchmod is async-signal-safe. Existing descriptors keep their access rights.
    unsafe {
        command.pre_exec(|| {
            for fd in [0, 1] {
                if libc::fchmod(fd, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    command
}

#[tokio::test]
async fn bridge_permission_denied_stdio_reopen_keeps_magic_and_binary_duplex() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("fixture socket");
    let mut child = denied_stdio(&d).spawn().expect("spawn bridge");
    let mut output = child.stdout.take().expect("stdout");
    let mut magic = [0; 16];
    bounded(output.read_exact(&mut magic))
        .await
        .expect("bridge must handle inherited pipes without reopening permission");
    assert_eq!(&magic, devbox_cli::bridge::MAGIC);
    let (mut peer, _) = bounded(listener.accept()).await.expect("daemon connection");
    let mut input = child.stdin.take().expect("stdin");
    let payload: Vec<u8> = (0..512 * 1024).map(|i| (i % 251) as u8).collect();
    let echo = async {
        let (mut read, mut write) = peer.split();
        tokio::io::copy(&mut read, &mut write).await.expect("echo");
    };
    let transfer = async {
        let send = async {
            input.write_all(&payload).await.expect("send bytes");
        };
        let receive = async {
            let mut bytes = vec![0; payload.len()];
            output.read_exact(&mut bytes).await.expect("receive bytes");
            assert_eq!(bytes, payload);
        };
        bounded(async {
            tokio::join!(send, receive);
        })
        .await;
    };
    tokio::select! { _ = echo => panic!("unexpected daemon EOF"), _ = transfer => {} }
    // Daemon EOF must interrupt the still-open, idle inherited stdin pipe.
    drop(peer);
    assert!(bounded(child.wait()).await.expect("bridge exit").success());
    drop(input);
}

#[tokio::test]
async fn bridge_permission_denied_stdout_backpressure_is_cancelled_by_stdin_eof() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("fixture socket");
    let mut child = denied_stdio(&d).spawn().expect("spawn bridge");
    let mut magic = [0; 16];
    bounded(
        child
            .stdout
            .as_mut()
            .expect("stdout")
            .read_exact(&mut magic),
    )
    .await
    .expect("magic");
    let (mut peer, _) = bounded(listener.accept()).await.expect("daemon connection");
    let payload = vec![0; 8 * 1024 * 1024];
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            peer.write_all(&payload)
        )
        .await
        .is_err()
    );
    drop(child.stdin.take());
    assert!(
        bounded(child.wait())
            .await
            .expect("bridge exits despite full stdout")
            .success()
    );
}

#[tokio::test]
#[allow(unsafe_code)]
async fn denied_reopen_fallback_preserves_parent_pipe_flags_after_sigkill() {
    use std::os::fd::{AsRawFd, FromRawFd};
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).expect("fixture socket");
    let mut input = [0; 2];
    let mut output = [0; 2];
    // SAFETY: both arrays receive fresh, owned descriptors.
    assert_eq!(
        unsafe { libc::pipe2(input.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    assert_eq!(
        unsafe { libc::pipe2(output.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    // SAFETY: transfer each descriptor once to a File.
    let input_read = unsafe { std::fs::File::from_raw_fd(input[0]) };
    let input_write = unsafe { std::fs::File::from_raw_fd(input[1]) };
    let output_read = unsafe { std::fs::File::from_raw_fd(output[0]) };
    let output_write = unsafe { std::fs::File::from_raw_fd(output[1]) };
    // The parent read end is a separate open-file description from child stdout.
    // SAFETY: only the fixture-owned read end is made nonblocking for this test.
    assert_eq!(
        unsafe { libc::fcntl(output_read.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) },
        0
    );
    let read = tokio::io::unix::AsyncFd::new(output_read).expect("parent output readiness");
    // SAFETY: live parent-owned descriptors; flags are never modified by the test.
    let before = [
        unsafe { libc::fcntl(input_read.as_raw_fd(), libc::F_GETFL) },
        unsafe { libc::fcntl(output_write.as_raw_fd(), libc::F_GETFL) },
    ];
    let mut child = denied_stdio(&d)
        .stdin(Stdio::from(input_read.try_clone().expect("child stdin")))
        .stdout(Stdio::from(output_write.try_clone().expect("child stdout")))
        .spawn()
        .expect("spawn bridge");
    let mut magic = Vec::new();
    bounded(async {
        while magic.len() < 16 {
            let mut ready = read.readable().await.expect("parent read readiness");
            let mut bytes = [0; 16];
            match ready.try_io(|fd| std::io::Read::read(&mut fd.get_ref(), &mut bytes)) {
                Ok(result) => {
                    let n = result.expect("read magic");
                    assert_ne!(n, 0);
                    magic.extend_from_slice(&bytes[..n]);
                }
                Err(_) => continue,
            }
        }
    })
    .await;
    assert_eq!(magic, devbox_cli::bridge::MAGIC);
    let (_peer, _) = bounded(listener.accept()).await.expect("daemon connection");
    // SAFETY: compare the inherited open-file descriptions while child runs and after SIGKILL.
    assert_eq!(
        unsafe { libc::fcntl(input_read.as_raw_fd(), libc::F_GETFL) },
        before[0]
    );
    assert_eq!(
        unsafe { libc::fcntl(output_write.as_raw_fd(), libc::F_GETFL) },
        before[1]
    );
    child.kill().await.expect("SIGKILL fixture");
    bounded(child.wait()).await.expect("reap bridge");
    assert_eq!(
        unsafe { libc::fcntl(input_read.as_raw_fd(), libc::F_GETFL) },
        before[0]
    );
    assert_eq!(
        unsafe { libc::fcntl(output_write.as_raw_fd(), libc::F_GETFL) },
        before[1]
    );
    drop(input_write);
}
