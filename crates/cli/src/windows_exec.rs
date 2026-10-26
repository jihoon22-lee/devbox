//! Runs inside the existing exec wrapper. Only failure to spawn permits native delegation.
use devbox_protocol::{message::ClientKind, windows::*};
use std::{
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};
use zeroize::Zeroizing;
#[path = "windows_observation.rs"]
mod observation;
pub(crate) use observation::{check_paths, listeners, observe};

#[cfg(test)]
#[path = "windows_exec_probe_tests.rs"]
mod probe_tests;
#[cfg(test)]
#[path = "windows_exec_ready_tests.rs"]
mod ready_tests;
#[derive(Debug, PartialEq, Eq)]
enum Direct {
    Unavailable,
    Finished(i32),
    Failed,
}
async fn direct(
    request: &RunWindows,
    executable: &Path,
    started: tokio::sync::oneshot::Sender<()>,
) -> Direct {
    if !request.tracked() {
        return Direct::Failed;
    }
    let request = request.clone();
    let executable = executable.to_path_buf();
    let cancel = tokio_util::sync::CancellationToken::new();
    let _guard = cancel.clone().drop_guard();
    tokio::spawn(async move {
        let mut child=match Command::new(executable).args(["-NoProfile","-NonInteractive","-Command",POWERSHELL_BOOTSTRAP])
            .stdin(Stdio::piped()).stdout(Stdio::inherit()).stderr(Stdio::inherit()).kill_on_drop(true).spawn() {
            Ok(child)=>child,Err(_)=>return Direct::Unavailable,
        };
        let bytes=Zeroizing::new(serde_json::to_vec(&request).expect("Windows request DTO"));
        drop(request);
        let mut input=child.stdin.take().expect("Windows stdin");
        let wrote=tokio::select! {_=cancel.cancelled()=>false,result=tokio::time::timeout(Duration::from_secs(10),input.write_all(&bytes))=>matches!(result,Ok(Ok(())))};
        drop(input);drop(bytes);
        if !wrote {let _=child.kill().await;let _=child.wait().await;return Direct::Failed;}
        let _ = started.send(());
        let status=tokio::select! {_=cancel.cancelled()=>None,status=child.wait()=>status.ok()};
        match status {
            Some(status)=>Direct::Finished(status.code().unwrap_or(1)),
            None=>{let _=child.kill().await;let _=child.wait().await;Direct::Failed},
        }
    }).await.unwrap_or(Direct::Failed)
}
/// The daemon gate is the only readiness authority. Not-ready replies and failures retry
/// at the cadence; a hint (native Ready event) only wakes an earlier retry.
/// Gate polling schedule. Each not-ready probe starts PowerShell on Windows, so after the
/// service's readiness window the wrapper backs off exponentially instead of polling every
/// second for the run's whole life. Not-ready semantics are unchanged; a native Ready hint
/// still wakes the next poll immediately.
#[derive(Clone, Copy, Debug)]
struct Cadence {
    base: Duration,
    settle: Duration,
    max: Duration,
}
impl Cadence {
    fn windows(ready_timeout_sec: u32) -> Self {
        Self {
            base: Duration::from_secs(1),
            settle: Duration::from_secs(ready_timeout_sec.into()),
            max: Duration::from_secs(30),
        }
    }
    #[cfg(test)]
    fn fixed(delay: Duration) -> Self {
        Self {
            base: delay,
            settle: Duration::MAX,
            max: delay,
        }
    }
    /// Delay after a not-ready reply at `elapsed` since polling began.
    fn next(&self, previous: Option<Duration>, elapsed: Duration) -> Duration {
        if elapsed < self.settle {
            return self.base;
        }
        previous
            .map_or(self.base, |delay| delay.saturating_mul(2))
            .clamp(self.base, self.max)
    }
}
async fn poll_ready<F, P>(mut accept: F, hint: &tokio::sync::Notify, cadence: Cadence)
where
    F: FnMut() -> P,
    P: std::future::Future<Output = bool>,
{
    let started = tokio::time::Instant::now();
    let mut delay = None;
    while !accept().await {
        let next = cadence.next(delay, started.elapsed());
        delay = Some(next);
        tokio::select! {_=tokio::time::sleep(next)=>{},_=hint.notified()=>{}}
    }
}
async fn accepted(client: &crate::client::Client, id: &RunIdentity) -> bool {
    let reply = client.raw_call("windows.ready", id, None).await;
    matches!(
        reply.ok().map(|text| serde_json::from_str(&text)),
        Some(Ok(WindowsReadiness::Ready))
    )
}
/// Never completes; the wrapper never asks the gate for a no-port service.
async fn readiness<F, P>(ports: &[u16], accept: F, hint: &tokio::sync::Notify, cadence: Cadence)
where
    F: FnMut() -> P,
    P: std::future::Future<Output = bool>,
{
    if !ports.is_empty() {
        poll_ready(accept, hint, cadence).await;
    }
    std::future::pending::<()>().await;
}
pub async fn run(run_id: String) -> anyhow::Result<ExitStatus> {
    let attempt_id = std::env::var("DEVBOX_RUN_ATTEMPT")
        .map_err(|_| anyhow::anyhow!("Windows attempt is unavailable"))?;
    let id = RunIdentity { run_id, attempt_id };
    let paths = crate::paths()?;
    let client =
        crate::client::Client::connect(&paths.socket, ClientKind::Cli, paths.instance.as_str())
            .await?;
    let input = client.raw_call("windows.prepare", &id, None).await?;
    let mut prepared: PreparedWindows = serde_json::from_str(&input)?;
    for key in &prepared.injected {
        let value = std::env::var(key)
            .map_err(|_| anyhow::anyhow!("Windows secret environment is unavailable"))?;
        prepared.request.env.insert(key.clone(), value);
    }
    let interop = std::path::PathBuf::from(prepared.interop);
    let cadence = Cadence::windows(prepared.ready_timeout_sec);
    let request = prepared.request;
    let (started, began) = tokio::sync::oneshot::channel();
    let direct = direct(&request, &interop, started);
    tokio::pin!(direct);
    let hint = tokio::sync::Notify::new();
    let ready = async {
        if began.await.is_ok() {
            let accept = || accepted(&client, &id);
            readiness(&request.ports, accept, &hint, cadence).await;
        }
        std::future::pending::<()>().await;
    };
    let result = tokio::select! {result=&mut direct=>result,_=ready=>unreachable!()};
    let code = match result {
        Direct::Finished(code) => Some(code),
        Direct::Failed => None,
        // Unknown completion must not trigger systemd service auto-restart.
        Direct::Unavailable => delegated(&client, &id, &request, cadence).await.ok(),
    };
    if let Some(code) = code {
        devbox_runtime::exit_record::ExitStore::new(&paths.data_dir.join("runs"), &id.run_id)?
            .record_windows_exit(&id.attempt_id, code)?;
        Ok(known_exit(code))
    } else {
        Ok(unknown_exit())
    }
}
async fn delegated(
    client: &crate::client::Client,
    id: &RunIdentity,
    request: &RunWindows,
    cadence: Cadence,
) -> anyhow::Result<i32> {
    if !request.tracked() {
        anyhow::bail!("Windows observation is unavailable");
    }
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        client.open_stream("windows.run", request),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Windows execution result is unknown"))??;
    let read = || async { stream.read_chunk().await.map_err(anyhow::Error::from) };
    let accept = || accepted(client, id);
    relay(read, id, &request.ports, accept, cadence).await
}
/// Reads native events while polling the daemon gate concurrently. A not-ready reply never
/// aborts the stream, so an unrelated listener seen first cannot strand the run.
async fn relay<R, RF, C, A, AF>(
    mut read: R,
    id: &RunIdentity,
    ports: &[u16],
    accept: A,
    cadence: Cadence,
) -> anyhow::Result<i32>
where
    R: FnMut() -> RF,
    RF: std::future::Future<Output = anyhow::Result<Option<C>>>,
    C: AsRef<[u8]>,
    A: FnMut() -> AF,
    AF: std::future::Future<Output = bool>,
{
    let hint = tokio::sync::Notify::new();
    let ready = readiness(ports, accept, &hint, cadence);
    tokio::pin!(ready);
    let mut bytes = Zeroizing::new(Vec::new());
    loop {
        let chunk = tokio::select! {chunk=read()=>chunk?,_=&mut ready=>unreachable!()};
        let Some(chunk) = chunk else { break };
        bytes.extend_from_slice(chunk.as_ref());
        loop {
            if bytes.len() < 4 {
                break;
            }
            let length = u32::from_le_bytes(bytes[..4].try_into().expect("event prefix")) as usize;
            if length > 64 * 1024 {
                anyhow::bail!("invalid Windows output");
            }
            if bytes.len() < length + 4 {
                break;
            }
            let event: WindowsEvent = serde_json::from_slice(&bytes[4..length + 4])?;
            bytes.drain(..length + 4);
            if event.run_id != id.run_id || event.attempt_id != id.attempt_id {
                anyhow::bail!("Windows attempt changed");
            }
            match event.event {
                WindowsOutput::Stdout { data } => tokio::io::stdout().write_all(&data).await?,
                WindowsOutput::Stderr { data } => tokio::io::stderr().write_all(&data).await?,
                WindowsOutput::Heartbeat => {}
                // A connect-level hint only; the gate decides ownership.
                WindowsOutput::Ready => hint.notify_one(),
                WindowsOutput::Finished { code } => return Ok(code),
                WindowsOutput::Failed => anyhow::bail!("Windows execution failed"),
            }
        }
    }
    anyhow::bail!("Windows execution result is unknown; no retry was attempted")
}

