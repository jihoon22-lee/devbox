use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_BATCH_ITEMS: usize = 32;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchInstallRequest {
    pub app_id: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchInstallResult {
    pub app_id: String,
    pub mode: String,
    pub ok: bool,
    pub message: String,
}

impl BatchInstallResult {
    pub fn success(request: &BatchInstallRequest, message: String) -> Self {
        Self {
            app_id: request.app_id.clone(),
            mode: request.mode.clone(),
            ok: true,
            message,
        }
    }

    /// Batch failures deliberately do not include lower-level network,
    /// process, or filesystem errors. Those values can contain a URL or a
    /// local path and are not part of the frontend contract.
    pub fn retryable_failure(request: &BatchInstallRequest) -> Self {
        Self {
            app_id: request.app_id.clone(),
            mode: request.mode.clone(),
            ok: false,
            message: "설치/업데이트에 실패했습니다. 앱 상태를 확인한 뒤 이 항목만 다시 시도하세요."
                .to_string(),
        }
    }

    pub fn shared_failure(request: &BatchInstallRequest) -> Self {
        Self {
            app_id: request.app_id.clone(),
            mode: request.mode.clone(),
            ok: false,
            message: "공통 설치 정보를 준비하지 못했습니다. 연결 상태를 확인한 뒤 다시 시도하세요."
                .to_string(),
        }
    }
}

pub fn validate_batch_requests(requests: &[BatchInstallRequest]) -> Result<(), String> {
    if requests.is_empty() {
        return Err("일괄 작업 대상을 하나 이상 선택하세요.".to_string());
    }
    if requests.len() > MAX_BATCH_ITEMS {
        return Err(format!(
            "일괄 작업은 한 번에 최대 {MAX_BATCH_ITEMS}개까지 실행할 수 있습니다."
        ));
    }

    let mut ids = HashSet::with_capacity(requests.len());
    for request in requests {
        if !valid_app_id(&request.app_id) {
            return Err("일괄 작업에 올바르지 않은 앱 ID가 포함되어 있습니다.".to_string());
        }
        if !matches!(request.mode.as_str(), "portable" | "installer") {
            return Err("일괄 작업에 지원하지 않는 설치 방식이 포함되어 있습니다.".to_string());
        }
        if !ids.insert(request.app_id.as_str()) {
            return Err("일괄 작업에 같은 앱이 두 번 포함되어 있습니다.".to_string());
        }
    }
    Ok(())
}

/// Returns true only when a batch item is an install or a strict SemVer
/// upgrade. Equal or newer installed versions are safe no-ops, so stale UI
/// state cannot turn a batch update into a downgrade.
pub fn is_install_or_upgrade(
    installed_version: Option<&str>,
    available_version: &str,
) -> Result<bool, String> {
    let available = semver::Version::parse(available_version)
        .map_err(|_| "사용 가능한 앱 버전 정보가 올바르지 않습니다.".to_string())?;
    let Some(installed_version) = installed_version else {
        return Ok(true);
    };
    let installed = semver::Version::parse(installed_version)
        .map_err(|_| "설치된 앱 버전 정보가 올바르지 않습니다.".to_string())?;
    Ok(available > installed)
}

fn valid_app_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(app_id: &str, mode: &str) -> BatchInstallRequest {
        BatchInstallRequest {
            app_id: app_id.to_string(),
            mode: mode.to_string(),
        }
    }

    #[test]
    fn accepts_unique_bounded_catalog_identities() {
        assert!(validate_batch_requests(&[
            request("port-manager", "portable"),
            request("code-pad", "installer"),
        ])
        .is_ok());
    }

    #[test]
    fn rejects_empty_duplicate_oversized_and_unsafe_requests() {
        assert!(validate_batch_requests(&[]).is_err());
        assert!(validate_batch_requests(&[
            request("port-manager", "portable"),
            request("port-manager", "installer"),
        ])
        .is_err());
        assert!(validate_batch_requests(
            &(0..=MAX_BATCH_ITEMS)
                .map(|index| request(&format!("app-{index}"), "portable"))
                .collect::<Vec<_>>()
        )
        .is_err());
        assert!(validate_batch_requests(&[request("../secret", "portable")]).is_err());
        assert!(validate_batch_requests(&[request("port-manager", "archive")]).is_err());
    }

    #[test]
    fn failure_result_never_reflects_a_lower_level_error() {
        let request = request("port-manager", "portable");
        let raw_error = r"C:\Users\private\token.txt download failed";

        let result = BatchInstallResult::retryable_failure(&request);
        let json = serde_json::to_string(&result).unwrap();

        assert!(!json.contains(raw_error));
        assert!(!json.contains("private"));
        assert_eq!(result.app_id, "port-manager");
        assert!(!result.ok);
    }

    #[test]
    fn batch_never_downgrades_equal_newer_or_prerelease_versions() {
        assert_eq!(is_install_or_upgrade(None, "0.4.0"), Ok(true));
        assert_eq!(is_install_or_upgrade(Some("0.3.1"), "0.4.0"), Ok(true));
        assert_eq!(is_install_or_upgrade(Some("0.4.0"), "0.4.0"), Ok(false));
        assert_eq!(is_install_or_upgrade(Some("0.5.0"), "0.4.0"), Ok(false));
        assert_eq!(
            is_install_or_upgrade(Some("0.4.0"), "0.4.0-rc.1"),
            Ok(false)
        );
        assert!(is_install_or_upgrade(Some("not-a-version"), "0.4.0").is_err());
    }
}
