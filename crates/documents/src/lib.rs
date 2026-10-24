//! Document storage: write scope, edit leases, CAS saves with atomic exchange,
//! data-folder journals, no-replace moves and external change observation.
//! Document text, titles and paths are never written to logs or error details.
mod commits;
mod files;
mod journal;
mod lease;
mod ledger;
mod model;
mod rename;
mod scope;
mod service;
mod startup;
mod sys;
mod watch;
pub use commits::{COMMIT_HISTORY, CommittedChanges};
use devbox_protocol::{Done, Exporter, method, topic};
pub use files::EditState;
pub use model::*;
pub use scope::{Root, WriteScope, in_trash, is_internal};
pub use service::{Clock, DocumentsConfig, DocumentsService, Hook, Hooks, Roots};

/// Note body limit in bytes on disk.
pub const NOTE_LIMIT: u64 = 2 * 1024 * 1024;
/// Code file limit in bytes on disk.
pub const CODE_LIMIT: u64 = 5 * 1024 * 1024;
/// JSON-escaped document text allowed in one response or request (frame limit 16 MiB).
pub const MAX_WIRE_TEXT: usize = 12 * 1024 * 1024;

method!(pub DocumentsClaim = "documents.claim", mutation, ClaimDocument => Claimed, DocumentsError);
method!(pub DocumentsTake = "documents.take", mutation, TakeDocument => Claimed, DocumentsError);
method!(pub DocumentsRenew = "documents.renew", mutation, RenewLeases => Renewed, DocumentsError);
method!(pub DocumentsRelease = "documents.release", mutation, LeaseRef => Done, DocumentsError);
method!(pub DocumentsSave = "documents.save", mutation, SaveDocument => Saved, DocumentsError);
method!(pub DocumentsRead = "documents.read", query, ReadDocument => DiskDocument, DocumentsError);
method!(pub DocumentsJournal = "documents.journal", mutation, WriteJournal => Done, DocumentsError);
method!(pub DocumentsJournalDrop = "documents.journal_drop", mutation, DropJournal => Done, DocumentsError);
method!(pub DocumentsJournalRead = "documents.journal_read", query, LeaseRef => JournalText, DocumentsError);
method!(pub DocumentsRename = "documents.rename", mutation, RenameDocument => Renamed, DocumentsError);
topic!(pub DocumentsChanged = "documents.changed", DocumentChanged);
topic!(pub DocumentsMoved = "documents.moved", DocumentMoved);
topic!(pub DocumentsLeaseLost = "documents.lease_lost", LeaseLost);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<DocumentsClaim>();
    x.method::<DocumentsTake>();
    x.method::<DocumentsRenew>();
    x.method::<DocumentsRelease>();
    x.method::<DocumentsSave>();
    x.method::<DocumentsRead>();
    x.method::<DocumentsJournal>();
    x.method::<DocumentsJournalDrop>();
    x.method::<DocumentsJournalRead>();
    x.method::<DocumentsRename>();
    x.topic::<DocumentsChanged>();
    x.topic::<DocumentsMoved>();
    x.topic::<DocumentsLeaseLost>();
}

pub(crate) type R<T> = Result<T, devbox_protocol::Fail<DocumentsError>>;

#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod journal_tests;
#[cfg(test)]
mod lease_tests;
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod sweep_tests;
#[cfg(test)]
mod sys_tests;
