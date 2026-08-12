//! Versioned JSON session schema.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SESSION_VERSION: u32 = 1;

/// One persisted document entry. The editable buffer is intentionally absent;
/// it is reconstructed from disk when the session is restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDoc {
    pub id: String,
    pub path: String,
    #[serde(rename = "cursor", alias = "cursorPosition", alias = "cursor_position")]
    pub cursor: usize,
    pub bookmarks: Vec<usize>,
}

impl SessionDoc {
    pub fn new(id: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            cursor: 0,
            bookmarks: Vec::new(),
        }
    }
}

/// Persisted top-level state. Arrays with two entries encode the two fixed views
/// without introducing a third split mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub workspace_folder: Option<String>,
    pub docs: Vec<SessionDoc>,
    pub views: [Vec<String>; 2],
    pub active_view: u8,
    pub active_doc_by_view: [Option<String>; 2],
    pub recent_files: Vec<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self::empty()
    }
}

impl Session {
    pub fn empty() -> Self {
        Self {
            version: SESSION_VERSION,
            workspace_folder: None,
            docs: Vec::new(),
            views: [Vec::new(), Vec::new()],
            active_view: 0,
            active_doc_by_view: [None, None],
            recent_files: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Strict parser used when callers want to surface corruption.
    pub fn from_json(input: &str) -> Result<Self, SessionError> {
        let session: Self = serde_json::from_str(input).map_err(SessionError::Json)?;
        session.validate()?;
        Ok(session)
    }

    /// Session files are disposable derived state: corruption, an unknown
    /// version, or an invariant violation starts an empty session.
    pub fn load(input: &str) -> Self {
        Self::from_json(input).unwrap_or_else(|_| Self::empty())
    }

    pub fn validate(&self) -> Result<(), SessionError> {
        if self.version != SESSION_VERSION {
            return Err(SessionError::UnsupportedVersion(self.version));
        }
        if self.active_view > 1 {
            return Err(SessionError::InvalidActiveView(self.active_view));
        }

        let mut ids = std::collections::HashSet::with_capacity(self.docs.len());
        let mut paths = std::collections::HashSet::with_capacity(self.docs.len());
        for doc in &self.docs {
            if doc.id.trim().is_empty()
                || doc.path.trim().is_empty()
                || !ids.insert(&doc.id)
                || !paths.insert(&doc.path)
            {
                return Err(SessionError::InvalidDocument);
            }
            if doc.bookmarks.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(SessionError::InvalidBookmarks);
            }
        }

        let known_ids: std::collections::HashSet<&str> =
            ids.into_iter().map(String::as_str).collect();
        let mut placed = std::collections::HashSet::new();
        for view in &self.views {
            for id in view {
                if !known_ids.contains(id.as_str()) || !placed.insert(id) {
                    return Err(SessionError::InvalidViewPlacement);
                }
            }
        }
        if placed.len() != known_ids.len() {
            return Err(SessionError::InvalidViewPlacement);
        }
        for (view_index, active) in self.active_doc_by_view.iter().enumerate() {
            if let Some(id) = active {
                if !self.views[view_index]
                    .iter()
                    .any(|placed_id| placed_id == id)
                {
                    return Err(SessionError::InvalidActiveDocument);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum SessionError {
    Json(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidActiveView(u8),
    InvalidDocument,
    InvalidBookmarks,
    InvalidViewPlacement,
    InvalidActiveDocument,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid session JSON: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported session schema version: {version}")
            }
            Self::InvalidActiveView(view) => write!(f, "invalid active view: {view}"),
            Self::InvalidDocument => f.write_str("invalid or duplicate session document"),
            Self::InvalidBookmarks => f.write_str("session bookmarks must be sorted and unique"),
            Self::InvalidViewPlacement => f.write_str("invalid or duplicate view document id"),
            Self::InvalidActiveDocument => f.write_str("active document is not in its view"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Compatibility aliases make the schema names clear to command callers.
pub type SessionDocument = SessionDoc;
pub type SessionState = Session;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_roundtrips_with_version_one() {
        let mut session = Session::empty();
        let mut doc = SessionDoc::new("doc-1", "/tmp/example.rs");
        doc.cursor = 42;
        doc.bookmarks = vec![2, 9];
        session.docs.push(doc);
        session.views[0].push("doc-1".into());
        session.active_doc_by_view[0] = Some("doc-1".into());
        session.recent_files.push("/tmp/example.rs".into());

        let json = session.to_json().unwrap();
        assert!(json.contains("\"version\": 1"));
        assert_eq!(Session::from_json(&json).unwrap(), session);
    }

    #[test]
    fn corrupt_or_unknown_version_starts_empty() {
        assert_eq!(Session::load("not json"), Session::empty());
        let mut unknown = Session::empty();
        unknown.version = SESSION_VERSION + 1;
        let json = serde_json::to_string(&unknown).unwrap();
        assert_eq!(Session::load(&json), Session::empty());
    }

    #[test]
    fn duplicate_view_placement_is_corrupt() {
        let mut session = Session::empty();
        session.docs.push(SessionDoc::new("doc-1", "/tmp/one"));
        session.views[0].push("doc-1".into());
        session.views[1].push("doc-1".into());
        assert!(matches!(
            session.validate(),
            Err(SessionError::InvalidViewPlacement)
        ));
    }

    #[test]
    fn orphan_document_is_corrupt() {
        let mut session = Session::empty();
        session.docs.push(SessionDoc::new("doc-1", "/tmp/one"));
        assert!(matches!(
            session.validate(),
            Err(SessionError::InvalidViewPlacement)
        ));
    }

    #[test]
    fn duplicate_document_path_is_corrupt() {
        let mut session = Session::empty();
        session.docs.push(SessionDoc::new("doc-1", "/tmp/one"));
        session.docs.push(SessionDoc::new("doc-2", "/tmp/one"));
        session.views[0].extend(["doc-1".into(), "doc-2".into()]);
        assert!(matches!(
            session.validate(),
            Err(SessionError::InvalidDocument)
        ));
    }
}
