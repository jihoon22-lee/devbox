use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Query text: at most this many UTF-8 bytes, no NUL.
pub const MAX_TEXT_BYTES: usize = 1024;
/// `search.query` `limit` upper bound (per scope).
pub const MAX_QUERY_LIMIT: u32 = 500;
/// `search.notes` `limit` upper bound.
pub const MAX_NOTES_LIMIT: u32 = 50;
/// Items per stream chunk.
pub const CHUNK: usize = 50;
/// `projectIds` entries at most.
pub const MAX_PROJECT_IDS: usize = 256;
/// Saved searches at most; saving more is refused.
pub const MAX_SAVED: usize = 256;
/// Saved search name: characters at most.
pub const MAX_NAME_CHARS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Notes,
    Files,
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    pub text: String,
    /// Results always arrive notes → files → code, whatever the order here.
    pub scopes: Vec<Scope>,
    /// `null`: every registered project (code and file names).
    pub project_ids: Option<Vec<String>>,
    pub regex: bool,
    pub case_sensitive: bool,
    /// Per scope, `1..=500`.
    pub limit: u32,
}

/// One result. `root` is the documents root id (`vault`, `project:<id>`), so
/// `{ root, path }` opens the file with `documents.read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchHit {
    pub root: String,
    pub path: String,
    /// Note title (notes scope).
    pub title: Option<String>,
    /// 1-based line (code scope).
    pub line: Option<u32>,
    /// A single-line excerpt (notes, code) or the path (files).
    pub snippet: String,
    /// `[from, to)` UTF-16 offsets into `snippet`.
    pub ranges_utf16: Vec<(u32, u32)>,
}

/// One JSON line of the `search.query` stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchChunk {
    pub scope: Scope,
    /// Search index revision when the query started.
    pub rev: String,
    pub items: Vec<SearchHit>,
    /// The last chunk of the stream.
    pub done: bool,
    /// This scope stopped at `limit` (or a size cap).
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NotesQuery {
    pub text: String,
    /// `1..=50`.
    pub limit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub ranges_utf16: Vec<(u32, u32)>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NoteHits {
    pub rev: String,
    pub items: Vec<NoteHit>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SaveSearch {
    pub name: String,
    pub query: SearchQuery,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SavedRef {
    pub id: String,
    pub rev: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SavedId {
    pub id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SavedSearch {
    pub id: String,
    pub name: String,
    pub query: SearchQuery,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SavedSearches {
    pub rev: String,
    pub items: Vec<SavedSearch>,
}

/// The note index content changed (never for a same-content reindex, 01 §5.5 R8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SearchChanged {
    pub rev: String,
}

// `invalid_regex.position`: UTF-16 offset into the query text.
domain_error! {
    pub enum SearchError {
        InvalidRegex { position: u32 } = "search.invalid_regex",
        InvalidInput = "search.invalid_input",
        ProjectNotFound = "search.project_not_found",
        NotFound = "search.not_found",
        StreamUnsupported = "search.stream_unsupported",
    }
}
