//! Opt-in Windows-local listener ownership regression; normal Linux tests use no Windows host resources.
use super::*;
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
$job=[DevboxObservation]::AssociateFresh('{job}')
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
    // Release and reap every owned fixture even when an assertion below fails.
    release(root).await;
    let exited = check(vec![v4]).await;
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
        "exited tree: {exited:?}"
    );
}
