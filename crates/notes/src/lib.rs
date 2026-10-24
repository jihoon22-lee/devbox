//! Note vault rules: frontmatter, wiki/Markdown links and backlinks, templates,
//! image attachments, sanitized rendering and the `index.db` `notes` domain.
//! Every write goes through `devbox-documents`. Note text, titles and paths are
//! never written to logs or error details.
pub mod attachments;
mod code_spans;
mod daily;
mod daily_ops;
pub mod frontmatter;
mod index;
pub mod links;
mod model;
mod ops;
mod rename;
mod rename_edits;
mod rename_journal;
mod rename_resume;
pub mod render;
mod service;
mod settings;
pub mod templates;
mod tree;
mod updates;
mod vault;

pub use daily::{SUMMARY_END, replace_summary, summary_start};
use devbox_protocol::{Done, Exporter, method, topic};
pub use index::{NOTES_SCHEMA, NoSink, NoteSink, SinkNote};
pub use model::*;
pub use rename::StopAt;
pub use service::{Clock, NotesConfig, NotesService, PcName, Today};
pub use settings::{NotesSettings, SettingsError};

method!(pub NotesRead = "notes.read", query, NotePath => NoteContent, NotesError);
method!(pub NotesTree = "notes.tree", query, TreeParams => NoteTree, NotesError);
method!(pub NotesCreate = "notes.create", mutation, CreateNote => CreatedNote, NotesError);
method!(pub NotesDelete = "notes.delete", mutation(reversible), NotePath => Trashed, NotesError);
method!(pub NotesRestore = "notes.restore", mutation, RestoreNote => Done, NotesError);
method!(pub NotesTags = "notes.tags", query, NoParams => Tags, NotesError);
method!(pub NotesBacklinks = "notes.backlinks", query, NotePath => Backlinks, NotesError);
method!(pub NotesResolve = "notes.resolve", query, ResolveLink => ResolvedLink, NotesError);
method!(pub NotesRender = "notes.render", query, RenderNote => RenderedNote, NotesError);
method!(pub NotesAttach = "notes.attach", mutation, AttachImage => Attached, NotesError);
method!(pub NotesTemplates = "notes.templates", query, NoParams => Templates, NotesError);
method!(pub NotesRename = "notes.rename", mutation, RenameNote => RenameStarted, NotesError);
method!(pub NotesRenameStatus = "notes.rename_status", query, RenameOp => RenameStatus, NotesError);
method!(pub NotesRewriteReply = "notes.rewrite_reply", mutation, RewriteReply => Done, NotesError);
method!(pub NotesDaily = "notes.daily", mutation, DailyParams => DailyNote, NotesError);
method!(pub NotesAppend = "notes.append", mutation, AppendNote => Appended, NotesError);
method!(pub NotesInsertSummary = "notes.insert_summary", mutation, InsertSummary => SummaryInserted, NotesError);
topic!(pub NotesChangedTopic = "notes.changed", NotesChanged);
topic!(pub NotesRewriteRequestTopic = "notes.rewrite_request", RewriteRequest);
topic!(pub NotesRenameIncompleteTopic = "notes.rename_incomplete", RenameIncomplete);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<NotesRead>();
    x.method::<NotesTree>();
    x.method::<NotesCreate>();
    x.method::<NotesDelete>();
    x.method::<NotesRestore>();
    x.method::<NotesTags>();
    x.method::<NotesBacklinks>();
    x.method::<NotesResolve>();
    x.method::<NotesRender>();
    x.method::<NotesAttach>();
    x.method::<NotesTemplates>();
    x.method::<NotesRename>();
    x.method::<NotesRenameStatus>();
    x.method::<NotesRewriteReply>();
    x.method::<NotesDaily>();
    x.method::<NotesAppend>();
    x.method::<NotesInsertSummary>();
    x.topic::<NotesChangedTopic>();
    x.topic::<NotesRewriteRequestTopic>();
    x.topic::<NotesRenameIncompleteTopic>();
}

pub(crate) type R<T> = Result<T, devbox_protocol::Fail<NotesError>>;

#[cfg(test)]
mod daily_guard_tests;
#[cfg(test)]
mod daily_tests;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod index_tests;
#[cfg(test)]
mod rename_guard_tests;
#[cfg(test)]
mod rename_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod sink_tests;
#[cfg(test)]
mod vault_tests;

#[cfg(test)]
mod scope_tests;
