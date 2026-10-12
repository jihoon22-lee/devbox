//! Raw stdio transport; protocol bytes are never interpreted here.
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
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
    let unit = format!("devbox@{}.socket", paths.instance.as_str());
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

// Tokio's blocking stdio workers outlive cancelled futures. Readiness keeps
// both pipe directions cancellable, even when stdout is full at stdin EOF.
fn readiness(fd: i32, file: std::fs::File) -> std::io::Result<Option<AsyncFd<std::fs::File>>> {
    let path = format!("/proc/self/fd/{fd}");
    // Regular files and /dev/null cannot be registered with epoll. Their finite
    // IO needs no background stdio worker and retains the inherited offset.
    if file.metadata()?.is_file() || std::fs::read_link(&path)? == std::path::Path::new("/dev/null")
    {
        Ok(None)
    } else {
        // Reopen the pipe with its own open-file description. O_NONBLOCK
        // never changes flags inherited by the parent, even on SIGKILL.
        let pipe = std::fs::OpenOptions::new()
            .read(fd == 0)
            .write(fd == 1)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        AsyncFd::new(pipe).map(Some)
    }
}
async fn write_output(
    output: &Option<AsyncFd<std::fs::File>>,
    mut bytes: &[u8],
) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let n = if let Some(output) = output {
            let mut ready = output.writable().await?;
            match ready.try_io(|fd| fd.get_ref().write(bytes)) {
                Ok(result) => result?,
                Err(_) => continue,
            }
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
                let mut ready = input.readable().await?;
                match ready.try_io(|fd| fd.get_ref().read(&mut buf)) {
                    Ok(result) => result?,
                    Err(_) => continue,
                }
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
