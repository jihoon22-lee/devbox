use crate::core::profile::ProjectProfile;
use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};

fn safe_windows_path(profile: &ProjectProfile) -> Option<String> {
    profile
        .windows_path
        .as_deref()
        .and_then(parse_safe_project_path)
        .filter(|path| path.kind() != ProjectPathKind::Posix)
        .map(|path| path.into_string())
}

/// 일반 Path handoff와 사용자 요청의 경로 복사는 안전한 Windows 경로를
/// 우선하고, 없으면 안전한 WSL/POSIX profile path로 폴백한다.
pub fn profile_path(profile: &ProjectProfile) -> Result<String, &'static str> {
    safe_windows_path(profile)
        .or_else(|| {
            profile
                .wsl
                .as_ref()
                .and_then(|wsl| parse_safe_project_path(&wsl.path))
                .filter(|path| path.kind() == ProjectPathKind::Posix)
                .map(|path| path.into_string())
        })
        .ok_or("프로필에 안전하게 사용할 프로젝트 경로가 없습니다")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::profile::WslProfile;

    fn profile(windows: Option<&str>, posix: Option<&str>) -> ProjectProfile {
        let mut profile = ProjectProfile::new("devbox");
        profile.id = "profile-1".to_string();
        profile.windows_path = windows.map(str::to_string);
        profile.wsl = posix.map(|path| WslProfile {
            distro: "Ubuntu".to_string(),
            path: path.to_string(),
        });
        profile
    }

    #[test]
    fn profile_paths_are_bounded_and_workspace_requires_windows() {
        let dual = profile(
            Some(" C:\\projects\\devbox\\ "),
            Some("/mnt/e/projects/devbox"),
        );
        assert_eq!(profile_path(&dual).unwrap(), "C:\\projects\\devbox");

        let posix = profile(None, Some("/home/me/devbox"));
        assert_eq!(profile_path(&posix).unwrap(), "/home/me/devbox");

        let secret = "C:\\projects\\..\\TOP_SECRET";
        let invalid = profile(Some(secret), None);
        let error = profile_path(&invalid).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));

        let wrong_wsl_kind = profile(None, Some("C:\\projects\\TOP_SECRET"));
        let error = profile_path(&wrong_wsl_kind).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));
    }
}
