/// 다운로드 허용 URL 정책.
///
/// GitHub release asset의 최초 URL과 redirect 대상 호스트만 허용한다.
/// redirect 대상이 바뀔 수 있으므로 상수로 두고 근거를 남긴다.
/// - github.com: `https://github.com/jihoon22-lee/devbox/...` (2026-08 확인)
/// - objects.githubusercontent.com: 예전 release asset redirect 대상 (2026-08 확인)
/// - release-assets.githubusercontent.com: 현재 release asset redirect 대상 (2026-08 확인)
pub const ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// 허용 저장소 경로 (github.com에만 적용).
pub const REPO_PATH: &str = "jihoon22-lee/devbox";

/// `https` + 허용 host + 저장소 경로 하위인지 검사한다.
pub fn is_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, String::new()),
    };
    if !ALLOWED_HOSTS.contains(&host) {
        return false;
    }
    if host == "github.com" {
        return path == format!("/{REPO_PATH}") || path.starts_with(&format!("/{REPO_PATH}/"));
    }
    // objects.githubusercontent.com은 경로에 저장소가 없으므로 host 검사만 한다.
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_github_release_asset() {
        assert!(is_allowed(
            "https://github.com/jihoon22-lee/devbox/releases/download/v0.4.0/life-log.exe"
        ));
    }

    #[test]
    fn allows_redirect_host() {
        assert!(is_allowed(
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/12345/life-log.exe"
        ));
    }

    #[test]
    fn allows_release_assets_redirect_host() {
        assert!(is_allowed(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1330301814/d9a60c20/life-log.exe"
        ));
    }

    #[test]
    fn rejects_other_host() {
        assert!(!is_allowed(
            "https://evil.example.com/jihoon22-lee/devbox/x.exe"
        ));
    }

    #[test]
    fn rejects_http() {
        assert!(!is_allowed(
            "http://github.com/jihoon22-lee/devbox/releases/download/v0.4.0/x.exe"
        ));
    }

    #[test]
    fn rejects_other_repo() {
        assert!(!is_allowed(
            "https://github.com/someone-else/other/releases/download/v1.0.0/x.exe"
        ));
    }

    #[test]
    fn rejects_github_root_only() {
        // /jihoon22-lee/devbox 는 허용하지만 다른 루트 경로는 아니다
        assert!(!is_allowed("https://github.com/other"));
    }
}
