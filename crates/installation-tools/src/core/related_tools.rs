//! The small, reviewed Related Tools catalog owned by Devbox Manager.
//!
//! This is intentionally separate from `apps/catalog.json`: Related Tools are
//! optional external complements, not devbox applications.  The catalog only
//! contains stable metadata and executable *names*.  It never contains a user
//! path, installer URL, command arguments, or a detected version.

#[cfg_attr(not(test), allow(dead_code))]
pub const MAX_RELATED_TOOLS: usize = 16;
pub const MAX_TOOL_ID_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelatedToolSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub summary: &'static str,
    pub winget_id: &'static str,
    pub official_url: &'static str,
    pub license_url: &'static str,
    pub license_summary: &'static str,
    pub executable_names: &'static [&'static str],
}

const POWER_TOYS_EXECUTABLES: &[&str] = &["PowerToys.exe"];
const WINDOWS_TERMINAL_EXECUTABLES: &[&str] = &["wt.exe"];
const VS_CODE_EXECUTABLES: &[&str] = &["code.exe"];
const BRUNO_EXECUTABLES: &[&str] = &["bruno.exe"];
const DBEAVER_EXECUTABLES: &[&str] = &["dbeaver.exe"];
const DB_BROWSER_EXECUTABLES: &[&str] = &["DB Browser for SQLite.exe", "sqlitebrowser.exe"];
const GITHUB_DESKTOP_EXECUTABLES: &[&str] = &["GitHubDesktop.exe", "github.exe"];
const PODMAN_DESKTOP_EXECUTABLES: &[&str] = &["podman-desktop.exe"];
const DOCKER_DESKTOP_EXECUTABLES: &[&str] = &["Docker Desktop.exe", "docker-desktop.exe"];

/// Reviewed external tools.  Keep this list small: adding a tool is a product
/// decision and requires an official homepage, an official license page, a
/// stable WinGet id, and a bounded launch detection strategy.
pub const CURATED_TOOLS: &[RelatedToolSpec] = &[
    RelatedToolSpec {
        id: "power-toys",
        display_name: "PowerToys",
        summary: "Windows 생산성 유틸리티 모음",
        winget_id: "Microsoft.PowerToys",
        official_url: "https://learn.microsoft.com/windows/powertoys/",
        license_url: "https://github.com/microsoft/PowerToys/blob/main/LICENSE",
        license_summary: "MIT (소스)",
        executable_names: POWER_TOYS_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "windows-terminal",
        display_name: "Windows Terminal",
        summary: "탭·프로필을 지원하는 Windows 터미널",
        winget_id: "Microsoft.WindowsTerminal",
        official_url: "https://github.com/microsoft/terminal",
        license_url: "https://github.com/microsoft/terminal/blob/main/LICENSE",
        license_summary: "MIT",
        executable_names: WINDOWS_TERMINAL_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "vs-code",
        display_name: "Visual Studio Code",
        summary: "경량 코드 편집기",
        winget_id: "Microsoft.VisualStudioCode",
        official_url: "https://code.visualstudio.com/",
        license_url: "https://code.visualstudio.com/License",
        license_summary: "Microsoft 배포 약관 · 소스 MIT",
        executable_names: VS_CODE_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "bruno",
        display_name: "Bruno",
        summary: "오프라인 우선 API 클라이언트",
        winget_id: "Bruno.Bruno",
        official_url: "https://www.usebruno.com/",
        license_url: "https://github.com/usebruno/bruno/blob/main/LICENSE.md",
        license_summary: "MIT",
        executable_names: BRUNO_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "dbeaver",
        display_name: "DBeaver Community",
        summary: "관계형 데이터베이스 탐색기",
        winget_id: "DBeaver.DBeaver.Community",
        official_url: "https://dbeaver.io/",
        license_url: "https://github.com/dbeaver/dbeaver/blob/devel/LICENSE",
        license_summary: "Apache-2.0",
        executable_names: DBEAVER_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "db-browser",
        display_name: "DB Browser for SQLite",
        summary: "SQLite 데이터베이스 브라우저",
        winget_id: "DBBrowserForSQLite.DBBrowserForSQLite",
        official_url: "https://sqlitebrowser.org/",
        license_url: "https://github.com/sqlitebrowser/sqlitebrowser/blob/master/LICENSE",
        license_summary: "MPL-2.0",
        executable_names: DB_BROWSER_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "github-desktop",
        display_name: "GitHub Desktop",
        summary: "GitHub 저장소용 데스크톱 클라이언트",
        winget_id: "GitHub.GitHubDesktop",
        official_url: "https://desktop.github.com/",
        license_url: "https://github.com/desktop/desktop/blob/development/LICENSE",
        license_summary: "MIT",
        executable_names: GITHUB_DESKTOP_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "podman-desktop",
        display_name: "Podman Desktop",
        summary: "컨테이너와 Pod를 관리하는 데스크톱 앱",
        winget_id: "RedHat.Podman-Desktop",
        official_url: "https://podman-desktop.io/",
        license_url: "https://github.com/containers/podman-desktop/blob/main/LICENSE",
        license_summary: "Apache-2.0",
        executable_names: PODMAN_DESKTOP_EXECUTABLES,
    },
    RelatedToolSpec {
        id: "docker-desktop",
        display_name: "Docker Desktop",
        summary: "Docker 컨테이너 개발 환경",
        winget_id: "Docker.DockerDesktop",
        official_url: "https://www.docker.com/products/docker-desktop/",
        license_url: "https://www.docker.com/legal/docker-software-license/",
        license_summary: "Docker Software License",
        executable_names: DOCKER_DESKTOP_EXECUTABLES,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum DetectionSource {
    Path,
    KnownLocation,
    NotFound,
    Unavailable,
}

impl DetectionSource {
    pub const fn as_code(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::KnownLocation => "known-location",
            Self::NotFound => "not-found",
            Self::Unavailable => "unavailable",
        }
    }
}

pub fn curated_tools() -> &'static [RelatedToolSpec] {
    CURATED_TOOLS
}

