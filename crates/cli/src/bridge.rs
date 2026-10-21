//! Raw stdio transport; protocol bytes are never interpreted here.
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, unix::AsyncFd};
use tokio::net::UnixStream;
use tokio::time::{Instant, timeout_at};

pub const MAGIC: &[u8; 16] = b"DEVBOX-BRIDGE/1\n";

async fn connect(paths: &devbox_core::Paths) -> anyhow::Result<UnixStream> {
    match UnixStream::connect(&paths.socket).await {
        Ok(socket) => return Ok(socket),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let unit = format!(
        "{}@{}.socket",
        paths.instance.channel().config().daemon_unit,
        paths.instance.as_str()
    );
    let mut child = tokio::process::Command::new("systemctl")
        .args(["--user", "start", &unit])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let status = match timeout_at(deadline, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.kill().await?;
            anyhow::bail!("socket activation timed out");
        }
    };
    anyhow::ensure!(status.success(), "socket activation failed: {status}");
    loop {
        match timeout_at(deadline, UnixStream::connect(&paths.socket)).await {
            Ok(Ok(socket)) => return Ok(socket),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow::bail!("daemon connection timed out"),
        }
        timeout_at(deadline, tokio::time::sleep(Duration::from_millis(100)))
            .await
            .map_err(|_| anyhow::anyhow!("daemon connection timed out"))?;
    }
}

// Tokio's blocking stdio workers outlive cancelled futures. All pollable IO
// below is nonblocking without changing an inherited open-file description.
struct ReadyStdio {
    fd: AsyncFd<std::fs::File>,
    staging: Option<StagingPipe>,
}
struct StagingPipe {
    read: std::fs::File,
    write: std::fs::File,
}
impl StagingPipe {
    #[allow(unsafe_code)]
    fn new() -> std::io::Result<Self> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 initializes two owned descriptors on success.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: transfer each freshly created descriptor exactly once.
        Ok(unsafe {
            Self {
                read: std::fs::File::from_raw_fd(fds[0]),
                write: std::fs::File::from_raw_fd(fds[1]),
            }
        })
    }
}
#[allow(unsafe_code)]
fn splice_pipe(from: &std::fs::File, to: &std::fs::File, len: usize) -> std::io::Result<usize> {
    loop {
        // SAFETY: both live descriptors are pipes; null offsets are required
        // for pipes. SPLICE_F_NONBLOCK applies to both pipe operations even
        // when the inherited descriptor itself is blocking.
        let n = unsafe {
            libc::splice(
                from.as_raw_fd(),
                std::ptr::null_mut(),
                to.as_raw_fd(),
                std::ptr::null_mut(),
                len,
                libc::SPLICE_F_NONBLOCK,
            )
        };
        if n >= 0 {
            return Ok(n as usize);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}
fn readiness(fd: i32, file: std::fs::File) -> std::io::Result<Option<ReadyStdio>> {
    let path = format!("/proc/self/fd/{fd}");
    let metadata = file.metadata()?;
    // Finite regular-file/null IO retains the inherited offset.
    if metadata.is_file() || std::fs::read_link(&path)? == std::path::Path::new("/dev/null") {
        return Ok(None);
    }
    match std::fs::OpenOptions::new()
        .read(fd == 0)
        .write(fd == 1)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(pipe) => Ok(Some(ReadyStdio {
            fd: AsyncFd::new(pipe)?,
            staging: None,
        })),
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                && metadata.file_type().is_fifo() =>
        {
            // Windows wsl.exe supplies root-owned 0600 pipes. Their inherited
            // handles are usable, but reopening /proc/self/fd fails with EACCES.
            // Splice via an owned pipe gives per-call nonblocking IO; no chmod,
            // privilege change, or F_SETFL on the inherited handle is needed.
            Ok(Some(ReadyStdio {
                fd: AsyncFd::new(file)?,
                staging: Some(StagingPipe::new()?),
            }))
        }
        Err(error) => Err(error),
    }
}
impl ReadyStdio {
    async fn read(&self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let mut ready = self.fd.readable().await?;
            match ready.try_io(|fd| {
                if let Some(staging) = &self.staging {
                    let n = splice_pipe(fd.get_ref(), &staging.write, bytes.len())?;
                    // Only this operation consumes staging; exactly n bytes
                    // are now present and its descriptor is nonblocking too.
                    (&staging.read).read_exact(&mut bytes[..n])?;
                    Ok(n)
                } else {
                    fd.get_ref().read(bytes)
                }
            }) {
                Ok(result) => return result,
                Err(_) => continue,
            }
        }
    }
    async fn write(&self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(staging) = &self.staging {
            // A successful call always drains staging before returning; a
            // cancelled call is followed by bridge shutdown and owned-fd drop.
            let count = (&staging.write).write(bytes)?;
            let mut left = count;
            while left > 0 {
                let mut ready = self.fd.writable().await?;
                match ready.try_io(|fd| splice_pipe(&staging.read, fd.get_ref(), left)) {
                    Ok(result) => {
                        let n = result?;
                        if n == 0 {
                            return Err(std::io::ErrorKind::WriteZero.into());
                        }
                        left -= n;
                    }
                    Err(_) => continue,
                }
            }
            Ok(count)
        } else {
            loop {
                let mut ready = self.fd.writable().await?;
                match ready.try_io(|fd| fd.get_ref().write(bytes)) {
                    Ok(result) => return result,
                    Err(_) => continue,
                }
            }
        }
    }
}
async fn write_output(output: &Option<ReadyStdio>, mut bytes: &[u8]) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let n = if let Some(output) = output {
            output.write(bytes).await?
        } else {
            std::io::stdout().lock().write(bytes)?
        };
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        bytes = &bytes[n..];
    }
    Ok(())
}
pub async fn run() -> anyhow::Result<()> {
    let output = readiness(
        1,
        std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?),
    )?;
    write_output(&output, MAGIC).await?;
    // Pipe writes above are unbuffered; flush the regular-file fallback too.
    if output.is_none() {
        std::io::stdout().lock().flush()?;
    }
    let paths = crate::paths()?;
    let socket = connect(&paths).await.map_err(|e| {
        anyhow::anyhow!(
            "devbox daemon unreachable at {}: {e}",
            paths.socket.display()
        )
    })?;
    let (mut reader, mut writer) = socket.into_split();
    let input = readiness(
        0,
        std::fs::File::from(std::io::stdin().as_fd().try_clone_to_owned()?),
    )?;
    let up = async {
        let mut buf = [0; 64 * 1024];
        loop {
            let n = if let Some(input) = &input {
                input.read(&mut buf).await?
            } else {
                std::io::stdin().lock().read(&mut buf)?
            };
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n]).await?;
        }
        writer.shutdown().await
    };
    let down = async {
        let mut buf = [0; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            write_output(&output, &buf[..n]).await?;
        }
        Ok::<(), std::io::Error>(())
    };
    tokio::select! {result=up=>result?,result=down=>result?}
    Ok(())
}
