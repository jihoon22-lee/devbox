//! Unified search: note full text (`index.db` domain `search`, FTS5 with a Hangul
//! bigram column), project code (`ignore` + `grep-searcher`), file names
//! (`git ls-files` + vault paths) and saved searches (`devbox.db` domain `search`).
//! Does not depend on `devbox-notes`: the CLI feeds note bodies in. Query text,
//! note text and paths are never written to logs or error details.
pub mod code;
pub mod files;
pub mod fts;
mod matcher;
mod model;
mod saved;
mod service;
mod settings;
mod snippet;

use devbox_protocol::{Done, Exporter, StreamOpened, method, topic};
pub use fts::{IndexedNote, SEARCH_SCHEMA, grams};
pub use matcher::compile;
pub use model::*;
pub use saved::MIGRATIONS;
pub use service::{ProjectRoot, ProjectRoots, SearchConfig, SearchService};
pub use settings::{SearchSettings, SettingsError};

method!(pub SearchQueryStream = "search.query", stream, SearchQuery => StreamOpened, SearchError);
method!(pub SearchNotes = "search.notes", query, NotesQuery => NoteHits, SearchError);
method!(pub SearchSaved = "search.saved", query, NoParams => SavedSearches, SearchError);
method!(pub SearchSave = "search.save", mutation, SaveSearch => SavedRef, SearchError);
method!(pub SearchUnsave = "search.unsave", mutation(reversible), SavedId => Done, SearchError);
topic!(pub SearchChangedTopic = "search.changed", SearchChanged);

pub fn export<X: Exporter>(x: &mut X) {
    x.method::<SearchQueryStream>();
    x.method::<SearchNotes>();
    x.method::<SearchSaved>();
    x.method::<SearchSave>();
    x.method::<SearchUnsave>();
    x.topic::<SearchChangedTopic>();
}

pub(crate) type R<T> = Result<T, devbox_protocol::Fail<SearchError>>;

#[cfg(test)]
mod code_tests;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod fts_tests;
#[cfg(test)]
mod service_tests;
