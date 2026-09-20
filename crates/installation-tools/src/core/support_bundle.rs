//! Privacy-safe support bundle construction.
//!
//! A bundle is a bounded metadata report, never a database or log archive.
//! Paths, usernames, environment values, credentials, authorization headers,
//! raw query text, and raw log lines are deliberately omitted or redacted.

use super::catalog::Catalog;
use super::data_inspector::{self, DataDatabaseInfo, QueryFailure, REDACTION_VERSION};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_SUPPORT_BUNDLE_BYTES: usize = 512 * 1024;
pub const SUPPORT_PREVIEW_TTL_MS: u64 = 5 * 60 * 1_000;
pub const MAX_LOG_METADATA_FILES: usize = 128;
pub const MAX_LOG_METADATA_ENTRIES: usize = 512;
pub const MAX_LOG_METADATA_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundlePreview {
    pub preview_id: String,
    pub catalog_revision: Option<u64>,
    pub expires_at_ms: u64,
    pub estimated_bytes: usize,
    pub database_count: usize,
    pub included_sections: Vec<String>,
    pub omitted_sections: Vec<String>,
    pub redaction_version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundleExport {
    pub filename: String,
    pub mime_type: String,
    pub content: String,
    pub byte_count: usize,
    pub redaction_version: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportDiagnostic {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportInstalledApp {
    pub app_id: String,
    pub version: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SupportBundleDocument {
    schema_version: u32,
    generated_at_ms: u64,
    redaction: RedactionContract,
    diagnosis: Vec<SupportDiagnostic>,
    catalog: SupportCatalog,
    databases: Vec<DataDatabaseInfo>,
    logs: Vec<LogMetadata>,
    omitted: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RedactionContract {
    version: &'static str,
    paths: &'static str,
    usernames: &'static str,
    secrets: &'static str,
    auth_headers: &'static str,
    raw_database: &'static str,
    raw_logs: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SupportCatalog {
    schema_version: u32,
    catalog_revision: Option<u64>,
    app_count: usize,
    apps: Vec<SupportCatalogApp>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SupportCatalogApp {
    app_id: String,
    display_name: String,
    identifier: String,
    manager_visible: bool,
    self_managed: bool,
    installed_version: Option<String>,
    installed_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LogMetadata {
    app_id: String,
    state: String,
    file_count: usize,
    byte_length: u64,
    truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleFailure {
    Cancelled,
    UnsafeDataRoot,
    Inspector(QueryFailure),
    TooLarge,
}

impl BundleFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "지원 번들 생성이 취소되었습니다.",
            Self::UnsafeDataRoot => "devbox 데이터 경로를 안전하게 확인할 수 없습니다.",
            Self::Inspector(error) => error.message(),
            Self::TooLarge => "지원 번들이 허용된 크기를 초과했습니다.",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BundleDraft {
    document: SupportBundleDocument,
    pub bytes: Vec<u8>,
    /// Revision of the catalog-derived database and log metadata used to
    /// build this exact byte document. Diagnosis/install details are retained
    /// as the reviewed snapshot rather than silently refreshed at export.
    pub source_revision: String,
}

impl BundleDraft {
    pub(crate) fn available_database_count(&self) -> usize {
        self.document
            .databases
            .iter()
            .filter(|database| database.state == "available")
            .count()
    }
}

/// Build a sanitized, bounded document. The caller retains `bytes` in the
/// preview token so an explicit export returns exactly what the user reviewed.
pub fn build_bundle(
    catalog: &Catalog,
    data_root: &Path,
    diagnosis: Vec<SupportDiagnostic>,
    installed: Vec<SupportInstalledApp>,
    cancel: Arc<AtomicBool>,
) -> Result<BundleDraft, BundleFailure> {
    if !data_root.is_absolute() || data_root.to_string_lossy().len() > 4096 {
        return Err(BundleFailure::UnsafeDataRoot);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(BundleFailure::Cancelled);
    }
    let data = data_inspector::inspect_databases(catalog, data_root, Some(cancel.clone()))
        .map_err(BundleFailure::Inspector)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(BundleFailure::Cancelled);
    }
    let logs = collect_log_metadata(catalog, data_root, &cancel)?;
    let installed_by_id = installed
        .into_iter()
        .map(|app| (app.app_id.clone(), app))
        .collect::<HashMap<_, _>>();
    let database_revisions = data
        .databases
        .iter()
        .map(|database| {
            format!(
                "{}:{}:{}",
                database.app_id,
                database.state,
                database.revision.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>();
    let source_logs = logs.clone();
    let document = SupportBundleDocument {
        schema_version: 1,
        generated_at_ms: now_ms(),
        redaction: RedactionContract {
            version: REDACTION_VERSION,
            paths: "omitted",
            usernames: "omitted",
            secrets: "omitted",
            auth_headers: "omitted",
            raw_database: "omitted",
            raw_logs: "omitted",
        },
        diagnosis: diagnosis
            .into_iter()
            .take(64)
            .map(|item| SupportDiagnostic {
                name: data_inspector::redact_text(&item.name, "support-bundle"),
                ok: item.ok,
                detail: data_inspector::redact_text(&item.detail, "support-bundle"),
            })
            .collect(),
        catalog: SupportCatalog {
            schema_version: catalog.schema_version,
            catalog_revision: catalog.catalog_revision,
            app_count: catalog.apps.len(),
            apps: catalog
                .apps
                .iter()
                .take(data_inspector::MAX_DATABASES)
                .map(|app| {
                    let installed = installed_by_id.get(&app.id);
                    SupportCatalogApp {
                        app_id: data_inspector::redact_text(&app.id, "support-bundle"),
                        display_name: data_inspector::redact_text(
                            &app.display_name,
                            "support-bundle",
                        ),
                        identifier: data_inspector::redact_text(&app.identifier, "support-bundle"),
                        manager_visible: app.manager_visible,
                        self_managed: app.self_managed,
                        installed_version: installed.map(|value| {
                            data_inspector::redact_text(&value.version, "support-bundle")
                        }),
                        installed_mode: installed.map(|value| {
                            data_inspector::redact_text(&value.mode, "support-bundle")
                        }),
                    }
                })
                .collect(),
        },
        databases: data.databases.into_iter().map(sanitize_database).collect(),
        logs: logs
            .into_iter()
            .map(|mut log| {
                log.app_id = data_inspector::redact_text(&log.app_id, "support-bundle");
                log
            })
            .collect(),
        omitted: vec![
            "raw-database-bytes",
            "raw-query-text",
            "raw-log-lines",
            "filesystem-paths",
            "environment-values",
            "credentials",
            "authorization-headers",
        ],
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| BundleFailure::TooLarge)?;
    if bytes.len() > MAX_SUPPORT_BUNDLE_BYTES {
        return Err(BundleFailure::TooLarge);
    }
    // Include every catalog entry and its state, not only available revisions.
    // That way a DB appearing, disappearing, or becoming unsafe invalidates a
    // preview before export even when the set of readable files is unchanged.
    let source_revision =
        source_revision(catalog.catalog_revision, &database_revisions, &source_logs);
    Ok(BundleDraft {
        document,
        bytes,
        source_revision,
    })
}

fn sanitize_database(mut database: DataDatabaseInfo) -> DataDatabaseInfo {
    database.app_id = data_inspector::redact_text(&database.app_id, "support-bundle");
    database.display_name = data_inspector::redact_text(&database.display_name, "support-bundle");
    database.identifier = data_inspector::redact_text(&database.identifier, "support-bundle");
    database.warning = database
        .warning
        .as_deref()
        .map(|value| data_inspector::redact_text(value, "support-bundle"));
    for object in database.tables.iter_mut().chain(database.views.iter_mut()) {
        object.name = data_inspector::redact_text(&object.name, "support-bundle");
    }
    database
}

/// Recompute only the source metadata that can change between preview and
/// export. This avoids rebuilding diagnosis/install DTOs (which would make
/// the exported bytes differ from the content the user reviewed).
pub fn current_source_revision(
    catalog: &Catalog,
    data_root: &Path,
) -> Result<String, BundleFailure> {
    let database_revisions = data_inspector::database_state_revisions(catalog, data_root)
        .map_err(BundleFailure::Inspector)?;
    let logs = collect_log_metadata(catalog, data_root, &AtomicBool::new(false))?;
    Ok(source_revision(
        catalog.catalog_revision,
        &database_revisions,
        &logs,
    ))
}

fn source_revision(
    catalog_revision: Option<u64>,
    database_revisions: &[String],
    logs: &[LogMetadata],
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"devbox-support-source-v1\0");
    digest.update(catalog_revision.unwrap_or_default().to_le_bytes());
    for revision in database_revisions {
        digest.update(revision.as_bytes());
        digest.update([0]);
    }
    for log in logs {
        digest.update(log.app_id.as_bytes());
        digest.update([0]);
        digest.update(log.state.as_bytes());
        digest.update([0]);
        digest.update(log.file_count.to_le_bytes());
        digest.update(log.byte_length.to_le_bytes());
        digest.update([u8::from(log.truncated)]);
    }
    format!("{:x}", digest.finalize())
}

pub fn export_bundle(draft: &BundleDraft) -> Result<SupportBundleExport, BundleFailure> {
    if draft.bytes.len() > MAX_SUPPORT_BUNDLE_BYTES {
        return Err(BundleFailure::TooLarge);
    }
    let content = String::from_utf8(draft.bytes.clone()).map_err(|_| BundleFailure::TooLarge)?;
    Ok(SupportBundleExport {
        filename: "devbox-support-bundle.json".to_string(),
        mime_type: "application/json".to_string(),
        content,
        byte_count: draft.bytes.len(),
        redaction_version: REDACTION_VERSION.to_string(),
    })
}

fn collect_log_metadata(
    catalog: &Catalog,
    data_root: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<LogMetadata>, BundleFailure> {
    let mut result = Vec::new();
    for app in catalog.apps.iter().take(data_inspector::MAX_DATABASES) {
        if cancel.load(Ordering::Relaxed) {
            return Err(BundleFailure::Cancelled);
        }
        let app_root = data_root.join(&app.identifier);
        let logs = app_root.join("logs");
        if !safe_log_root(data_root, &app.identifier, &logs) {
            result.push(LogMetadata {
                app_id: app.id.clone(),
                state: "unsafe-path".to_string(),
                file_count: 0,
                byte_length: 0,
                truncated: false,
            });
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&logs) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                result.push(LogMetadata {
                    app_id: app.id.clone(),
                    state: "missing".to_string(),
                    file_count: 0,
                    byte_length: 0,
                    truncated: false,
                });
                continue;
            }
            Err(_) => {
                result.push(LogMetadata {
                    app_id: app.id.clone(),
                    state: "unreadable".to_string(),
                    file_count: 0,
                    byte_length: 0,
                    truncated: false,
                });
                continue;
            }
        };
        if !metadata.is_dir() || data_inspector::is_link_or_reparse(&metadata) {
            result.push(LogMetadata {
                app_id: app.id.clone(),
                state: "unsafe-path".to_string(),
                file_count: 0,
                byte_length: 0,
                truncated: false,
            });
            continue;
        }
        let mut file_count = 0usize;
        let mut byte_length = 0u64;
        let mut truncated = false;
        let entries = match std::fs::read_dir(&logs) {
            Ok(entries) => entries,
            Err(_) => {
                result.push(LogMetadata {
                    app_id: app.id.clone(),
                    state: "unreadable".to_string(),
                    file_count: 0,
                    byte_length: 0,
                    truncated: false,
                });
                continue;
            }
        };
        for (entries_seen, entry) in entries.enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(BundleFailure::Cancelled);
            }
            if entries_seen >= MAX_LOG_METADATA_ENTRIES
                || file_count >= MAX_LOG_METADATA_FILES
                || byte_length >= MAX_LOG_METADATA_BYTES
            {
                truncated = true;
                break;
            }
            let Ok(entry) = entry else {
                truncated = true;
                continue;
            };
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                truncated = true;
                continue;
            };
            if data_inspector::is_link_or_reparse(&metadata) {
                truncated = true;
                continue;
            }
            if metadata.is_file() {
                file_count += 1;
                byte_length = byte_length.saturating_add(
                    metadata
                        .len()
                        .min(MAX_LOG_METADATA_BYTES.saturating_sub(byte_length)),
                );
            }
        }
        result.push(LogMetadata {
            app_id: app.id.clone(),
            state: "available".to_string(),
            file_count,
            byte_length,
            truncated,
        });
    }
    Ok(result)
}

fn safe_log_root(data_root: &Path, identifier: &str, logs: &Path) -> bool {
    if !identifier.starts_with("com.devbox.")
        || !identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return false;
    }
    data_inspector::safe_derived_path(data_root, logs)
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
    use crate::core::catalog::CatalogApp;
    use std::fs;

    fn catalog() -> Catalog {
        Catalog {
            schema_version: 2,
            catalog_revision: Some(7),
            apps: vec![CatalogApp {
                id: "testapp".into(),
                display_name: "Test App".into(),
                product_name: "Test".into(),
                identifier: "com.devbox.testapp".into(),
                cargo_package: "testapp".into(),
                app_dir: "apps/testapp".into(),
                release: true,
                manager_visible: true,
                self_managed: false,
                accepts: vec![],
                produces: vec![],
                actions: vec![],
            }],
        }
    }

    #[test]
    fn bundle_contract_omits_raw_data_and_paths() {
        let root = std::env::temp_dir().join("devbox-support-bundle-empty");
        let _ = fs::create_dir_all(&root);
        let draft = build_bundle(
            &catalog(),
            &root,
            vec![SupportDiagnostic {
                name: "log".into(),
                ok: true,
                detail: "Authorization: Bearer secret /home/alice/project".into(),
            }],
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let text = String::from_utf8(draft.bytes).unwrap();
        assert!(text.contains("raw-database-bytes"));
        assert!(text.contains("omitted"));
        assert!(!text.contains("Bearer secret"));
        assert!(!text.contains("/home/alice/project"));
        assert_eq!(draft.source_revision.len(), 64);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_returns_the_exact_bytes_shown_by_preview() {
        let root = std::env::temp_dir().join("devbox-support-bundle-exact");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let draft = build_bundle(
            &catalog(),
            &root,
            Vec::new(),
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let preview_bytes = draft.bytes.clone();
        let export = export_bundle(&draft).unwrap();
        assert_eq!(export.content.as_bytes(), preview_bytes.as_slice());
        assert_eq!(export.byte_count, preview_bytes.len());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_log_root_is_reported_without_following_or_archiving_it() {
        let root = std::env::temp_dir().join("devbox-support-bundle-log-link");
        let app_root = root.join("com.devbox.testapp");
        let outside = root.join("outside-logs");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&app_root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("raw.log"), "Authorization: secret").unwrap();
        std::os::unix::fs::symlink(&outside, app_root.join("logs")).unwrap();

        let draft = build_bundle(
            &catalog(),
            &root,
            Vec::new(),
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(draft.document.logs[0].state, "unsafe-path");
        assert!(!String::from_utf8(draft.bytes).unwrap().contains("raw.log"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancellation_is_a_fixed_failure() {
        let root = std::env::temp_dir().join("devbox-support-bundle-cancel");
        let _ = fs::create_dir_all(&root);
        let error = build_bundle(
            &catalog(),
            &root,
            Vec::new(),
            Vec::new(),
            Arc::new(AtomicBool::new(true)),
        )
        .unwrap_err();
        assert_eq!(error, BundleFailure::Cancelled);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn preview_ttl_and_size_are_bounded() {
        assert_eq!(SUPPORT_PREVIEW_TTL_MS, 300_000);
        assert_eq!(MAX_SUPPORT_BUNDLE_BYTES, 512 * 1024);
        assert_eq!(MAX_LOG_METADATA_FILES, 128);
        assert_eq!(MAX_LOG_METADATA_ENTRIES, 512);
        assert_eq!(MAX_LOG_METADATA_BYTES, 4 * 1024 * 1024);
    }
}
