//! Tauri command boundary for the Manager's diagnostics tools.
//!
//! Commands accept catalog app IDs and opaque preview/cancel IDs only. Raw
//! filesystem paths never come from the frontend and are never reflected in
//! public errors.

use crate::commands::doctor::DiagnosisItem;
use crate::core::catalog::parse_catalog;
use crate::core::data_inspector::{
    self, DataExport, DataInspectorSnapshot, DataQueryRequest, DataQueryResult, ExportFormat,
    QueryFailure,
};
use crate::core::support_bundle::{
    self, BundleFailure, SupportBundleExport, SupportBundlePreview, SupportDiagnostic,
    SupportInstalledApp, SUPPORT_PREVIEW_TTL_MS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const CATALOG_JSON: &str = include_str!("../../../../catalog.json");
const MAX_STORED_QUERY_PREVIEWS: usize = 16;
const MAX_STORED_QUERY_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
const MAX_STORED_BUNDLE_PREVIEWS: usize = 8;

#[derive(Default)]
pub struct DiagnosticsState {
    active_queries: Mutex<HashMap<String, Arc<AtomicBool>>>,
    active_bundles: Mutex<HashMap<String, Arc<AtomicBool>>>,
    query_previews: Mutex<HashMap<String, StoredQueryPreview>>,
    bundle_previews: Mutex<HashMap<String, StoredBundlePreview>>,
}

#[derive(Debug, Clone)]
struct StoredQueryPreview {
    result: DataQueryResult,
}

#[derive(Debug, Clone)]
struct StoredBundlePreview {
    expires_at_ms: u64,
    catalog_revision: Option<u64>,
    source_revision: String,
    draft: support_bundle::BundleDraft,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataExportRequest {
    pub preview_id: String,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelDiagnosticsRequest {
    pub operation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundleStatus {
    pub status: String,
    pub message: String,
}

fn data_root() -> Result<PathBuf, String> {
    dirs::data_local_dir()
        .ok_or_else(|| "devbox 데이터 경로를 안전하게 확인할 수 없습니다.".to_string())
}

fn catalog() -> Result<crate::core::catalog::Catalog, String> {
    parse_catalog(CATALOG_JSON).map_err(|_| "catalog를 안전하게 읽을 수 없습니다.".to_string())
}

fn lock_map<T>(map: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, String> {
    map.lock()
        .map_err(|_| "진단 작업을 시작할 수 없습니다.".to_string())
}

fn validate_operation_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > data_inspector::MAX_QUERY_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("진단 요청이 올바르지 않습니다.".to_string());
    }
    Ok(())
}

fn register_operation(
    map: &Mutex<HashMap<String, Arc<AtomicBool>>>,
    id: &str,
) -> Result<Arc<AtomicBool>, String> {
    validate_operation_id(id)?;
    let mut operations = lock_map(map)?;
    if operations.contains_key(id) {
        return Err("같은 진단 작업이 이미 실행 중입니다.".to_string());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    operations.insert(id.to_string(), cancel.clone());
    Ok(cancel)
}

fn finish_operation(map: &Mutex<HashMap<String, Arc<AtomicBool>>>, id: &str) {
    if let Ok(mut operations) = map.lock() {
        operations.remove(id);
    }
}

fn query_error(error: QueryFailure) -> String {
    error.message().to_string()
}

fn bundle_error(error: BundleFailure) -> String {
    error.message().to_string()
}

fn take_query_preview(
    state: &DiagnosticsState,
    preview_id: &str,
) -> Result<StoredQueryPreview, String> {
    let mut previews = lock_map(&state.query_previews)?;
    previews
        .remove(preview_id)
        .ok_or_else(|| "조회 미리 보기가 만료되었거나 없습니다.".to_string())
}

fn take_bundle_preview(
    state: &DiagnosticsState,
    preview_id: &str,
) -> Result<StoredBundlePreview, String> {
    let mut previews = lock_map(&state.bundle_previews)?;
    previews
        .remove(preview_id)
        .ok_or_else(|| "지원 번들 미리 보기가 만료되었거나 없습니다.".to_string())
}

fn generated_id(prefix: &str, input: &str) -> String {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    digest.update(input.as_bytes());
    digest.update(sequence.to_le_bytes());
    digest.update(now.to_le_bytes());
    format!("{prefix}-{:x}", digest.finalize())
}

#[tauri::command]
pub async fn inspect_data_databases(
    state: tauri::State<'_, DiagnosticsState>,
    operation_id: String,
) -> Result<DataInspectorSnapshot, String> {
    let cancel = register_operation(&state.active_queries, &operation_id)?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog()?;
        let root = data_root()?;
        data_inspector::inspect_databases(&catalog, &root, Some(cancel)).map_err(query_error)
    });
    let result = match task.await {
        Ok(result) => result,
        Err(_) => Err("데이터베이스 진단 작업을 완료할 수 없습니다.".to_string()),
    };
    finish_operation(&state.active_queries, &operation_id);
    result
}

#[tauri::command]
pub async fn preview_data_query(
    state: tauri::State<'_, DiagnosticsState>,
    request: DataQueryRequest,
) -> Result<DataQueryResult, String> {
    let operation_id = request.query_id.clone();
    let cancel = register_operation(&state.active_queries, &operation_id)?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog()?;
        let root = data_root()?;
        data_inspector::preview_query(&catalog, &root, &request, cancel)
            .map(|(result, _)| result)
            .map_err(query_error)
    });
    let result = match task.await {
        Ok(result) => result,
        Err(_) => Err("읽기 전용 조회 작업을 완료할 수 없습니다.".to_string()),
    };
    finish_operation(&state.active_queries, &operation_id);
    let mut result = result?;
    result.preview_id = generated_id("query", &operation_id);
    let mut previews = lock_map(&state.query_previews)?;
    let mut retained_bytes = previews
        .values()
        .map(|preview| preview.result.result_bytes)
        .sum::<usize>();
    while previews.len() >= MAX_STORED_QUERY_PREVIEWS
        || retained_bytes.saturating_add(result.result_bytes) > MAX_STORED_QUERY_PREVIEW_BYTES
    {
        if let Some(key) = previews.keys().next().cloned() {
            if let Some(evicted) = previews.remove(&key) {
                retained_bytes = retained_bytes.saturating_sub(evicted.result.result_bytes);
            }
        } else {
            break;
        }
    }
    previews.insert(
        result.preview_id.clone(),
        StoredQueryPreview {
            result: result.clone(),
        },
    );
    Ok(result)
}

