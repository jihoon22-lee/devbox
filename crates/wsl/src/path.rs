use crate::WslError;

const MAX_WSL_PROJECT_PATH_BYTES: usize = 4_096;

/// A Windows UNC path that addresses a project inside one WSL distribution.
///
/// The distro name is kept as supplied for `wsl.exe -d`, while the Linux path
/// preserves its case exactly. `\\wsl$` and `\\wsl.localhost` are transport
/// aliases only and never become part of the Linux path identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslUncPath {
    distro: String,
    linux_path: String,
}

impl WslUncPath {
    pub fn distro(&self) -> &str {
        &self.distro
    }

    pub fn linux_path(&self) -> &str {
        &self.linux_path
    }

    pub fn identity(&self) -> String {
        format!(
            "wsl:{}:{}",
            self.distro.to_ascii_lowercase(),
            self.linux_path.trim_start_matches('/')
        )
    }
}

/// Parse `\\wsl$\<distro>\...`, `//wsl$/<distro>/...`, and the
/// `wsl.localhost` alias without touching the filesystem or starting a distro.
/// Ordinary UNC paths return `Ok(None)` so callers can keep their native
/// Windows handling. A path that names a WSL server but has an unsafe/missing
/// distro or Linux tail fails closed.
pub fn parse_wsl_unc_path(raw: &str) -> Result<Option<WslUncPath>, WslError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_WSL_PROJECT_PATH_BYTES {
        return Ok(None);
    }
    let mut normalized = trimmed.replace('\\', "/");
    // `std::fs::canonicalize` on Windows returns an extended UNC spelling.
    // Treat that internal spelling as the same transport alias while keeping
    // ordinary device paths outside the WSL parser.
    if normalized
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("//?/UNC/"))
    {
        normalized = format!("//{}", &normalized[8..]);
    }
    let Some(rest) = normalized.strip_prefix("//") else {
        return Ok(None);
    };
    let mut parts = rest.split('/');
    let server = parts.next().unwrap_or_default();
    if !server.eq_ignore_ascii_case("wsl$") && !server.eq_ignore_ascii_case("wsl.localhost") {
        return Ok(None);
    }

    let distro = parts.next().unwrap_or_default().trim();
    crate::distro::validate_distro_name(distro)?;
    let components = parts
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|component| {
            matches!(*component, "." | "..") || component.chars().any(char::is_control)
        })
    {
        return Err(WslError::InvalidPath(
            "WSL UNC 경로에는 안전한 Linux project 경로가 필요하다".into(),
        ));
    }
    let linux_path = format!("/{}", components.join("/"));
    if linux_path.len() > MAX_WSL_PROJECT_PATH_BYTES {
        return Err(WslError::InvalidPath(
            "WSL project 경로가 크기 제한을 초과한다".into(),
        ));
    }
    Ok(Some(WslUncPath {
        distro: distro.to_owned(),
        linux_path,
    }))
}

/// Return whether `candidate` is the same WSL path as `root` or a descendant.
///
/// `Ok(None)` means `root` is not a WSL UNC path and lets callers apply their
/// native path rules. Once `root` is WSL, an ordinary/native candidate is a
/// definite non-match. Distribution names and transport aliases are
/// case-insensitive, but every Linux path component remains case-sensitive.
pub fn wsl_unc_contains(root: &str, candidate: &str) -> Result<Option<bool>, WslError> {
    let Some(root) = parse_wsl_unc_path(root)? else {
        return Ok(None);
    };
    let Some(candidate) = parse_wsl_unc_path(candidate)? else {
        return Ok(Some(false));
    };
    if !root.distro.eq_ignore_ascii_case(&candidate.distro) {
        return Ok(Some(false));
    }
    if root.linux_path == candidate.linux_path {
        return Ok(Some(true));
    }
    Ok(Some(
        candidate
            .linux_path
            .strip_prefix(&root.linux_path)
            .is_some_and(|tail| tail.starts_with('/')),
    ))
}

/// Windows 드라이브 경로를 WSL 경로로 정규화한다 (프로세스 실행 없이).
///
/// `E:\a\b` → `/mnt/e/a/b`, `E:/a\b/` → `/mnt/e/a/b`.
/// 드라이브 경로가 아니면(상대 경로, UNC) 오류를 낸다.
pub fn windows_to_wsl(path: &str) -> Result<String, WslError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(WslError::InvalidPath("빈 경로".into()));
    }
    let bytes = path.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return Err(WslError::InvalidPath("드라이브 문자 경로가 아니다".into()));
    }
    let drive = path[..1].to_ascii_lowercase();
    let rest = &path[2..];
    let rest = rest.trim_start_matches(['\\', '/']);
    let rest = rest.replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        Ok(format!("/mnt/{drive}"))
    } else {
        Ok(format!("/mnt/{drive}/{rest}"))
    }
}

