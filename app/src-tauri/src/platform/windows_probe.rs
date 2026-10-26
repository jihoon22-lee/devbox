//! Bounded read-only Windows observation. A missing/failed helper never means empty.
use devbox_protocol::windows::*;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_probe(
    request: WindowsProbeRequest,
    output: tokio::sync::mpsc::Sender<WindowsProbeReply>,
    cancel: CancellationToken,
) {
    let result = query(Path::new("powershell.exe"), request.probe, cancel.clone()).await;
    if !cancel.is_cancelled() {
        tokio::select! { _ = cancel.cancelled() => {}, _ = output.send(WindowsProbeReply {probe_id:request.probe_id,result}) => {} }
    }
}
async fn query(
    executable: &Path,
    request: WindowsProbe,
    cancel: CancellationToken,
) -> WindowsProbeResult {
    let timeout = if matches!(
        request,
        WindowsProbe::Paths { .. } | WindowsProbe::Listeners { .. }
    ) {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(3)
    };
    query_with_timeout(executable, request, cancel, timeout).await
}
async fn query_with_timeout(
    executable: &Path,
    request: WindowsProbe,
    cancel: CancellationToken,
    timeout: Duration,
) -> WindowsProbeResult {
    let unavailable = request.unavailable();
    if !request.valid() || cancel.is_cancelled() {
        return unavailable;
    }
    let Ok(bytes) = serde_json::to_vec(&request) else {
        return unavailable;
    };
    if bytes.len() > MAX_WINDOWS_PROBE_BYTES {
        return unavailable;
    }
    let executable = executable.to_path_buf();
    let owned = cancel.child_token();
    let _guard = owned.clone().drop_guard();
    let result = tokio::spawn(async move {
        let mut child = match Command::new(executable)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                POWERSHELL_OBSERVATION_PROBE,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return request.unavailable(),
        };
        let operation = async {
            let mut input = child.stdin.take()?;
            input.write_all(&bytes).await.ok()?;
            drop(input);
            let mut output = Vec::new();
            child
                .stdout
                .take()?
                .take(4097)
                .read_to_end(&mut output)
                .await
                .ok()?;
            let status = child.wait().await.ok()?;
            if !status.success() || output.len() > 4096 {
                return None;
            }
            let result: WindowsProbeResult = serde_json::from_slice(&output).ok()?;
            result.valid_for(&request).then_some(result)
        };
        let result = tokio::select! {
            biased;
            _ = owned.cancelled() => None,
            result = tokio::time::timeout(timeout,operation) => result.ok().flatten(),
        };
        if let Some(result) = result {
            return result;
        }
        let _ = child.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        request.unavailable()
    })
    .await
    .unwrap_or_else(|_| unavailable.clone());
    if cancel.is_cancelled() {
        unavailable
    } else {
        result
    }
}

#[cfg(all(test, unix))]
#[path = "windows_probe_tests.rs"]
mod tests;
