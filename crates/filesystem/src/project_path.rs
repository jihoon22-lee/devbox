//! 앱 간 snapshot에 넣을 수 있는 프로젝트 경로의 공통 안전 규칙.
//!
//! 이 모듈은 경로가 실제로 존재하는지 확인하지 않는다. producer와 consumer가 동일하게
//! 사용할 수 있도록 문자열만으로 absolute/root/traversal/device alias를 판정한다.

/// snapshot 하나에서 허용하는 프로젝트 경로 문자열의 최대 크기.
pub const MAX_PROJECT_PATH_BYTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectPathKind {
    Posix,
    WindowsDrive,
    WindowsUnc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeProjectPath {
    value: String,
    name: String,
    identity: String,
    kind: ProjectPathKind,
}

impl SafeProjectPath {
    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn into_string(self) -> String {
        self.value
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Windows 경로는 slash/case 차이를 접고 POSIX 경로는 표기를 보존한 identity다.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn kind(&self) -> ProjectPathKind {
        self.kind
    }
}

/// root 자체가 아닌 안전한 절대 Windows drive/UNC/POSIX 프로젝트 경로를 검증한다.
pub fn parse_safe_project_path(raw: &str) -> Option<SafeProjectPath> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_PROJECT_PATH_BYTES
        || trimmed.chars().any(char::is_control)
        || is_windows_device_path(trimmed)
    {
        return None;
    }

    let (kind, without_root, required_components) =
        if let Some(rest) = windows_drive_suffix(trimmed) {
            (ProjectPathKind::WindowsDrive, rest, 1)
        } else if let Some(rest) = unc_suffix(trimmed) {
            // server/share 아래의 실제 프로젝트만 허용한다. UNC share root 자체는 제외한다.
            (ProjectPathKind::WindowsUnc, rest, 3)
        } else {
            let rest = trimmed.strip_prefix('/')?;
            (ProjectPathKind::Posix, rest, 1)
        };

    let components = match kind {
        ProjectPathKind::Posix => without_root.split('/').collect::<Vec<_>>(),
        ProjectPathKind::WindowsDrive | ProjectPathKind::WindowsUnc => {
            without_root.split(['/', '\\']).collect::<Vec<_>>()
        }
    }
    .into_iter()
    .filter(|component| !component.is_empty())
    .collect::<Vec<_>>();
    if components.len() < required_components
        || components.iter().any(|component| {
            matches!(*component, "." | "..")
                || (kind != ProjectPathKind::Posix && !windows_component_is_safe(component))
        })
    {
        return None;
    }

    let value = match kind {
        ProjectPathKind::Posix => trimmed.trim_end_matches('/'),
        ProjectPathKind::WindowsDrive | ProjectPathKind::WindowsUnc => {
            trimmed.trim_end_matches(['/', '\\'])
        }
    }
    .to_string();
    let name = components.last()?.to_string();
    let identity = match kind {
        ProjectPathKind::Posix => value.clone(),
        ProjectPathKind::WindowsDrive | ProjectPathKind::WindowsUnc => {
            value.replace('/', "\\").to_ascii_lowercase()
        }
    };
    Some(SafeProjectPath {
        value,
        name,
        identity,
        kind,
    })
}

fn windows_drive_suffix(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    (bytes.len() >= 4
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
    .then(|| &path[3..])
}

fn unc_suffix(path: &str) -> Option<&str> {
    path.strip_prefix("\\\\")
        .or_else(|| path.strip_prefix("//"))
}

fn is_windows_device_path(path: &str) -> bool {
    path.starts_with("\\\\?\\")
        || path.starts_with("\\\\.\\")
        || path.starts_with("//?/")
        || path.starts_with("//./")
}

fn windows_component_is_safe(component: &str) -> bool {
    if component.ends_with(' ')
        || component.ends_with('.')
        || component
            .chars()
            .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = component.split('.').next().unwrap_or_default();
    let upper = stem.to_ascii_uppercase();
    !matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) && !is_numbered_windows_device(&upper, "COM")
        && !is_numbered_windows_device(&upper, "LPT")
}

fn is_numbered_windows_device(value: &str, prefix: &str) -> bool {
    value
        .strip_prefix(prefix)
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_project_paths_and_normalizes_identity_only() {
        let drive = parse_safe_project_path(" C:/Work/Devbox/ ").unwrap();
        assert_eq!(drive.as_str(), "C:/Work/Devbox");
        assert_eq!(drive.name(), "Devbox");
        assert_eq!(drive.identity(), "c:\\work\\devbox");
        assert_eq!(drive.kind(), ProjectPathKind::WindowsDrive);

        let unc = parse_safe_project_path("\\\\server\\share\\project\\").unwrap();
        assert_eq!(unc.as_str(), "\\\\server\\share\\project");
        assert_eq!(unc.kind(), ProjectPathKind::WindowsUnc);

        let posix = parse_safe_project_path("/mnt/e/Projects/Devbox/").unwrap();
        assert_eq!(posix.as_str(), "/mnt/e/Projects/Devbox");
        assert_eq!(posix.identity(), "/mnt/e/Projects/Devbox");
        assert_eq!(posix.kind(), ProjectPathKind::Posix);
    }

    #[test]
    fn folds_windows_case_and_separator_spelling_but_not_posix_case() {
        let first = parse_safe_project_path("C:\\Work\\Devbox").unwrap();
        let second = parse_safe_project_path("c:/work/devbox/").unwrap();
        assert_eq!(first.identity(), second.identity());

        let first = parse_safe_project_path("/work/Devbox").unwrap();
        let second = parse_safe_project_path("/work/devbox").unwrap();
        assert_ne!(first.identity(), second.identity());
    }

    #[test]
    fn rejects_relative_traversal_roots_devices_and_unsafe_aliases() {
        for path in [
            "relative/path",
            "C:\\work\\..\\escape",
            "/work/./escape",
            "/",
            "C:\\",
            "\\\\server\\share",
            "\\\\?\\C:\\work\\devbox",
            "\\\\.\\PhysicalDrive0\\project",
            "C:\\work\\NUL.txt",
            "C:\\work\\COM1",
            "C:\\work\\bad:name",
            "C:\\work\\wild*card",
            "C:\\work\\trailing.\\child",
            "C:\\work\\line\nfeed",
        ] {
            assert!(parse_safe_project_path(path).is_none(), "accepted {path:?}");
        }
    }

    #[test]
    fn rejects_oversized_utf8_path_by_bytes() {
        let oversized = format!("C:\\work\\{}", "가".repeat(MAX_PROJECT_PATH_BYTES));
        assert!(parse_safe_project_path(&oversized).is_none());
    }
}