fn known_exit(code: i32) -> ExitStatus {
    ExitStatus::from_raw((if code == 0 { 0 } else { (code & 255).max(1) }) << 8)
}
fn unknown_exit() -> ExitStatus {
    ExitStatus::from_raw(nix::sys::signal::Signal::SIGUSR2 as i32)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn fake(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("fake powershell");
        fs::write(&path, format!("#!/usr/bin/python3\n{body}\n")).expect("fixture result");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("fixture result");
        path
    }
    fn request() -> RunWindows {
        RunWindows {
            run_id: "owned-run".into(),
            attempt_id: "owned-attempt".into(),
            cwd: "C:\\owned path's\\repo".into(),
            command: "Write-Output $env:TOKEN".into(),
            env: [("TOKEN".into(), "owned-non-ascii-비밀".into())].into(),
            ports: vec![],
            observation: devbox_protocol::windows::ObservationControl {
                schema: 1,
                launch_id: "owned-launch".into(),
                nonce: "0123456789abcdef0123456789abcdef".into(),
                job_name: "Local\\Devbox.Observe.0123456789abcdef0123456789abcdef".into(),
                assigned_path: "C:\\owned\\control\\assigned.json".into(),
                grant_path: "C:\\owned\\control\\grant.json".into(),
                closed_path: "C:\\owned\\control\\closed.json".into(),
            },
        }
    }
    #[tokio::test]
    async fn windows_direct_spawn_failure_is_only_native_fallback_condition() {
        let temp = tempfile::tempdir().expect("fixture result");
        let (started, _) = tokio::sync::oneshot::channel();
        assert_eq!(
            direct(&request(), &temp.path().join("absent"), started).await,
            Direct::Unavailable
        );
        let path = fake(
            temp.path(),
            "import sys,json\nr=json.load(sys.stdin)\nassert r['env']['TOKEN']=='owned-non-ascii-비밀'\nassert not any('owned-non-ascii-비밀' in a for a in sys.argv)\nsys.exit(78)",
        );
        let (started, began) = tokio::sync::oneshot::channel();
        assert_eq!(
            direct(&request(), &path, started).await,
            Direct::Finished(78)
        );
        assert!(began.await.is_ok());
    }
    #[tokio::test]
    async fn windows_direct_invalid_observation_never_spawns_a_live_adapter() {
        let temp = tempfile::tempdir().expect("fixture");
        let marker = temp.path().join("executed");
        let executable = fake(
            temp.path(),
            &format!(
                "open({:?},'w').write('executed')",
                marker.to_str().expect("path")
            ),
        );
        let mut invalid = request();
        invalid.observation.nonce = "invalid".into();
        let (started, _) = tokio::sync::oneshot::channel();
        assert_eq!(direct(&invalid, &executable, started).await, Direct::Failed);
        assert!(!marker.exists());
    }
    #[tokio::test]
    async fn windows_direct_started_failure_never_becomes_unavailable() {
        let temp = tempfile::tempdir().expect("fixture result");
        let path = fake(temp.path(), "import sys\nsys.exit(9)");
        let (started, _) = tokio::sync::oneshot::channel();
        assert!(matches!(
            direct(&request(), &path, started).await,
            Direct::Finished(9) | Direct::Failed
        ));
    }
    #[test]
    fn windows_unknown_signal_is_distinct_from_known_exit_78() {
        assert_eq!(known_exit(78).code(), Some(78));
        assert_eq!(known_exit(256).code(), Some(1));
        assert_eq!(
            unknown_exit().signal(),
            Some(nix::sys::signal::Signal::SIGUSR2 as i32)
        );
        assert_eq!(unknown_exit().code(), None);
    }
}
