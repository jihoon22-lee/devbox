use crate::{AttachSession, TerminalError, tmux::Tmux};
use devbox_protocol::{Fail, stream::StreamEnds};
use portable_pty::{MasterPty, PtySize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct Attachment {
    master: Box<dyn MasterPty + Send>,
    read_only: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Attachments(Arc<Mutex<HashMap<u32, Attachment>>>);
fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}
pub(crate) fn pty_error(error: impl std::fmt::Display) -> Fail<TerminalError> {
    Fail::Internal(format!("terminal PTY: {error}"))
}
pub(crate) fn validate(cols: u16, rows: u16) -> Result<(), TerminalError> {
    if cols == 0 || rows == 0 {
        Err(TerminalError::InvalidSize { cols, rows })
    } else {
        Ok(())
    }
}
impl Attachments {
    pub fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), Fail<TerminalError>> {
        let map = self.0.lock().expect("attachments");
        let attachment = map
            .get(&id)
            .ok_or_else(|| TerminalError::NotFound { id: id.to_string() })?;
        if !attachment.read_only {
            attachment
                .master
                .resize(size(cols, rows))
                .map_err(pty_error)?;
        }
        Ok(())
    }
}
pub(crate) async fn dimensions(
    tmux: &Tmux,
    p: &AttachSession,
) -> Result<PtySize, Fail<TerminalError>> {
    let dimensions = if p.read_only {
        let value = tmux
            .run(&[
                "list-panes",
                "-t",
                &format!("={}", p.id),
                "-F",
                "#{pane_width} #{pane_height}",
            ])
            .await
            .map_err(pty_error)?;
        let mut fields = value.split_whitespace();
        let cols = fields
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| pty_error("missing pane width"))?;
        let rows = fields
            .next()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| pty_error("missing pane height"))?;
        validate(cols, rows)?;
        size(cols, rows)
    } else {
        size(p.cols, p.rows)
    };
    Ok(dimensions)
}
#[cfg(not(unix))]
pub(crate) fn start(
    _: &Tmux,
    _: &Attachments,
    _: AttachSession,
    _: Vec<u8>,
    _: StreamEnds,
    _: PtySize,
) -> Result<(), Fail<TerminalError>> {
    Err(pty_error("terminal attach requires the WSL daemon"))
}
#[cfg(unix)]
pub(crate) fn start(
    tmux: &Tmux,
    attachments: &Attachments,
    p: AttachSession,
    history: Vec<u8>,
    mut ends: StreamEnds,
    dimensions: PtySize,
) -> Result<(), Fail<TerminalError>> {
    use portable_pty::{CommandBuilder, native_pty_system};
    use std::os::fd::AsRawFd;
    use tokio::io::unix::AsyncFd;
    let pair = native_pty_system().openpty(dimensions).map_err(pty_error)?;
    let raw = pair
        .master
        .as_raw_fd()
        .ok_or_else(|| pty_error("PTY has no descriptor"))?;
    struct Raw(i32);
    impl AsRawFd for Raw {
        fn as_raw_fd(&self) -> i32 {
            self.0
        }
    }
    // Duplication sets CLOEXEC; nonblocking flags affect only this owned PTY.
    let mut file = filedescriptor::FileDescriptor::dup(&Raw(raw)).map_err(pty_error)?;
    file.set_non_blocking(true).map_err(pty_error)?;
    struct Descriptor(Mutex<filedescriptor::FileDescriptor>, i32);
    impl AsRawFd for Descriptor {
        fn as_raw_fd(&self) -> i32 {
            self.1
        }
    }
    impl Descriptor {
        fn read_bytes(&self, bytes: &mut [u8]) -> std::io::Result<usize> {
            use std::io::Read;
            self.0.lock().expect("PTY reader").read(bytes)
        }
        fn write_bytes(&self, bytes: &[u8]) -> std::io::Result<usize> {
            use std::io::Write;
            self.0.lock().expect("PTY writer").write(bytes)
        }
    }
    let raw = file.as_raw_fd();
    let io = AsyncFd::new(Descriptor(Mutex::new(file), raw)).map_err(pty_error)?;
    let mut command = CommandBuilder::new("tmux");
    command.args(["-L", tmux.socket(), "-f"]);
    command.arg(tmux.conf());
    command.arg("attach");
    if p.read_only {
        command.args(["-f", "read-only,ignore-size"]);
    }
    command.args(["-t", &format!("={}", p.id)]);
    command.env("TERM", "xterm-256color");
    command.env_remove("TMUX");
    command.env_remove("TMUX_TMPDIR");
    let child = pair.slave.spawn_command(command).map_err(pty_error)?;
    drop(pair.slave);
    attachments.0.lock().expect("attachments").insert(
        ends.id,
        Attachment {
            master: pair.master,
            read_only: p.read_only,
        },
    );
    let guard = Guard {
        attachments: attachments.clone(),
        id: ends.id,
        child: Some(child),
    };
    tokio::spawn(async move {
        let mut guard = guard;
        let closed = ends.tx.closed();
        let output = async {
            if ends.tx.write(&history).await.is_err() {
                return Ok::<(), Fail<TerminalError>>(());
            }
            let mut buffer = [0u8; 32 * 1024];
            loop {
                let mut ready = io.readable().await.map_err(pty_error)?;
                let n = match ready.try_io(|fd| fd.get_ref().read_bytes(&mut buffer)) {
                    Ok(Err(error)) if error.raw_os_error() == Some(libc::EIO) => return Ok(()),
                    Ok(result) => result.map_err(pty_error)?,
                    Err(_) => continue,
                };
                if n == 0 {
                    return Ok::<(), Fail<TerminalError>>(());
                }
                if ends.tx.write(&buffer[..n]).await.is_err() {
                    return Ok(());
                }
            }
        };
        let input = async {
            while let Some(bytes) = ends.input.recv().await {
                if p.read_only {
                    continue;
                }
                let mut bytes = bytes.as_ref();
                while !bytes.is_empty() {
                    let mut ready = io.writable().await.map_err(pty_error)?;
                    let n = match ready.try_io(|fd| fd.get_ref().write_bytes(bytes)) {
                        Ok(Err(error)) if error.raw_os_error() == Some(libc::EIO) => return Ok(()),
                        Ok(result) => result.map_err(pty_error)?,
                        Err(_) => continue,
                    };
                    if n == 0 {
                        return Err(pty_error("PTY write returned zero"));
                    }
                    bytes = &bytes[n..];
                }
            }
            // Input FIN is a half-close: retain output until explicit close or exit.
            std::future::pending::<Result<(), Fail<TerminalError>>>().await
        };
        let outcome = tokio::select! {
            _ = closed.cancelled() => None,
            result = output => Some(result),
            result = input => Some(result),
            _ = async {
                loop {
                    if let Some(child) = &mut guard.child
                        && matches!(child.try_wait(), Ok(Some(_))) { guard.child = None; break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
                // Reaping the client must not discard its buffered final output.
                std::future::pending::<()>().await;
            } => None,
        };
        if let Some(Err(Fail::Internal(reason))) = outcome {
            tracing::warn!(stream = ends.id, reason, "terminal attachment ended");
        }
        guard.reap().await;
        ends.tx.finish().await;
    });
    Ok(())
}
#[cfg(unix)]
struct Guard {
    attachments: Attachments,
    id: u32,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
}
#[cfg(unix)]
impl Guard {
    async fn reap(&mut self) {
        self.attachments
            .0
            .lock()
            .expect("attachments")
            .remove(&self.id);
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            // Reap without blocking the async executor, even during shutdown.
            for _ in 0..100 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    self.child = None;
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }
}
#[cfg(unix)]
impl Drop for Guard {
    fn drop(&mut self) {
        self.attachments
            .0
            .lock()
            .expect("attachments")
            .remove(&self.id);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Abort/panic/runtime-drop fallback owns only our attach client.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}
