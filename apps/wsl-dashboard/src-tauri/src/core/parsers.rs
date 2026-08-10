use crate::core::models::{ContainerInfo, DistroInfo, GitStatus};

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

/// `git status --porcelain --branch` 출력 파싱.
/// 형식:
/// ```text
/// ## main...origin/main
///  M file.txt
/// ?? new.txt
/// ```
pub fn parse_git_status(path: &str, input: &str) -> GitStatus {
    let mut branch = "(detached)".to_string();
    let mut changes = 0u32;
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            branch = rest.split("...").next().unwrap_or(rest).to_string();
        } else if !line.trim().is_empty() {
            changes += 1;
        }
    }
    GitStatus {
        path: path.to_string(),
        branch,
        changes,
        clean: changes == 0,
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
    fn parses_git_status_clean() {
        let s = parse_git_status("C:/proj", "## main...origin/main\n");
        assert_eq!(s.branch, "main");
        assert_eq!(s.changes, 0);
        assert!(s.clean);
    }

    #[test]
    fn parses_git_status_dirty() {
        let s = parse_git_status("C:/proj", "## dev...origin/dev\n M a.rs\n?? b.txt\n");
        assert_eq!(s.branch, "dev");
        assert_eq!(s.changes, 2);
        assert!(!s.clean);
    }
}
