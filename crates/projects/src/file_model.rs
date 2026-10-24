use devbox_documents::{DiskRev, DocKey, EntryKind};
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileTreeQuery {
    pub project_id: String,
    pub dir: String,
    pub show_hidden: bool,
    pub cursor: Option<FileTreeCursor>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileTreeCursor {
    pub after: String,
    pub rev: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub key: DocKey,
    pub name: String,
    pub kind: EntryKind,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileTree {
    pub entries: Vec<FileEntry>,
    pub next_cursor: Option<FileTreeCursor>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct FileKey {
    pub key: DocKey,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DeleteFile {
    pub key: DocKey,
    pub expect: DiskRev,
}

domain_error! { pub enum FilesError {
    InvalidInput = "files.invalid_input",
    OutOfScope = "files.out_of_scope",
    NotFound = "files.not_found",
    Exists = "files.exists",
    StateChanged = "files.state_changed",
    NotEmpty = "files.not_empty",
    LimitExceeded = "files.limit_exceeded",
    EditorUnavailable = "files.editor_unavailable",
} }

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenSourceEditor {
    pub key: DocKey,
    pub editor: crate::SourceEditor,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SourceEditorOpened {
    pub opened: bool,
    pub path: String,
}
