//! Daemon-owned response export; no renderer response bytes are accepted.
mod format;
mod receipts;
use crate::{ApiError, ApiService, HistoryId, R};
pub(crate) use receipts::Receipts;

#[derive(Debug)]
pub struct ResponseNoteExport {
    pub history_id: String,
    pub markdown: String,
}
pub(crate) fn failed(reason: &str) -> ApiError {
    ApiError::ResponseExportFailed {
        reason: reason.into(),
    }
}
impl ApiService {
    /// Receipts expire on daemon restart or after 1,024 newer sends. Old files alone never authorize export.
    pub async fn export_response(&self, id: HistoryId) -> R<ResponseNoteExport> {
        let (receipt, authority) = self.inner.exports.get(&id.id).map_err(failed)?;
        if !authority
            .valid(&self.inner.config_dir, &self.projects)
            .await
        {
            return Err(failed("origin").into());
        }
        let entry = self
            .inner
            .history
            .export_get(receipt)
            .await
            .map_err(|_| failed("history"))?;
        let markdown = format::render(&entry)?;
        // Formatting/disk reads can yield; never authorize against an earlier project/root snapshot.
        if !authority
            .valid(&self.inner.config_dir, &self.projects)
            .await
        {
            return Err(failed("origin").into());
        }
        Ok(ResponseNoteExport {
            history_id: id.id,
            markdown,
        })
    }
}
