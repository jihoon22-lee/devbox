//! Note vault rules: frontmatter, wiki/Markdown links and backlinks, templates,
//! image attachments, sanitized rendering and the `index.db` `notes` domain.
//! Every write goes through `devbox-documents`. Note text, titles and paths are
//! never written to logs or error details.
pub mod attachments;
pub mod frontmatter;
mod index;
pub mod links;
mod model;
mod ops;
pub mod render;
mod service;
mod settings;
pub mod templates;
mod tree;
mod vault;

use devbox_protocol::{Done, Exporter, method, topic};
pub use index::{NOTES_SCHEMA, NoSink, NoteSink};
pub use model::*;
pub use service::{Clock, NotesConfig, NotesService, PcName, Today};
pub use settings::{NotesSettings, SettingsError};

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
topic!(pub NotesChangedTopic = "notes.changed", NotesChanged);

pub fn export<X: Exporter>(x: &mut X) {
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
    x.topic::<NotesChangedTopic>();
}

pub(crate) type R<T> = Result<T, devbox_protocol::Fail<NotesError>>;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod index_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod vault_tests;
