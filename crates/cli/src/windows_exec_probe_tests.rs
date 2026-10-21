//! Opt-in Windows-local socket regression; normal Linux tests use no Windows host resources.
use super::*;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
#[ignore = "requires DEVBOX_TEST_WINDOWS_POWERSHELL pointing to Windows PowerShell via WSL interop"]
async fn windows_direct_probe_owned_ipv6_ipv4_and_all_required_ports() {
    let executable = std::env::var_os("DEVBOX_TEST_WINDOWS_POWERSHELL")
        .expect("explicit Windows PowerShell executable is required");
    let executable = Path::new(&executable);
    // Each listener belongs to this child and is released on stdin EOF or child exit.
    let fixture = r#"
$ErrorActionPreference='Stop'
$v4=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
$v6=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::IPv6Loopback,0)
$v6.Server.DualMode=$false
$closed=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
try {
    $v4.Start(); $v6.Start(); $closed.Start()
    $ports=@($v4.LocalEndpoint.Port,$v6.LocalEndpoint.Port,$closed.LocalEndpoint.Port)
    $closed.Stop()
    [Console]::Out.WriteLine(($ports | ConvertTo-Json -Compress))
    [Console]::Out.Flush()
    [Console]::In.ReadToEnd() | Out-Null
} finally { $v4.Stop(); $v6.Stop(); $closed.Stop() }
"#;
    let mut child = Command::new(executable)
        .args(["-NoProfile", "-NonInteractive", "-Command", fixture])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .expect("fixture result");
    let mut output = BufReader::new(child.stdout.take().expect("fixture value"));
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), output.read_line(&mut line))
        .await
        .expect("fixture result")
        .expect("fixture result");
    let ports: Vec<u16> = serde_json::from_str(line.trim()).expect("fixture result");
    eprintln!("owned Windows listener ports: {ports:?}");
    let ipv6 = probe(executable, &[ports[1]]).await;
    let ipv4 = probe(executable, &[ports[0]]).await;
    let both = probe(executable, &ports[..2]).await;
    let closed = probe(executable, &[ports[2]]).await;
    let partly_closed = probe(executable, &ports).await;
    // Release and reap the owned Windows fixture even when a readiness assertion fails.
    drop(child.stdin.take());
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .expect("fixture result")
            .expect("fixture result")
            .success()
    );
    assert!(ipv6, "IPv6-only listener");
    assert!(ipv4, "IPv4-only listener");
    assert!(both, "both required ports");
    assert!(!closed, "closed port");
    assert!(!partly_closed, "one required port closed");
}
