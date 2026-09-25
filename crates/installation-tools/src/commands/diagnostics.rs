//! Tauri command boundary for the Manager's diagnostics tools.
//!
//! Commands accept opaque preview/cancel IDs only. Raw
//! filesystem paths never come from the frontend and are never reflected in
//! public errors.

use crate::commands::doctor::DiagnosisItem;
use crate::core::redaction;
use crate::core::support_bundle::{
    self, BundleFailure, SupportBundleExport, SupportBundlePreview, SupportDiagnostic,
    SUPPORT_PREVIEW_TTL_MS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STORED_BUNDLE_PREVIEWS: usize = 8;

#[derive(Default)]
pub struct DiagnosticsState {
    active_bundles: Mutex<HashMap<String, Arc<AtomicBool>>>,
    bundle_previews: Mutex<HashMap<String, StoredBundlePreview>>,
}

#[derive(Debug, Clone)]
struct StoredBundlePreview {
    expires_at_ms: u64,
    draft: support_bundle::BundleDraft,
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

fn lock_map<T>(map: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, String> {
    map.lock()
        .map_err(|_| "진단 작업을 시작할 수 없습니다.".to_string())
}

fn validate_operation_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
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

fn bundle_error(error: BundleFailure) -> String {
    error.message().to_string()
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

/// Suite members share the installation suffix of their data identifiers
/// (`<product identifier>.i<suffix>`), so Control Center can find the other
/// products' log folders. Portable builds have per-product suffixes and
/// simply report the other folders as missing.
pub(crate) fn suite_log_dirs(
    data_root: &std::path::Path,
    own_identifier: &str,
    products: &[(String, String)],
) -> Vec<(String, PathBuf)> {
    let Some((_, suffix)) = own_identifier.rsplit_once(".i") else {
        return Vec::new();
    };
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Vec::new();
    }
    products
        .iter()
        .map(|(id, identifier)| {
            (
                id.clone(),
                data_root
                    .join(format!("{identifier}.i{suffix}"))
                    .join("logs"),
            )
        })
        .collect()
}

fn operation_summaries(
    app: &tauri::AppHandle,
    data_root: &std::path::Path,
) -> Vec<support_bundle::OperationLogSummary> {
    let Ok(catalog) =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)
    else {
        return Vec::new();
    };
    let products: Vec<(String, String)> = catalog
        .products
        .iter()
        .map(|product| (product.id.clone(), product.identifier.clone()))
        .collect();
    suite_log_dirs(data_root, &app.config().identifier, &products)
        .into_iter()
        .map(|(product, dir)| support_bundle::OperationLogSummary {
            product,
            summary: if redaction::safe_derived_path(data_root, &dir) {
                product_contract::operation_log::summarize(&dir, 100, 2 * 1024 * 1024)
            } else {
                product_contract::operation_log::Summary {
                    state: "unreadable".into(),
                    ..Default::default()
                }
            },
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
        let catalog =
            devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)
                .map_err(str::to_owned)?;
        let root = data_root()?;
        let draft = support_bundle::build_bundle(
            diagnosis_for_bundle(&app),
            catalog
                .products
                .iter()
                .map(|product| support_bundle::SupportProduct {
                    id: product.id.clone(),
                    version: app.package_info().version.to_string(),
                })
                .collect(),
            operation_summaries(&app, &root),
            cancel,
        )
        .map_err(bundle_error)?;
        Ok::<_, String>(draft)
    });
    let result = match task.await {
        Ok(result) => result,
        Err(_) => Err("지원 번들 작업을 완료할 수 없습니다.".to_string()),
    };
    finish_operation(&state.active_bundles, &operation_id);
    let draft = result?;
    let preview_id = generated_id("support", &operation_id);
    let expires_at_ms = now_ms().saturating_add(SUPPORT_PREVIEW_TTL_MS);
    let preview = SupportBundlePreview {
        preview_id: preview_id.clone(),
        expires_at_ms,
        estimated_bytes: draft.bytes.len(),
        database_count: 0,
        included_sections: vec![
            "diagnosis".into(),
            "products".into(),
            "operation-log".into(),
        ],
        omitted_sections: vec![
            "raw-database".to_string(),
            "raw-logs".to_string(),
            "paths".to_string(),
            "environment-values".to_string(),
            "credentials".to_string(),
            "authorization".to_string(),
        ],
        redaction_version: redaction::REDACTION_VERSION.to_string(),
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
    #[test]
    fn suite_log_dirs_share_the_installation_suffix() {
        let products = vec![
            (
                "knowledge".to_string(),
                "com.devbox.v08.knowledge".to_string(),
            ),
            (
                "control-center".to_string(),
                "com.devbox.v08.controlcenter".to_string(),
            ),
        ];
        let root = std::path::Path::new("/data");
        let dirs = suite_log_dirs(root, "com.devbox.v08.controlcenter.iabc123", &products);
        assert_eq!(
            dirs,
            vec![
                (
                    "knowledge".to_string(),
                    root.join("com.devbox.v08.knowledge.iabc123").join("logs")
                ),
                (
                    "control-center".to_string(),
                    root.join("com.devbox.v08.controlcenter.iabc123")
                        .join("logs")
                ),
            ]
        );
        assert!(suite_log_dirs(root, "com.devbox.v08.controlcenter", &products).is_empty());
        assert!(suite_log_dirs(root, "com.devbox.v08.controlcenter.i../x", &products).is_empty());
    }
    use super::*;
    use std::thread;

    #[test]
    fn one_time_bundle_preview_claim_allows_only_one_export() {
        let state = Arc::new(DiagnosticsState::default());
        state.bundle_previews.lock().unwrap().insert(
            "preview".into(),
            StoredBundlePreview {
                expires_at_ms: u64::MAX,
                draft: support_bundle::BundleDraft { bytes: vec![] },
            },
        );
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let state = state.clone();
                thread::spawn(move || take_bundle_preview(&state, "preview").is_ok())
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| usize::from(worker.join().unwrap()))
                .sum::<usize>(),
            1
        );
    }
}