pub fn find_tool(id: &str) -> Option<&'static RelatedToolSpec> {
    CURATED_TOOLS.iter().find(|tool| tool.id == id)
}

/// Keep the public command boundary opaque: only a catalog id can select a
/// tool.  In particular, paths, flags, and arbitrary executable names are not
/// accepted as ids.
pub fn is_valid_tool_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TOOL_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(any(windows, test))]
pub fn classify_detection(
    path_found: bool,
    known_location_found: bool,
    probe_available: bool,
) -> DetectionSource {
    if path_found {
        DetectionSource::Path
    } else if known_location_found {
        DetectionSource::KnownLocation
    } else if probe_available {
        DetectionSource::NotFound
    } else {
        DetectionSource::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn curated_catalog_is_small_unique_and_official() {
        assert!(!CURATED_TOOLS.is_empty());
        assert!(CURATED_TOOLS.len() <= MAX_RELATED_TOOLS);
        assert_eq!(
            CURATED_TOOLS.iter().map(|tool| tool.id).collect::<Vec<_>>(),
            vec![
                "power-toys",
                "windows-terminal",
                "vs-code",
                "bruno",
                "dbeaver",
                "db-browser",
                "github-desktop",
                "podman-desktop",
                "docker-desktop",
            ]
        );

        let mut ids = HashSet::new();
        let mut winget_ids = HashSet::new();
        for tool in CURATED_TOOLS {
            assert!(is_valid_tool_id(tool.id));
            assert!(ids.insert(tool.id));
            assert!(winget_ids.insert(tool.winget_id));
            assert!(tool.official_url.starts_with("https://"));
            assert!(tool.license_url.starts_with("https://"));
            assert!(!tool.executable_names.is_empty());
            assert!(tool
                .executable_names
                .iter()
                .all(|name| { !name.is_empty() && name.len() <= MAX_TOOL_ID_BYTES * 2 }));
        }
    }

    #[test]
    fn finds_only_curated_ids() {
        assert!(find_tool("vs-code").is_some());
        assert!(find_tool("unknown-tool").is_none());
        assert!(find_tool("../PowerShell").is_none());
    }

    #[test]
    fn rejects_unbounded_or_argument_like_ids() {
        assert!(!is_valid_tool_id(""));
        assert!(!is_valid_tool_id("VS-Code"));
        assert!(!is_valid_tool_id("vs-code --id evil"));
        assert!(!is_valid_tool_id(&"x".repeat(MAX_TOOL_ID_BYTES + 1)));
        assert!(is_valid_tool_id("db-browser"));
    }

    #[test]
    fn detection_prefers_path_then_known_location() {
        assert_eq!(classify_detection(true, true, true), DetectionSource::Path);
        assert_eq!(
            classify_detection(false, true, true),
            DetectionSource::KnownLocation
        );
        assert_eq!(
            classify_detection(false, false, true),
            DetectionSource::NotFound
        );
        assert_eq!(
            classify_detection(false, false, false),
            DetectionSource::Unavailable
        );
    }
}
