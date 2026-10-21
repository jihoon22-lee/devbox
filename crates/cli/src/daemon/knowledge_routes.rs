use super::state::Services;
use devbox_documents as documents;
use devbox_documents::{Claimed, DocumentsError};
use devbox_protocol::{Fail, Router};

/// Claim and take bind their lease to the requesting channel, so `server.rs`
/// dispatches them with the channel token instead of through mutation dedupe.
pub(super) fn channel_scoped(method: &str) -> bool {
    matches!(method, "documents.claim" | "documents.take")
}

/// `None` means the owner token was already cancelled and no lease was inserted;
/// staying pending lets `Router::dispatch` answer `cancelled` from that token.
async fn leased(
    result: Result<Option<Claimed>, Fail<DocumentsError>>,
) -> Result<Claimed, Fail<DocumentsError>> {
    match result? {
        Some(claimed) => Ok(claimed),
        None => std::future::pending().await,
    }
}

pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<documents::DocumentsClaim, _, _>(|s, ctx, p| async move {
            leased(s.documents.claim(ctx.cancel, p).await).await
        })
        .register::<documents::DocumentsTake, _, _>(|s, ctx, p| async move {
            leased(s.documents.take(ctx.cancel, p).await).await
        })
        .register::<documents::DocumentsRenew, _, _>(
            |s, _, p| async move { s.documents.renew(p).await },
        )
        .register::<documents::DocumentsRelease, _, _>(|s, _, p| async move {
            s.documents.release(p).await
        })
        .register::<documents::DocumentsSave, _, _>(
            |s, _, p| async move { s.documents.save(p).await },
        )
        .register::<documents::DocumentsRead, _, _>(
            |s, _, p| async move { s.documents.read(p).await },
        )
        .register::<documents::DocumentsJournal, _, _>(|s, _, p| async move {
            s.documents.journal(p).await
        })
        .register::<documents::DocumentsJournalDrop, _, _>(|s, _, p| async move {
            s.documents.journal_drop(p).await
        })
        .register::<documents::DocumentsJournalRead, _, _>(|s, _, p| async move {
            s.documents.journal_read(p).await
        })
        .register::<documents::DocumentsRename, _, _>(|s, _, p| async move {
            s.documents.rename(p).await
        })
}
