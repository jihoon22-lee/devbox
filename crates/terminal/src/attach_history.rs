//! Immutable history captured by the same client queue as its first redraw.
use crate::{AttachSession, TerminalError, attach::pty_error, tmux::Tmux};
use devbox_protocol::Fail;
use portable_pty::CommandBuilder;
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

pub(crate) fn viewport_padding(history: &[u8], rows: u16) -> Vec<u8> {
    if history.is_empty() {
        return Vec::new();
    }
    let rows = usize::from(rows) - usize::from(history.ends_with(b"\n"));
    b"\r\n".repeat(rows)
}

pub(crate) struct GridClient {
    child: tokio::process::Child,
    drain: tokio::task::JoinHandle<()>,
}
impl GridClient {
    pub async fn stop(&mut self) {
        let _ = self.child.kill().await;
        self.drain.abort();
    }
}
impl Drop for GridClient {
    fn drop(&mut self) {
        self.drain.abort();
        // kill_on_drop retains Tokio's owned-child reaping on task abort.
    }
}

pub(crate) struct History {
    tmux: Tmux,
    buffer: String,
}
impl History {
    pub fn new(tmux: &Tmux) -> Self {
        Self {
            tmux: tmux.clone(),
            buffer: format!("devbox-history-{}", uuid::Uuid::new_v4().simple()),
        }
    }
    pub fn queue(&self, command: &mut CommandBuilder, p: &AttachSession) {
        // tmux ignores empty paste buffers. The one-byte sentinel is replaced
        // only when scrollback exists; capture -E -1 with zero history includes
        // visible row zero and would duplicate it in the prefill.
        command.args([
            "if-shell",
            "-F",
            "-t",
            &format!("={}:", p.id),
            "#{history_size}",
        ]);
        command.arg(format!(
            "capture-pane -e -J -S -2000 -E -1 -b {} -t ={}:",
            self.buffer, p.id
        ));
        command.arg(format!("set-buffer -b {} ' '", self.buffer));
        command.args([";", "attach"]);
        if p.read_only {
            command.args(["-f", "read-only,ignore-size"]);
        }
        command.args(["-t", &format!("={}", p.id)]);
    }
    pub async fn read(&self, pid: u32) -> Result<Vec<u8>, Fail<TerminalError>> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let clients = self
                    .tmux
                    .run(&["list-clients", "-F", "#{client_pid}"])
                    .await
                    .map_err(pty_error)?;
                if !clients.lines().any(|line| line == pid.to_string()) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                // The server drains capture+attach without yielding and then
                // redraws before processing another event. This command reads
                // the immutable boundary, never a later pane snapshot.
                if let Ok(value) = self
                    .tmux
                    .output(&["save-buffer", "-b", &self.buffer, "-"])
                    .await
                {
                    if value == b" " {
                        return Ok(vec![]);
                    }
                    let mut bytes = Vec::with_capacity(value.len());
                    for byte in value {
                        if byte == b'\n' {
                            bytes.push(b'\r');
                        }
                        bytes.push(byte);
                    }
                    return Ok(bytes);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(pty_error)?
    }
    pub async fn remove(&self) {
        let _ = self.tmux.run(&["delete-buffer", "-b", &self.buffer]).await;
    }
    pub fn remove_sync(&self) {
        let Ok(mut child) = std::process::Command::new("tmux")
            .args([
                "-L",
                self.tmux.socket(),
                "delete-buffer",
                "-b",
                &self.buffer,
            ])
            .env_remove("TMUX")
            .env_remove("TMUX_TMPDIR")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Apply the writable client's resize before its history boundary. A control
/// client's explicit size refresh is synchronous and preserves window-size
/// policy; normal PTY attach schedules resize after its command queue.
pub(crate) async fn prepare_grid(
    tmux: &Tmux,
    p: &AttachSession,
    tx: &devbox_protocol::stream::StreamTx,
    record: impl Fn(u32),
) -> Result<Option<GridClient>, Fail<TerminalError>> {
    if p.read_only {
        return Ok(None);
    }
    let marker = format!("devbox-grid-ready-{}", uuid::Uuid::new_v4().simple());
    let mut child = tokio::process::Command::new("tmux")
        .args(["-L", tmux.socket(), "-f"])
        .arg(tmux.conf())
        .args([
            "-C",
            "attach",
            "-t",
            &format!("={}", p.id),
            ";",
            "refresh-client",
            "-C",
            &format!("{},{}", p.cols, p.rows),
            ";",
            "display-message",
            "-p",
            &marker,
        ])
        .env_remove("TMUX")
        .env_remove("TMUX_TMPDIR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(pty_error)?;
    let pid = child
        .id()
        .ok_or_else(|| pty_error("resize client has no pid"))?;
    record(pid);
    let mut output = BufReader::new(child.stdout.take().expect("piped resize output"));
    let ready = tokio::time::timeout(Duration::from_secs(10), async {
        let mut line = String::new();
        loop {
            line.clear();
            if output.read_line(&mut line).await.map_err(pty_error)? == 0 {
                return Err(pty_error("resize client exited"));
            }
            if line.starts_with("%error ") {
                return Err(pty_error("resize client command failed"));
            }
            if line.trim_end() == marker {
                return Ok(());
            }
        }
    });
    let closed = tx.closed();
    let result = tokio::select! {
        biased;
        _ = closed.cancelled() => Err(TerminalError::StreamUnavailable {}.into()),
        result = ready => result.map_err(pty_error).and_then(|result| result),
    };
    if let Err(error) = result {
        let _ = child.kill().await;
        return Err(error);
    }
    // Keep the control client's stdout open and drained until PTY capture+attach
    // has completed; dropping it early could detach and undo the prepared grid.
    let drain = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut output, &mut tokio::io::sink()).await;
    });
    Ok(Some(GridClient { child, drain }))
}

#[cfg(test)]
mod padding_tests {
    use super::*;
    #[test]
    fn padding_moves_complete_and_partial_history_out_of_the_initial_viewport() {
        assert!(viewport_padding(b"", 50).is_empty());
        assert_eq!(viewport_padding(b"history\r\n", 3), b"\r\n\r\n");
        assert_eq!(
            viewport_padding(b"partial-wrapped-history", 3),
            b"\r\n\r\n\r\n"
        );
        assert!(viewport_padding(b"history\r\n", 1).is_empty());
        assert_eq!(viewport_padding(b"partial", 1), b"\r\n");
    }
}
