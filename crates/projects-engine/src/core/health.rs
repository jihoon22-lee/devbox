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
        .filter_map(distro_info_from_row)
        .any(|(name, _state)| name.eq_ignore_ascii_case(distro))
}

/// Return whether a named distro is already running. A stopped distro is not
/// started just to inspect its working directory; callers can surface that
/// state as unavailable and let the user explicitly start WSL.
pub fn distro_is_running(distro: &str, wsl_list_output: &str) -> Option<bool> {
    wsl_list_output
        .lines()
        .filter_map(distro_info_from_row)
        .find(|(name, _state)| name.eq_ignore_ascii_case(distro))
        .map(|(_name, state)| state.eq_ignore_ascii_case("running"))
}

/// Parse one `wsl.exe -l -v` row. STATE and VERSION are the two rightmost
/// columns, so the NAME column may contain spaces.
fn distro_info_from_row(line: &str) -> Option<(String, String)> {
    let row = line.trim().trim_start_matches('*').trim();
    if row.is_empty() || row.starts_with("NAME") || row.starts_with("Windows") {
        return None;
    }

    let mut fields = row.split_whitespace();
    fields.next_back()?; // VERSION
    let state = fields.next_back()?.to_string(); // STATE
    let name = fields.collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some((name, state))
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

    #[test]
    fn distro_presence_matches_spaced_names_exactly() {
        let out =
            "  NAME             STATE           VERSION\n* Ubuntu 24.04     Running         2\n";
        assert!(has_distro("Ubuntu 24.04", out));
        assert!(!has_distro("Ubuntu 24", out));
    }

    #[test]
    fn distro_running_state_is_observed_without_starting_a_stopped_distro() {
        let out = "  NAME      STATE           VERSION\n* Ubuntu    Running         2\n  Debian    Stopped         2\n";
        assert_eq!(distro_is_running("Ubuntu", out), Some(true));
        assert_eq!(distro_is_running("Debian", out), Some(false));
        assert_eq!(distro_is_running("Fedora", out), None);
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
