//! Workspace-bounded LSP document state and deterministic sync payloads.

use super::positions::{offset_to_position, LspRange, PositionEncoding, PositionError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use url::Url;

#[derive(Debug)]
pub enum DocumentError {
    Io(std::io::Error),
    InvalidFileUri(String),
    PathOutsideWorkspace(PathBuf),
    DocumentAlreadyOpen(String),
    DocumentNotOpen(String),
    DuplicateAtomicChange(String),
    VersionMismatch {
        uri: String,
        expected: i32,
        actual: i32,
    },
    EmptyLanguageId,
    NonNormalizedLineEndings,
    VersionOverflow,
    Position(PositionError),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "document path resolution failed: {error}"),
            Self::InvalidFileUri(uri) => write!(f, "invalid local file URI: {uri}"),
            Self::PathOutsideWorkspace(path) => {
                write!(f, "path is outside the LSP workspace: {}", path.display())
            }
            Self::DocumentAlreadyOpen(uri) => write!(f, "document is already open: {uri}"),
            Self::DocumentNotOpen(uri) => write!(f, "document is not open: {uri}"),
            Self::DuplicateAtomicChange(uri) => {
                write!(
                    f,
                    "document appears more than once in an atomic change: {uri}"
                )
            }
            Self::VersionMismatch {
                uri,
                expected,
                actual,
            } => write!(
                f,
                "document version conflict for {uri}: expected {expected}, current {actual}"
            ),
            Self::EmptyLanguageId => f.write_str("language id cannot be empty"),
            Self::NonNormalizedLineEndings => {
                f.write_str("LSP documents must contain LF-normalized text")
            }
            Self::VersionOverflow => f.write_str("document version overflowed"),
            Self::Position(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Position(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DocumentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<PositionError> for DocumentError {
    fn from(error: PositionError) -> Self {
        Self::Position(error)
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceRoot {
    canonical_path: PathBuf,
    uri: Url,
}

impl WorkspaceRoot {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let canonical_path = fs::canonicalize(path)?;
        if !canonical_path.is_dir() {
            return Err(DocumentError::PathOutsideWorkspace(canonical_path));
        }
        let uri = file_uri_from_absolute_path(&canonical_path)?;
        Ok(Self {
            canonical_path,
            uri,
        })
    }

    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    pub fn uri(&self) -> &Url {
        &self.uri
    }

    pub fn resolve_document(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<(PathBuf, Url), DocumentError> {
        let canonical = fs::canonicalize(path)?;
        if !path_is_within(&self.canonical_path, &canonical) || !canonical.is_file() {
            return Err(DocumentError::PathOutsideWorkspace(canonical));
        }
        let uri = file_uri_from_absolute_path(&canonical)?;
        Ok((canonical, uri))
    }

    pub fn resolve_uri(&self, uri: &Url) -> Result<PathBuf, DocumentError> {
        let path = file_uri_to_path(uri)?;
        let canonical = fs::canonicalize(path)?;
        if !path_is_within(&self.canonical_path, &canonical) || !canonical.is_file() {
            return Err(DocumentError::PathOutsideWorkspace(canonical));
        }
        Ok(canonical)
    }
}

pub fn file_uri_from_absolute_path(path: &Path) -> Result<Url, DocumentError> {
    if !path.is_absolute() {
        return Err(DocumentError::InvalidFileUri(path.display().to_string()));
    }
    Url::from_file_path(path).map_err(|_| DocumentError::InvalidFileUri(path.display().to_string()))
}

pub fn file_uri_to_path(uri: &Url) -> Result<PathBuf, DocumentError> {
    if uri.scheme() != "file" || uri.query().is_some() || uri.fragment().is_some() {
        return Err(DocumentError::InvalidFileUri(uri.as_str().to_owned()));
    }
    uri.to_file_path()
        .map_err(|_| DocumentError::InvalidFileUri(uri.as_str().to_owned()))
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    let mut root_components = root.components();
    let mut candidate_components = candidate.components();
    loop {
        match root_components.next() {
            None => return true,
            Some(root_component) => {
                let Some(candidate_component) = candidate_components.next() else {
                    return false;
                };
                if !components_equal(root_component, candidate_component) {
                    return false;
                }
            }
        }
    }
}

#[cfg(windows)]
fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    use std::os::windows::ffi::OsStrExt;

    left.as_os_str()
        .encode_wide()
        .map(fold_ascii_case)
        .eq(right.as_os_str().encode_wide().map(fold_ascii_case))
}

#[cfg(windows)]
const fn fold_ascii_case(unit: u16) -> u16 {
    if unit >= b'A' as u16 && unit <= b'Z' as u16 {
        unit + (b'a' - b'A') as u16
    } else {
        unit
    }
}

#[cfg(not(windows))]
fn components_equal(left: Component<'_>, right: Component<'_>) -> bool {
    left == right
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SyncKind {
    Full,
    Incremental,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextChange {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<LspRange>,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DidOpen {
    pub uri: String,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DidChange {
    pub uri: String,
    pub version: i32,
    pub content_changes: Vec<TextChange>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DidSave {
    pub uri: String,
    pub version: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DidClose {
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSnapshot {
    pub uri: String,
    pub path: PathBuf,
    pub language_id: String,
    pub version: i32,
    pub text: String,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSnapshot {
    pub uri: String,
    pub version: i32,
}

/// A fully computed replacement used by a protocol adapter that must update
/// several documents atomically.  The text is still LF-normalized; callers
/// should validate ranges and replacements before constructing this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicDocumentChange {
    pub uri: String,
    pub expected_version: i32,
    pub text: String,
}

#[derive(Debug, Clone)]
struct DocumentState {
    path: PathBuf,
    uri: Url,
    language_id: String,
    version: i32,
    text: String,
    dirty: bool,
}

#[derive(Debug, Clone)]
pub struct DocumentStore {
    workspace: WorkspaceRoot,
    encoding: PositionEncoding,
    sync_kind: SyncKind,
    documents: BTreeMap<String, DocumentState>,
}

impl DocumentStore {
    pub fn new(workspace: WorkspaceRoot, encoding: PositionEncoding, sync_kind: SyncKind) -> Self {
        Self {
            workspace,
            encoding,
            sync_kind,
            documents: BTreeMap::new(),
        }
    }

    pub fn workspace(&self) -> &WorkspaceRoot {
        &self.workspace
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.encoding
    }

    pub fn open(
        &mut self,
        path: impl AsRef<Path>,
        language_id: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<DidOpen, DocumentError> {
        let language_id = language_id.into();
        if language_id.trim().is_empty() {
            return Err(DocumentError::EmptyLanguageId);
        }
        let text = text.into();
        validate_text(&text)?;
        let (path, uri) = self.workspace.resolve_document(path)?;
        let key = uri.as_str().to_owned();
        if self.documents.contains_key(&key) {
            return Err(DocumentError::DocumentAlreadyOpen(key));
        }
        let state = DocumentState {
            path,
            uri: uri.clone(),
            language_id: language_id.clone(),
            version: 1,
            text: text.clone(),
            dirty: false,
        };
        self.documents.insert(key.clone(), state);
        Ok(DidOpen {
            uri: key,
            language_id,
            version: 1,
            text,
        })
    }

    /// Import an authoritative buffer into a fresh server session. The new
    /// session always starts at version one, while the editor's dirty state is
    /// retained for the eventual didSave boundary.
    pub fn open_snapshot(&mut self, snapshot: &DocumentSnapshot) -> Result<DidOpen, DocumentError> {
        let opened = self.open(
            &snapshot.path,
            snapshot.language_id.clone(),
            snapshot.text.clone(),
        )?;
        if snapshot.dirty {
            if let Some(state) = self.documents.get_mut(&opened.uri) {
                state.dirty = true;
            }
        }
        Ok(opened)
    }

    pub fn change(
        &mut self,
        uri: &str,
        new_text: impl Into<String>,
        dirty: bool,
    ) -> Result<DidChange, DocumentError> {
        self.change_with_kind(uri, new_text.into(), dirty, self.sync_kind)
    }

    /// Reload uses a full sync without resetting the session-local version.
    pub fn reload(
        &mut self,
        uri: &str,
        new_text: impl Into<String>,
    ) -> Result<DidChange, DocumentError> {
        self.change_with_kind(uri, new_text.into(), false, SyncKind::Full)
    }

    fn change_with_kind(
        &mut self,
        uri: &str,
        new_text: String,
        dirty: bool,
        sync_kind: SyncKind,
    ) -> Result<DidChange, DocumentError> {
        validate_text(&new_text)?;
        let state = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| DocumentError::DocumentNotOpen(uri.to_owned()))?;
        let version = state
            .version
            .checked_add(1)
            .ok_or(DocumentError::VersionOverflow)?;
        let content_changes = match sync_kind {
            SyncKind::Full => vec![TextChange {
                range: None,
                text: new_text.clone(),
            }],
            SyncKind::Incremental => {
                vec![incremental_change(&state.text, &new_text, self.encoding)?]
            }
        };
        state.version = version;
        state.text = new_text;
        state.dirty = dirty;
        Ok(DidChange {
            uri: uri.to_owned(),
            version,
            content_changes,
        })
    }

    pub fn mark_saved(&mut self, uri: &str) -> Result<DidSave, DocumentError> {
        let state = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| DocumentError::DocumentNotOpen(uri.to_owned()))?;
        state.dirty = false;
        Ok(DidSave {
            uri: uri.to_owned(),
            version: state.version,
        })
    }

    /// Applies already-computed document replacements as one transaction.
    ///
    /// Every URI, version, and replacement is checked against a cloned state
    /// first.  If any check fails, neither the live document map nor its
    /// versions are changed.  This is the mutation boundary used by rename
    /// and formatting adapters so a multi-document edit cannot partially
    /// apply.
    pub fn apply_atomic_changes(
        &mut self,
        changes: &[AtomicDocumentChange],
    ) -> Result<Vec<DidChange>, DocumentError> {
        let mut staged = self.documents.clone();
        let mut seen = HashSet::with_capacity(changes.len());
        let mut notifications = Vec::with_capacity(changes.len());

        for change in changes {
            if !seen.insert(change.uri.clone()) {
                return Err(DocumentError::DuplicateAtomicChange(change.uri.clone()));
            }
            validate_text(&change.text)?;
            let state = staged
                .get_mut(&change.uri)
                .ok_or_else(|| DocumentError::DocumentNotOpen(change.uri.clone()))?;
            if state.version != change.expected_version {
                return Err(DocumentError::VersionMismatch {
                    uri: change.uri.clone(),
                    expected: change.expected_version,
                    actual: state.version,
                });
            }
            let version = state
                .version
                .checked_add(1)
                .ok_or(DocumentError::VersionOverflow)?;
            let content_changes = match self.sync_kind {
                SyncKind::Full => vec![TextChange {
                    range: None,
                    text: change.text.clone(),
                }],
                SyncKind::Incremental => vec![incremental_change(
                    &state.text,
                    &change.text,
                    self.encoding,
                )?],
            };
            state.version = version;
            state.text = change.text.clone();
            state.dirty = true;
            notifications.push(DidChange {
                uri: change.uri.clone(),
                version,
                content_changes,
            });
        }

        self.documents = staged;
        Ok(notifications)
    }

    pub fn close(&mut self, uri: &str) -> Result<DidClose, DocumentError> {
        self.documents
            .remove(uri)
            .ok_or_else(|| DocumentError::DocumentNotOpen(uri.to_owned()))?;
        Ok(DidClose {
            uri: uri.to_owned(),
        })
    }

    pub fn snapshot(&self, uri: &str) -> Option<DocumentSnapshot> {
        self.documents.get(uri).map(DocumentState::snapshot)
    }

    /// Return all open buffers in the store's stable URI order for session
    /// replay and crash recovery.
    pub fn snapshots(&self) -> Vec<DocumentSnapshot> {
        self.documents
            .values()
            .map(DocumentState::snapshot)
            .collect()
    }

    pub fn request_snapshot(&self, uri: &str) -> Result<RequestSnapshot, DocumentError> {
        let state = self
            .documents
            .get(uri)
            .ok_or_else(|| DocumentError::DocumentNotOpen(uri.to_owned()))?;
        Ok(RequestSnapshot {
            uri: uri.to_owned(),
            version: state.version,
        })
    }

    pub fn is_current(&self, request: &RequestSnapshot) -> bool {
        self.documents
            .get(&request.uri)
            .is_some_and(|state| state.version == request.version)
    }

    pub fn diagnostics_are_current(&self, uri: &str, version: Option<i32>) -> bool {
        version.is_some_and(|version| {
            self.documents
                .get(uri)
                .is_some_and(|state| state.version == version)
        })
    }

    /// A restarted server is a new LSP session. All current buffers are kept,
    /// reset to version 1, and returned in stable URI order for didOpen replay.
    pub fn restart_session(&mut self) -> Vec<DidOpen> {
        self.documents
            .values_mut()
            .map(|state| {
                state.version = 1;
                DidOpen {
                    uri: state.uri.as_str().to_owned(),
                    language_id: state.language_id.clone(),
                    version: 1,
                    text: state.text.clone(),
                }
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

impl DocumentState {
    fn snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot {
            uri: self.uri.as_str().to_owned(),
            path: self.path.clone(),
            language_id: self.language_id.clone(),
            version: self.version,
            text: self.text.clone(),
            dirty: self.dirty,
        }
    }
}

fn validate_text(text: &str) -> Result<(), DocumentError> {
    if text.contains('\r') {
        Err(DocumentError::NonNormalizedLineEndings)
    } else {
        Ok(())
    }
}

fn incremental_change(
    old_text: &str,
    new_text: &str,
    encoding: PositionEncoding,
) -> Result<TextChange, PositionError> {
    let prefix = common_prefix_bytes(old_text, new_text);
    let suffix = common_suffix_bytes(&old_text[prefix..], &new_text[prefix..]);
    let old_end = old_text.len() - suffix;
    let new_end = new_text.len() - suffix;
    Ok(TextChange {
        range: Some(LspRange {
            start: offset_to_position(old_text, prefix, encoding)?,
            end: offset_to_position(old_text, old_end, encoding)?,
        }),
        text: new_text[prefix..new_end].to_owned(),
    })
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    let mut bytes = 0;
    for (left_char, right_char) in left.chars().zip(right.chars()) {
        if left_char != right_char {
            break;
        }
        bytes += left_char.len_utf8();
    }
    bytes
}

fn common_suffix_bytes(left: &str, right: &str) -> usize {
    let mut bytes = 0;
    for (left_char, right_char) in left.chars().rev().zip(right.chars().rev()) {
        if left_char != right_char {
            break;
        }
        bytes += left_char.len_utf8();
    }
    bytes.min(left.len()).min(right.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn store(
        encoding: PositionEncoding,
        sync_kind: SyncKind,
    ) -> (tempfile::TempDir, PathBuf, DocumentStore) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("space 한글 %.rs");
        fs::write(&path, "").unwrap();
        let workspace = WorkspaceRoot::new(directory.path()).unwrap();
        let store = DocumentStore::new(workspace, encoding, sync_kind);
        (directory, path, store)
    }

    #[test]
    fn file_uri_round_trips_percent_encoded_names() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Full);
        let opened = store.open(&path, "rust", "fn main() {}\n").unwrap();
        assert!(opened.uri.contains("space%20"));
        assert!(opened.uri.contains("%25.rs"));
        let parsed = Url::parse(&opened.uri).unwrap();
        assert_eq!(
            store.workspace().resolve_uri(&parsed).unwrap(),
            fs::canonicalize(path).unwrap()
        );
    }

    #[test]
    fn open_is_once_per_session_and_versions_are_monotonic() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Full);
        let opened = store.open(&path, "rust", "one\n").unwrap();
        assert_eq!(opened.version, 1);
        assert!(matches!(
            store.open(&path, "rust", "one\n"),
            Err(DocumentError::DocumentAlreadyOpen(_))
        ));
        let first = store.change(&opened.uri, "two\n", true).unwrap();
        let second = store.change(&opened.uri, "three\n", true).unwrap();
        assert_eq!((first.version, second.version), (2, 3));
        let saved = store.mark_saved(&opened.uri).unwrap();
        assert_eq!(saved.version, 3);
        assert!(!store.snapshot(&opened.uri).unwrap().dirty);
    }

    #[test]
    fn incremental_sync_emits_one_old_document_utf16_range() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Incremental);
        let opened = store.open(&path, "rust", "a😀b\nsecond\n").unwrap();
        let change = store.change(&opened.uri, "a한b\nchanged\n", true).unwrap();
        assert_eq!(change.version, 2);
        assert_eq!(change.content_changes.len(), 1);
        assert_eq!(
            change.content_changes[0],
            TextChange {
                range: Some(LspRange {
                    start: super::super::positions::LspPosition::new(0, 1),
                    end: super::super::positions::LspPosition::new(1, 5),
                }),
                text: "한b\nchange".into(),
            }
        );
    }

    #[test]
    fn reload_is_full_and_keeps_version_sequence() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf8, SyncKind::Incremental);
        let opened = store.open(&path, "rust", "old\n").unwrap();
        store.change(&opened.uri, "dirty\n", true).unwrap();
        let reload = store.reload(&opened.uri, "disk\n").unwrap();
        assert_eq!(reload.version, 3);
        assert_eq!(reload.content_changes[0].range, None);
        assert_eq!(reload.content_changes[0].text, "disk\n");
        assert!(!store.snapshot(&opened.uri).unwrap().dirty);
    }

    #[test]
    fn stale_requests_and_diagnostics_do_not_become_current() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Full);
        let opened = store.open(&path, "rust", "one").unwrap();
        let request = store.request_snapshot(&opened.uri).unwrap();
        assert!(store.is_current(&request));
        store.change(&opened.uri, "two", true).unwrap();
        assert!(!store.is_current(&request));
        assert!(!store.diagnostics_are_current(&opened.uri, Some(1)));
        assert!(store.diagnostics_are_current(&opened.uri, Some(2)));
        assert!(!store.diagnostics_are_current(&opened.uri, None));
    }

    #[test]
    fn restart_replays_buffers_at_version_one_without_losing_dirty_state() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Full);
        let opened = store.open(&path, "rust", "one").unwrap();
        store.change(&opened.uri, "dirty", true).unwrap();
        let replay = store.restart_session();
        assert_eq!(replay.len(), 1);
        assert_eq!((replay[0].version, replay[0].text.as_str()), (1, "dirty"));
        let snapshot = store.snapshot(&opened.uri).unwrap();
        assert_eq!(snapshot.version, 1);
        assert!(snapshot.dirty);
    }

    #[test]
    fn open_snapshot_imports_dirty_buffer_at_version_one() {
        let (_directory, path, mut source) = store(PositionEncoding::Utf16, SyncKind::Full);
        let opened = source.open(&path, "rust", "one").unwrap();
        source.change(&opened.uri, "dirty", true).unwrap();
        let snapshot = source.snapshot(&opened.uri).unwrap();
        let workspace = source.workspace().clone();
        let mut replacement =
            DocumentStore::new(workspace, PositionEncoding::Utf8, SyncKind::Incremental);

        let replay = replacement.open_snapshot(&snapshot).unwrap();
        assert_eq!(replay.version, 1);
        assert_eq!(replay.text, "dirty");
        assert_eq!(replacement.snapshot(&replay.uri).unwrap().version, 1);
        assert!(replacement.snapshot(&replay.uri).unwrap().dirty);
    }

    #[test]
    fn non_lf_text_and_outside_paths_are_rejected() {
        let (_directory, path, mut store) = store(PositionEncoding::Utf16, SyncKind::Full);
        assert!(matches!(
            store.open(&path, "rust", "a\r\nb"),
            Err(DocumentError::NonNormalizedLineEndings)
        ));
        let outside_directory = tempdir().unwrap();
        let outside = outside_directory.path().join("outside-lsp-test.rs");
        fs::write(&outside, "").unwrap();
        assert!(matches!(
            store.open(&outside, "rust", ""),
            Err(DocumentError::PathOutsideWorkspace(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn canonicalization_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("secret.rs");
        fs::write(&outside_file, "").unwrap();
        let link = directory.path().join("linked.rs");
        symlink(&outside_file, &link).unwrap();
        let mut store = DocumentStore::new(
            WorkspaceRoot::new(directory.path()).unwrap(),
            PositionEncoding::Utf16,
            SyncKind::Full,
        );
        assert!(matches!(
            store.open(link, "rust", ""),
            Err(DocumentError::PathOutsideWorkspace(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_drive_and_unc_file_uris_use_url_path_rules() {
        for path in [
            Path::new(r"C:\work\a b\한글.rs"),
            Path::new(r"\\server\share\a b.rs"),
        ] {
            let uri = file_uri_from_absolute_path(path).unwrap();
            assert_eq!(file_uri_to_path(&uri).unwrap(), path);
            assert!(uri.as_str().contains("%20"));
        }
    }
}
