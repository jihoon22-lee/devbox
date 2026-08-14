//! 프로젝트 health의 순수 파싱·판정 로직.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub path: String,
    pub branch: String,
    pub changes: u32,
    pub clean: bool,
}

/// `git -C <path> status --porcelain --branch` 출력 파싱.
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

/// `wsl.exe -l -v` 출력에서 distro 이름 존재 여부 판정 (문자열 수준).
pub fn has_distro(distro: &str, wsl_list_output: &str) -> bool {
    wsl_list_output
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .filter(|l| !l.is_empty() && !l.starts_with("NAME") && !l.starts_with("Windows"))
        .filter_map(|l| l.split_whitespace().next())
        .any(|name| name.eq_ignore_ascii_case(distro))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_clean_and_dirty() {
        let clean = parse_git_status("C:/p", "## main...origin/main\n");
        assert!(clean.clean);
        assert_eq!(clean.branch, "main");
        let dirty = parse_git_status("C:/p", "## dev\n M a.rs\n?? b.txt\n");
        assert_eq!(dirty.changes, 2);
        assert!(!dirty.clean);
    }

    #[test]
    fn distro_presence() {
        let out = "  NAME      STATE           VERSION\n* Ubuntu    Running         2\n  docker-desktop Running     2\n";
        assert!(has_distro("ubuntu", out));
        assert!(has_distro("docker-desktop", out));
        assert!(!has_distro("Debian", out));
    }
}
