//! The renderer names a completed history entry and a vault selection, never response text.
use super::state::Services;
use devbox_notes::{AppendResponseNote, NotesError, ResponseNoteAppended};
use devbox_protocol::Fail;
use std::sync::Arc;

pub(super) async fn append(
    s: Arc<Services>,
    p: AppendResponseNote,
) -> Result<ResponseNoteAppended, Fail<NotesError>> {
    if let Some(done) = s.notes.response_append_result(&p).await? {
        return Ok(done);
    }
    let response = s
        .api
        .export_response(devbox_api::HistoryId {
            id: p.history_id.clone(),
        })
        .await
        .map_err(|error| {
            let reason = match error {
                Fail::Domain(devbox_api::ApiError::ResponseExportFailed { reason })
                    if matches!(
                        reason.as_str(),
                        "pending"
                            | "provenance"
                            | "origin"
                            | "history"
                            | "incomplete"
                            | "redaction"
                            | "size"
                            | "encoding"
                    ) =>
                {
                    reason
                }
                _ => "history".to_owned(),
            };
            Fail::from(NotesError::ResponseUnavailable { reason })
        })?;
    if response.history_id != p.history_id {
        return Err(NotesError::ResponseUnavailable {
            reason: "provenance".into(),
        }
        .into());
    }
    s.notes.append_response(p, response.markdown).await
}
