use crate::core::models::{ContainerInfo, DistroInfo};

pub const MAX_DASHBOARD_DISTROS: usize = 64;
pub const MAX_DASHBOARD_LIST_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_DASHBOARD_OUTPUT_LINE_BYTES: usize = 16 * 1024;
pub const MAX_DASHBOARD_DOCKER_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DASHBOARD_CONTAINERS: usize = 256;
pub const MAX_CONTAINER_FIELD_BYTES: usize = 16 * 1024;
const WSL_LIST_ERROR: &str = "WSL 배포판 목록 형식이 올바르지 않습니다.";

/// Strict parser used by every `wsl.exe -l -v` dashboard path. A malformed row must not become a
/// misleading empty or stopped snapshot.
pub fn parse_wsl_list_checked(input: &str) -> Result<Vec<DistroInfo>, &'static str> {
    if input.len() > MAX_DASHBOARD_LIST_OUTPUT_BYTES {
        return Err(WSL_LIST_ERROR);
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw_line in input.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.len() > MAX_DASHBOARD_OUTPUT_LINE_BYTES {
            return Err(WSL_LIST_ERROR);
        }
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with("NAME") || line.starts_with("Windows") {
            continue;
        }
        let default = line.starts_with('*');
        let cleaned = line.trim_start_matches('*').trim();
        let mut parts = cleaned.split_whitespace();
        let version = parts
            .next_back()
            .ok_or(WSL_LIST_ERROR)?
            .parse::<u32>()
            .map_err(|_| WSL_LIST_ERROR)?;
        let state = parts.next_back().ok_or(WSL_LIST_ERROR)?.to_owned();
        let name = parts.collect::<Vec<_>>().join(" ");
        if name.is_empty()
            || name.len() > crate::core::runtime_snapshot::MAX_DISTRO_NAME_BYTES
            || state.is_empty()
            || !matches!(state.to_ascii_lowercase().as_str(), "running" | "stopped")
            || !devbox_wsl::distro::validate_distro_name(&name).is_ok()
            || !seen.insert(name.clone())
        {
            return Err(WSL_LIST_ERROR);
        }
        if out.len() >= MAX_DASHBOARD_DISTROS {
            return Err(WSL_LIST_ERROR);
        }
        out.push(DistroInfo {
            name,
            version,
            default,
            state,
        });
    }
    if out.iter().filter(|distro| distro.default).count() > 1 {
        return Err(WSL_LIST_ERROR);
    }
    Ok(out)
}

/// `docker ps -a --format '{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}'`
/// 출력 파싱.
/// 형식:
/// ```text
/// abc12345defg\tpg\tpostgres:16\tUp 2 hours\t0.0.0.0:5432->5432/tcp
/// ```
pub fn parse_docker_ps(input: &str) -> Result<Vec<ContainerInfo>, &'static str> {
    if input.len() > MAX_DASHBOARD_DOCKER_OUTPUT_BYTES {
        return Err("컨테이너 목록 형식이 올바르지 않습니다.");
    }
    let mut containers = Vec::new();
    for line in input.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_DASHBOARD_OUTPUT_LINE_BYTES {
            return Err("컨테이너 목록 형식이 올바르지 않습니다.");
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
        if fields.iter().any(|field| {
            field.len() > MAX_CONTAINER_FIELD_BYTES || field.chars().any(char::is_control)
        }) {
            return Err("컨테이너 목록 형식이 올바르지 않습니다.");
        }

        containers.push(ContainerInfo {
            id: fields[0].to_string(),
            name: fields[1].to_string(),
            image: fields[2].to_string(),
            status: fields[3].to_string(),
            ports: fields[4].to_string(),
        });
        if containers.len() > MAX_DASHBOARD_CONTAINERS {
            return Err("컨테이너 목록 형식이 올바르지 않습니다.");
        }
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
        let distros = parse_wsl_list_checked(input).unwrap();
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
        let distros = parse_wsl_list_checked(input).unwrap();
        assert_eq!(distros.len(), 1);
        assert!(!distros[0].default);
        assert_eq!(distros[0].state, "Running");
    }

    #[test]
    fn parses_wsl_list_stopped_distro_state() {
        let input = "  NAME      STATE           VERSION\n  Ubuntu    Stopped         2\n";
        let distros = parse_wsl_list_checked(input).unwrap();
        assert_eq!(distros.len(), 1);
        assert_eq!(distros[0].state, "Stopped");
    }

    #[test]
    fn checked_wsl_list_is_bounded_and_preserves_complete_rows() {
        let input = "\u{feff}  NAME      STATE           VERSION\r\n* Ubuntu 24.04 Running 2\r\n  Debian-12 Stopped 2\r\n";
        let distros = parse_wsl_list_checked(input).unwrap();
        assert_eq!(distros[0].name, "Ubuntu 24.04");
        assert!(distros[0].default);
        assert_eq!(distros[1].state, "Stopped");
        assert!(parse_wsl_list_checked(&"x".repeat(MAX_DASHBOARD_OUTPUT_LINE_BYTES + 1)).is_err());
        assert!(parse_wsl_list_checked(&" ".repeat(64 * 1024 + 1)).is_err());
    }

    #[test]
    fn checked_wsl_list_rejects_ambiguous_or_unsafe_rows() {
        for input in [
            "NAME STATE VERSION\nUbuntu Running nope\n",
            "NAME STATE VERSION\nUbuntu Paused 2\n",
            "NAME STATE VERSION\nUbuntu Running 2\nUbuntu Running 2\n",
            "NAME STATE VERSION\n* Ubuntu Running 2\n* Debian-12 Running 2\n",
            "NAME STATE VERSION\nUbuntu;rm Running 2\n",
            &format!(
                "NAME STATE VERSION\n{} Running 2\n",
                "x".repeat(crate::core::runtime_snapshot::MAX_DISTRO_NAME_BYTES + 1)
            ),
        ] {
            assert_eq!(parse_wsl_list_checked(input).unwrap_err(), WSL_LIST_ERROR);
        }
    }

    #[test]
    fn parses_wsl_list_with_spaces_in_distro_name() {
        let input =
            "  NAME             STATE           VERSION\n  Ubuntu 24.04     Running         2\n";
        let distros = parse_wsl_list_checked(input).unwrap();
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
    fn docker_parser_rejects_unbounded_or_controlled_output() {
        assert!(parse_docker_ps(&"\n".repeat(MAX_DASHBOARD_DOCKER_OUTPUT_BYTES + 1)).is_err());
        assert!(parse_docker_ps("abc123\tapi\trunning\tports\u{0000}\n").is_err());
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
