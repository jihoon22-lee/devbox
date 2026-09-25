//! Bounded support metadata, never a database or raw-log archive.
use super::redaction::{redact_text, REDACTION_VERSION};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
pub const MAX_SUPPORT_BUNDLE_BYTES: usize = 512 * 1024;
pub const SUPPORT_PREVIEW_TTL_MS: u64 = 5 * 60 * 1000;
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundlePreview {
    pub preview_id: String,
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
pub struct SupportProduct {
    pub id: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperationLogSummary {
    pub product: String,
    pub summary: product_contract::operation_log::Summary,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleFailure {
    Cancelled,
    TooLarge,
}
impl BundleFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Cancelled => "지원 번들 생성이 취소되었습니다.",
            Self::TooLarge => "지원 번들이 허용된 크기를 초과했습니다.",
        }
    }
}
#[derive(Debug, Clone)]
pub struct BundleDraft {
    pub bytes: Vec<u8>,
}
pub fn build_bundle(
    diagnosis: Vec<SupportDiagnostic>,
    products: Vec<SupportProduct>,
    operations: Vec<OperationLogSummary>,
    cancel: Arc<AtomicBool>,
) -> Result<BundleDraft, BundleFailure> {
    if cancel.load(Ordering::Relaxed) {
        return Err(BundleFailure::Cancelled);
    }
    let diagnosis: Vec<_> = diagnosis
        .into_iter()
        .map(|row| SupportDiagnostic {
            name: redact_text(&row.name, "support"),
            ok: row.ok,
            detail: redact_text(&row.detail, "support"),
        })
        .collect();
    let products: Vec<_> = products
        .into_iter()
        .map(|row| SupportProduct {
            id: redact_text(&row.id, "support"),
            version: redact_text(&row.version, "support"),
        })
        .collect();
    let document = serde_json::json!({"schemaVersion":3,
        "generatedAtMs":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64,
        "redaction":{"version":REDACTION_VERSION,"paths":"omitted","usernames":"omitted","secrets":"omitted","authHeaders":"omitted","rawDatabase":"omitted","rawLogs":"omitted"},
        "diagnosis":diagnosis,"products":products,"operations":operations,
        "omitted":["raw-database-bytes","raw-log-lines","filesystem-paths","environment-values","credentials","authorization-headers"]});
    let bytes = serde_json::to_vec_pretty(&document).map_err(|_| BundleFailure::TooLarge)?;
    if bytes.len() > MAX_SUPPORT_BUNDLE_BYTES {
        return Err(BundleFailure::TooLarge);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(BundleFailure::Cancelled);
    }
    Ok(BundleDraft { bytes })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundle_contains_current_products_and_redacted_diagnosis() {
        let draft = build_bundle(
            vec![SupportDiagnostic {
                name: "git".into(),
                ok: true,
                detail: "git 2.50 /home/alice".into(),
            }],
            vec![SupportProduct {
                id: "knowledge".into(),
                version: "0.8.1".into(),
            }],
            vec![],
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&draft.bytes).unwrap();
        assert_eq!(value["schemaVersion"], 3);
        assert_eq!(value["products"][0]["id"], "knowledge");
        assert!(
            value.get("databases").is_none()
                && value.get("catalog").is_none()
                && value.get("logs").is_none()
        );
        let exported = export_bundle(&draft).unwrap();
        assert!(!exported.content.contains("/home/alice"));
        assert_eq!(exported.content.as_bytes(), draft.bytes);
    }
    #[test]
    fn cancellation_and_output_limits_remain_enforced() {
        assert!(matches!(
            build_bundle(vec![], vec![], vec![], Arc::new(AtomicBool::new(true))),
            Err(BundleFailure::Cancelled)
        ));
        let rows = (0..20)
            .map(|_| SupportDiagnostic {
                name: "git".into(),
                ok: false,
                detail: "x".repeat(64 * 1024),
            })
            .collect();
        assert!(matches!(
            build_bundle(rows, vec![], vec![], Arc::new(AtomicBool::new(false))),
            Err(BundleFailure::TooLarge)
        ));
    }
}