#[tauri::command]
pub fn cancel_data_diagnostics(
    state: tauri::State<'_, DiagnosticsState>,
    request: CancelDiagnosticsRequest,
) -> Result<SupportBundleStatus, String> {
    validate_operation_id(&request.operation_id)?;
    let operations = lock_map(&state.active_queries)?;
    if let Some(cancel) = operations.get(&request.operation_id) {
        cancel.store(true, Ordering::Relaxed);
        return Ok(SupportBundleStatus {
            status: "cancel-requested".to_string(),
            message: "진단 취소를 요청했습니다.".to_string(),
        });
    }
    Err("진단 작업이 이미 끝났거나 없습니다.".to_string())
}

#[tauri::command]
pub async fn export_data_preview(
    state: tauri::State<'_, DiagnosticsState>,
    request: DataExportRequest,
) -> Result<DataExport, String> {
    validate_operation_id(&request.preview_id)?;
    // Claim the preview under the mutex. Looking it up, doing revision I/O,
    // and removing it later would allow concurrent export commands to clone
    // the same one-time result.
    let stored = take_query_preview(&state, &request.preview_id)?;
    let app_id = stored.result.app_id.clone();
    let current_revision = tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog()?;
        let root = data_root()?;
        data_inspector::database_revision(&catalog, &root, &app_id).map_err(query_error)
    })
    .await
    .map_err(|_| "조회 원본 상태를 확인할 수 없습니다.".to_string())??;
    if current_revision != stored.result.database_revision {
        return Err(QueryFailure::Stale.message().to_string());
    }
    data_inspector::export_query(&stored.result, request.format).map_err(query_error)
}

fn diagnosis_for_bundle(app: &tauri::AppHandle) -> Vec<SupportDiagnostic> {
    crate::commands::doctor::collect_diagnosis(app)
        .into_iter()
        .map(|item: DiagnosisItem| SupportDiagnostic {
            name: item.name,
            ok: item.ok,
            detail: item.detail,
        })
        .collect()
}

fn installed_for_bundle(app: &tauri::AppHandle) -> Vec<SupportInstalledApp> {
    crate::commands::manager::installed(app.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|item| SupportInstalledApp {
            app_id: item.app,
            version: item.version,
            mode: item.mode,
        })
        .collect()
}

