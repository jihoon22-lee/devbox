use crate::support::{ChildGuard, TestDaemon, bounded, exits};
use devbox_protocol::system::{Ping, PingParams};
use std::os::unix::net::UnixListener;

#[tokio::test]
async fn startup_preserves_foreign_file_live_socket_and_database_before_migration() {
    for socket in [false, true] {
        let mut d = TestDaemon::fixture();
        let foreign = if socket {
            Some(UnixListener::bind(&d.socket).expect("bind fixture socket"))
        } else {
            std::fs::write(&d.socket, b"foreign").expect("write fixture file");
            None
        };
        let status = exits(ChildGuard(
            d.command().spawn().expect("spawn fixture process"),
        ));
        assert!(!status.success());
        assert!(
            !d.paths.db_path.exists(),
            "ownership must be checked before DB open"
        );
        if socket {
            assert!(std::os::unix::net::UnixStream::connect(&d.socket).is_ok());
        } else {
            assert_eq!(
                std::fs::read(&d.socket).expect("read fixture file"),
                b"foreign"
            );
        }
        // Removing only the fixture-owned obstruction allows retry after failure.
        drop(foreign);
        std::fs::remove_file(&d.socket).expect("remove fixture file");
        d.launch();
        assert!(d.client().await.call::<Ping>(&PingParams {}).await.is_ok());
    }
}

#[tokio::test]
async fn same_data_instance_cannot_start_again_with_another_runtime_dir() {
    let d = TestDaemon::start();
    let alternate = d.home.path().join("alternate-runtime");
    std::fs::create_dir(&alternate).expect("create fixture directory");
    let mut command = d.command();
    command.env("XDG_RUNTIME_DIR", &alternate);
    assert!(!exits(ChildGuard(command.spawn().expect("spawn fixture process"))).success());
    assert!(
        !alternate
            .join(format!("devbox-{}.sock", d.instance))
            .exists()
    );
    assert!(d.client().await.call::<Ping>(&PingParams {}).await.is_ok());
}

#[tokio::test]
async fn cli_instance_overrides_environment_without_changing_it() {
    let mut d = TestDaemon::fixture();
    let mut command = d.command();
    command
        .env("DEVBOX_INSTANCE", "wrong")
        .args(["--instance", &d.instance]);
    d.child = Some(ChildGuard(command.spawn().expect("spawn fixture process")));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::os::unix::net::UnixStream::connect(&d.socket).is_err() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(d.client().await.call::<Ping>(&PingParams {}).await.is_ok());
    assert!(
        !d.paths
            .data_dir
            .parent()
            .expect("fixture path has parent")
            .join("wrong")
            .exists()
    );
}

#[tokio::test]
async fn stale_activation_environment_does_not_claim_unrelated_descriptor() {
    let d = TestDaemon::fixture();
    let mut command = d.command();
    command.env("LISTEN_FDS", "1").env("LISTEN_PID", "0");
    assert!(!exits(ChildGuard(command.spawn().expect("spawn fixture process"))).success());
    assert!(!d.socket.exists());
    assert!(!d.paths.db_path.exists());
}

#[allow(unsafe_code)]
fn activation_command(d: &TestDaemon, fd: std::os::fd::RawFd) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut command = d.environment("bash");
    command.args([
        "-c",
        "export LISTEN_PID=$$; exec \"$@\"",
        "devbox-test",
        env!("CARGO_BIN_EXE_devbox"),
        "daemon",
        "--foreground",
    ]);
    command.env("LISTEN_FDS", "1");
    // SAFETY: only dup2/fcntl async-signal-safe syscalls run between fork and exec;
    // fd is owned by the parent fixture and remains open until spawn completes.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

#[tokio::test]
async fn owned_systemd_descriptor_and_abstract_ready_notification_work() {
    use std::os::fd::AsRawFd;
    use std::os::linux::net::SocketAddrExt;
    let mut d = TestDaemon::fixture();
    let listener = UnixListener::bind(&d.socket).expect("bind fixture socket");
    let name = format!("devbox-test-{}", uuid::Uuid::new_v4());
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
        .expect("create abstract fixture address");
    let notify =
        std::os::unix::net::UnixDatagram::bind_addr(&address).expect("bind fixture socket");
    notify
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("inspect fixture notification socket");
    let mut command = activation_command(&d, listener.as_raw_fd());
    command.env("NOTIFY_SOCKET", format!("@{name}"));
    d.child = Some(ChildGuard(command.spawn().expect("spawn fixture process")));
    drop(listener);
    let mut message = [0; 32];
    let n = notify.recv(&mut message).expect("receive fixture message");
    assert_eq!(&message[..n], b"READY=1");
    assert!(
        bounded(d.client())
            .await
            .call::<Ping>(&PingParams {})
            .await
            .is_ok()
    );
    let pid = d.child.as_ref().expect("fixture child exists").0.id();
    let info = std::fs::read_to_string(format!("/proc/{pid}/fdinfo/3")).expect("read fixture text");
    let flags = info
        .lines()
        .find_map(|line| line.strip_prefix("flags:\t"))
        .expect("fixture descriptor has flags");
    assert_ne!(
        u32::from_str_radix(flags, 8).expect("parse fixture flags") & libc::O_CLOEXEC as u32,
        0
    );
}

#[tokio::test]
async fn activation_rejects_regular_file_and_nonlistening_socket_without_db_open() {
    use std::os::fd::AsRawFd;
    let d = TestDaemon::fixture();
    let file =
        std::fs::File::create(d.home.path().join("foreign-fd")).expect("create fixture file");
    assert!(
        !exits(ChildGuard(
            activation_command(&d, file.as_raw_fd())
                .spawn()
                .expect("spawn fixture process")
        ))
        .success()
    );
    assert!(!d.paths.db_path.exists());
    let (stream, _peer) =
        std::os::unix::net::UnixStream::pair().expect("create fixture socket pair");
    assert!(
        !exits(ChildGuard(
            activation_command(&d, stream.as_raw_fd())
                .spawn()
                .expect("spawn fixture process")
        ))
        .success()
    );
    assert!(!d.paths.db_path.exists());
    assert!(file.metadata().is_ok());
}
