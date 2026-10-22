use devbox_documents::DiskRev;
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Parameters for methods that take none.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TreeParams {
    /// `null` starts a new listing; otherwise the previous page's `nextCursor`.
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TreeItem {
    /// Vault-relative `/` path.
    pub path: String,
    pub dir: bool,
    pub title: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteTree {
    pub rev: String,
    pub items: Vec<TreeItem>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateNote {
    /// Vault-relative folder, `""` for the vault root.
    pub folder: String,
    pub title: String,
    /// Template name from `notes.templates`, or `null` for the default body.
    pub template: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreatedNote {
    pub path: String,
    pub rev: DiskRev,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NotePath {
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trashed {
    pub trash_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreNote {
    pub trash_path: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TagCount {
    pub tag: String,
    pub count: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Tags {
    pub rev: String,
    pub items: Vec<TagCount>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Backlink {
    pub src: String,
    pub title: String,
    /// 1-based line of the link in `src`.
    pub line: u32,
    pub from_utf16: u32,
    pub to_utf16: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Backlinks {
    pub rev: String,
    pub items: Vec<Backlink>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// `[[target]]`: resolved through note keys.
    Wiki,
    /// `[text](relative.md)`: a path relative to `from`.
    Md,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResolveLink {
    /// `[[target]]` text (`#heading`, `|alias` allowed) or a Markdown link destination.
    pub target: String,
    pub kind: LinkKind,
    /// The note that contains the link; nearer candidates win ties. `null` = vault root.
    pub from: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ResolvedLink {
    pub paths: Vec<String>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RenderNote {
    /// Note whose folder anchors relative images; read from disk when `text` is `null`.
    pub path: Option<String>,
    pub text: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RenderedNote {
    pub html: String,
    pub mermaid: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachImage {
    pub note_path: String,
    pub name: String,
    /// Standard base64 of the image bytes.
    pub data: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Attached {
    pub path: String,
    pub markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TemplateItem {
    pub name: String,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Templates {
    pub items: Vec<TemplateItem>,
}

/// Published only when the index actually changed (memory counter rev).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NotesChanged {
    pub rev: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TemplateProblem {
    NotFound,
    UnknownPlaceholder,
    InvalidContent,
    InvalidTarget,
    InvalidTitle,
    InvalidDate,
    InvalidTime,
    OutputTooLarge,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ImageProblem {
    Empty,
    TooLarge,
    Unsupported,
    Dimensions,
    Encoding,
}

domain_error! {
    pub enum NotesError {
        NotFound = "notes.not_found",
        Exists = "notes.exists",
        OutOfVault = "notes.out_of_vault",
        InvalidName = "notes.invalid_name",
        InvalidTemplate { problem: TemplateProblem } = "notes.invalid_template",
        InvalidImage { problem: ImageProblem } = "notes.invalid_image",
        InvalidInput = "notes.invalid_input",
        Busy = "notes.busy",
        StaleCursor = "notes.stale_cursor",
        TooLarge { limit: u64 } = "notes.too_large",
    }
}
