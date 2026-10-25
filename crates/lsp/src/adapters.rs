use crate::{LspError, Restarted};
use std::{future::Future, pin::Pin, sync::Arc};
#[derive(Debug, Clone)]
pub struct DocumentGrant {
    pub uri: String,
    pub disk_text: String,
    pub disk_rev: Option<devbox_documents::DiskRev>,
    pub lease_id: Option<String>,
}
pub type DocumentAccess = Arc<
    dyn Fn(
            String,
            Option<String>,
        ) -> Pin<Box<dyn Future<Output = Result<DocumentGrant, LspError>> + Send>>
        + Send
        + Sync,
>;
pub type RestartSink = Arc<dyn Fn(Restarted) + Send + Sync>;
pub type InstallAdapter = Arc<
    dyn Fn(String, String) -> Pin<Box<dyn Future<Output = Result<String, LspError>> + Send>>
        + Send
        + Sync,
>;

/// Captures only through the same canonical documents authority used by access.
pub type CaptureAccess = Arc<
    dyn Fn(
            Vec<String>,
            tokio_util::sync::CancellationToken,
        )
            -> Pin<Box<dyn Future<Output = Result<devbox_documents::CodeCapture, LspError>> + Send>>
        + Send
        + Sync,
>;
