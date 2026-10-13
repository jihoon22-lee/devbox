use crate::{AttachSession, TerminalError, tmux::Tmux};
use devbox_protocol::{Fail, stream::StreamEnds};
use portable_pty::{MasterPty, PtySize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct Attachment {
    master: Box<dyn MasterPty + Send>,
    read_only: bool,
    session: String,
    tmux: Tmux,
    pid: u32,
    tx: devbox_protocol::stream::StreamTx,
    cleanup_under_lease: tokio::sync::watch::Sender<bool>,
}
#[derive(Clone, Default)]
pub(crate) struct Attachments {
    clients: Arc<Mutex<HashMap<u32, Attachment>>>,
    gates: crate::attach_gate::Gates,
}
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
    pub async fn resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), Fail<TerminalError>> {
        let missing = move || TerminalError::NotFound { id: id.to_string() };
        let (session, tmux) = {
            let clients = self.clients.lock().expect("attachments");
            let attachment = clients.get(&id).ok_or_else(missing)?;
            (attachment.session.clone(), attachment.tmux.clone())
        };
        let lease = self.gates.enter(&session, &tmux).await?;
        let (tmux, pid, closed, cleanup_under_lease) = {
            let map = self.clients.lock().expect("attachments");
            let attachment = map.get(&id).ok_or_else(missing)?;
            if attachment.read_only {
                return Ok(());
            }
            attachment
                .master
                .resize(size(cols, rows))
                .map_err(pty_error)?;
            (
                attachment.tmux.clone(),
                attachment.pid,
                attachment.tx.closed(),
                attachment.cleanup_under_lease.clone(),
            )
        };
        // Once ioctl is issued, request cancellation must not release the gate
        // before tmux has processed SIGWINCH and applied the new pane grid.
        tokio::spawn(async move {
            lease.record(pid);
            let result = tokio::select! {
                _ = closed.cancelled() => Err(missing().into()),
                result = resize_ack(&tmux, pid, cols, rows) => result,
            };
            if result.is_err() {
                // Stop the owned client on failed ACK, and retain its session
                // gate until tmux removed the peer and all queued resize input.
                cleanup_under_lease.send_replace(true);
                closed.cancel();
                if let Err(Fail::Internal(reason)) = lease.cleanup(&tmux).await {
                    tracing::warn!(stream = id, reason, "terminal resize cleanup unresolved");
                }
            }
            result
        })
        .await
        .map_err(pty_error)?
    }
}
async fn resize_ack(
    tmux: &Tmux,
    pid: u32,
    cols: u16,
    rows: u16,
) -> Result<(), Fail<TerminalError>> {
    let expected = format!("{pid} {cols} {rows}");
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let clients = tmux
                .run(&[
                    "list-clients",
                    "-F",
                    "#{client_pid} #{client_width} #{client_height}",
                ])
                .await
                .map_err(pty_error)?;
            if clients.lines().any(|line| line == expected) {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(pty_error)?
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
pub(crate) async fn start(
    _: &Tmux,
    _: &Attachments,
    _: AttachSession,
    _: StreamEnds,
) -> Result<PtySize, Fail<TerminalError>> {
    Err(pty_error("terminal attach requires the WSL daemon"))
}
#[cfg(unix)]
pub(crate) async fn start(
    tmux: &Tmux,
    attachments: &Attachments,
    p: AttachSession,
    ends: StreamEnds,
) -> Result<PtySize, Fail<TerminalError>> {
    struct Startup {
        tx: devbox_protocol::stream::StreamTx,
        pending: bool,
    }
    impl Drop for Startup {
        fn drop(&mut self) {
            if self.pending {
                self.tx.closed().cancel();
            }
        }
    }
    // The request may be cancelled while preparing its grid, after open_stream
    // registered the stream but before the long-lived attachment owns it.
    let mut startup = Startup {
        tx: ends.tx.clone(),
        pending: true,
    };
    let tmux = tmux.clone();
    let attachments = attachments.clone();
    let result = tokio::spawn(async move { start_owned(tmux, attachments, p, ends).await })
        .await
        .map_err(pty_error)?;
    startup.pending = false;
    result
}
#[cfg(unix)]
async fn start_owned(
    tmux: Tmux,
    attachments: Attachments,
    p: AttachSession,
    ends: StreamEnds,
) -> Result<PtySize, Fail<TerminalError>> {
    let startup_closed = ends.tx.closed();
    let initialization = tokio::select! {
        biased;
        _ = startup_closed.cancelled() => return Err(TerminalError::StreamUnavailable {}.into()),
        result = attachments.gates.enter(&p.id,&tmux) => result?,
    };
    let result = initialize(&tmux, &attachments, p, ends, &initialization).await;
    if result.is_err()
        && let Err(Fail::Internal(reason)) = initialization.cleanup(&tmux).await
    {
        tracing::warn!(reason, "terminal startup cleanup unresolved");
    }
    result
}
#[cfg(unix)]
async fn initialize(
    tmux: &Tmux,
    attachments: &Attachments,
    p: AttachSession,
    mut ends: StreamEnds,
    initialization: &crate::attach_gate::Lease,
) -> Result<PtySize, Fail<TerminalError>> {
    use portable_pty::{CommandBuilder, native_pty_system};
    use std::os::fd::AsRawFd;
    use tokio::io::unix::AsyncFd;
    let startup_closed = ends.tx.closed();
    let dimensions = dimensions(tmux, &p).await?;
    let resize_helper =
        crate::attach_history::prepare_grid(tmux, &p, &ends.tx, |pid| initialization.record(pid))
            .await?;
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
    let history = crate::attach_history::History::new(tmux);
    history.queue(&mut command, &p);
    command.env("TERM", "xterm-256color");
    command.env_remove("TMUX");
    command.env_remove("TMUX_TMPDIR");
    let child = pair.slave.spawn_command(command).map_err(pty_error)?;
    let pid = child
        .process_id()
        .ok_or_else(|| pty_error("attach client has no pid"))?;
    initialization.record(pid);
    drop(pair.slave);
    let (cleanup_under_lease, _) = tokio::sync::watch::channel(false);
    let mut guard = Guard {
        attachments: attachments.clone(),
        id: ends.id,
        child: Some(child),
        resize_helper,
        history: Some(history),
        session: p.id.clone(),
        tmux: tmux.clone(),
        pid,
        cleanup_under_lease: cleanup_under_lease.clone(),
    };
    let bytes = tokio::select! {
        biased;
        _ = startup_closed.cancelled() => Err(TerminalError::StreamUnavailable {}.into()),
        result = guard.history.as_ref().expect("history boundary").read(pid) => result,
    };
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            guard.reap().await;
            return Err(error);
        }
    };
    guard
        .history
        .as_ref()
        .expect("history boundary")
        .remove()
        .await;
    if let Some(mut helper) = guard.resize_helper.take() {
        helper.stop().await;
    }
    attachments.clients.lock().expect("attachments").insert(
        ends.id,
        Attachment {
            master: pair.master,
            read_only: p.read_only,
            session: p.id.clone(),
            tmux: tmux.clone(),
            pid,
            tx: ends.tx.clone(),
            cleanup_under_lease,
        },
    );
    let padding = crate::attach_history::viewport_padding(&bytes, dimensions.rows);
    tokio::spawn(async move {
        let mut guard = guard;
        let closed = ends.tx.closed();
        let output = async {
            if ends.tx.write(&bytes).await.is_err() {
                return Ok::<(), Fail<TerminalError>>(());
            }
            // tmux clears the viewport for its first redraw. Flush all captured
            // history into xterm scrollback before handing over that viewport.
            if ends.tx.write(&padding).await.is_err() {
                return Ok(());
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
        guard.reap_serialized().await;
        ends.tx.finish().await;
    });
    Ok(dimensions)
}
#[cfg(unix)]
struct Guard {
    attachments: Attachments,
    id: u32,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    resize_helper: Option<crate::attach_history::GridClient>,
    history: Option<crate::attach_history::History>,
    session: String,
    tmux: Tmux,
    pid: u32,
    cleanup_under_lease: tokio::sync::watch::Sender<bool>,
}
#[cfg(unix)]
impl Guard {
    async fn reap_serialized(&mut self) {
        // Failed resize already holds the lease and waits for this reaper.
        // Its signal also wakes a normal close already waiting for that lease.
        let mut cleanup_signal = self.cleanup_under_lease.subscribe();
        let lease = tokio::select! {
            biased;
            _ = cleanup_signal.wait_for(|leased| *leased) => None,
            lease = self.attachments.gates.cleanup_lease(&self.session) => Some(lease),
        };
        self.reap().await;
        if let Some(lease) = lease {
            lease.record(self.pid);
            if let Err(Fail::Internal(reason)) = lease.cleanup(&self.tmux).await {
                tracing::warn!(
                    stream = self.id,
                    reason,
                    "terminal close cleanup unresolved"
                );
            }
        }
    }
    async fn reap(&mut self) {
        self.attachments
            .clients
            .lock()
            .expect("attachments")
            .remove(&self.id);
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            // Reap without blocking the async executor, even during shutdown.
            for _ in 0..100 {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    self.child = None;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        if let Some(mut helper) = self.resize_helper.take() {
            helper.stop().await;
        }
        if self.child.is_none()
            && let Some(history) = self.history.take()
        {
            history.remove().await;
        }
    }
}
#[cfg(unix)]
impl Drop for Guard {
    fn drop(&mut self) {
        self.attachments
            .clients
            .lock()
            .expect("attachments")
            .remove(&self.id);
        let child = self.child.take();
        let history = self.history.take();
        if child.is_some() || history.is_some() {
            // Abort/panic/runtime-drop fallback owns only our attach client.
            std::thread::spawn(move || {
                if let Some(mut child) = child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                if let Some(history) = history {
                    history.remove_sync();
                }
            });
        }
    }
}
