//! Native execution adapter. Cancellation only attempts to stop PowerShell itself.
use crate::core::windows::powershell_args;
use devbox_protocol::windows::{RunIdentity, RunWindows, WindowsEvent, WindowsOutput};
use std::{ffi::OsStr, future::Future, io, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    process::Command,
    sync::mpsc::Sender,
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;
#[path = "windows_probe.rs"]
mod observation;
pub(crate) use observation::run_probe;

struct Length(usize);
impl io::Write for Length {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn stdin_payload(request: &RunWindows) -> Result<Zeroizing<Vec<u8>>, ()> {
    if !request.tracked() {
        return Err(());
    }
    // Exact capacity avoids reallocating and leaving old plaintext buffers behind.
    let mut length = Length(0);
    serde_json::to_writer(&mut length, request).map_err(|_| ())?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(length.0));
    serde_json::to_writer(&mut *bytes, request).map_err(|_| ())?;
    Ok(bytes)
}

async fn write_stdin(
    mut stdin: impl AsyncWrite + Unpin,
    payload: &[u8],
    timeout: Duration,
) -> Result<(), ()> {
    tokio::time::timeout(timeout, async {
        stdin.write_all(payload).await?;
        stdin.shutdown().await
    })
    .await
    .map_err(|_| ())?
    .map_err(|_| ())
}

async fn child_and_output(
    child: impl Future<Output = Result<i32, ()>>,
    output: impl Future<Output = Result<(), ()>>,
    drain_timeout: Duration,
) -> Result<i32, ()> {
    tokio::pin!(child, output);
    let code = tokio::select! {
        code = &mut child => code?,
        drained = &mut output => {
            drained?;
            return child.await;
        }
    };
    // Descendants may retain inherited pipes. Their lifetime must not replace
    // the direct child's exit status or keep the run active indefinitely.
    if let Ok(drained) = tokio::time::timeout(drain_timeout, &mut output).await {
        drained?;
    }
    Ok(code)
}

async fn emit(
    output: &Sender<WindowsEvent>,
    identity: &RunIdentity,
    event: WindowsOutput,
) -> Result<(), ()> {
    output
        .send(WindowsEvent {
            run_id: identity.run_id.clone(),
            attempt_id: identity.attempt_id.clone(),
            event,
        })
        .await
        .map_err(|_| ())
}

async fn stream(
    mut reader: impl AsyncRead + Unpin,
    output: &Sender<WindowsEvent>,
    identity: &RunIdentity,
    stderr: bool,
) -> Result<(), ()> {
    let mut buffer = Zeroizing::new([0u8; 8192]);
    loop {
        let count = reader.read(&mut buffer[..]).await.map_err(|_| ())?;
        if count == 0 {
            return Ok(());
        }
        // Preserve bytes across UTF-8/chunk boundaries for the daemon's decoder.
        let data = buffer[..count].to_vec();
        let event = if stderr {
            WindowsOutput::Stderr { data }
        } else {
            WindowsOutput::Stdout { data }
        };
        emit(output, identity, event).await?;
    }
}

async fn readiness<F, P>(
    ports: &[u16],
    output: &Sender<WindowsEvent>,
    identity: &RunIdentity,
    done: &CancellationToken,
    cadence: Duration,
    mut probe: F,
) -> Result<(), ()>
where
    F: FnMut(u16) -> P,
    P: Future<Output = bool>,
{
    if ports.is_empty() {
        return Ok(());
    }
    let mut interval = tokio::time::interval(cadence);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = done.cancelled() => return Ok(()),
            _ = interval.tick() => {}
        }
        let open = tokio::select! {
            biased;
            _ = done.cancelled() => return Ok(()),
            open = async {
                let mut open = true;
                for port in ports {
                    open &= probe(*port).await;
                }
                open
            } => open,
        };
        if open {
            tokio::select! {
                biased;
                _ = done.cancelled() => return Ok(()),
                result = emit(output, identity, WindowsOutput::Ready) => return result,
            }
        }
    }
}

async fn heartbeat(
    output: &Sender<WindowsEvent>,
    identity: &RunIdentity,
    done: &CancellationToken,
    cadence: Duration,
) -> Result<(), ()> {
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + cadence, cadence);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            _ = done.cancelled() => return Ok(()),
            _ = interval.tick() => {}
        }
        tokio::select! {
            biased;
            _ = done.cancelled() => return Ok(()),
            sent = emit(output, identity, WindowsOutput::Heartbeat) => sent?,
        }
    }
}

#[cfg(windows)]
async fn port_open(port: u16) -> bool {
    loopback_port_open(port).await
}

