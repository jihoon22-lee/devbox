//! §6.4 portable 설치 layout의 경로 계산. IO 없이 PathBuf 계산만 한다.
//!
//! ```text
//! apps/<app-id>/
//! ├─ versions/
//! │  ├─ 0.2.0/<app-id>.exe
//! │  └─ 0.3.0/<app-id>.exe
//! ├─ current.json
//! └─ download/<version>.partial
//! ```

use serde::{Deserialize, Serialize};

/// current.json 스키마. 최소 필드.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Current {
    pub version: String,
    pub exe_path: String,
    /// epoch ms
    pub installed_at: i64,
    pub previous_version: Option<String>,
}

/// `base/apps/<app-id>`
pub fn apps_root(base: &std::path::Path, app_id: &str) -> std::path::PathBuf {
    base.join("apps").join(app_id)
}

/// `base/apps/<app-id>/versions`
pub fn versions_root(base: &std::path::Path, app_id: &str) -> std::path::PathBuf {
    apps_root(base, app_id).join("versions")
}

/// `base/apps/<app-id>/versions/<version>`
pub fn version_dir(
    base: &std::path::Path,
    app_id: &str,
    version: &str,
) -> std::path::PathBuf {
    versions_root(base, app_id).join(version)
}

/// `base/apps/<app-id>/versions/<version>/<app-id>.exe`
pub fn version_exe(
    base: &std::path::Path,
    app_id: &str,
    version: &str,
) -> std::path::PathBuf {
    version_dir(base, app_id, version).join(format!("{app_id}.exe"))
}

/// `base/apps/<app-id>/current.json`
pub fn current_json(base: &std::path::Path, app_id: &str) -> std::path::PathBuf {
    apps_root(base, app_id).join("current.json")
}

/// `base/apps/<app-id>/download/<version>.partial`
pub fn partial_file(
    base: &std::path::Path,
    app_id: &str,
    version: &str,
) -> std::path::PathBuf {
    apps_root(base, app_id)
        .join("download")
        .join(format!("{version}.partial"))
}

/// rollback 대상 선택. 직전 정상 버전이 아직 설치되어 있으면 그것을 반환한다.
pub fn pick_rollback_target(current: &Current, installed_versions: &[String]) -> Option<String> {
    let previous = current.previous_version.as_ref()?;
    if installed_versions.iter().any(|v| v == previous) {
        Some(previous.clone())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> std::path::PathBuf {
        std::path::PathBuf::from("/data")
    }

    #[test]
    fn computes_layout_paths() {
        assert_eq!(
            version_dir(&base(), "life-log", "0.2.2"),
            std::path::Path::new("/data/apps/life-log/versions/0.2.2")
        );
        assert_eq!(
            version_exe(&base(), "life-log", "0.2.2"),
            std::path::Path::new("/data/apps/life-log/versions/0.2.2/life-log.exe")
        );
        assert_eq!(
            current_json(&base(), "life-log"),
            std::path::Path::new("/data/apps/life-log/current.json")
        );
        assert_eq!(
            partial_file(&base(), "life-log", "0.2.2"),
            std::path::Path::new("/data/apps/life-log/download/0.2.2.partial")
        );
    }

    #[test]
    fn rollback_target_prefers_previous_version() {
        let current = Current {
            version: "0.3.0".into(),
            exe_path: "/x".into(),
            installed_at: 1,
            previous_version: Some("0.2.2".into()),
        };
        let installed = vec!["0.2.2".to_string(), "0.3.0".to_string()];
        assert_eq!(
            pick_rollback_target(&current, &installed),
            Some("0.2.2".to_string())
        );
    }

    #[test]
    fn rollback_target_none_when_previous_missing() {
        let current = Current {
            version: "0.3.0".into(),
            exe_path: "/x".into(),
            installed_at: 1,
            previous_version: Some("0.2.2".into()),
        };
        let installed = vec!["0.3.0".to_string()];
        assert_eq!(pick_rollback_target(&current, &installed), None);
    }

    #[test]
    fn rollback_target_none_when_no_previous() {
        let current = Current {
            version: "0.2.0".into(),
            exe_path: "/x".into(),
            installed_at: 1,
            previous_version: None,
        };
        assert_eq!(pick_rollback_target(&current, &["0.2.0".to_string()]), None);
    }
}
