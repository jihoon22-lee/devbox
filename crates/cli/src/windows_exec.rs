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
const PROBE: &str = r#"
$ErrorActionPreference='Stop'
try {
    $ports=[Console]::In.ReadToEnd() | ConvertFrom-Json
    foreach($port in $ports) {
        $open=$false
        foreach($address in @('127.0.0.1','::1')) {
            $family=if($address -eq '::1') { [System.Net.Sockets.AddressFamily]::InterNetworkV6 } else { [System.Net.Sockets.AddressFamily]::InterNetwork }
            $c=[System.Net.Sockets.TcpClient]::new($family)
            try {
                $t=$c.ConnectAsync($address,[int]$port)
                if($t.Wait(200) -and $c.Connected) { $open=$true; break }
            } catch {} finally { $c.Dispose() }
        }
        if(!$open) { exit 1 }
    }
    exit 0
} catch { exit 1 }
"#;

#[cfg(test)]
#[path = "windows_exec_probe_tests.rs"]
mod probe_tests;
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
async fn probe(executable: &Path, ports: &[u16]) -> bool {
    probe_with_timeout(executable, ports, Duration::from_secs(2)).await
}
async fn probe_with_timeout(executable: &Path, ports: &[u16], timeout: Duration) -> bool {
    let executable = executable.to_path_buf();
    let bytes = serde_json::to_vec(ports).expect("port DTO");
    let cancel = tokio_util::sync::CancellationToken::new();
    let _guard = cancel.clone().drop_guard();
    tokio::spawn(async move {
        let mut child=match Command::new(executable).args(["-NoProfile","-NonInteractive","-Command",PROBE])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true).spawn(){Ok(child)=>child,Err(_)=>return false};
        let mut input=child.stdin.take().expect("probe stdin");
        let operation=async {input.write_all(&bytes).await?;drop(input);child.wait().await};
        let status=tokio::select! {_=cancel.cancelled()=>None,result=tokio::time::timeout(timeout,operation)=>result.ok().and_then(Result::ok)};
        match status {Some(status)=>status.success(),None=>{let _=child.kill().await;let _=child.wait().await;false}}
    }).await.unwrap_or(false)
}
async fn readiness(
    client: &crate::client::Client,
    id: &RunIdentity,
    request: &RunWindows,
    executable: &Path,
) {
    if request.ports.is_empty() {
        std::future::pending::<()>().await;
        return;
    }
    loop {
        if probe(executable, &request.ports).await
            && client.raw_call("windows.ready", id, None).await.is_ok()
        {
            std::future::pending::<()>().await;
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
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
    let request = prepared.request;
    let (started, began) = tokio::sync::oneshot::channel();
    let direct = direct(&request, &interop, started);
    tokio::pin!(direct);
    let result = tokio::select! {result=&mut direct=>result,_=async { if began.await.is_ok() {readiness(&client,&id,&request,&interop).await;} std::future::pending::<()>().await; }=>unreachable!()};
    let code = match result {
        Direct::Finished(code) => Some(code),
        Direct::Failed => None,
        // Unknown completion must not trigger systemd service auto-restart.
        Direct::Unavailable => delegated(&client, &id, &request).await.ok(),
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
) -> anyhow::Result<i32> {
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        client.open_stream("windows.run", request),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Windows execution result is unknown"))??;
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = stream.read_chunk().await? {
        bytes.extend_from_slice(&chunk);
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
                WindowsOutput::Ready => {
                    client.raw_call("windows.ready", id, None).await?;
                }
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
    #[tokio::test]
    async fn windows_probe_timeout_and_cancellation_reap_owned_process() {
        let temp = tempfile::tempdir().expect("fixture result");
        let pidfile = temp.path().join("pid");
        let body = format!(
            "import os,time\nf=open({:?},'w')\ntime.sleep(0.03)\nf.write(str(os.getpid()))\nf.flush()\ntime.sleep(60)",
            pidfile.to_str().expect("fixture value")
        );
        let path = fake(temp.path(), &body);
        assert!(!probe_with_timeout(&path, &[12345], Duration::from_millis(250)).await);
        let pid = fs::read_to_string(&pidfile).expect("fixture result");
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        fs::remove_file(&pidfile).expect("fixture result");
        let task = tokio::spawn(async move {
            probe_with_timeout(&path, &[12345], Duration::from_secs(60)).await
        });
        // File creation precedes the PID write; wait for a complete positive identity.
        let pid = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(text) = fs::read_to_string(&pidfile)
                    && let Ok(pid) = text.parse::<u32>()
                    && pid > 1
                {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("fixture result");
        eprintln!("owned cancelled probe pid={pid}");
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(2), async {
            while Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("fixture result");
    }
    #[tokio::test]
    async fn windows_direct_probe_passes_ports_to_windows_process_stdin() {
        let temp = tempfile::tempdir().expect("fixture result");
        let path = fake(
            temp.path(),
            "import sys,json\nassert json.load(sys.stdin)==[12345,12346]\nassert 'TcpClient' in sys.argv[-1]\nsys.exit(0)",
        );
        assert!(probe(&path, &[12345, 12346]).await);
    }
}