/// WSL 경로를 Windows 경로로 정규화한다.
///
/// `/mnt/e/a/b` → `E:\a\b`. `/mnt/<letter>` → `<LETTER>:\`.
/// 그 외 절대 WSL 경로(`/home/...`)는 `\\wsl$\<distro>\...` UNC로 표현한다.
pub fn wsl_to_windows(distro: &str, path: &str) -> Result<String, WslError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(WslError::InvalidPath("빈 경로".into()));
    }
    if let Some(rest) = path.strip_prefix("/mnt/") {
        let mut parts = rest.splitn(2, '/');
        let drive = parts.next().unwrap_or_default();
        if drive.len() != 1 || !drive.chars().next().unwrap().is_ascii_alphabetic() {
            return Err(WslError::InvalidPath(format!(
                "/mnt/ 다음은 드라이브 문자여야 한다: {path}"
            )));
        }
        let drive = drive.to_ascii_uppercase();
        let tail = parts.next().unwrap_or_default().replace('/', "\\");
        let tail = tail.trim_end_matches('\\');
        if tail.is_empty() {
            Ok(format!("{drive}:\\"))
        } else {
            Ok(format!("{drive}:\\{tail}"))
        }
    } else {
        let tail = path.trim_start_matches('/').replace('/', "\\");
        Ok(format!("\\\\wsl$\\{distro}\\{tail}"))
    }
}

/// 같은 프로젝트를 하나로 식별하는 정규 키.
///
/// 규칙:
/// - Windows 경로: 드라이브 문자를 소문자로, 구분자를 `/`로, 후행 `/` 제거한
///   `win:<drive>:/...` 형태로 정규화한다.
/// - 일반 UNC 경로는 case/slash를 접은 `unc:<server>/<share>/...` 키를 쓴다.
///   `\\wsl$`·`\\wsl.localhost` 경로는 WSL identity로 접어 WSL profile과 중복되지 않는다.
/// - WSL 경로가 `/mnt/<드라이브>/...`면 같은 `win:` 키로 변환한다
///   (Windows/WSL 어느 쪽에서 등록해도 하나로 식별된다 — §10.2 ProjectProfile).
/// - `/mnt/` 밖의 WSL 경로는 distro를 포함한 `wsl:<distro>:...` 키를 쓴다.
pub fn canonical_project_key(
    windows: Option<&str>,
    wsl: Option<(&str, &str)>,
) -> Result<String, WslError> {
    if let Some(w) = windows {
        return normalize_windows_project_key(w);
    }
    if let Some((distro, p)) = wsl {
        crate::distro::validate_distro_name(distro)?;
        if !p.trim().starts_with('/')
            || p.len() > MAX_WSL_PROJECT_PATH_BYTES
            || p.chars().any(char::is_control)
            || p.trim()
                .split('/')
                .any(|component| matches!(component, "." | ".."))
        {
            return Err(WslError::InvalidPath(
                "WSL project 경로가 올바르지 않다".into(),
            ));
        }
        if let Some(rest) = p.trim().strip_prefix("/mnt/") {
            let drive = rest.chars().next().unwrap_or_default();
            if drive.is_ascii_alphabetic() {
                return Ok(format!(
                    "win:{}:/{}",
                    drive.to_ascii_lowercase(),
                    rest[1..].trim_matches('/')
                ));
            }
        }
        let normalized = p.trim().trim_matches('/').to_owned();
        return Ok(format!(
            "wsl:{}:{normalized}",
            distro.trim().to_ascii_lowercase()
        ));
    }
    Err(WslError::InvalidPath(
        "Windows 경로 또는 WSL 경로가 필요하다".into(),
    ))
}

fn normalize_windows_project_key(path: &str) -> Result<String, WslError> {
    let trimmed = path.trim();
    if trimmed.starts_with("\\\\?\\")
        || trimmed.starts_with("\\\\.\\")
        || trimmed.starts_with("//?/")
        || trimmed.starts_with("//./")
    {
        return Err(WslError::InvalidPath(
            "Windows device 경로는 프로젝트 identity로 사용할 수 없다".into(),
        ));
    }
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return normalize_unc_project_key(trimmed);
    }
    Ok(format!("win:{}", normalize_windows_drive(trimmed)?))
}

