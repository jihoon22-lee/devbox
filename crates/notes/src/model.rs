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
/// A bounded, lease-less vault read for MCP and the read-only search preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NoteContent {
    pub path: String,
    pub text: String,
    pub rev: DiskRev,
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
        TargetUnsaved = "notes.target_unsaved",
        StaleCursor = "notes.stale_cursor",
        TooLarge { limit: u64 } = "notes.too_large",
        OpenUnsaved = "notes.open_unsaved",
        SummaryBroken = "notes.summary_broken",
        Conflict = "notes.conflict",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameNote {
    pub path: String,
    pub new_path: String,
    /// Rewrite links in other notes (and the note's own relative links) that would break.
    pub rewrite_links: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameStarted {
    pub op_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameOp {
    pub op_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RenameState {
    Running,
    Done,
    Incomplete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RewriteFailure {
    /// The note changed after the rename planned it; it was not overwritten.
    Conflict,
    /// Open with unsaved edits and the window did not apply the request in time.
    Open,
    /// The note was gone when its turn came.
    Missing,
    /// A link form that cannot be rewritten safely (reference-style Markdown links).
    Unsupported,
    /// The rewritten note would be larger than the note limit on disk.
    TooLarge,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct FailedRewrite {
    pub path: String,
    pub reason: RewriteFailure,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RenameStatus {
    pub state: RenameState,
    pub failed: Vec<FailedRewrite>,
    pub truncated: bool,
    /// Why an `incomplete` rename stopped; `null` while running or when done.
    pub reason: Option<IncompleteReason>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    /// Some notes were left as they were (`failed` lists them).
    Failed,
    /// Resumed after an exit, but another note took the new name meanwhile.
    TargetExists,
    /// Resumed after an exit, but the note itself was gone.
    SourceMissing,
    /// Resumed after an exit, but the note could not be moved.
    MoveFailed,
    /// The progress journal could not be read; it was set aside as `<opId>.json.corrupt`.
    Unreadable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextEdit {
    pub from_utf16: u32,
    pub to_utf16: u32,
    pub text: String,
}
/// Sent to the window holding `leaseId` when the note has unsaved edits. The edits
/// apply to the buffer saved as journal `journalSeq`; a window whose buffer moved on
/// should answer `applied: false`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RewriteRequest {
    pub op_id: String,
    pub key: devbox_documents::DocKey,
    pub lease_id: String,
    pub journal_seq: u64,
    pub edits: Vec<TextEdit>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewriteReply {
    pub op_id: String,
    pub key: devbox_documents::DocKey,
    /// True after the window applied the edits and saved the note.
    pub applied: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenameIncomplete {
    pub op_id: String,
    /// Notes left as they were.
    pub count: u32,
    pub reason: IncompleteReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DailyParams {
    /// `YYYY-MM-DD`; `null` is today (local).
    pub date: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DailyNote {
    pub path: String,
    pub created: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AppendTarget {
    /// The `[notes] inbox` note.
    Inbox,
    /// Today's daily note (created when missing).
    Daily,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AppendFormat {
    /// The text as it is, on its own line.
    Plain,
    /// `- hh:mm <text>`, later lines indented by two spaces.
    Capture,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AppendNote {
    pub target: AppendTarget,
    pub text: String,
    pub format: AppendFormat,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Appended {
    pub path: String,
    pub rev: DiskRev,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct InsertSummary {
    /// `YYYY-MM-DD`; `null` is today (local).
    pub date: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SummaryInserted {
    pub path: String,
    pub rev: DiskRev,
}
