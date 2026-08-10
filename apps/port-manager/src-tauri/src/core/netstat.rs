use crate::core::models::PortInfo;

/// `netstat -ano` (Windows) 출력을 파싱한다.
///
/// 파싱만 담당하는 순수 함수라서 OS 의존성이 없어 어느 환경에서든 유닛 테스트할 수 있다.
/// 실제 실행은 src-tauri 쪽 command에서 `netstat -ano`를 호출하고 이 함수에 넘긴다.
///
/// 예상 형식 (헤더/빈 줄은 건너뜀):
/// ```text
/// Active Connections
///
///   Proto  Local Address          Foreign Address        State           PID
///   TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1168
///   TCP    [::1]:3000             [::]:0                 LISTENING       18231
///   UDP    0.0.0.0:5353           *:*                                    1829
///   TCP    10.0.0.5:50261         52.142.120.23:443      ESTABLISHED     1044
/// ```
pub fn parse_netstat_output(input: &str) -> Vec<PortInfo> {
    let mut out = Vec::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("Active")
            || line.starts_with("Proto")
            || line.starts_with('[')
        {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        let proto = fields[0].to_string();
        if !proto.starts_with("TCP") && !proto.starts_with("UDP") {
            continue;
        }

        let local_addr = fields[1].to_string();
        let state = if proto.starts_with("TCP") {
            // TCP: Proto Local Foreign State PID (5필드)
            fields.get(3).map(|s| s.to_string()).unwrap_or_default()
        } else {
            // UDP: Proto Local Foreign PID (4필드, 상태 없음)
            String::new()
        };
        let pid = fields.last().and_then(|s| s.parse::<u32>().ok());

        out.push(PortInfo {
            port: extract_port(&local_addr),
            proto,
            local_addr,
            state,
            pid,
        });
    }
    out
}

/// 주소 문자열에서 포트 번호를 추출한다.
/// - `0.0.0.0:3000` → 3000
/// - `[::1]:3000` → 3000
/// - `*:*` → 0 (추출 불가)
pub fn extract_port(addr: &str) -> u16 {
    let port_str = if let Some(close) = addr.find(']') {
        // IPv6: "[...]:port" 에서 ']' 뒤의 ":port"만 취한다
        addr[close + 1..].trim_start_matches(':')
    } else {
        match addr.rfind(':') {
            Some(idx) => &addr[idx + 1..],
            None => "",
        }
    };
    port_str.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1168
  TCP    127.0.0.1:3000         0.0.0.0:0              LISTENING       18231
  TCP    [::1]:5173             [::]:0                 LISTENING       21324
  TCP    10.0.0.5:50261         52.142.120.23:443      ESTABLISHED     1044
  UDP    0.0.0.0:5353           *:*                                    1829
  UDP    [::]:5353              *:*                                    1829
"#;

    #[test]
    fn parses_tcp_rows() {
        let rows = parse_netstat_output(SAMPLE);
        let tcp = rows
            .iter()
            .filter(|r| r.proto.starts_with("TCP"))
            .collect::<Vec<_>>();
        assert_eq!(tcp.len(), 4);
    }

    #[test]
    fn parses_ports() {
        let rows = parse_netstat_output(SAMPLE);
        assert_eq!(rows[1].port, 3000);
        assert_eq!(rows[2].port, 5173);
        assert_eq!(rows[3].port, 50261);
    }

    #[test]
    fn parses_state_and_pid_for_tcp() {
        let rows = parse_netstat_output(SAMPLE);
        assert_eq!(rows[0].state, "LISTENING");
        assert_eq!(rows[0].pid, Some(1168));
        assert_eq!(rows[3].state, "ESTABLISHED");
        assert_eq!(rows[3].pid, Some(1044));
    }

    #[test]
    fn udp_has_empty_state_and_pid() {
        let rows = parse_netstat_output(SAMPLE);
        let udp = rows
            .iter()
            .filter(|r| r.proto.starts_with("UDP"))
            .collect::<Vec<_>>();
        assert_eq!(udp.len(), 2);
        assert_eq!(udp[0].state, "");
        assert_eq!(udp[0].pid, Some(1829));
        assert_eq!(udp[0].port, 5353);
    }

    #[test]
    fn skips_header_and_empty_lines() {
        assert_eq!(
            parse_netstat_output(
                "Active Connections\n\n  Proto  Local Address...\n  TCP    0.0.0.0:1  0.0.0.0:0  LISTENING  1\n"
            )
            .len(),
            1
        );
        assert!(parse_netstat_output("").is_empty());
    }

    #[test]
    fn extract_port_handles_ipv4_ipv6_and_star() {
        assert_eq!(extract_port("0.0.0.0:135"), 135);
        assert_eq!(extract_port("127.0.0.1:3000"), 3000);
        assert_eq!(extract_port("[::1]:5173"), 5173);
        assert_eq!(extract_port("[::]:5353"), 5353);
        assert_eq!(extract_port("*:*"), 0);
    }
}