#[tauri::command]
pub async fn preview_support_bundle(
    app: tauri::AppHandle,
    state: tauri::State<'_, DiagnosticsState>,
    operation_id: String,
) -> Result<SupportBundlePreview, String> {
    let cancel = register_operation(&state.active_bundles, &operation_id)?;
    let task = tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog()?;
        let root = data_root()?;
        let draft = support_bundle::build_bundle(
            &catalog,
            &root,
            diagnosis_for_bundle(&app),
            installed_for_bundle(&app),
            cancel,
        )
        .map_err(bundle_error)?;
        Ok::<_, String>((draft, catalog.catalog_revision))
    });
    let result = match task.await {
        Ok(result) => result,
        Err(_) => Err("지원 번들 작업을 완료할 수 없습니다.".to_string()),
    };
    finish_operation(&state.active_bundles, &operation_id);
    let (draft, catalog_revision) = result?;
    let preview_id = generated_id("support", &operation_id);
    let expires_at_ms = now_ms().saturating_add(SUPPORT_PREVIEW_TTL_MS);
    let preview = SupportBundlePreview {
        preview_id: preview_id.clone(),
        catalog_revision,
        expires_at_ms,
        estimated_bytes: draft.bytes.len(),
        database_count: draft.available_database_count(),
        included_sections: vec![
            "app-metadata".to_string(),
            "catalog-metadata".to_string(),
            "schema-metadata".to_string(),
            "log-metadata".to_string(),
            "diagnosis".to_string(),
        ],
        omitted_sections: vec![
            "raw-database".to_string(),
            "raw-logs".to_string(),
            "paths".to_string(),
            "environment-values".to_string(),
            "credentials".to_string(),
            "authorization".to_string(),
        ],
        redaction_version: data_inspector::REDACTION_VERSION.to_string(),
    };
    let mut previews = lock_map(&state.bundle_previews)?;
    if previews.len() >= MAX_STORED_BUNDLE_PREVIEWS {
        if let Some(key) = previews.keys().next().cloned() {
            previews.remove(&key);
        }
    }
    previews.insert(
        preview_id,
        StoredBundlePreview {
            expires_at_ms,
            catalog_revision,
            source_revision: draft.source_revision.clone(),
            draft,
        },
    );
    Ok(preview)
}

#[tauri::command]
pub fn cancel_support_bundle(
    state: tauri::State<'_, DiagnosticsState>,
    request: CancelDiagnosticsRequest,
) -> Result<SupportBundleStatus, String> {
    validate_operation_id(&request.operation_id)?;
    let operations = lock_map(&state.active_bundles)?;
    if let Some(cancel) = operations.get(&request.operation_id) {
        cancel.store(true, Ordering::Relaxed);
        return Ok(SupportBundleStatus {
            status: "cancel-requested".to_string(),
            message: "지원 번들 생성 취소를 요청했습니다.".to_string(),
        });
    }
    Err("지원 번들 작업이 이미 끝났거나 없습니다.".to_string())
}

#[tauri::command]
pub async fn export_support_bundle(
    state: tauri::State<'_, DiagnosticsState>,
    preview_id: String,
) -> Result<SupportBundleExport, String> {
    validate_operation_id(&preview_id)?;
    // Claim the token before any I/O. A clone-then-remove sequence lets two
    // concurrent export commands both pass the preview lookup and emit the
    // supposedly one-time bundle.
    let stored = take_bundle_preview(&state, &preview_id)?;
    if now_ms() >= stored.expires_at_ms {
        return Err("지원 번들 미리 보기가 만료되었습니다. 다시 미리 확인하세요.".to_string());
    }
    let expected_catalog_revision = stored.catalog_revision;
    let expected_source_revision = stored.source_revision.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let catalog = catalog()?;
        let root = data_root()?;
        let current_source_revision =
            support_bundle::current_source_revision(&catalog, &root).map_err(bundle_error)?;
        if catalog.catalog_revision != expected_catalog_revision
            || current_source_revision != expected_source_revision
        {
            return Err(
                "진단 상태가 바뀌었습니다. 최신 지원 번들을 다시 미리 확인하세요.".to_string(),
            );
        }
        Ok(())
    })
    .await
    .map_err(|_| "지원 번들 원본 상태를 확인할 수 없습니다.".to_string())??;
    support_bundle::export_bundle(&stored.draft).map_err(bundle_error)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn query_preview() -> StoredQueryPreview {
        StoredQueryPreview {
            result: DataQueryResult {
                preview_id: "preview".into(),
                query_id: "query".into(),
                app_id: "app".into(),
                database_revision: "revision".into(),
                columns: vec!["value".into()],
                rows: vec![vec![serde_json::Value::String("ok".into())]],
                row_count: 1,
                result_bytes: 4,
                truncated: false,
                elapsed_ms: 1,
            },
        }
    }

    #[test]
    fn one_time_query_preview_claim_allows_only_one_concurrent_export() {
        let state = Arc::new(DiagnosticsState::default());
        state
            .query_previews
            .lock()
            .unwrap()
            .insert("preview".into(), query_preview());
        let handles = (0..2)
            .map(|_| {
                let state = Arc::clone(&state);
                thread::spawn(move || take_query_preview(&state, "preview").is_ok())
            })
            .collect::<Vec<_>>();
        let claimed = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|claimed| *claimed)
            .count();
        assert_eq!(claimed, 1);
    }
}
