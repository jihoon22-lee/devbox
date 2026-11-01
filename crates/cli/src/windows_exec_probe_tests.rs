//! Opt-in Windows-local regressions (listener ownership, the job holder's receipt); normal
//! Linux tests use no Windows host resources.
use super::*;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

/// Starts an owned Windows fixture process and reads its single JSON announcement line.
async fn fixture(executable: &Path, script: &str) -> (tokio::process::Child, serde_json::Value) {
    let mut child = Command::new(executable)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("fixture result");
    let mut output = BufReader::new(child.stdout.take().expect("fixture value"));
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(20), output.read_line(&mut line))
        .await
        .expect("fixture result")
        .expect("fixture result");
    (
        child,
        serde_json::from_str(line.trim()).expect("fixture result"),
    )
}
async fn release(mut child: tokio::process::Child) {
    drop(child.stdin.take());
    let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
}

#[tokio::test]
#[ignore = "requires DEVBOX_TEST_WINDOWS_POWERSHELL pointing to Windows PowerShell via WSL interop"]
async fn windows_listener_ownership_accepts_only_the_assigned_job_tree() {
    let executable = std::env::var_os("DEVBOX_TEST_WINDOWS_POWERSHELL")
        .expect("explicit Windows PowerShell executable is required");
    let executable = Path::new(&executable);
    // An unrelated listener owned by this test, outside the assigned job.
    let unrelated = r#"
$ErrorActionPreference='Stop'
$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
try {
    $l.Start()
    [Console]::Out.WriteLine((@{port=$l.LocalEndpoint.Port} | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
    [Console]::In.ReadToEnd() | Out-Null
} finally { $l.Stop() }
"#;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let job = format!("{OBSERVATION_JOB_PREFIX}{nonce}");
    // The assigned root joins a fresh job, listens on IPv4/IPv6 loopback, and starts a
    // descendant listening on the dual-stack wildcard. All are released on stdin EOF.
    let owned = format!(
        r#"{native}
$job=[DevboxObservation]::CreateFresh('{job}')
[DevboxObservation]::AssociateFresh($job)
$v4=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
$v6=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::IPv6Loopback,0)
$v6.Server.DualMode=$false
$info=New-Object System.Diagnostics.ProcessStartInfo 'powershell.exe','-NoProfile -NonInteractive -Command "$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::IPv6Any,0);$l.Server.DualMode=$true;$l.Start();[Console]::Out.WriteLine($l.LocalEndpoint.Port);[Console]::Out.Flush();[Console]::In.ReadToEnd()|Out-Null;$l.Stop()"'
$info.UseShellExecute=$false; $info.RedirectStandardOutput=$true; $info.RedirectStandardInput=$true
$descendant=$null
try {{
    $v4.Start(); $v6.Start()
    $descendant=[System.Diagnostics.Process]::Start($info)
    $port=[int]$descendant.StandardOutput.ReadLine()
    [Console]::Out.WriteLine((@{{pid=$PID;creationTime=[DevboxObservation]::CreationTime();sid=[DevboxObservation]::Sid();sessionId=[DevboxObservation]::Session();v4=$v4.LocalEndpoint.Port;v6=$v6.LocalEndpoint.Port;descendant=$port}} | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
    [Console]::In.ReadToEnd() | Out-Null
}} finally {{
    if($descendant) {{ $descendant.StandardInput.Close(); [void]$descendant.WaitForExit(5000) }}
    $v4.Stop(); $v6.Stop()
}}
"#,
        native = include_str!("../../protocol/src/windows/native.ps1"),
    );
    let (other, announced) = fixture(executable, unrelated).await;
    let foreign = announced["port"].as_u64().expect("port") as u16;
    let (root, announced) = fixture(executable, &owned).await;
    let port = |key: &str| announced[key].as_u64().expect("port") as u16;
    let (v4, v6, descendant) = (port("v4"), port("v6"), port("descendant"));
    let assigned = ObservationAssigned {
        schema: OBSERVATION_SCHEMA,
        run_id: "owned-run".into(),
        launch_id: "owned-launch".into(),
        attempt_id: "owned-attempt".into(),
        nonce,
        job_name: job,
        pid: announced["pid"].as_u64().expect("pid") as u32,
        creation_time: announced["creationTime"].as_str().expect("creation").into(),
        sid: announced["sid"].as_str().expect("sid").into(),
        session_id: announced["sessionId"].as_u64().expect("session") as u32,
    };
    eprintln!(
        "owned Windows listeners v4={v4} v6={v6} descendant={descendant} unrelated={foreign}"
    );
    let check = |ports: Vec<u16>| {
        let assigned = assigned.clone();
        async move { listeners(executable, &assigned, &ports, CancellationToken::new()).await }
    };
    let only_unrelated = check(vec![foreign]).await;
    let ipv4 = check(vec![v4]).await;
    let ipv6 = check(vec![v6]).await;
    let child = check(vec![descendant]).await;
    let all = check(vec![v4, v6, descendant]).await;
    let mixed = check(vec![v4, foreign]).await;
    let timed_out = observation::query_with_timeout(
        executable,
        WindowsProbe::Listeners {
            assigned: assigned.clone(),
            ports: vec![v4],
        },
        CancellationToken::new(),
        Duration::from_millis(1),
    )
    .await;
    // An owned holder keeps a handle to the root, so the exited root stays openable while its
    // job name vanishes with the root's last job handle: exit must still read as gone.
    let holder = format!(
        r#"$ErrorActionPreference='Stop'
$p=[System.Diagnostics.Process]::GetProcessById({pid})
try {{
    [void]$p.Handle
    [Console]::Out.WriteLine((@{{held=$p.Id}} | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
    [Console]::In.ReadToEnd() | Out-Null
}} finally {{ $p.Dispose() }}
"#,
        pid = assigned.pid
    );
    let (holder, _) = fixture(executable, &holder).await;
    // Release and reap every owned fixture even when an assertion below fails.
    release(root).await;
    let exited = check(vec![v4]).await;
    release(holder).await;
    let reaped = check(vec![v4]).await;
    release(other).await;
    assert_eq!(
        only_unrelated,
        ListenerObservation::NotOwned,
        "unrelated only"
    );
    assert_eq!(ipv4, ListenerObservation::Owned, "IPv4 loopback root");
    assert_eq!(ipv6, ListenerObservation::Owned, "IPv6 loopback root");
    assert_eq!(child, ListenerObservation::Owned, "dual-stack descendant");
    assert_eq!(all, ListenerObservation::Owned, "all required ports");
    assert_eq!(mixed, ListenerObservation::NotOwned, "one port unrelated");
    assert_eq!(
        timed_out,
        WindowsProbeResult::Listeners {
            state: ListenerObservation::Unavailable
        },
        "timeout is never success"
    );
    assert!(
        matches!(
            exited,
            ListenerObservation::NotFound | ListenerObservation::Empty
        ),
        "exited tree with a held handle: {exited:?}"
    );
    assert!(
        matches!(
            reaped,
            ListenerObservation::NotFound | ListenerObservation::Empty
        ),
        "exited tree: {reaped:?}"
    );
}

/// A hidden PowerShell that sleeps 5 s, started without waiting: it stays in the job after the
/// bootstrap exits, as a build server would, and ends by itself. The test never ends it.
const ORPHAN: &str = "Start-Process -FilePath powershell.exe -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 5'";
/// The holder's `{"nonce":…}` receipt, read as strictly as the daemon reads it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    nonce: String,
}
/// Publish like the daemon: a complete temporary file, then a hard link at the final name.
fn publish(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_file_name(format!(".control-{}.tmp", uuid::Uuid::new_v4().simple()));
    std::fs::write(&temp, bytes)?;
    let linked = std::fs::hard_link(&temp, path);
    std::fs::remove_file(&temp)?;
    linked
}
/// When an owned file was first seen, polled as the daemon polls `assigned`.
async fn appears(path: &Path, within: Duration) -> Option<Instant> {
    let deadline = Instant::now() + within;
    while !path.exists() {
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Some(Instant::now())
}

/// I1 on real Windows: the production bootstrap, through the same direct interop path as a
/// Windows run, with a command that leaves a 5 s orphan. In order: the holder is ready, then
/// `assigned`; no receipt while the orphan outlives the bootstrap; the receipt after it ended.
///
/// S7-14 W1 runs it on the Windows PC from WSL, under the acceptance lock, and again after any
/// fix to the bootstrap, the holder or the receipt:
///
/// ```text
/// python3 scripts/verify-resources.py --wait --lock windows-acceptance -- env \
///   DEVBOX_TEST_WINDOWS_POWERSHELL=/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe \
///   'DEVBOX_TEST_WINDOWS_DIR=C:\Users\<name>\AppData\Local\Temp' \
///   cargo test -p devbox-cli --lib -j 2 \
///   windows_exec::probe_tests::the_holder_receipt_waits_for_an_orphan_that_outlives_the_bootstrap \
///   -- --ignored --exact
/// ```
///
/// `DEVBOX_TEST_WINDOWS_DIR` is an existing local folder of the tester with no reparse point on
/// its path (the bootstrap refuses one). The test creates its own `devbox-w1-<uuid>` folder in
/// it, plays the daemon's part (it grants exactly the `assigned` receipt within the bootstrap's
/// 15 s) and removes only that folder.
#[tokio::test]
#[ignore = "real Windows only: S7-14 W1 runs it under the windows-acceptance lock with DEVBOX_TEST_WINDOWS_POWERSHELL and DEVBOX_TEST_WINDOWS_DIR"]
async fn the_holder_receipt_waits_for_an_orphan_that_outlives_the_bootstrap() {
    let executable = std::env::var_os("DEVBOX_TEST_WINDOWS_POWERSHELL")
        .expect("explicit Windows PowerShell executable is required");
    let executable = Path::new(&executable);
    let parent =
        std::env::var("DEVBOX_TEST_WINDOWS_DIR").expect("an owned Windows folder is required");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let folder = |parent: &str, name: &str| {
        devbox_runtime::windows::target_directory(parent, name).expect("a Windows folder")
    };
    let wsl = |path: &str| devbox_runtime::windows::windows_to_wsl(path).expect("a mapped path");
    let owned = folder(&parent, &format!("devbox-w1-{id}"));
    let (directory, cwd) = (folder(&owned, "control"), folder(&owned, "files"));
    // create_dir never adopts an existing folder.
    let owned_wsl = wsl(&owned);
    std::fs::create_dir(&owned_wsl).expect("the owned test folder");
    std::fs::create_dir(wsl(&directory)).expect("the private control folder");
    std::fs::create_dir(wsl(&cwd)).expect("the command folder");
    let control = ObservationControl::new(format!("w1-{id}"), id.clone(), &directory);
    let assigned_path = wsl(&control.assigned_path);
    let grant_path = wsl(&control.grant_path);
    let ready_path = wsl(control.holder_ready_path.as_deref().expect("ready marker"));
    let receipt_path = wsl(control.emptied_path.as_deref().expect("receipt"));
    let request = RunWindows {
        run_id: "w1-run".into(),
        attempt_id: "w1-attempt".into(),
        cwd,
        command: ORPHAN.into(),
        env: Default::default(),
        ports: vec![],
        observation: control.clone(),
    };
    assert!(request.tracked());
    let daemon = async {
        appears(&assigned_path, Duration::from_secs(60))
            .await
            .ok_or("no `assigned` receipt within 60 s")?;
        // The bootstrap writes `assigned` only once its holder's marker exists.
        let ready_first = ready_path.exists();
        let bytes = std::fs::read(&assigned_path).map_err(|_| "unreadable `assigned`")?;
        let assigned: ObservationAssigned =
            serde_json::from_slice(&bytes).map_err(|_| "malformed `assigned`")?;
        let holder = std::fs::read(&ready_path).ok();
        let holder: Option<HolderIdentity> = holder.and_then(|b| serde_json::from_slice(&b).ok());
        let grant = serde_json::to_vec(&assigned).map_err(|_| "unencodable grant")?;
        publish(&grant_path, &grant).map_err(|_| "no grant via a temporary file and hard link")?;
        Ok::<_, &str>((ready_first, assigned, holder, Instant::now()))
    };
    let (started, _) = tokio::sync::oneshot::channel();
    let outcome = tokio::time::timeout(Duration::from_secs(120), async {
        tokio::join!(direct(&request, executable, started), daemon)
    })
    .await;
    let receipt_at_exit = receipt_path.exists();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let receipt_while_orphan = receipt_path.exists();
    let receipt_at = appears(&receipt_path, Duration::from_secs(30)).await;
    let receipt = std::fs::read(&receipt_path).ok();
    let receipt: Option<Receipt> = receipt.and_then(|b| serde_json::from_slice(&b).ok());
    // Only this test's folder. The orphan has ended once the receipt exists.
    let removed = std::fs::remove_dir_all(&owned_wsl);
    let (exit, seen) = outcome.expect("the bootstrap ends in time");
    let (ready_first, assigned, holder, granted) =
        seen.unwrap_or_else(|why| panic!("the daemon's part: {why}"));
    let run = RunIdentity {
        run_id: request.run_id.clone(),
        attempt_id: request.attempt_id.clone(),
    };
    // 1. The holder was ready first, and its marker names it the way the daemon checks it.
    assert!(ready_first, "the holder's ready marker before `assigned`");
    let holder = holder.expect("the holder identity in its ready marker");
    assert!(holder.belongs_to(&assigned), "{holder:?} for {assigned:?}");
    // 2. `assigned` is this attempt's own, and the command ran to its end.
    assert!(control.accepts(&run, &assigned), "{assigned:?}");
    assert_eq!(exit, Direct::Finished(0));
    // 3. No receipt while the orphan outlived the bootstrap; without it, one comes in 500 ms.
    assert!(!receipt_at_exit, "a receipt as the bootstrap exited");
    assert!(!receipt_while_orphan, "a receipt while the orphan ran");
    // 4. The receipt once the orphan ended by itself, at least its 5 s after the grant.
    let receipt_at = receipt_at.expect("the holder's receipt after the orphan");
    assert!(
        receipt_at.duration_since(granted) >= Duration::from_secs(4),
        "receipt {:?} after the grant",
        receipt_at.duration_since(granted)
    );
    assert_eq!(receipt.map(|r| r.nonce), Some(control.nonce.clone()));
    removed.expect("the owned test folder is removed");
}
