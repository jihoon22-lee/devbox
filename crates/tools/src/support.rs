//! Support exports are assembled locally; no uploader or automatic transmission.
mod archive;
mod files;
mod redact;
mod summary;
use crate::{R, ToolsError};
use devbox_core::Paths;
use devbox_protocol::system::DoctorReport;
use devbox_secrets::SecretsService;
use serde::{Deserialize, Serialize};

pub struct BundleInput {
    pub paths: Paths,
    pub doctor: DoctorReport,
    pub version: String,
}
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundle {
    pub path: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub logs_omitted: Option<String>,
}
/// A locked/unavailable store is not an export failure: omit logs and crashes.
pub async fn create_bundle(input: BundleInput, secrets: &SecretsService) -> R<SupportBundle> {
    let values = match secrets.public_status().await {
        Ok(status) if !status.locked => match secrets.list().await {
            Ok(names) => {
                let names: Vec<_> = names
                    .into_iter()
                    .filter(|name| name.has_value)
                    .map(|name| name.name)
                    .collect();
                secrets.resolve(&names).await.ok()
            }
            Err(_) => None,
        },
        _ => None,
    };
    tokio::task::spawn_blocking(move || archive::build(input, values))
        .await
        .map_err(failed)?
}
pub(super) fn failed(_: impl std::fmt::Display) -> devbox_protocol::Fail<ToolsError> {
    ToolsError::SupportFailed.into()
}
