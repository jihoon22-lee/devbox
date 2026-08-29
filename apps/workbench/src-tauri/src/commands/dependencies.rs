//! Workbench package-dependency summary command.

use crate::commands::workspace::load_store;
use crate::core::dependency_summary::{
    read_package_dependency_summary_in, PackageDependencySummary,
};
use crate::core::profile::validate_profile_id;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const READ_ERROR: &str = "패키지 의존성 요약을 읽을 수 없습니다";

#[tauri::command]
pub async fn package_dependency_summary(
    app: AppHandle,
    profile_id: String,
) -> Result<PackageDependencySummary, String> {
    validate_profile_id(&profile_id)?;
    tokio::task::spawn_blocking(move || {
        let store = load_store(&app)?;
        let profile = store
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())?;
        let canonical_key = profile
            .canonical_key()
            .map_err(|_| READ_ERROR.to_string())?;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .unwrap_or(0);
        Ok(read_package_dependency_summary_in(
            &devbox_integration::integration_root(),
            &profile_id,
            &canonical_key,
            now_ms,
        ))
    })
    .await
    .map_err(|_| READ_ERROR.to_string())?
}
