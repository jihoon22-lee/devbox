use crate::{
    host::{pids, read_text},
    processes::{parse_stat, project_for},
    *,
};
use std::{
    collections::HashMap,
    fs,
    net::{Ipv4Addr, Ipv6Addr},
    path::Path,
};

pub(crate) struct TcpRow {
    pub port: u16,
    pub addr: String,
    pub inode: u64,
}
pub(crate) fn parse_net_tcp(text: &str, ipv6: bool) -> Vec<TcpRow> {
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.get(3)? != &"0A" {
                return None;
            }
            let (addr, port) = fields.get(1)?.split_once(':')?;
            let port = u16::from_str_radix(port, 16).ok()?;
            let addr = if ipv6 {
                if addr.len() != 32 || !addr.is_ascii() {
                    return None;
                }
                let mut bytes = [0; 16];
                for i in 0..4 {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(
                        &u32::from_str_radix(&addr[i * 8..i * 8 + 8], 16)
                            .ok()?
                            .to_ne_bytes(),
                    );
                }
                Ipv6Addr::from(bytes).to_string()
            } else {
                if addr.len() != 8 {
                    return None;
                }
                Ipv4Addr::from(u32::from_str_radix(addr, 16).ok()?.to_ne_bytes()).to_string()
            };
            Some(TcpRow {
                port,
                addr,
                inode: fields.get(9)?.parse().ok()?,
            })
        })
        .collect()
}
fn tcp_rows(root: &Path) -> R<Vec<TcpRow>> {
    let mut result = vec![];
    let mut available = false;
    for (file, ipv6) in [("tcp", false), ("tcp6", true)] {
        match read_text(&root.join("net").join(file), 4 * 1024 * 1024) {
            Ok(text) => {
                available = true;
                result.extend(parse_net_tcp(&text, ipv6));
            }
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                return Err(ObserveError::LimitExceeded.into());
            }
            Err(_) => (),
        }
    }
    if !available {
        return Err(ObserveError::SourceUnavailable.into());
    }
    Ok(result)
}
pub fn is_listening(port: u16) -> bool {
    is_listening_in(Path::new("/proc"), port)
}
pub fn is_listening_in(root: &Path, port: u16) -> bool {
    ports_listening_in(root, &[port])
}
/// One proc-net snapshot for all required ports. No connections or PID scans.
pub fn ports_listening_in(root: &Path, ports: &[u16]) -> bool {
    ports.is_empty()
        || tcp_rows(root).is_ok_and(|rows| {
            ports
                .iter()
                .all(|port| rows.iter().any(|row| row.port == *port))
        })
}
pub(crate) fn unit_from_cgroup(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.splitn(3, ':').nth(2))
        .find_map(|path| {
            path.rsplit('/')
                .find(|part| part.ends_with(".service") || part.ends_with(".scope"))
        })
        .map(str::to_owned)
}
pub(crate) fn socket_inode_to_pid(root: &Path, wanted: &[u64]) -> R<HashMap<u64, u32>> {
    let mut result = HashMap::new();
    let wanted: std::collections::HashSet<_> = wanted.iter().copied().collect();
    let mut scanned = 0;
    for pid in pids(root)? {
        let Ok(entries) = fs::read_dir(root.join(pid.to_string()).join("fd")) else {
            continue;
        };
        for entry in entries {
            scanned += 1;
            if scanned > 262_144 {
                return Err(ObserveError::LimitExceeded.into());
            }
            let Ok(entry) = entry else { continue };
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            let Some(inode) = target
                .to_str()
                .and_then(|s| s.strip_prefix("socket:["))
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            if wanted.contains(&inode) {
                result.entry(inode).or_insert(pid);
            }
        }
    }
    Ok(result)
}
pub(crate) fn ports_snapshot(
    root: &Path,
    projects: &[ProjectRoot],
    runs: &[UnitRun],
) -> R<Vec<PortInfo>> {
    let rows = tcp_rows(root)?;
    let pids = socket_inode_to_pid(root, &rows.iter().map(|r| r.inode).collect::<Vec<_>>())?;
    let mut result = vec![];
    for row in rows {
        let pid = pids.get(&row.inode).copied();
        let dir = pid.map(|pid| root.join(pid.to_string()));
        let process = dir
            .as_ref()
            .and_then(|p| read_text(&p.join("stat"), 65536).ok())
            .and_then(|s| parse_stat(&s))
            .map(|s| s.name);
        let unit = dir
            .as_ref()
            .and_then(|p| read_text(&p.join("cgroup"), 65536).ok())
            .and_then(|s| unit_from_cgroup(&s));
        let run = runs
            .iter()
            .find(|r| Some(r.unit.as_str()) == unit.as_deref());
        let cwd = dir.as_ref().and_then(|p| fs::read_link(p.join("cwd")).ok());
        let project_id = run
            .map(|r| r.project_id.clone())
            .or_else(|| project_for(cwd.as_deref(), projects).map(str::to_owned));
        result.push(PortInfo {
            port: row.port,
            proto: "tcp".into(),
            addr: row.addr,
            pid,
            process,
            run_id: run.map(|r| r.run_id.clone()),
            container_id: None,
            project_id,
        });
    }
    Ok(result)
}
