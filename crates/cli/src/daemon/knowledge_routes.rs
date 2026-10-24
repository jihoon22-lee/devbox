use super::state::Services;
use devbox_activity as activity;
use devbox_documents as documents;
use devbox_documents::{Claimed, DocumentsError};
use devbox_notes as notes;
use devbox_protocol::{Fail, Router};
use devbox_search as search;

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
        .register::<notes::NotesRead, _, _>(|s, _, p| async move { s.notes.read(p).await })
        .register::<notes::NotesTree, _, _>(|s, _, p| async move { s.notes.tree(p).await })
        .register::<notes::NotesCreate, _, _>(|s, _, p| async move { s.notes.create(p).await })
        .register::<notes::NotesDelete, _, _>(|s, _, p| async move { s.notes.delete(p).await })
        .register::<notes::NotesRestore, _, _>(|s, _, p| async move { s.notes.restore(p).await })
        .register::<notes::NotesTags, _, _>(|s, _, p| async move { s.notes.tags(p).await })
        .register::<notes::NotesBacklinks, _, _>(
            |s, _, p| async move { s.notes.backlinks(p).await },
        )
        .register::<notes::NotesResolve, _, _>(|s, _, p| async move { s.notes.resolve(p).await })
        .register::<notes::NotesRender, _, _>(|s, _, p| async move { s.notes.render(p).await })
        .register::<notes::NotesAttach, _, _>(|s, _, p| async move { s.notes.attach(p).await })
        .register::<notes::NotesTemplates, _, _>(
            |s, _, p| async move { s.notes.templates(p).await },
        )
        .register::<notes::NotesRename, _, _>(|s, _, p| async move { s.notes.rename(p).await })
        .register::<notes::NotesRenameStatus, _, _>(|s, _, p| async move {
            s.notes.rename_status(p).await
        })
        .register::<notes::NotesRewriteReply, _, _>(|s, _, p| async move {
            s.notes.rewrite_reply(p).await
        })
        .register::<notes::NotesDaily, _, _>(|s, _, p| async move { s.notes.daily(p).await })
        .register::<notes::NotesAppend, _, _>(|s, _, p| async move { s.notes.append(p).await })
        .register::<notes::NotesInsertSummary, _, _>(|s, _, p| super::summary::insert(s, p))
        .register::<search::SearchQueryStream, _, _>(|s, ctx, p| async move {
            s.search.query(ctx, p).await
        })
        .register::<search::SearchNotes, _, _>(|s, _, p| async move { s.search.notes(p).await })
        .register::<search::SearchSaved, _, _>(|s, _, p| async move { s.search.saved(p).await })
        .register::<search::SearchSave, _, _>(|s, _, p| async move { s.search.save(p).await })
        .register::<search::SearchUnsave, _, _>(|s, _, p| async move { s.search.unsave(p).await })
        .register::<activity::ActivityIngest, _, _>(
            |s, _, p| async move { s.activity.ingest(p).await },
        )
        .register::<activity::ActivityGetStatus, _, _>(|s, _, p| async move {
            s.activity.status(p).await
        })
        .register::<activity::ActivityRules, _, _>(
            |s, _, p| async move { s.activity.rules(p).await },
        )
        .register::<activity::ActivitySetRules, _, _>(|s, _, p| async move {
            s.activity.set_rules(p).await
        })
        .register::<activity::ActivityDay, _, _>(|s, _, p| async move { s.activity.day(p).await })
        .register::<activity::ActivityRange, _, _>(
            |s, _, p| async move { s.activity.range(p).await },
        )
        .register::<activity::ActivityExport, _, _>(
            |s, _, p| async move { s.activity.export(p).await },
        )
}