fn normalize_unc_project_key(path: &str) -> Result<String, WslError> {
    if let Some(wsl) = parse_wsl_unc_path(path)? {
        return Ok(wsl.identity());
    }
    let normalized = path.replace('\\', "/");
    let parts = normalized
        .strip_prefix("//")
        .unwrap_or_default()
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| matches!(*part, "." | "..")) {
        return Err(WslError::InvalidPath(
            "UNC 경로에는 server와 share가 필요하다".into(),
        ));
    }

    Ok(format!("unc:{}", parts.join("/").to_ascii_lowercase()))
}

/// `win:` 키에 쓰는 드라이브 경로 정규화: 소문자 드라이브, `/` 구분, 후행 제거.
fn normalize_windows_drive(path: &str) -> Result<String, WslError> {
    let path = path.trim();
    let bytes = path.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return Err(WslError::InvalidPath("드라이브 문자 경로가 아니다".into()));
    }
    let drive = path[..1].to_ascii_lowercase();
    let rest = path[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() {
        Ok(format!("{drive}:/"))
    } else {
        Ok(format!("{drive}:/{rest}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_to_wsl_basic() {
        assert_eq!(windows_to_wsl("E:\\a\\b").unwrap(), "/mnt/e/a/b");
        assert_eq!(windows_to_wsl("E:/a/b").unwrap(), "/mnt/e/a/b");
    }

    #[test]
    fn windows_to_wsl_lowercases_drive_and_trims() {
        assert_eq!(windows_to_wsl("e:\\A\\B\\").unwrap(), "/mnt/e/A/B");
        assert_eq!(windows_to_wsl("E:").unwrap(), "/mnt/e");
        assert_eq!(windows_to_wsl("E:\\").unwrap(), "/mnt/e");
    }

    #[test]
    fn windows_to_wsl_rejects_non_drive() {
        assert!(windows_to_wsl("relative/path").is_err());
        assert!(windows_to_wsl("\\\\wsl$\\Ubuntu\\x").is_err());
        assert!(windows_to_wsl("").is_err());
    }

    #[test]
    fn wsl_to_windows_basic() {
        assert_eq!(wsl_to_windows("Ubuntu", "/mnt/e/a/b").unwrap(), "E:\\a\\b");
        assert_eq!(wsl_to_windows("Ubuntu", "/mnt/e").unwrap(), "E:\\");
        assert_eq!(wsl_to_windows("Ubuntu", "/mnt/e/").unwrap(), "E:\\");
    }

    #[test]
    fn wsl_to_windows_unc_for_non_mnt() {
        assert_eq!(
            wsl_to_windows("Ubuntu", "/home/user/x").unwrap(),
            "\\\\wsl$\\Ubuntu\\home\\user\\x"
        );
    }

    #[test]
    fn wsl_to_windows_rejects_bad_mnt() {
        assert!(wsl_to_windows("Ubuntu", "/mnt/ab/c").is_err());
    }

    #[test]
    fn canonical_key_windows_normalizes() {
        assert_eq!(
            canonical_project_key(Some("E:\\Projects\\Devbox\\"), None).unwrap(),
            "win:e:/Projects/Devbox"
        );
        assert_eq!(
            canonical_project_key(Some("e:/projects/devbox"), None).unwrap(),
            "win:e:/projects/devbox"
        );
    }

    #[test]
    fn canonical_key_unc_normalizes_case_and_slashes() {
        assert_eq!(
            canonical_project_key(Some("\\\\Server\\Share\\Projects\\Devbox\\"), None).unwrap(),
            "unc:server/share/projects/devbox"
        );
        assert_eq!(
            canonical_project_key(Some("//server/share/projects/devbox"), None).unwrap(),
            "unc:server/share/projects/devbox"
        );
    }

    #[test]
    fn canonical_key_wsl_unc_matches_wsl_profile() {
        let unc = canonical_project_key(
            Some("\\\\wsl.localhost\\Ubuntu\\home\\jihoon\\Devbox"),
            None,
        )
        .unwrap();
        let profile = canonical_project_key(None, Some(("Ubuntu", "/home/jihoon/Devbox"))).unwrap();
        assert_eq!(unc, profile);
        assert_eq!(unc, "wsl:ubuntu:home/jihoon/Devbox");
    }

    #[test]
    fn parses_wsl_unc_aliases_and_preserves_linux_case() {
        let backslash = parse_wsl_unc_path("\\\\wsl$\\Ubuntu\\home\\jihoon\\DevBox\\")
            .unwrap()
            .unwrap();
        let slash = parse_wsl_unc_path("//wsl.localhost/ubuntu/home/jihoon/DevBox")
            .unwrap()
            .unwrap();
        assert_eq!(backslash.distro(), "Ubuntu");
        assert_eq!(backslash.linux_path(), "/home/jihoon/DevBox");
        assert_eq!(backslash.identity(), slash.identity());
        assert_ne!(
            backslash.identity(),
            parse_wsl_unc_path("//wsl$/Ubuntu/home/jihoon/devbox")
                .unwrap()
                .unwrap()
                .identity()
        );
    }

    #[test]
    fn parses_extended_unc_from_windows_canonicalize() {
        let parsed = parse_wsl_unc_path(
            "\\\\?\\UNC\\wsl.localhost\\Ubuntu\\home\\jihoon\\한글 project\\DevBox",
        )
        .unwrap()
        .unwrap();
        assert_eq!(parsed.distro(), "Ubuntu");
        assert_eq!(parsed.linux_path(), "/home/jihoon/한글 project/DevBox");
        assert_eq!(
            parsed.identity(),
            "wsl:ubuntu:home/jihoon/한글 project/DevBox"
        );
    }

    #[test]
    fn wsl_containment_folds_transport_and_distro_but_not_linux_case() {
        let root = "\\\\wsl$\\Ubuntu\\home\\jihoon\\한글 project\\DevBox";
        assert_eq!(
            wsl_unc_contains(
                root,
                "//wsl.localhost/ubuntu/home/jihoon/한글 project/DevBox/src/main.rs"
            )
            .unwrap(),
            Some(true)
        );
        assert_eq!(
            wsl_unc_contains(root, "//wsl$/Ubuntu/home/jihoon/한글 project/devbox/a.rs").unwrap(),
            Some(false)
        );
        assert_eq!(
            wsl_unc_contains(root, "//wsl$/Debian/home/jihoon/한글 project/DevBox/a.rs").unwrap(),
            Some(false)
        );
        assert_eq!(
            wsl_unc_contains("C:/projects/devbox", "C:/projects/devbox/a.rs").unwrap(),
            None
        );
    }

    #[test]
    fn distinguishes_ordinary_unc_and_rejects_unsafe_wsl_unc() {
        assert_eq!(
            parse_wsl_unc_path("\\\\server\\share\\project").unwrap(),
            None
        );
        assert!(parse_wsl_unc_path("\\\\wsl$\\Ubuntu").is_err());
        assert!(parse_wsl_unc_path("//wsl$/Ubuntu/home/../secret").is_err());
        assert!(parse_wsl_unc_path("//wsl$/--help/home/project").is_err());
    }

    #[test]
    fn canonical_key_rejects_unc_root_traversal_and_device_paths() {
        assert!(canonical_project_key(Some("\\\\server"), None).is_err());
        assert!(canonical_project_key(Some("\\\\server\\share\\..\\escape"), None).is_err());
        assert!(canonical_project_key(Some("\\\\?\\C:\\project"), None).is_err());
    }

    #[test]
    fn canonical_key_mnt_matches_windows() {
        // Windows와 WSL(/mnt) 표기가 같은 키가 되어야 한다
        let win = canonical_project_key(Some("E:\\projects\\devbox"), None).unwrap();
        let wsl = canonical_project_key(None, Some(("Ubuntu", "/mnt/e/projects/devbox"))).unwrap();
        assert_eq!(win, wsl);
        assert_eq!(win, "win:e:/projects/devbox");
    }

    #[test]
    fn canonical_key_distro_scoped_for_non_mnt() {
        assert_eq!(
            canonical_project_key(None, Some(("Ubuntu", "/home/user/proj/"))).unwrap(),
            "wsl:ubuntu:home/user/proj"
        );
    }

    #[test]
    fn canonical_key_requires_one_input() {
        assert!(canonical_project_key(None, None).is_err());
    }

    #[test]
    fn windows_to_wsl_roundtrip() {
        for p in ["E:\\a\\b", "E:/a/b", "e:\\A\\B\\"] {
            let wsl = windows_to_wsl(p).unwrap();
            let back = wsl_to_windows("Ubuntu", &wsl).unwrap();
            // 왕복 결과는 드라이브 대문자 + 백슬래시로 정규화된다
            assert!(back.to_lowercase().starts_with("e:\\a\\b"), "{back}");
        }
    }
}
