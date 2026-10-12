//! Instance ownership is acquired before opening or migrating its database.
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::fd::FromRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use tokio::net::UnixListener;

#[derive(Deserialize, Serialize)]
struct Receipt {
    owner: String,
    socket: Option<(u64, u64)>,
}

pub struct Socket {
    pub listener: UnixListener,
    // Never unlink the lock: another process may already have opened its inode.
    _lock: File,
}

fn invalid(reason: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

impl Socket {
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| invalid("socket has no parent"))?;
        std::fs::create_dir_all(parent)?;
        let lock_path = path.with_extension("lock");
        let mut opts = OpenOptions::new();
        opts.read(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let (mut lock, new) = match opts.create_new(true).open(&lock_path) {
            Ok(file) => (file, true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                (opts.create_new(false).open(&lock_path)?, false)
            }
            Err(e) => return Err(e.into()),
        };
        if !lock.metadata()?.is_file() {
            return Err(invalid("instance lock is not a regular file").into());
        }
        lock.try_lock()
            .map_err(|e| anyhow::anyhow!("instance already running: {e}"))?;
        let mut receipt = Receipt {
            owner: "devbox-daemon-v1".into(),
            socket: None,
        };
        if !new {
            let mut text = String::new();
            lock.read_to_string(&mut text)?;
            receipt = serde_json::from_str(&text).map_err(|_| invalid("foreign instance lock"))?;
            if receipt.owner != "devbox-daemon-v1" {
                return Err(invalid("foreign instance lock").into());
            }
        }
        if new {
            lock.write_all(serde_json::to_string(&receipt)?.as_bytes())?;
            lock.sync_all()?;
        }
        let listener = if let Some(listener) = activated(path)? {
            listener
        } else {
            match std::fs::symlink_metadata(path) {
                Ok(meta) => {
                    if !meta.file_type().is_socket()
                        || receipt.socket != Some((meta.dev(), meta.ino()))
                    {
                        return Err(invalid("socket path is not owned by this daemon").into());
                    }
                    match std::os::unix::net::UnixStream::connect(path) {
                        Ok(_) => return Err(invalid("socket is already listening").into()),
                        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {}
                        Err(e) => return Err(e.into()),
                    }
                    let current = std::fs::symlink_metadata(path)?;
                    if (current.dev(), current.ino()) != (meta.dev(), meta.ino()) {
                        return Err(invalid("socket changed during ownership check").into());
                    }
                    std::fs::remove_file(path)?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            let listener = UnixListener::bind(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            let meta = std::fs::symlink_metadata(path)?;
            receipt.socket = Some((meta.dev(), meta.ino()));
            listener
        };
        lock.rewind()?;
        lock.set_len(0)?;
        lock.write_all(serde_json::to_string(&receipt)?.as_bytes())?;
        lock.sync_all()?;
        Ok(Self {
            listener,
            _lock: lock,
        })
    }
}

#[allow(unsafe_code)]
fn activated(path: &Path) -> io::Result<Option<UnixListener>> {
    let Some(fds) = std::env::var_os("LISTEN_FDS") else {
        return Ok(None);
    };
    if fds != "1"
        || std::env::var("LISTEN_PID").ok().as_deref() != Some(&std::process::id().to_string())
    {
        return Err(invalid(
            "socket activation requires LISTEN_FDS=1 and current LISTEN_PID",
        ));
    }
    for (option, expected) in [(libc::SO_TYPE, libc::SOCK_STREAM), (libc::SO_ACCEPTCONN, 1)] {
        let mut value: libc::c_int = 0;
        let mut len = std::mem::size_of_val(&value) as libc::socklen_t;
        // SAFETY: fd is borrowed, and both output pointers point to initialized writable storage.
        if unsafe {
            libc::getsockopt(
                3,
                libc::SOL_SOCKET,
                option,
                (&mut value as *mut libc::c_int).cast(),
                &mut len,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if value != expected {
            return Err(invalid("fd 3 is not a listening stream socket"));
        }
    }
    // SAFETY: sockaddr_storage has no invalid bit patterns, and getsockname
    // writes only within its initialized output buffer and length pointer.
    let mut address: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
    let mut address_len = std::mem::size_of_val(&address) as libc::socklen_t;
    if unsafe {
        libc::getsockname(
            3,
            (&mut address as *mut libc::sockaddr_storage).cast(),
            &mut address_len,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if address.ss_family as libc::c_int != libc::AF_UNIX {
        return Err(invalid("fd 3 is not a Unix socket"));
    }
    // Validate the exact instance path using a duplicated descriptor;
    // fd 3 remains unowned on every validation error.
    // SAFETY: fcntl duplicates a borrowed descriptor without transferring its ownership.
    let duplicate = unsafe { libc::fcntl(3, libc::F_DUPFD_CLOEXEC, 4) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: duplicate is a newly allocated descriptor owned solely by this function.
    let borrowed = unsafe { std::os::unix::net::UnixListener::from_raw_fd(duplicate) };
    if borrowed.local_addr()?.as_pathname() != Some(path) {
        return Err(invalid("activation socket instance mismatch"));
    }
    drop(borrowed);
    // SAFETY: fd 3 remains a validated borrowed descriptor; preserve flags while
    // preventing future subprocesses from inheriting the daemon listener.
    let flags = unsafe { libc::fcntl(3, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(3, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: current LISTEN_PID and socket type/listening/path validation establish
    // that systemd supplied fd 3 for this process and no Rust owner exists yet.
    let listener = unsafe { std::os::unix::net::UnixListener::from_raw_fd(3) };
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener).map(Some)
}

pub fn notify_ready() -> io::Result<()> {
    let Some(addr) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::ffi::OsStrExt;
    let bytes = addr.as_bytes();
    let address = if let Some(name) = bytes.strip_prefix(b"@") {
        std::os::unix::net::SocketAddr::from_abstract_name(name)?
    } else {
        std::os::unix::net::SocketAddr::from_pathname(addr)?
    };
    std::os::unix::net::UnixDatagram::unbound()?.send_to_addr(b"READY=1", &address)?;
    Ok(())
}

/// Serializes the DB instance even when callers choose different runtime dirs.
pub(super) fn database_lease(data_dir: &Path) -> anyhow::Result<File> {
    std::fs::create_dir_all(data_dir)?;
    let path = data_dir.join(".daemon.lock");
    let mut opts = OpenOptions::new();
    opts.read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let (mut file, new) = match opts.create_new(true).open(&path) {
        Ok(file) => (file, true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            (opts.create_new(false).open(&path)?, false)
        }
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(invalid("database lease is not a file").into());
    }
    file.try_lock()
        .map_err(|e| anyhow::anyhow!("database instance already running: {e}"))?;
    if new {
        file.write_all(b"devbox-daemon-v1")?;
        file.sync_all()?;
    } else {
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        if text != "devbox-daemon-v1" {
            return Err(invalid("foreign database lease").into());
        }
    }
    Ok(file)
}
