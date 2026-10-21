use crate::PortInfo;
use serde::Deserialize;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
struct Container {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Ports")]
    ports: String,
}
pub(crate) fn parse(text: &str) -> Option<Vec<PortInfo>> {
    let mut result = vec![];
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        let container: Container = serde_json::from_str(line).ok()?;
        if container.id.len() > 256 {
            return None;
        }
        for mapping in container.ports.split(',').map(str::trim) {
            // Exposed but unpublished container ports have no host endpoint.
            let Some((host, target)) = mapping.split_once("->") else {
                continue;
            };
            let (addr, port) = host.rsplit_once(':')?;
            let addr = addr.trim_matches(['[', ']']);
            addr.parse::<std::net::IpAddr>().ok()?;
            let (_, proto) = target.rsplit_once('/')?;
            if !matches!(proto, "tcp" | "udp") {
                continue;
            }
            let (first, last) = match port.split_once('-') {
                Some((a, b)) => (a.parse::<u16>().ok()?, b.parse::<u16>().ok()?),
                None => {
                    let port = port.parse::<u16>().ok()?;
                    (port, port)
                }
            };
            if first > last || u32::from(last) - u32::from(first) > 4096 {
                return None;
            }
            for port in first..=last {
                result.push(PortInfo {
                    port,
                    proto: proto.into(),
                    addr: addr.into(),
                    pid: None,
                    start_time_ms: None,
                    process: None,
                    run_id: None,
                    container_id: Some(container.id.clone()),
                    project_id: None,
                });
                if result.len() > 4096 {
                    return None;
                }
            }
        }
    }
    Some(result)
}
pub(crate) async fn snapshot(program: &Path) -> Vec<PortInfo> {
    let Ok(mut child) = tokio::process::Command::new(program)
        .args(["ps", "--format", "json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return vec![];
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        return vec![];
    };
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut bytes = vec![];
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() > 1024 * 1024 {
            return None;
        }
        let status = child.wait().await.ok()?;
        if !status.success() {
            return None;
        }
        parse(std::str::from_utf8(&bytes).ok()?)
    })
    .await
    .ok()
    .flatten();
    // timeout, oversized output and parse failures do not hide host sockets.
    let _ = child.kill().await;
    let _ = child.wait().await;
    result.unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn docker_ports_json_published_ipv4_ipv6_and_ranges() {
        let rows = parse(r#"{"ID":"abc","Ports":"0.0.0.0:8080->80/tcp, [::]:8081->80/tcp, 127.0.0.1:9000-9002->9000-9002/udp, 80/tcp"}"#).expect("fixture value");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].addr, "::");
        assert_eq!(rows[4].port, 9002);
        assert!(
            rows.iter()
                .all(|r| r.container_id.as_deref() == Some("abc"))
        );
        assert!(parse(r#"{"ID":"abc","Ports":"0.0.0.0:1-65535->1-65535/tcp"}"#).is_none());
    }
    #[tokio::test]
    async fn docker_ports_fake_cli_checks_argv_and_unavailable() {
        let dir = tempfile::tempdir().expect("fixture result");
        let script = dir.path().join("docker");
        std::fs::write(&script, "#!/bin/sh\n[ \"$*\" = 'ps --format json' ] || exit 9\nprintf '%s\\n' '{\"ID\":\"fake\",\"Ports\":\"0.0.0.0:4321->80/tcp\"}'\n").expect("fixture result");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("fixture result");
        assert_eq!(snapshot(&script).await[0].port, 4321);
        assert!(snapshot(&dir.path().join("missing")).await.is_empty());
    }
}
