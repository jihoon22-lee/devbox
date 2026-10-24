use std::net::{IpAddr, Ipv4Addr};
pub fn bind_address(lan: bool) -> IpAddr {
    IpAddr::V4(if lan {
        Ipv4Addr::UNSPECIFIED
    } else {
        Ipv4Addr::LOCALHOST
    })
}
/// Read Linux's local routing table. Never create a socket or change networking.
pub fn addresses(port: u16, lan: bool) -> Vec<String> {
    let mut ips = vec![Ipv4Addr::LOCALHOST];
    if lan {
        if let Ok(table) = std::fs::read_to_string("/proc/net/fib_trie") {
            let lines: Vec<_> = table.lines().collect();
            for pair in lines.windows(2) {
                if pair[1].contains("/32 host LOCAL") {
                    if let Some(ip) = pair[0]
                        .split_whitespace()
                        .last()
                        .and_then(|s| s.parse::<Ipv4Addr>().ok())
                    {
                        if !ip.is_loopback() && !ip.is_unspecified() {
                            ips.push(ip);
                        }
                    }
                }
            }
        }
    }
    ips.sort();
    ips.dedup();
    ips.into_iter().map(|ip| format!("{ip}:{port}")).collect()
}
