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

    /// `wsl.exe -l -v`는 실제로 UTF-16LE(BOM 있음)로 출력한다. 호출부가 이를
    /// `devbox_wsl::output::decode_output`로 디코딩해서 넘긴다는 가정을 고정한다 —
    /// 그 계약이 깨지면(예: `from_utf8_lossy`로 되돌아가면) 이 테스트가 실패해야 한다.
    #[test]
    fn distro_presence_after_utf16le_decode() {
        let raw = "  NAME      STATE           VERSION\n* Ubuntu-24.04    Running         2\n";
        let mut bytes = vec![0xFF, 0xFE]; // UTF-16LE BOM
        for unit in raw.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let decoded = devbox_wsl::output::decode_output(&bytes);
        assert!(has_distro("Ubuntu-24.04", &decoded));

        // 회귀 방지: from_utf8_lossy로 같은 바이트를 읽으면 매치가 깨진다는 것을 확인해
        // "decode_output을 실제로 거쳤는가"를 이 테스트가 검증하고 있음을 보인다.
        let wrongly_decoded = String::from_utf8_lossy(&bytes).into_owned();
        assert!(!has_distro("Ubuntu-24.04", &wrongly_decoded));
    }
}
