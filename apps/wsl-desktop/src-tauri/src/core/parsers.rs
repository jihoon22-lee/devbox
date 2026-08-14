use crate::core::models::{ContainerInfo, DistroInfo};

/// `wsl.exe -l -v` 출력 파싱.
/// 형식:
/// ```text
///   NAME      STATE           VERSION
/// * Ubuntu    Running         2
///   docker-desktop Running     2
/// ```
pub fn parse_wsl_list(input: &str) -> Vec<DistroInfo> {
    let mut out = Vec::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("NAME") || line.starts_with("Windows") {
            continue;
        }
        // 별표(*)는 기본 배포판 표시 (Windows 10에서 `*` 접두)
        let default = line.starts_with('*');
        let cleaned = line.trim_start_matches('*').trim();
        let mut parts = cleaned.split_whitespace();
        let name = parts.next().unwrap_or("").to_string();
        let _state = parts.next();
        let version = parts
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        if name.is_empty() {
            continue;
        }
        out.push(DistroInfo {
            name,
            version,
            default,
        });
    }
    out
}

/// `docker ps -a` 출력 파싱.
/// 형식:
/// ```text
/// CONTAINER ID   IMAGE     COMMAND   CREATED   STATUS   PORTS   NAMES
/// abc12345defg   postgres  ...       ...       Up 2h    5432/tcp pg
/// ```
pub fn parse_docker_ps(input: &str) -> Vec<ContainerInfo> {
    let mut out = Vec::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("CONTAINER ID") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let id = parts.next().unwrap_or("").to_string();
        let image = parts.next().unwrap_or("").to_string();
        // COMMAND는 공백 포함 가능 → 앞 2칸(id,image) 뒤에서 뒤 4칸(created,status,ports,names) 추출이 어려움.
        // 안정적으로: 마지막 3개 = status, ports, names? docker는 7개 컬럼이지만 COMMAND가 유동적.
        // 간단한 접근: 상태/이름만 앞에서부터 건너뛰기로 파싱하는 대신 전체 라인에서 status 패턴 검색.
        let status = extract_docker_status(line);
        let name = line.split_whitespace().last().unwrap_or("").to_string();
        let ports = extract_docker_ports(line);
        if id.is_empty() {
            continue;
        }
        out.push(ContainerInfo {
            id,
            name,
            image,
            status,
            ports,
        });
    }
    out
}

fn extract_docker_status(line: &str) -> String {
    // STATUS 컬럼은 "Up 2 hours", "Exited (0) 1 minute ago" 등. "Up"/"Exited"/"Created"/"Paused" 로 시작하는 토큰부터 끝까지.
    let tokens: Vec<&str> = line.split_whitespace().collect();
    for (i, t) in tokens.iter().enumerate() {
        if matches!(
            *t,
            "Up" | "Exited" | "Created" | "Paused" | "Restarting" | "Dead"
        ) {
            // status 뒤에 ports가 올 수도 있으니, name 직전 토큰까지로 제한 (대략 4단어)
            let end = (i + 5).min(tokens.len().saturating_sub(1));
            return tokens[i..end].join(" ");
        }
    }
    "Unknown".into()
}

fn extract_docker_ports(line: &str) -> String {
    // PORTS 컬럼은 "0.0.0.0:5432->5432/tcp, 0.0.0.0:8080->80/tcp" 형태. "-&gt;" 포함 토큰 수집.
    line.split_whitespace()
        .filter(|t| t.contains("->") || t.contains(':'))
        .filter(|t| !t.starts_with("Up") && !t.starts_with("Exited") && !t.starts_with("Created"))
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// wsl.exe 출력 디코딩.
///
/// 파이프로 실행된 `wsl.exe -l -v`는 UTF-16LE(BOM 또는 다량의 NUL)로 출력한다.
/// 이 경우 UTF-16LE로, 그 외에는 UTF-8로 해석한다. (distro 이름에 NUL이 남아
/// 다음 명령의 인자가 되면 "null byte found in provided data" 오류가 발생한다)
///
/// 병합 참고: wsl-dashboard와 wsl-desktop에 동일한 구현이 두 벌 있었다. 두 구현을
/// diff로 대조한 결과 로직이 완전히 같아(IDENTICAL) wsl-desktop의 한 벌만 남겼다.
pub fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let null_ratio = if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|b| **b == 0).count() * 4 / bytes.len()
    };
    if bom || (bytes.len() >= 2 && null_ratio >= 1) {
        let start = if bom { 2 } else { 0 };
        let units: Vec<u16> = bytes[start..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wsl_list() {
        let input = "  NAME      STATE           VERSION\n* Ubuntu    Running         2\n  docker-desktop Running         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 2);
        assert_eq!(distros[0].name, "Ubuntu");
        assert!(distros[0].default);
        assert_eq!(distros[0].version, 2);
        assert!(!distros[1].default);
    }

    #[test]
    fn parses_wsl_list_without_default_marker() {
        let input = "  NAME      STATE           VERSION\n  Ubuntu    Running         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 1);
        assert!(!distros[0].default);
    }

    #[test]
    fn parses_docker_ps() {
        let input = "CONTAINER ID   IMAGE     COMMAND                  CREATED       STATUS        PORTS                    NAMES\nabc123def456   postgres  \"docker-entrypoint.s…\"   2 hours ago   Up 2 hours    0.0.0.0:5432->5432/tcp   pg\nxyz789          redis     \"docker-entrypoint.s…\"   2 hours ago   Exited (0)    6379/tcp                 cache\n";
        let containers = parse_docker_ps(input);
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[0].id, "abc123def456");
        assert_eq!(containers[0].name, "pg");
        assert!(containers[0].status.contains("Up"));
        assert!(containers[1].status.contains("Exited"));
    }

    #[test]
    fn decodes_plain_utf8() {
        assert_eq!(decode_output(b"Ubuntu\n"), "Ubuntu\n");
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for ch in "Ubuntu".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let s = decode_output(&bytes);
        assert_eq!(s, "Ubuntu");
    }

    #[test]
    fn decodes_utf16le_without_bom() {
        let mut bytes = Vec::new();
        for ch in "docker-desktop".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let s = decode_output(&bytes);
        assert_eq!(s, "docker-desktop");
    }

    #[test]
    fn keeps_utf8_with_non_ascii() {
        let s = decode_output("프로토콜".as_bytes());
        assert!(s.contains("프로토콜"));
    }
}
