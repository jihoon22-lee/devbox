use crate::{PortInfo, ProjectRoot};
use serde::Deserialize;
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncReadExt;

#[derive(Deserialize)]
struct Container {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Ports")]
    ports: String,
    #[serde(
        rename = "Labels",
        default,
        deserialize_with = "devbox_core::project_root::docker_labels"
    )]
    labels: String,
}
pub(crate) fn parse(text: &str, projects: &[ProjectRoot]) -> Option<Vec<PortInfo>> {
    let mut result = vec![];
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        let container: Container = serde_json::from_str(line).ok()?;
        if container.id.len() > 256 {
            return None;
        }
        let project_id = devbox_core::project_root::compose_project(&container.labels, projects);
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
                    project_id: project_id.map(str::to_owned),
                });
                if result.len() > 4096 {
                    return None;
                }
            }
        }
    }
    Some(result)
}
/// A host listener on a published endpoint takes that container's project unless it has one.
pub(crate) fn inherit_projects(host: &mut [PortInfo], published: &[PortInfo]) {
    for row in host.iter_mut().filter(|row| row.project_id.is_none()) {
        row.project_id = published
            .iter()
            .find(|p| (p.port, &p.proto, &p.addr) == (row.port, &row.proto, &row.addr))
            .and_then(|p| p.project_id.clone());
    }
}
pub(crate) async fn snapshot(program: &Path, projects: &[ProjectRoot]) -> Vec<PortInfo> {
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
        parse(std::str::from_utf8(&bytes).ok()?, projects)
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
    // The column set of one `docker ps --format json` line for a Compose service.
    const COMPOSE_ROW: &str = r#"{"Command":"\"/docker-entrypoint.…\"","CreatedAt":"2026-01-01 00:00:00 +0000 UTC","ID":"0123456789ab","Image":"nginx:alpine","Labels":"com.docker.compose.config-hash=0f0f,com.docker.compose.container-number=1,com.docker.compose.depends_on=db:service_started:false,cache:service_started:false,com.docker.compose.oneoff=False,com.docker.compose.project=app,com.docker.compose.project.config_files=/home/u/projects/app/compose.yaml,com.docker.compose.project.working_dir=/home/u/projects/app,com.docker.compose.service=web,com.docker.compose.version=2.29.0","LocalVolumes":"0","Mounts":"","Names":"app-web-1","Networks":"app_default","Ports":"0.0.0.0:8080->80/tcp, [::]:8080->80/tcp","RunningFor":"2 hours ago","Size":"0B","State":"running","Status":"Up 2 hours"}"#;
    fn roots() -> Vec<ProjectRoot> {
        vec![ProjectRoot {
            id: "app".into(),
            path: "/home/u/projects/app".into(),
        }]
    }
    fn host(addr: &str, port: u16, project_id: Option<&str>) -> PortInfo {
        PortInfo {
            port,
            proto: "tcp".into(),
            addr: addr.into(),
            pid: Some(7),
            process: Some("docker-proxy".into()),
            start_time_ms: Some(1),
            run_id: None,
            container_id: None,
            project_id: project_id.map(str::to_owned),
        }
    }
    #[test]
    fn docker_ports_json_published_ipv4_ipv6_and_ranges() {
        let rows = parse(r#"{"ID":"abc","Ports":"0.0.0.0:8080->80/tcp, [::]:8081->80/tcp, 127.0.0.1:9000-9002->9000-9002/udp, 80/tcp"}"#, &[]).expect("fixture value");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[1].addr, "::");
        assert_eq!(rows[4].port, 9002);
        assert!(
            rows.iter()
                .all(|r| r.container_id.as_deref() == Some("abc") && r.project_id.is_none())
        );
        assert!(
            parse(
                r#"{"ID":"abc","Ports":"0.0.0.0:1-65535->1-65535/tcp"}"#,
                &[]
            )
            .is_none()
        );
    }
    #[test]
    fn docker_ports_take_the_project_of_the_compose_working_dir_label() {
        let rows = parse(COMPOSE_ROW, &roots()).expect("fixture value");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.project_id.as_deref() == Some("app")));
        let sibling = COMPOSE_ROW.replace(
            "working_dir=/home/u/projects/app,",
            "working_dir=/home/u/projects/app2,",
        );
        let rows = parse(&sibling, &roots()).expect("fixture value");
        assert!(rows.iter().all(|r| r.project_id.is_none()));
        // Labels that are not a string cost the attribution, never the published ports.
        for odd in ["null", "7", r#"["a=b"]"#] {
            let row = format!(r#"{{"ID":"abc","Labels":{odd},"Ports":"0.0.0.0:8080->80/tcp"}}"#);
            let rows = parse(&row, &roots()).expect("fixture value");
            assert_eq!((rows.len(), rows[0].project_id.as_deref()), (1, None));
        }
    }
    #[test]
    fn host_port_inherits_the_container_project_without_overriding_its_own() {
        let published = parse(COMPOSE_ROW, &roots()).expect("fixture value");
        let mut rows = vec![
            host("0.0.0.0", 8080, None),
            host("::", 8080, Some("run-project")),
            host("127.0.0.1", 8080, None),
            host("0.0.0.0", 8081, None),
        ];
        inherit_projects(&mut rows, &published);
        assert_eq!(rows[0].project_id.as_deref(), Some("app"));
        assert_eq!(
            rows[1].project_id.as_deref(),
            Some("run-project"),
            "a Devbox run or cwd attribution keeps priority"
        );
        assert_eq!(
            rows[2].project_id, None,
            "another address is another listener"
        );
        assert_eq!(rows[3].project_id, None, "another port");
        assert!(
            rows.iter().all(|r| r.container_id.is_none()),
            "a host process stays a host process"
        );
        let mut udp = vec![PortInfo {
            proto: "udp".into(),
            ..host("0.0.0.0", 8080, None)
        }];
        inherit_projects(&mut udp, &published);
        assert_eq!(udp[0].project_id, None, "protocols are not mixed");
    }
    #[tokio::test]
    async fn docker_ports_fake_cli_checks_argv_and_unavailable() {
        let dir = tempfile::tempdir().expect("fixture result");
        let script = dir.path().join("docker");
        std::fs::write(&script, "#!/bin/sh\n[ \"$*\" = 'ps --format json' ] || exit 9\nprintf '%s\\n' '{\"ID\":\"fake\",\"Labels\":\"com.docker.compose.project.working_dir=/home/u/projects/app/deploy\",\"Ports\":\"0.0.0.0:4321->80/tcp\"}'\n").expect("fixture result");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
            .expect("fixture result");
        let rows = snapshot(&script, &roots()).await;
        assert_eq!(rows[0].port, 4321);
        assert_eq!(rows[0].project_id.as_deref(), Some("app"));
        assert!(
            snapshot(&dir.path().join("missing"), &roots())
                .await
                .is_empty()
        );
    }
}
