//! Explicit, read-only local-quality inspection. The response is deliberately
//! path-free, bounded, memory-only, and never sent to a remote service.

use crate::core::local_quality::{
    build_local_quality_snapshot, LocalQualitySnapshot, MAX_LOCAL_QUALITY_BYTES,
};
use std::time::{SystemTime, UNIX_EPOCH};

fn collect_local_quality(app: &tauri::AppHandle) -> Result<LocalQualitySnapshot, String> {
    let observed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "로컬 품질 상태를 확인할 수 없습니다.".to_string())?;
    let catalog = crate::commands::manager::local_quality_catalog_observation().ok();
    let registry = catalog
        .as_ref()
        .and_then(|_| crate::commands::manager::local_quality_registry_observation(app).ok());
    let snapshot = build_local_quality_snapshot(
        observed_at_ms,
        catalog,
        registry,
        devbox_integration::discover_report(),
    );
    let encoded = serde_json::to_vec(&snapshot)
        .map_err(|_| "로컬 품질 상태를 확인할 수 없습니다.".to_string())?;
    if encoded.len() > MAX_LOCAL_QUALITY_BYTES {
        return Err("로컬 품질 상태를 확인할 수 없습니다.".into());
    }
    Ok(snapshot)
}

#[tauri::command]
pub async fn inspect_local_quality(app: tauri::AppHandle) -> Result<LocalQualitySnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || collect_local_quality(&app))
        .await
        .map_err(|_| "로컬 품질 상태를 확인할 수 없습니다.".to_string())?
}
