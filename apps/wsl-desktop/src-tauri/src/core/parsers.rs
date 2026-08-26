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
        // NAME can contain spaces. STATE and VERSION are the two rightmost
        // columns, so consume those from the end and join the remainder.
        let version = parts
            .next_back()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let state = parts.next_back().unwrap_or("").to_string();
        let name = parts.collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            continue;
        }
        out.push(DistroInfo {
            name,
            version,
            default,
            state,
        });
    }
    out
}

/// `docker ps -a --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}'`
/// 출력 파싱.
/// 형식:
/// ```text
/// abc12345defg\tpg\tpostgres:16\tUp 2 hours\t0.0.0.0:5432->5432/tcp
/// ```
pub fn parse_docker_ps(input: &str) -> Result<Vec<ContainerInfo>, &'static str> {
    let mut containers = Vec::new();
    for line in input.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 5
            || fields[0].is_empty()
            || fields[1].is_empty()
            || fields[2].is_empty()
            || fields[3].is_empty()
        {
            // frontend의 기존 설치 안내 분류는 실행 파일 이름도 검사한다. 이 고정 오류에는
            // source field나 실행 파일 이름을 넣지 않아 parser 손상을 부재로 오인하지 않는다.
            return Err("컨테이너 목록 형식이 올바르지 않습니다.");
        }

        containers.push(ContainerInfo {
            id: fields[0].to_string(),
            name: fields[1].to_string(),
            image: fields[2].to_string(),
            status: fields[3].to_string(),
            ports: fields[4].to_string(),
        });
    }
    Ok(containers)
}

/// `wsl.exe` 출력 디코딩 — 공용 `crates/wsl`로 추출됨 (두 번째 소비자: devbox-manager).
pub use devbox_wsl::output::decode_output;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wsl_list() {
        let input = "  NAME      STATE           VERSION\n* Ubuntu    Running         2\n  docker-desktop Stopped         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 2);
        assert_eq!(distros[0].name, "Ubuntu");
        assert!(distros[0].default);
        assert_eq!(distros[0].version, 2);
        assert_eq!(distros[0].state, "Running");
        assert!(!distros[1].default);
        assert_eq!(distros[1].state, "Stopped");
    }

    #[test]
    fn parses_wsl_list_without_default_marker() {
        let input = "  NAME      STATE           VERSION\n  Ubuntu    Running         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 1);
        assert!(!distros[0].default);
        assert_eq!(distros[0].state, "Running");
    }

    #[test]
    fn parses_wsl_list_stopped_distro_state() {
        let input = "  NAME      STATE           VERSION\n  Ubuntu    Stopped         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 1);
        assert_eq!(distros[0].state, "Stopped");
    }

    #[test]
    fn parses_wsl_list_with_spaces_in_distro_name() {
        let input =
            "  NAME             STATE           VERSION\n  Ubuntu 24.04     Running         2\n";
        let distros = parse_wsl_list(input);
        assert_eq!(distros.len(), 1);
        assert_eq!(distros[0].name, "Ubuntu 24.04");
        assert_eq!(distros[0].state, "Running");
        assert_eq!(distros[0].version, 2);
    }

    #[test]
    fn parses_docker_ps() {
        let input = "abc123def456\tpg\tpostgres:16\tUp 2 hours\t0.0.0.0:5432->5432/tcp\nxyz789\tcache\tredis:7\tExited (0) 1 minute ago\t6379/tcp\n";
        let containers = parse_docker_ps(input).unwrap();
        assert_eq!(containers.len(), 2);
        assert_eq!(containers[0].id, "abc123def456");
        assert_eq!(containers[0].name, "pg");
        assert_eq!(containers[0].image, "postgres:16");
        assert_eq!(containers[0].status, "Up 2 hours");
        assert_eq!(containers[0].ports, "0.0.0.0:5432->5432/tcp");
        assert_eq!(containers[1].status, "Exited (0) 1 minute ago");
        assert_eq!(containers[1].ports, "6379/tcp");
    }

    #[test]
    fn preserves_empty_ports_and_crlf() {
        let containers = parse_docker_ps("abc123\tworker\tjobs:latest\tCreated\t\r\n").unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].status, "Created");
        assert!(containers[0].ports.is_empty());
    }

    #[test]
    fn rejects_malformed_rows_without_reflecting_source_content() {
        for input in [
            "not docker output\n",
            "\tmissing-id\timage\tUp 1 minute\t80/tcp\n",
            "abc123\tname\t\tUp 1 minute\t80/tcp\n",
            "abc123\tname\timage\t\t80/tcp\n",
            "abc123\tname\timage\tDead\t\textra\n",
        ] {
            assert_eq!(
                parse_docker_ps(input).unwrap_err(),
                "컨테이너 목록 형식이 올바르지 않습니다."
            );
        }
    }

    #[test]
    fn accepts_an_empty_container_list() {
        assert!(parse_docker_ps("").unwrap().is_empty());
        assert!(parse_docker_ps("\r\n").unwrap().is_empty());
    }
}
