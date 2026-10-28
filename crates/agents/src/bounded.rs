//! Short-lived helper processes the daemon reads one answer from: the interactive shell that
//! finds an agent CLI, its `--version`, and an opencode session export.
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

/// Runs `command` in a new session with no terminal, no stdin and no stderr, reading at most
/// `limit` bytes of stdout within `timeout`. Whatever it started is killed when it ends or is
/// dropped. A failed start, a timeout or more output than `limit` is `None`.
pub(crate) async fn run(
    mut command: Command,
    timeout: Duration,
    limit: usize,
) -> Option<(bool, Vec<u8>)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    detach(&mut command);
    let mut child = command.spawn().ok()?;
    let group = Group(child.id());
    let mut stdout = child.stdout.take()?;
    let read = async {
        let mut bytes = Vec::new();
        (&mut stdout)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(drop)?;
        if bytes.len() > limit {
            return Err(());
        }
        Ok(bytes)
    };
    let wait = async {
        let status = child.wait().await.map_err(drop)?;
        // Background jobs it left could hold the pipe open; its own output is complete.
        group.kill();
        Ok(status)
    };
    let (stdout, status) = tokio::time::timeout(timeout, async { tokio::try_join!(read, wait) })
        .await
        .ok()?
        .ok()?;
    Some((status.success(), stdout))
}

/// The process group of a child started by [`detach`] (its id is the child's).
struct Group(Option<u32>);

impl Group {
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(id) = self.0.and_then(|id| i32::try_from(id).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn detach(command: &mut Command) {
    // SAFETY: setsid is async-signal-safe and touches no memory of this process. Without a
    // controlling terminal an interactive bash cannot stop itself on SIGTTIN or take over the
    // daemon's terminal, and everything it starts stays in a group that can be killed.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(drop)
                .map_err(std::io::Error::from)
        });
    }
}

#[cfg(not(unix))]
fn detach(_: &mut Command) {}
