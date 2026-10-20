use crate::{
    host::{ProcFs, pids, read_text},
    processes::{Stat, parse_stat, project_for},
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
/// All hinted ports must belong to the launched PID or a process in its unit.
/// A host listener alone is insufficient, including when /proc ownership is unreadable.
pub fn run_ports_listening_in(root: &Path, ports: &[u16], unit: &str, pid: Option<u32>) -> bool {
    if ports.is_empty() {
        return true;
    }
    let Ok(rows) = tcp_rows(root) else {
        return false;
    };
    let wanted: Vec<_> = rows
        .iter()
        .filter(|row| ports.contains(&row.port))
        .map(|row| row.inode)
        .collect();
    let Ok(owners) = socket_inode_to_pid_matching(root, &wanted, |candidate| {
        pid == Some(candidate)
            || (!unit.is_empty()
                && read_text(&root.join(candidate.to_string()).join("cgroup"), 65536).is_ok_and(
                    |text| {
                        text.lines()
                            .filter_map(|line| line.splitn(3, ':').nth(2))
                            .any(|path| path.split('/').any(|part| part == unit))
                    },
                ))
    }) else {
        return false;
    };
    ports.iter().all(|port| {
        rows.iter().any(|row| {
            row.port == *port
                && owners
                    .get(&row.inode)
                    .is_some_and(|(_, stat)| stat.is_some())
        })
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
pub(crate) fn socket_inode_to_pid(
    root: &Path,
    wanted: &[u64],
) -> R<HashMap<u64, (u32, Option<Stat>)>> {
    socket_inode_to_pid_matching(root, wanted, |_| true)
}
fn socket_inode_to_pid_matching(
    root: &Path,
    wanted: &[u64],
    matches: impl Fn(u32) -> bool,
) -> R<HashMap<u64, (u32, Option<Stat>)>> {
    let mut result = HashMap::new();
    let wanted: std::collections::HashSet<_> = wanted.iter().copied().collect();
    let mut scanned = 0;
    for pid in pids(root)? {
        if !matches(pid) {
            continue;
        }
        let stat_path = root.join(pid.to_string()).join("stat");
        let read_stat = || {
            read_text(&stat_path, 65536)
                .ok()
                .and_then(|s| parse_stat(&s))
        };
        let before = read_stat();
        let mut owned = vec![];
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
                owned.push(inode);
            }
        }
        let after = read_stat();
        let verified = before.filter(|before| {
            after
                .as_ref()
                .is_some_and(|after| before.start_ticks == after.start_ticks)
        });
        for inode in owned {
            result
                .entry(inode)
                .or_insert_with(|| (pid, verified.clone()));
        }
    }
    Ok(result)
}
pub(crate) fn ports_snapshot(
    proc: &ProcFs,
    projects: &[ProjectRoot],
    runs: &[UnitRun],
) -> R<Vec<PortInfo>> {
    let root = &proc.root;
    let boot = proc.boot_ms().ok();
    let rows = tcp_rows(root)?;
    let pids = socket_inode_to_pid(root, &rows.iter().map(|r| r.inode).collect::<Vec<_>>())?;
    let mut result = vec![];
    for row in rows {
        let owner = pids.get(&row.inode);
        let pid = owner.map(|(pid, _)| *pid);
        let stat = owner.and_then(|(_, stat)| stat.as_ref());
        let dir = pid.map(|pid| root.join(pid.to_string()));
        let process = stat.map(|stat| stat.name.clone());
        let start_time_ms =
            stat.and_then(|stat| boot.map(|boot| proc.start_ms(boot, stat.start_ticks)));
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
            start_time_ms,
            run_id: run.map(|r| r.run_id.clone()),
            container_id: None,
            project_id,
        });
    }
    Ok(result)
}