#[cfg(any(windows, test))]
async fn loopback_port_open(port: u16) -> bool {
    for address in [
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
        std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
    ] {
        if matches!(
            tokio::time::timeout(
                Duration::from_millis(200),
                tokio::net::TcpStream::connect((address, port)),
            )
            .await,
            Ok(Ok(_))
        ) {
            return true;
        }
    }
    false
}

// Linux compiles this adapter for protocol tests; it never probes WSL localhost.
#[cfg(not(windows))]
async fn port_open(_port: u16) -> bool {
    false
}

// Only the Linux protocol tests (`core::windows::platform_compile`) drive the adapter directly;
// Windows always goes through `run_windows_tracked`.
#[cfg(all(test, not(windows)))]
pub async fn run_windows(
    request: RunWindows,
    output: Sender<WindowsEvent>,
    cancel: CancellationToken,
) {
    run_with_executable(request, output, cancel, OsStr::new("powershell.exe")).await;
}

#[cfg(windows)]
pub async fn run_windows_tracked(
    request: RunWindows,
    output: Sender<WindowsEvent>,
    cancel: CancellationToken,
    spawned: impl FnOnce(Option<&tokio::process::Child>) -> bool + Send + 'static,
) {
    run_observed(
        request,
        output,
        cancel,
        OsStr::new("powershell.exe"),
        spawned,
    )
    .await;
}

#[cfg(all(test, not(windows)))]
pub(crate) async fn run_with_executable(
    request: RunWindows,
    output: Sender<WindowsEvent>,
    cancel: CancellationToken,
    executable: &OsStr,
) {
    run_observed(request, output, cancel, executable, |_| true).await;
}

async fn run_observed(
    request: RunWindows,
    output: Sender<WindowsEvent>,
    cancel: CancellationToken,
    executable: &OsStr,
    spawned: impl FnOnce(Option<&tokio::process::Child>) -> bool + Send,
) {
    if cancel.is_cancelled() || output.is_closed() {
        spawned(None);
        return;
    }
    let identity = RunIdentity {
        run_id: request.run_id.clone(),
        attempt_id: request.attempt_id.clone(),
    };
    let ports = request.ports.clone();
    let payload = stdin_payload(&request);
    drop(request);
    let Ok(payload) = payload else {
        spawned(None);
        let _ = emit(&output, &identity, WindowsOutput::Failed).await;
        return;
    };
    let child = Command::new(executable)
        .args(powershell_args())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let Ok(mut child) = child else {
        spawned(None);
        tokio::select! {
            _ = cancel.cancelled() => {}
            _ = emit(&output, &identity, WindowsOutput::Failed) => {}
        }
        return;
    };
    // Capture lifetime authority before the bootstrap can receive a user command.
    if !spawned(Some(&child)) {
        drop(child.stdin.take());
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        let _ = emit(&output, &identity, WindowsOutput::Failed).await;
        return;
    }
    let completed = tokio::select! {
        _ = cancel.cancelled() => None,
        _ = output.closed() => None,
        result = async {
            let stdin = child.stdin.take().ok_or(())?;
            write_stdin(stdin, &payload, Duration::from_secs(10)).await?;
            drop(payload);
            let stdout = child.stdout.take().ok_or(())?;
            let stderr = child.stderr.take().ok_or(())?;
            let done = CancellationToken::new();
            let (code, _, _) = tokio::try_join!(
                child_and_output(async {
                    let status = child.wait().await.map_err(|_| ());
                    done.cancel();
                    status?.code().ok_or(())
                }, async {
                    tokio::try_join!(
                        stream(stdout, &output, &identity, false),
                        stream(stderr, &output, &identity, true),
                    )?;
                    Ok(())
                }, Duration::from_secs(1)),
                readiness(&ports, &output, &identity, &done, Duration::from_secs(1), port_open),
                heartbeat(&output, &identity, &done, Duration::from_secs(1)),
            )?;
            Ok(code)
        } => Some(result),
    };
    if !matches!(completed, Some(Ok(_))) {
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
    let Some(completed) = completed else { return };
    let event = match completed {
        Ok(code) => WindowsOutput::Finished { code },
        Err(()) => WindowsOutput::Failed,
    };
    tokio::select! {
        _ = cancel.cancelled() => {}
        _ = emit(&output, &identity, event) => {}
    }
}

#[cfg(all(test, unix))]
#[path = "windows_tracking_tests.rs"]
mod tracking_tests;

#[cfg(test)]
#[path = "windows_adapter_tests.rs"]
mod tests;
