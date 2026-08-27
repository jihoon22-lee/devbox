//! Bounded, offline text extraction for the Everything+ content index.
//!
//! Plain text/source/Markdown, PDF, and legacy XLS are intentionally separate
//! extractor formats. The PDF path uses lopdf only for text objects and the
//! XLS path uses calamine's pure-Rust Xls reader; neither renders pages, runs
//! OCR, follows external resources, evaluates formulas, or executes document
//! content.

use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use calamine::{Data, Reader, Xls, XlsError};

/// Maximum bytes read from one content-index candidate.
pub const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;
/// Maximum Unicode scalar values retained in the FTS document.
pub const MAX_TEXT_CHARS: usize = 2_000_000;
/// Cooperative wall-clock budget for one candidate.
pub const PROCESSING_LIMIT: Duration = Duration::from_secs(10);
/// Bumped only when the plain-text extraction rules change.
pub const EXTRACTOR_VERSION: &str = "text-v1";
/// Bumped only when PDF parsing or text normalization rules change.  Keeping
/// this separate from `EXTRACTOR_VERSION` lets startup reindex only PDFs when
/// the PDF implementation changes.
pub const PDF_EXTRACTOR_VERSION: &str = "pdf-v1";
/// Bumped only when XLS parsing or cell-to-text normalization rules change.
/// This is deliberately independent from both the plain-text and PDF versions
/// so a spreadsheet upgrade never rereads unrelated content.
pub const XLS_EXTRACTOR_VERSION: &str = "xls-v1";
/// Defensive logical bounds for a legacy workbook.  A preflight of the CFB
/// Workbook stream rejects oversized sheet/dimension declarations before
/// calamine reserves its in-memory ranges; the post-parse check is retained as
/// a second guard for sparse cell coordinates not covered by Dimensions.
pub const XLS_MAX_SHEETS: usize = 256;
pub const XLS_MAX_CELLS: usize = 4_000_000;
pub const XLS_MAX_RECORDS: usize = 1_000_000;
pub const XLS_MAX_SHARED_STRINGS: usize = 200_000;
pub const XLS_MAX_SHARED_STRING_CHARS: usize = 8_000_000;
pub const XLS_MAX_EXPANDED_STRING_CHARS: usize = 16_000_000;
pub const XLS_MAX_FORMULAS: usize = 100_000;
pub const XLS_MAX_METADATA_RECORDS: usize = 200_000;
pub const XLS_MAX_ESTIMATED_MEMORY_BYTES: usize = 256 * 1024 * 1024;
/// Maximum decompressed bytes allowed for one PDF page/object stream.  A PDF
/// can be much smaller than its inflated content, so the file-size bound alone
/// is not sufficient to defend the parser from decompression bombs.
pub const PDF_MAX_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
/// Maximum parsed indirect objects retained for one PDF.
pub const PDF_MAX_OBJECTS: usize = 100_000;
/// Maximum pages traversed for one PDF.
pub const PDF_MAX_PAGES: usize = 10_000;
/// Maximum snippet size returned to the frontend for one result.
pub const MAX_SNIPPET_CHARS: usize = 4 * 1024;

const READ_CHUNK_BYTES: usize = 64 * 1024;
const UTF16_SAMPLE_BYTES: usize = 16 * 1024;

/// Extensions whose contents are useful for local developer search.  The list
/// is intentionally explicit: a content root never means "read every file".
pub const TEXT_EXTENSIONS: &[&str] = &[
    "asm",
    "bat",
    "c",
    "cc",
    "cfg",
    "clj",
    "cljs",
    "conf",
    "cpp",
    "cs",
    "css",
    "csv",
    "dart",
    "diff",
    "ex",
    "exs",
    "fish",
    "gitattributes",
    "gitignore",
    "go",
    "graphql",
    "h",
    "hpp",
    "hrl",
    "htm",
    "html",
    "ini",
    "java",
    "jl",
    "js",
    "json",
    "jsx",
    "kt",
    "kts",
    "less",
    "log",
    "lua",
    "md",
    "mdown",
    "mkd",
    "patch",
    "php",
    "pl",
    "ps1",
    "py",
    "rb",
    "rs",
    "rst",
    "scss",
    "sh",
    "sql",
    "swift",
    "tex",
    "toml",
    "ts",
    "tsx",
    "txt",
    "vue",
    "xml",
    "xsd",
    "yaml",
    "yml",
    "zsh",
];

/// Machine-readable status values persisted with each eligible file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentStatus {
    Indexed,
    TooLarge,
    UnsupportedEncoding,
    ReadError,
    Timeout,
    ChangedDuringRead,
    SkippedSensitive,
    NoText,
    UnsupportedEncrypted,
    ExtractError,
}

impl ContentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::TooLarge => "too_large",
            Self::UnsupportedEncoding => "unsupported_encoding",
            Self::ReadError => "read_error",
            Self::Timeout => "timeout",
            Self::ChangedDuringRead => "changed_during_read",
            Self::SkippedSensitive => "skipped_sensitive",
            Self::NoText => "no_text",
            Self::UnsupportedEncrypted => "unsupported_encrypted",
            Self::ExtractError => "extract_error",
        }
    }
}

/// Result of one bounded extraction.  Failed records deliberately contain no
/// source bytes, and `error_code` is a fixed code rather than an IO detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRecord {
    pub text: String,
    pub status: ContentStatus,
    pub extractor_version: &'static str,
    pub encoding: Option<&'static str>,
    pub truncated: bool,
    pub error_code: Option<&'static str>,
    pub text_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileFingerprint {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    fn changed_from(self, initial: Self, expected_size: u64) -> bool {
        initial.len != expected_size || self.len != expected_size || self != initial
    }
}

impl ContentRecord {
    fn failure(status: ContentStatus, error_code: &'static str) -> Self {
        Self::failure_for(EXTRACTOR_VERSION, status, error_code)
    }

    fn failure_for(
        extractor_version: &'static str,
        status: ContentStatus,
        error_code: &'static str,
    ) -> Self {
        Self {
            text: String::new(),
            status,
            extractor_version,
            encoding: None,
            truncated: false,
            error_code: Some(error_code),
            text_chars: 0,
        }
    }
}

/// Returns true only for a known content extension.  The comparison is
/// case-insensitive and does not inspect file bytes.
pub fn is_text_ext(ext: &str) -> bool {
    let lower = ext.trim_start_matches('.').to_ascii_lowercase();
    TEXT_EXTENSIONS.iter().any(|candidate| *candidate == lower)
}

/// Returns true for a PDF extension without inspecting file bytes.  The
/// explicit extension dispatch ensures a corrupt `.pdf` receives
/// `extract_error` rather than being treated as an arbitrary binary text file.
pub fn is_pdf_ext(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("pdf")
}

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_pdf_ext)
}

/// Returns true for legacy binary Excel workbooks only.  XLSX/ODS are separate
/// format features and must not enter this extractor through auto-detection.
pub fn is_xls_ext(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("xls")
}

pub fn is_xls_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_xls_ext)
}

/// Returns true for files that commonly contain credentials or private key
/// material.  These are skipped before reading, even when their extension is a
/// text extension.  Normal source files containing a word such as `secret` are
/// still indexable; this is a filename policy, not a content classifier.
pub fn is_sensitive_filename(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    lower == ".env"
        || lower.starts_with(".env.")
        || lower == ".npmrc"
        || lower == ".netrc"
        || lower == "credentials"
        || lower.starts_with("credentials.")
        || lower == "secrets"
        || lower.starts_with("secrets.")
        || lower.ends_with(".secret")
        || lower.ends_with(".secrets")
        || lower.starts_with("id_rsa")
        || lower.starts_with("id_ed25519")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
}

/// Whether an extension is eligible for a content status record.  Oversized
/// files intentionally remain eligible so their `too_large` status is visible
/// instead of silently looking as if indexing was disabled.
pub fn is_content_candidate(path: &Path) -> bool {
    if is_sensitive_filename(path) {
        return true;
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                ".dockerignore"
                    | ".editorconfig"
                    | ".gitattributes"
                    | ".gitignore"
                    | "dockerfile"
                    | "makefile"
                    | "license"
                    | "readme"
            )
        })
    {
        return true;
    }
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    is_text_ext(ext) || is_pdf_ext(ext) || is_xls_ext(ext)
}

/// Extract one file while enforcing byte, character, encoding, race, and time
/// bounds.  No error returned by this function contains the source path.
pub fn extract_file(path: &Path, expected_size: u64, started: Instant) -> ContentRecord {
    let extractor_version = if is_pdf_path(path) {
        PDF_EXTRACTOR_VERSION
    } else if is_xls_path(path) {
        XLS_EXTRACTOR_VERSION
    } else {
        EXTRACTOR_VERSION
    };
    if is_sensitive_filename(path) {
        return ContentRecord::failure_for(
            extractor_version,
            ContentStatus::SkippedSensitive,
            "sensitive_file",
        );
    }
    if expected_size > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            extractor_version,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            extractor_version,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }

    let before = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        _ => {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::ReadError,
                "read_error",
            )
        }
    };
    let before_fingerprint = FileFingerprint::from_metadata(&before);
    if before_fingerprint.len != expected_size {
        return ContentRecord::failure_for(
            extractor_version,
            ContentStatus::ChangedDuringRead,
            "changed_during_read",
        );
    }

    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::ReadError,
                "read_error",
            )
        }
    };
    let mut bytes = Vec::with_capacity(expected_size.min(MAX_FILE_BYTES) as usize);
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        if timed_out(started) {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::Timeout,
                "processing_timeout",
            );
        }
        let read = match file.read(&mut chunk) {
            Ok(read) => read,
            Err(_) => {
                return ContentRecord::failure_for(
                    extractor_version,
                    ContentStatus::ReadError,
                    "read_error",
                )
            }
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::TooLarge,
                "file_too_large",
            );
        }
    }

    let opened_after = match file.metadata() {
        Ok(metadata) if metadata.file_type().is_file() => FileFingerprint::from_metadata(&metadata),
        _ => {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::ChangedDuringRead,
                "changed_during_read",
            )
        }
    };

    let after = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => metadata,
        _ => {
            return ContentRecord::failure_for(
                extractor_version,
                ContentStatus::ChangedDuringRead,
                "changed_during_read",
            )
        }
    };
    let after_fingerprint = FileFingerprint::from_metadata(&after);
    if bytes.len() as u64 != expected_size
        || opened_after.changed_from(before_fingerprint, expected_size)
        || after_fingerprint.changed_from(before_fingerprint, expected_size)
    {
        return ContentRecord::failure_for(
            extractor_version,
            ContentStatus::ChangedDuringRead,
            "changed_during_read",
        );
    }

    if is_pdf_path(path) {
        extract_pdf_bytes(&bytes, started)
    } else if is_xls_path(path) {
        extract_xls_bytes(&bytes, started)
    } else {
        extract_bytes(&bytes, started)
    }
}

/// Decode a bounded byte fixture.  Kept public for deterministic unit tests and
/// for the watcher/indexer to share exactly the same UTF-8/UTF-16 rules.
pub fn extract_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure(ContentStatus::TooLarge, "file_too_large");
    }
    if timed_out(started) {
        return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
    }

    let (text, encoding) = match decode(bytes, started) {
        Ok(decoded) => decoded,
        Err(DecodeFailure::Timeout) => {
            return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout")
        }
        Err(DecodeFailure::Unsupported) => {
            return ContentRecord::failure(
                ContentStatus::UnsupportedEncoding,
                "unsupported_encoding",
            )
        }
    };
    if text.contains('\0') {
        return ContentRecord::failure(ContentStatus::UnsupportedEncoding, "unsupported_encoding");
    }
    let (text, truncated, text_chars) = truncate_text(text, started);
    if timed_out(started) {
        return ContentRecord::failure(ContentStatus::Timeout, "processing_timeout");
    }
    ContentRecord {
        text,
        status: ContentStatus::Indexed,
        extractor_version: EXTRACTOR_VERSION,
        encoding: Some(encoding),
        truncated,
        error_code: truncated.then_some("text_limit"),
        text_chars,
    }
}

/// Extract text-only content from a PDF byte fixture using the MIT-licensed
/// lopdf parser.  The caller must enforce the path/regular-file race checks;
/// this function intentionally accepts bytes so it cannot leak a path through
/// parser diagnostics or logs.
pub fn extract_pdf_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }

    let document = match lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions::with_max_decompressed_size(PDF_MAX_DECOMPRESSED_BYTES),
    ) {
        Ok(document) => document,
        Err(_) => {
            return ContentRecord::failure_for(
                PDF_EXTRACTOR_VERSION,
                ContentStatus::ExtractError,
                "extract_error",
            )
        }
    };

    // lopdf automatically attempts an empty password.  `was_encrypted` is
    // therefore required in addition to `is_encrypted`: an encrypted PDF with
    // an empty password must still be isolated instead of silently indexed.
    if document.is_encrypted() || document.was_encrypted() {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncrypted,
            "unsupported_encrypted",
        );
    }
    // Reject an oversized object graph before walking the page tree.  The
    // loader's per-stream inflate cap bounds decompression; this separate
    // count cap bounds later traversal work.
    if pdf_object_limit_exceeded(document.objects.len()) {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "resource_limit",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }

    // `take(limit + 1)` avoids materializing an unbounded page map merely to
    // decide whether this document is eligible. lopdf's iterator also bounds
    // page-tree depth and total visits by the parsed object count.
    let page_count = document.page_iter().take(PDF_MAX_PAGES + 1).count();
    if pdf_page_limit_exceeded(page_count) {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "resource_limit",
        );
    }
    if page_count == 0 {
        return ContentRecord::failure_for(PDF_EXTRACTOR_VERSION, ContentStatus::NoText, "no_text");
    }

    let mut text = String::new();
    let mut text_chars = 0usize;
    let mut truncated = false;
    for page_number in 1..=page_count as u32 {
        if timed_out(started) {
            return ContentRecord::failure_for(
                PDF_EXTRACTOR_VERSION,
                ContentStatus::Timeout,
                "processing_timeout",
            );
        }
        let chunks =
            document.extract_text_chunks_with_limit(&[page_number], PDF_MAX_DECOMPRESSED_BYTES);
        for chunk in chunks {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    return ContentRecord::failure_for(
                        PDF_EXTRACTOR_VERSION,
                        ContentStatus::ExtractError,
                        "extract_error",
                    )
                }
            };
            for character in chunk.chars() {
                if text_chars == MAX_TEXT_CHARS {
                    truncated = true;
                    break;
                }
                text.push(character);
                text_chars += 1;
            }
            if truncated {
                break;
            }
        }
        if truncated {
            break;
        }
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            PDF_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    if !text.chars().any(|character| !character.is_whitespace()) {
        return ContentRecord::failure_for(PDF_EXTRACTOR_VERSION, ContentStatus::NoText, "no_text");
    }

    ContentRecord {
        text,
        status: ContentStatus::Indexed,
        extractor_version: PDF_EXTRACTOR_VERSION,
        encoding: Some("pdf"),
        truncated,
        error_code: truncated.then_some("text_limit"),
        text_chars,
    }
}

fn pdf_object_limit_exceeded(object_count: usize) -> bool {
    object_count > PDF_MAX_OBJECTS
}

fn pdf_page_limit_exceeded(page_count: usize) -> bool {
    page_count > PDF_MAX_PAGES
}

/// Extracts cell values from a legacy binary Excel workbook using calamine's
/// pure-Rust `Xls` reader.  Only worksheet values are retained: formulas are
/// never evaluated, VBA projects are never opened, images/styles are ignored,
/// and no external resource is followed.  The caller supplies bytes so parser
/// diagnostics cannot expose a filesystem path.
pub fn extract_xls_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            XLS_EXTRACTOR_VERSION,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            XLS_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    match preflight_xls(bytes, started) {
        Ok(()) => {}
        Err(XlsPreflightFailure::Encrypted) => {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::UnsupportedEncrypted,
                "unsupported_encrypted",
            )
        }
        Err(XlsPreflightFailure::ResourceLimit) => {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::ExtractError,
                "resource_limit",
            )
        }
        Err(XlsPreflightFailure::Timeout) => {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::Timeout,
                "processing_timeout",
            )
        }
        Err(XlsPreflightFailure::Malformed) => {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::ExtractError,
                "extract_error",
            )
        }
    }

    let workbook_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Xls::new(Cursor::new(bytes))
    }));
    let mut workbook = match workbook_result {
        Ok(Ok(workbook)) => workbook,
        Ok(Err(XlsError::Password)) => {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::UnsupportedEncrypted,
                "unsupported_encrypted",
            )
        }
        Ok(Err(_)) | Err(_) => {
            // calamine is a pure parser, but keep a malformed BIFF panic from
            // taking down the indexing worker or exposing parser diagnostics.
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::ExtractError,
                "extract_error",
            );
        }
    };
    if timed_out(started) {
        return ContentRecord::failure_for(
            XLS_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }

    let sheet_names = workbook.sheet_names();
    if sheet_names.len() > XLS_MAX_SHEETS {
        return ContentRecord::failure_for(
            XLS_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "resource_limit",
        );
    }

    let mut output = XlsTextAccumulator::default();
    let mut logical_cells = 0usize;
    for sheet_name in sheet_names {
        if timed_out(started) {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::Timeout,
                "processing_timeout",
            );
        }
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(_) => {
                return ContentRecord::failure_for(
                    XLS_EXTRACTOR_VERSION,
                    ContentStatus::ExtractError,
                    "extract_error",
                )
            }
        };
        let (rows, columns) = range.get_size();
        logical_cells = logical_cells.saturating_add(rows.saturating_mul(columns));
        if logical_cells > XLS_MAX_CELLS {
            return ContentRecord::failure_for(
                XLS_EXTRACTOR_VERSION,
                ContentStatus::ExtractError,
                "resource_limit",
            );
        }

        let mut visited_cells = 0usize;
        for row in range.rows() {
            let mut row_has_value = false;
            for cell in row {
                visited_cells = visited_cells.saturating_add(1);
                if visited_cells.is_multiple_of(8192) && timed_out(started) {
                    return ContentRecord::failure_for(
                        XLS_EXTRACTOR_VERSION,
                        ContentStatus::Timeout,
                        "processing_timeout",
                    );
                }
                if matches!(cell, Data::Empty) {
                    continue;
                }
                let value = cell.to_string();
                if value.is_empty() {
                    continue;
                }
                if row_has_value {
                    output.push_separator('\t');
                } else if output.has_value {
                    output.push_separator('\n');
                }
                row_has_value = true;
                match output.push_value(&value, started) {
                    XlsAppendResult::Complete => {}
                    XlsAppendResult::Truncated => {
                        return ContentRecord {
                            text: output.text,
                            status: ContentStatus::Indexed,
                            extractor_version: XLS_EXTRACTOR_VERSION,
                            encoding: Some("xls"),
                            truncated: true,
                            error_code: Some("text_limit"),
                            text_chars: output.text_chars,
                        }
                    }
                    XlsAppendResult::Timeout => {
                        return ContentRecord::failure_for(
                            XLS_EXTRACTOR_VERSION,
                            ContentStatus::Timeout,
                            "processing_timeout",
                        )
                    }
                }
            }
        }
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            XLS_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    if !output.has_value {
        return ContentRecord::failure_for(XLS_EXTRACTOR_VERSION, ContentStatus::NoText, "no_text");
    }

    ContentRecord {
        text: output.text,
        status: ContentStatus::Indexed,
        extractor_version: XLS_EXTRACTOR_VERSION,
        encoding: Some("xls"),
        truncated: false,
        error_code: None,
        text_chars: output.text_chars,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XlsPreflightFailure {
    Encrypted,
    ResourceLimit,
    Timeout,
    Malformed,
}

#[derive(Debug, Default)]
struct XlsPreflightBudget {
    records: usize,
    logical_cells: usize,
    formulas: usize,
    metadata_records: usize,
    expanded_string_chars: usize,
    estimated_memory_bytes: usize,
}

impl XlsPreflightBudget {
    fn record(&mut self) -> Result<(), XlsPreflightFailure> {
        self.records = self.records.saturating_add(1);
        if self.records > XLS_MAX_RECORDS {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }

    fn metadata(&mut self, count: usize) -> Result<(), XlsPreflightFailure> {
        self.metadata_records = self.metadata_records.saturating_add(count);
        if self.metadata_records > XLS_MAX_METADATA_RECORDS {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }

    fn logical_cells(&mut self, count: usize) -> Result<(), XlsPreflightFailure> {
        self.logical_cells = self.logical_cells.saturating_add(count);
        if self.logical_cells > XLS_MAX_CELLS {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }

    fn formulas(&mut self, count: usize) -> Result<(), XlsPreflightFailure> {
        self.formulas = self.formulas.saturating_add(count);
        if self.formulas > XLS_MAX_FORMULAS {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }

    fn expanded_string(&mut self, chars: usize) -> Result<(), XlsPreflightFailure> {
        self.expanded_string_chars = self.expanded_string_chars.saturating_add(chars);
        if self.expanded_string_chars > XLS_MAX_EXPANDED_STRING_CHARS {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }

    fn memory(&mut self, bytes: usize) -> Result<(), XlsPreflightFailure> {
        self.estimated_memory_bytes = self.estimated_memory_bytes.saturating_add(bytes);
        if self.estimated_memory_bytes > XLS_MAX_ESTIMATED_MEMORY_BYTES {
            Err(XlsPreflightFailure::ResourceLimit)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy)]
struct BiffRecordRef<'a> {
    typ: u16,
    payload: &'a [u8],
    next_offset: usize,
}

fn biff_record_at(
    stream: &[u8],
    offset: usize,
) -> Result<Option<BiffRecordRef<'_>>, XlsPreflightFailure> {
    if offset == stream.len() {
        return Ok(None);
    }
    let header_end = offset
        .checked_add(4)
        .ok_or(XlsPreflightFailure::Malformed)?;
    if header_end > stream.len() {
        return Err(XlsPreflightFailure::Malformed);
    }
    let typ = u16::from_le_bytes([stream[offset], stream[offset + 1]]);
    let len = u16::from_le_bytes([stream[offset + 2], stream[offset + 3]]) as usize;
    let payload_end = header_end
        .checked_add(len)
        .ok_or(XlsPreflightFailure::Malformed)?;
    if payload_end > stream.len() {
        return Err(XlsPreflightFailure::Malformed);
    }
    Ok(Some(BiffRecordRef {
        typ,
        payload: &stream[header_end..payload_end],
        next_offset: payload_end,
    }))
}

/// Fail-closed admission control for calamine's eager legacy-XLS parser.
/// The CFB and BIFF structures are validated from the already bounded byte
/// buffer. Shared-string expansion, dense ranges, formulas and metadata are
/// charged before `Xls::new` can reserve or clone attacker-controlled data.
fn preflight_xls(bytes: &[u8], started: Instant) -> Result<(), XlsPreflightFailure> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut container = cfb::CompoundFile::open(Cursor::new(bytes))
            .map_err(|_| XlsPreflightFailure::Malformed)?;
        for stream_name in ["/Workbook", "/Book", "/WORKBOOK", "/BOOK"] {
            let entry = match container.entry(stream_name) {
                Ok(entry) if entry.is_stream() => entry,
                _ => continue,
            };
            if entry.len() > MAX_FILE_BYTES {
                return Err(XlsPreflightFailure::ResourceLimit);
            }
            let mut stream = container
                .open_stream(stream_name)
                .map_err(|_| XlsPreflightFailure::Malformed)?;
            let mut workbook = Vec::with_capacity(entry.len() as usize);
            stream
                .read_to_end(&mut workbook)
                .map_err(|_| XlsPreflightFailure::Malformed)?;
            if timed_out(started) {
                return Err(XlsPreflightFailure::Timeout);
            }
            return preflight_biff(&workbook, started);
        }
        Err(XlsPreflightFailure::Malformed)
    }))
    .unwrap_or(Err(XlsPreflightFailure::Malformed))
}

fn preflight_biff(stream: &[u8], started: Instant) -> Result<(), XlsPreflightFailure> {
    if detect_xls_encryption(stream, started)? {
        return Err(XlsPreflightFailure::Encrypted);
    }

    let mut budget = XlsPreflightBudget::default();
    let mut sheet_starts = Vec::new();
    let mut shared_string_chars = Vec::new();
    let mut offset = 0usize;
    let mut found_global_eof = false;
    while let Some(record) = biff_record_at(stream, offset)? {
        budget.record()?;
        if budget.records.is_multiple_of(8192) && timed_out(started) {
            return Err(XlsPreflightFailure::Timeout);
        }
        offset = record.next_offset;
        match record.typ {
            0x0085 => {
                if record.payload.len() < 4 {
                    return Err(XlsPreflightFailure::Malformed);
                }
                if sheet_starts.len() == XLS_MAX_SHEETS {
                    return Err(XlsPreflightFailure::ResourceLimit);
                }
                sheet_starts.push(u32::from_le_bytes(
                    record.payload[..4]
                        .try_into()
                        .map_err(|_| XlsPreflightFailure::Malformed)?,
                ) as usize);
                budget.metadata(1)?;
            }
            0x00FC => {
                let mut parts = vec![record.payload];
                while let Some(next) = biff_record_at(stream, offset)? {
                    if next.typ != 0x003C {
                        break;
                    }
                    budget.record()?;
                    parts.push(next.payload);
                    offset = next.next_offset;
                }
                shared_string_chars = preflight_sst(&parts, &mut budget, started)?;
            }
            0x013D => budget.metadata(record.payload.len() / 2)?,
            0x0017 => budget.metadata(record.payload.len() / 2)?,
            0x041E | 0x00E0 | 0x0018 => {
                budget.metadata(1)?;
                budget.memory(record.payload.len().saturating_mul(4))?;
            }
            0x000A => {
                found_global_eof = true;
                break;
            }
            _ => {}
        }
    }
    if !found_global_eof {
        return Err(XlsPreflightFailure::Malformed);
    }

    for sheet_start in sheet_starts {
        preflight_biff_sheet(
            stream,
            sheet_start,
            &shared_string_chars,
            &mut budget,
            started,
        )?;
    }
    Ok(())
}

fn detect_xls_encryption(stream: &[u8], started: Instant) -> Result<bool, XlsPreflightFailure> {
    let mut offset = 0usize;
    let mut records = 0usize;
    while let Some(record) = biff_record_at(stream, offset)? {
        records = records.saturating_add(1);
        if records.is_multiple_of(8192) && timed_out(started) {
            return Err(XlsPreflightFailure::Timeout);
        }
        offset = record.next_offset;
        if record.typ == 0x002F {
            if record.payload.len() < 2 {
                return Err(XlsPreflightFailure::Malformed);
            }
            return Ok(u16::from_le_bytes([record.payload[0], record.payload[1]]) != 0);
        }
        if record.typ == 0x000A {
            return Ok(false);
        }
    }
    Err(XlsPreflightFailure::Malformed)
}

struct SstCursor<'a> {
    parts: &'a [&'a [u8]],
    part: usize,
    offset: usize,
}

impl<'a> SstCursor<'a> {
    fn new(parts: &'a [&'a [u8]]) -> Self {
        Self {
            parts,
            part: 0,
            offset: 0,
        }
    }

    fn current(&self) -> Option<&'a [u8]> {
        self.parts.get(self.part).copied()
    }

    fn advance_empty_parts(&mut self) {
        while self.current().is_some_and(|part| self.offset == part.len()) {
            self.part += 1;
            self.offset = 0;
        }
    }

    fn is_done(&mut self) -> bool {
        self.advance_empty_parts();
        self.current().is_none()
    }

    fn local_bytes(&mut self, len: usize) -> Result<&'a [u8], XlsPreflightFailure> {
        self.advance_empty_parts();
        let part = self.current().ok_or(XlsPreflightFailure::Malformed)?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or(XlsPreflightFailure::Malformed)?;
        if end > part.len() {
            return Err(XlsPreflightFailure::Malformed);
        }
        let bytes = &part[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn skip_plain(&mut self, mut len: usize) -> Result<(), XlsPreflightFailure> {
        while len > 0 {
            self.advance_empty_parts();
            let part = self.current().ok_or(XlsPreflightFailure::Malformed)?;
            let available = part.len().saturating_sub(self.offset);
            let take = available.min(len);
            self.offset += take;
            len -= take;
        }
        Ok(())
    }

    fn skip_characters(
        &mut self,
        mut chars: usize,
        mut high_byte: bool,
    ) -> Result<(), XlsPreflightFailure> {
        while chars > 0 {
            self.advance_empty_parts();
            let part = self.current().ok_or(XlsPreflightFailure::Malformed)?;
            let width = if high_byte { 2 } else { 1 };
            let available = part.len().saturating_sub(self.offset);
            let available_chars = available / width;
            if available_chars > 0 {
                let take = available_chars.min(chars);
                self.offset += take * width;
                chars -= take;
            }
            if chars == 0 {
                break;
            }
            if self.offset != part.len() {
                return Err(XlsPreflightFailure::Malformed);
            }
            self.part += 1;
            self.offset = 0;
            let flag = self.local_bytes(1)?[0];
            high_byte = flag & 0x01 != 0;
        }
        Ok(())
    }
}

fn preflight_sst(
    parts: &[&[u8]],
    budget: &mut XlsPreflightBudget,
    started: Instant,
) -> Result<Vec<usize>, XlsPreflightFailure> {
    let first = parts.first().ok_or(XlsPreflightFailure::Malformed)?;
    if first.len() < 8 {
        return Err(XlsPreflightFailure::Malformed);
    }
    let total = u32::from_le_bytes(
        first[..4]
            .try_into()
            .map_err(|_| XlsPreflightFailure::Malformed)?,
    ) as usize;
    let unique = u32::from_le_bytes(
        first[4..8]
            .try_into()
            .map_err(|_| XlsPreflightFailure::Malformed)?,
    ) as usize;
    if unique > total {
        return Err(XlsPreflightFailure::Malformed);
    }
    if total > XLS_MAX_CELLS || unique > XLS_MAX_SHARED_STRINGS {
        return Err(XlsPreflightFailure::ResourceLimit);
    }

    let mut cursor = SstCursor::new(parts);
    cursor.local_bytes(8)?;
    let mut lengths = Vec::with_capacity(unique.min(XLS_MAX_SHARED_STRINGS));
    let mut total_chars = 0usize;
    while !cursor.is_done() {
        if lengths.len().is_multiple_of(8192) && timed_out(started) {
            return Err(XlsPreflightFailure::Timeout);
        }
        if lengths.len() == XLS_MAX_SHARED_STRINGS {
            return Err(XlsPreflightFailure::ResourceLimit);
        }
        let header = cursor.local_bytes(3)?;
        let chars = u16::from_le_bytes([header[0], header[1]]) as usize;
        let flags = header[2];
        let rich_runs = if flags & 0x08 != 0 {
            let bytes = cursor.local_bytes(2)?;
            u16::from_le_bytes([bytes[0], bytes[1]]) as usize
        } else {
            0
        };
        let extension_bytes = if flags & 0x04 != 0 {
            let bytes = cursor.local_bytes(4)?;
            let signed = i32::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| XlsPreflightFailure::Malformed)?,
            );
            usize::try_from(signed).map_err(|_| XlsPreflightFailure::Malformed)?
        } else {
            0
        };
        total_chars = total_chars.saturating_add(chars);
        if total_chars > XLS_MAX_SHARED_STRING_CHARS {
            return Err(XlsPreflightFailure::ResourceLimit);
        }
        cursor.skip_characters(chars, flags & 0x01 != 0)?;
        cursor.skip_plain(
            rich_runs
                .checked_mul(4)
                .ok_or(XlsPreflightFailure::Malformed)?,
        )?;
        cursor.skip_plain(extension_bytes)?;
        lengths.push(chars);
    }
    if lengths.len() != unique {
        return Err(XlsPreflightFailure::Malformed);
    }
    budget.memory(
        total_chars
            .saturating_mul(4)
            .saturating_add(unique.saturating_mul(std::mem::size_of::<String>())),
    )?;
    Ok(lengths)
}

fn preflight_biff_sheet(
    stream: &[u8],
    start: usize,
    shared_string_chars: &[usize],
    budget: &mut XlsPreflightBudget,
    started: Instant,
) -> Result<(), XlsPreflightFailure> {
    if start >= stream.len() {
        return Err(XlsPreflightFailure::Malformed);
    }
    let mut offset = start;
    let mut found_eof = false;
    let mut row_start = u32::MAX;
    let mut row_end = 0u32;
    let mut column_start = u32::MAX;
    let mut column_end = 0u32;
    let mut formula_row_start = u32::MAX;
    let mut formula_row_end = 0u32;
    let mut formula_column_start = u32::MAX;
    let mut formula_column_end = 0u32;
    let mut has_cells = false;
    let mut has_formulas = false;
    let mut cell_entries = 0usize;
    let mut formula_entries = 0usize;

    while let Some(record) = biff_record_at(stream, offset)? {
        budget.record()?;
        if budget.records.is_multiple_of(8192) && timed_out(started) {
            return Err(XlsPreflightFailure::Timeout);
        }
        offset = record.next_offset;
        let payload = record.payload;
        match record.typ {
            0x0200 => {
                let cells = dimensions_cell_count_checked(payload)?;
                if cells > XLS_MAX_CELLS {
                    return Err(XlsPreflightFailure::ResourceLimit);
                }
                budget.memory(cells.saturating_mul(std::mem::size_of::<calamine::Cell<Data>>()))?;
            }
            0x0203 | 0x0204 | 0x00D6 | 0x0205 | 0x027E => {
                let (row, column) = biff_cell_position(payload)?;
                update_biff_cell_span(
                    &mut row_start,
                    &mut row_end,
                    &mut column_start,
                    &mut column_end,
                    row,
                    column,
                );
                has_cells = true;
                cell_entries = cell_entries.saturating_add(1);
                if matches!(record.typ, 0x0204 | 0x00D6) {
                    budget.memory(payload.len().saturating_mul(4))?;
                }
            }
            0x0207 => {
                // A String record belongs to the preceding Formula and has no
                // row/column header of its own. Calamine reuses `fmla_pos`.
                budget.memory(payload.len().saturating_mul(4))?;
            }
            0x00FD => {
                if payload.len() < 10 {
                    return Err(XlsPreflightFailure::Malformed);
                }
                let (row, column) = biff_cell_position(payload)?;
                let index = u32::from_le_bytes(
                    payload[6..10]
                        .try_into()
                        .map_err(|_| XlsPreflightFailure::Malformed)?,
                ) as usize;
                let chars = *shared_string_chars
                    .get(index)
                    .ok_or(XlsPreflightFailure::Malformed)?;
                budget.expanded_string(chars)?;
                budget.memory(
                    chars
                        .saturating_mul(4)
                        .saturating_add(std::mem::size_of::<String>()),
                )?;
                update_biff_cell_span(
                    &mut row_start,
                    &mut row_end,
                    &mut column_start,
                    &mut column_end,
                    row,
                    column,
                );
                has_cells = true;
                cell_entries = cell_entries.saturating_add(1);
            }
            0x00BD => {
                if payload.len() < 6 {
                    return Err(XlsPreflightFailure::Malformed);
                }
                let row = u16::from_le_bytes([payload[0], payload[1]]) as u32;
                let first = u16::from_le_bytes([payload[2], payload[3]]) as u32;
                let last =
                    u16::from_le_bytes([payload[payload.len() - 2], payload[payload.len() - 1]])
                        as u32;
                if last < first {
                    return Err(XlsPreflightFailure::Malformed);
                }
                let count = last.saturating_sub(first) as usize + 1;
                if payload.len() != 6usize.saturating_add(count.saturating_mul(6)) {
                    return Err(XlsPreflightFailure::Malformed);
                }
                update_biff_cell_span(
                    &mut row_start,
                    &mut row_end,
                    &mut column_start,
                    &mut column_end,
                    row,
                    first,
                );
                update_biff_cell_span(
                    &mut row_start,
                    &mut row_end,
                    &mut column_start,
                    &mut column_end,
                    row,
                    last,
                );
                has_cells = true;
                cell_entries = cell_entries.saturating_add(count);
            }
            0x0006 => {
                if payload.len() < 20 {
                    return Err(XlsPreflightFailure::Malformed);
                }
                let (row, column) = biff_cell_position(payload)?;
                update_biff_cell_span(
                    &mut row_start,
                    &mut row_end,
                    &mut column_start,
                    &mut column_end,
                    row,
                    column,
                );
                update_biff_cell_span(
                    &mut formula_row_start,
                    &mut formula_row_end,
                    &mut formula_column_start,
                    &mut formula_column_end,
                    row,
                    column,
                );
                has_cells = true;
                has_formulas = true;
                cell_entries = cell_entries.saturating_add(1);
                formula_entries = formula_entries.saturating_add(1);
                budget.formulas(1)?;
                budget.memory(payload.len().saturating_mul(4))?;
            }
            0x00E5 => {
                if payload.len() < 2 {
                    return Err(XlsPreflightFailure::Malformed);
                }
                let count = u16::from_le_bytes([payload[0], payload[1]]) as usize;
                let expected = 2usize
                    .checked_add(count.checked_mul(8).ok_or(XlsPreflightFailure::Malformed)?)
                    .ok_or(XlsPreflightFailure::Malformed)?;
                if expected > payload.len() {
                    return Err(XlsPreflightFailure::Malformed);
                }
                budget.metadata(count)?;
            }
            0x000A => {
                found_eof = true;
                break;
            }
            _ => {}
        }
    }
    if !found_eof {
        return Err(XlsPreflightFailure::Malformed);
    }

    let range_cells = if has_cells {
        biff_span_cells(row_start, row_end, column_start, column_end)
    } else {
        0
    };
    budget.logical_cells(range_cells)?;
    budget.memory(range_cells.saturating_mul(std::mem::size_of::<Data>()))?;
    budget.memory(cell_entries.saturating_mul(std::mem::size_of::<calamine::Cell<Data>>()))?;
    if has_formulas {
        let formula_cells = biff_span_cells(
            formula_row_start,
            formula_row_end,
            formula_column_start,
            formula_column_end,
        );
        budget.memory(formula_cells.saturating_mul(std::mem::size_of::<String>()))?;
        budget.memory(
            formula_entries.saturating_mul(std::mem::size_of::<calamine::Cell<String>>()),
        )?;
    }
    Ok(())
}

fn biff_cell_position(payload: &[u8]) -> Result<(u32, u32), XlsPreflightFailure> {
    if payload.len() < 4 {
        return Err(XlsPreflightFailure::Malformed);
    }
    Ok((
        u16::from_le_bytes([payload[0], payload[1]]) as u32,
        u16::from_le_bytes([payload[2], payload[3]]) as u32,
    ))
}

fn update_biff_cell_span(
    row_start: &mut u32,
    row_end: &mut u32,
    column_start: &mut u32,
    column_end: &mut u32,
    row: u32,
    column: u32,
) {
    *row_start = (*row_start).min(row);
    *row_end = (*row_end).max(row);
    *column_start = (*column_start).min(column);
    *column_end = (*column_end).max(column);
}

fn biff_span_cells(row_start: u32, row_end: u32, col_start: u32, col_end: u32) -> usize {
    (row_end.saturating_sub(row_start) as usize + 1)
        .saturating_mul(col_end.saturating_sub(col_start) as usize + 1)
}

fn dimensions_cell_count_checked(payload: &[u8]) -> Result<usize, XlsPreflightFailure> {
    let (row_start, row_end, column_start, column_end) = match payload.len() {
        10 => (
            u16::from_le_bytes([payload[0], payload[1]]) as usize,
            u16::from_le_bytes([payload[2], payload[3]]) as usize,
            u16::from_le_bytes([payload[4], payload[5]]) as usize,
            u16::from_le_bytes([payload[6], payload[7]]) as usize,
        ),
        14 => (
            u32::from_le_bytes(
                payload[0..4]
                    .try_into()
                    .map_err(|_| XlsPreflightFailure::Malformed)?,
            ) as usize,
            u32::from_le_bytes(
                payload[4..8]
                    .try_into()
                    .map_err(|_| XlsPreflightFailure::Malformed)?,
            ) as usize,
            u16::from_le_bytes([payload[8], payload[9]]) as usize,
            u16::from_le_bytes([payload[10], payload[11]]) as usize,
        ),
        _ => return Err(XlsPreflightFailure::Malformed),
    };
    if row_end == 0 || column_end == 0 {
        return Ok(1);
    }
    if row_end <= row_start || column_end <= column_start || column_start > 0xFF {
        return Err(XlsPreflightFailure::Malformed);
    }
    Ok(row_end
        .saturating_sub(row_start)
        .saturating_mul(column_end.saturating_sub(column_start)))
}

#[derive(Debug, Default)]
struct XlsTextAccumulator {
    text: String,
    text_chars: usize,
    has_value: bool,
}

enum XlsAppendResult {
    Complete,
    Truncated,
    Timeout,
}

impl XlsTextAccumulator {
    fn push_separator(&mut self, separator: char) {
        if self.text_chars < MAX_TEXT_CHARS {
            self.text.push(separator);
            self.text_chars += 1;
        }
    }

    fn push_value(&mut self, value: &str, started: Instant) -> XlsAppendResult {
        for (index, character) in value.chars().enumerate() {
            if index % 8192 == 0 && timed_out(started) {
                return XlsAppendResult::Timeout;
            }
            if self.text_chars == MAX_TEXT_CHARS {
                return XlsAppendResult::Truncated;
            }
            self.text.push(character);
            self.text_chars += 1;
        }
        self.has_value = true;
        XlsAppendResult::Complete
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecodeFailure {
    Unsupported,
    Timeout,
}

fn decode(bytes: &[u8], started: Instant) -> Result<(String, &'static str), DecodeFailure> {
    if bytes.starts_with(b"\xEF\xBB\xBF") {
        let text = std::str::from_utf8(&bytes[3..])
            .map_err(|_| DecodeFailure::Unsupported)?
            .to_owned();
        return Ok((text, "utf8-bom"));
    }
    if bytes.starts_with(b"\xFF\xFE") {
        return decode_utf16(&bytes[2..], true, started).map(|text| (text, "utf16-le"));
    }
    if bytes.starts_with(b"\xFE\xFF") {
        return decode_utf16(&bytes[2..], false, started).map(|text| (text, "utf16-be"));
    }
    if let Some(little_endian) = detect_utf16_without_bom(bytes) {
        return decode_utf16(bytes, little_endian, started).map(|text| {
            (
                text,
                if little_endian {
                    "utf16-le"
                } else {
                    "utf16-be"
                },
            )
        });
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DecodeFailure::Unsupported)?
        .to_owned();
    Ok((text, "utf8"))
}

fn decode_utf16(
    bytes: &[u8],
    little_endian: bool,
    started: Instant,
) -> Result<String, DecodeFailure> {
    if !bytes.len().is_multiple_of(2) {
        return Err(DecodeFailure::Unsupported);
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        if index % 8192 == 0 && timed_out(started) {
            return Err(DecodeFailure::Timeout);
        }
        let unit = if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        };
        units.push(unit);
    }
    String::from_utf16(&units).map_err(|_| DecodeFailure::Unsupported)
}

fn detect_utf16_without_bom(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let sample = &bytes[..bytes.len().min(UTF16_SAMPLE_BYTES)];
    let odd_zero = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    let even_zero = sample.iter().step_by(2).filter(|byte| **byte == 0).count();
    let pairs = sample.len() / 2;
    if odd_zero * 4 >= pairs * 3 {
        Some(true)
    } else if even_zero * 4 >= pairs * 3 {
        Some(false)
    } else {
        None
    }
}

fn truncate_text(text: String, started: Instant) -> (String, bool, usize) {
    let mut output = String::with_capacity(text.len().min(MAX_TEXT_CHARS * 4));
    let mut chars = 0;
    let mut truncated = false;
    for (index, character) in text.chars().enumerate() {
        if index == MAX_TEXT_CHARS {
            truncated = true;
            break;
        }
        if index % 8192 == 0 && timed_out(started) {
            // The caller performs the final timeout check.  Returning the
            // bounded prefix keeps this helper allocation-safe; the record is
            // discarded as timeout by `extract_bytes` immediately afterwards.
            break;
        }
        output.push(character);
        chars += 1;
    }
    if text.chars().nth(MAX_TEXT_CHARS).is_some() {
        truncated = true;
    }
    (output, truncated, chars)
}

fn timed_out(started: Instant) -> bool {
    started.elapsed() >= PROCESSING_LIMIT
}

/// Redacts common credential values from snippets before they cross the UI
/// boundary.  The full content remains local to the app-owned index; this is
/// intentionally conservative and deterministic rather than a claim of secret
/// detection completeness.
pub fn redact_snippet(input: &str) -> String {
    let lower_input = input.to_ascii_lowercase();
    let redacted = if lower_input.contains("-----begin") {
        "[REDACTED PRIVATE KEY]".to_string()
    } else {
        let lower = lower_input;
        let keys = [
            "authorization:",
            "\"authorization\":",
            "authorization=",
            "bearer ",
            "cookie:",
            "\"cookie\":",
            "set-cookie:",
            "\"set-cookie\":",
            "password=",
            "password:",
            "\"password\":",
            "token=",
            "token:",
            "\"token\":",
            "secret=",
            "secret:",
            "\"secret\":",
            "api_key=",
            "api_key:",
            "\"api_key\":",
            "api-key=",
            "api-key:",
            "\"api-key\":",
            "apikey=",
            "apikey:",
            "\"apikey\":",
            "access_token=",
            "access_token:",
            "\"access_token\":",
            "refresh_token=",
            "refresh_token:",
            "\"refresh_token\":",
            "client_secret=",
            "client_secret:",
            "\"client_secret\":",
            "private_key:",
            "\"private_key\":",
        ];
        let mut spans = Vec::new();
        for key in keys {
            let mut offset = 0;
            while let Some(relative) = lower[offset..].find(key) {
                let start = offset + relative;
                let mut value_start = start + key.len();
                while let Some(character) = input[value_start..].chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    value_start += character.len_utf8();
                }
                let (value_start, value_end) = match input[value_start..].chars().next() {
                    Some(quote @ ('"' | '\'')) => {
                        let content_start = value_start + quote.len_utf8();
                        let mut escaped = false;
                        let mut end = input.len();
                        for (index, character) in input[content_start..].char_indices() {
                            if escaped {
                                escaped = false;
                            } else if character == '\\' {
                                escaped = true;
                            } else if character == quote {
                                end = content_start + index;
                                break;
                            }
                        }
                        (content_start, end)
                    }
                    Some(_) => {
                        let end = input[value_start..]
                            .char_indices()
                            .find(|(_, character)| {
                                character.is_whitespace()
                                    || matches!(*character, ',' | ';' | ']' | '}' | ')')
                            })
                            .map(|(index, _)| value_start + index)
                            .unwrap_or(input.len());
                        (value_start, end)
                    }
                    None => (value_start, value_start),
                };
                if value_start < value_end {
                    spans.push((value_start, value_end));
                }
                offset = value_end.max(start + key.len());
                if offset >= lower.len() {
                    break;
                }
            }
        }
        if spans.is_empty() {
            input.to_string()
        } else {
            spans.sort_unstable();
            let mut output = String::with_capacity(input.len());
            let mut cursor = 0;
            for (start, end) in spans {
                if start < cursor {
                    continue;
                }
                output.push_str(&input[cursor..start]);
                output.push_str("[REDACTED]");
                cursor = end;
            }
            output.push_str(&input[cursor..]);
            output
        }
    };
    let redacted = redact_known_token_patterns(&redacted);
    if redacted.chars().count() <= MAX_SNIPPET_CHARS {
        return redacted;
    }
    let mut bounded: String = redacted
        .chars()
        .take(MAX_SNIPPET_CHARS.saturating_sub(1))
        .collect();
    bounded.push('…');
    bounded
}

fn redact_known_token_patterns(input: &str) -> String {
    let mut spans = Vec::new();
    let mut candidate_start = None;
    for (index, character) in input
        .char_indices()
        .chain(std::iter::once((input.len(), ' ')))
    {
        let is_candidate =
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.');
        match (candidate_start, is_candidate) {
            (None, true) => candidate_start = Some(index),
            (Some(start), false) => {
                let candidate = &input[start..index];
                if looks_like_known_token(candidate) {
                    spans.push((start, index));
                }
                candidate_start = None;
            }
            _ => {}
        }
    }
    if spans.is_empty() {
        return input.to_string();
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for (start, end) in spans {
        output.push_str(&input[cursor..start]);
        output.push_str("[REDACTED TOKEN]");
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn looks_like_known_token(candidate: &str) -> bool {
    let prefixed = [
        "sk-",
        "ghp_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
    ];
    if prefixed
        .iter()
        .any(|prefix| candidate.starts_with(prefix) && candidate.len() >= prefix.len() + 12)
    {
        return true;
    }
    if candidate.len() == 20
        && (candidate.starts_with("AKIA") || candidate.starts_with("ASIA"))
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return true;
    }
    let mut jwt_parts = candidate.split('.');
    let parts = (
        jwt_parts.next(),
        jwt_parts.next(),
        jwt_parts.next(),
        jwt_parts.next(),
    );
    matches!(parts, (Some(a), Some(b), Some(c), None)
    if [a, b, c].iter().all(|part| {
        part.len() >= 10
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{
        dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream,
    };

    fn now() -> Instant {
        Instant::now()
    }

    fn utf16(value: &str, little_endian: bool) -> Vec<u8> {
        let mut bytes = if little_endian {
            vec![0xFF, 0xFE]
        } else {
            vec![0xFE, 0xFF]
        };
        for unit in value.encode_utf16() {
            bytes.extend(if little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        bytes
    }

    fn pdf_bytes(text: Option<&str>) -> Vec<u8> {
        pdf_bytes_with_compression(text, false)
    }

    fn pdf_bytes_with_compression(text: Option<&str>, compress: bool) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let page_id = if let Some(text) = text {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                    Operation::new("Td", vec![100.into(), 600.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                lopdf::Dictionary::new(),
                content.encode().unwrap(),
            ));
            document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Resources" => resources_id,
                "Contents" => content_id,
            })
        } else {
            document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Resources" => resources_id,
            })
        };
        document.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal(b"fixture-id-a"),
                Object::string_literal(b"fixture-id-b"),
            ]),
        );
        if compress {
            document.compress();
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    fn encrypted_pdf_bytes(user_password: &str) -> Vec<u8> {
        let mut document =
            lopdf::Document::load_mem(&pdf_bytes(Some("protected fixture"))).unwrap();
        let version = EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner",
            user_password,
            key_length: 40,
            permissions: Permissions::all(),
        };
        let state = EncryptionState::try_from(version).unwrap();
        document.encrypt(&state).unwrap();
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn recognizes_developer_text_extensions_case_insensitively() {
        assert!(is_text_ext("RS"));
        assert!(is_text_ext("md"));
        assert!(is_text_ext("JSON"));
        assert!(is_text_ext("tsx"));
        assert!(!is_text_ext("png"));
        assert!(!is_text_ext("exe"));
    }

    #[test]
    fn recognizes_pdf_candidates_without_treating_office_formats_as_pdf() {
        assert!(is_pdf_ext("pdf"));
        assert!(is_pdf_ext(".PDF"));
        assert!(is_pdf_path(Path::new("report.PdF")));
        assert!(is_content_candidate(Path::new("report.pdf")));
        assert!(!is_pdf_path(Path::new("report.docx")));
        assert!(!is_content_candidate(Path::new("report.docx")));
    }

    #[test]
    fn recognizes_legacy_xls_candidates_without_treating_modern_formats_as_xls() {
        assert!(is_xls_ext("xls"));
        assert!(is_xls_ext(".XLS"));
        assert!(is_xls_path(Path::new("report.XlS")));
        assert!(is_content_candidate(Path::new("report.xls")));
        assert!(!is_xls_path(Path::new("report.xlsx")));
        assert!(!is_content_candidate(Path::new("report.xlsx")));
        assert!(!is_content_candidate(Path::new("report.ods")));
    }

    #[test]
    fn extracts_xls_cell_values_with_a_separate_extractor_version() {
        let record = extract_xls_bytes(&xls_fixture(), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.extractor_version, XLS_EXTRACTOR_VERSION);
        assert_eq!(record.encoding, Some("xls"));
        assert!(record.text.contains("sheetjs"));
        assert!(!record.text.contains("ThisWorkbook"));
    }

    #[test]
    fn isolates_corrupt_oversized_and_timed_out_xls_inputs() {
        let corrupt = extract_xls_bytes(b"not an XLS workbook", now());
        assert_eq!(corrupt.status, ContentStatus::ExtractError);
        assert_eq!(corrupt.error_code, Some("extract_error"));
        assert!(corrupt.text.is_empty());

        let oversized = extract_xls_bytes(&vec![b'x'; MAX_FILE_BYTES as usize + 1], now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.error_code, Some("file_too_large"));

        let timeout = extract_xls_bytes(&xls_fixture(), Instant::now() - PROCESSING_LIMIT);
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));
    }

    #[test]
    fn rejects_sparse_xls_dimensions_before_calamine_allocation() {
        let mut sparse = xls_fixture();
        let record_start = sparse
            .windows(4)
            .position(|window| window == [0x00, 0x02, 0x0A, 0x00])
            .expect("fixture dimensions record");
        // BIFF5 Dimensions stores exclusive row/column ends as u16 values.
        sparse[record_start + 6..record_start + 8].copy_from_slice(&u16::MAX.to_le_bytes());
        sparse[record_start + 10..record_start + 12].copy_from_slice(&256_u16.to_le_bytes());
        let record = extract_xls_bytes(&sparse, now());
        assert_eq!(record.status, ContentStatus::ExtractError);
        assert_eq!(record.error_code, Some("resource_limit"));
    }

    #[test]
    fn rejects_malformed_and_reversed_xls_structures_fail_closed() {
        let mut malformed_record = xls_fixture();
        malformed_record[0x602..0x604].copy_from_slice(&u16::MAX.to_le_bytes());
        let malformed = extract_xls_bytes(&malformed_record, now());
        assert_eq!(malformed.status, ContentStatus::ExtractError);
        assert_eq!(malformed.error_code, Some("extract_error"));

        let mut reversed = xls_fixture();
        let dimensions = reversed
            .windows(4)
            .position(|window| window == [0x00, 0x02, 0x0A, 0x00])
            .expect("fixture dimensions record");
        reversed[dimensions + 4..dimensions + 6].copy_from_slice(&10_u16.to_le_bytes());
        reversed[dimensions + 6..dimensions + 8].copy_from_slice(&2_u16.to_le_bytes());
        let record = extract_xls_bytes(&reversed, now());
        assert_eq!(record.status, ContentStatus::ExtractError);
        assert_eq!(record.error_code, Some("extract_error"));
    }

    #[test]
    fn bounds_shared_string_clone_amplification_before_parser_entry() {
        let mut stream = Vec::new();
        let bound_sheet = push_biff_record(&mut stream, 0x0085, &[0, 0, 0, 0]);
        let string_chars = u16::MAX as usize - 11;
        let mut sst = Vec::with_capacity(u16::MAX as usize);
        sst.extend(245_u32.to_le_bytes());
        sst.extend(1_u32.to_le_bytes());
        sst.extend((string_chars as u16).to_le_bytes());
        sst.push(0);
        sst.resize(u16::MAX as usize, b'x');
        push_biff_record(&mut stream, 0x00FC, &sst);
        push_biff_record(&mut stream, 0x000A, &[]);
        let sheet_start = stream.len() as u32;
        stream[bound_sheet + 4..bound_sheet + 8].copy_from_slice(&sheet_start.to_le_bytes());
        for row in 0..245_u16 {
            let mut label = Vec::with_capacity(10);
            label.extend(row.to_le_bytes());
            label.extend(0_u16.to_le_bytes());
            label.extend(0_u16.to_le_bytes());
            label.extend(0_u32.to_le_bytes());
            push_biff_record(&mut stream, 0x00FD, &label);
        }
        push_biff_record(&mut stream, 0x000A, &[]);

        assert_eq!(
            preflight_biff(&stream, now()),
            Err(XlsPreflightFailure::ResourceLimit)
        );
    }

    #[test]
    fn isolates_password_protected_xls_without_exposing_parser_details() {
        let mut encrypted = xls_fixture();
        // The fixture's workbook stream starts at 0x600. Replace a harmless
        // two-byte BIFF record with FilePass and a non-zero encryption flag.
        encrypted[0x610..0x614].copy_from_slice(&[0x2f, 0x00, 0x02, 0x00]);
        encrypted[0x614..0x616].copy_from_slice(&[0x01, 0x00]);
        let record = extract_xls_bytes(&encrypted, now());
        assert_eq!(record.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(record.error_code, Some("unsupported_encrypted"));
        assert!(record.text.is_empty());
    }

    #[test]
    fn xls_file_boundary_failures_keep_the_xls_extractor_version() {
        let oversized = extract_file(Path::new("report.xls"), MAX_FILE_BYTES + 1, now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.extractor_version, XLS_EXTRACTOR_VERSION);

        let timeout = extract_file(
            Path::new("report.xls"),
            0,
            Instant::now() - PROCESSING_LIMIT,
        );
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.extractor_version, XLS_EXTRACTOR_VERSION);
    }

    #[test]
    fn bounds_xls_text_and_keeps_output_on_a_unicode_boundary() {
        let mut accumulator = XlsTextAccumulator::default();
        let result = accumulator.push_value(&"가😊".repeat(MAX_TEXT_CHARS), now());
        assert!(matches!(result, XlsAppendResult::Truncated));
        assert_eq!(accumulator.text_chars, MAX_TEXT_CHARS);
        assert!(accumulator.text.is_char_boundary(accumulator.text.len()));
    }

    #[test]
    fn extracts_pdf_text_and_keeps_pdf_extractor_version_separate() {
        let record = extract_pdf_bytes(&pdf_bytes(Some("offline PDF fixture")), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.extractor_version, PDF_EXTRACTOR_VERSION);
        assert_eq!(record.encoding, Some("pdf"));
        assert!(record.text.contains("offline PDF fixture"));
        assert!(!record.text.contains("/Type"));
    }

    #[test]
    fn bounds_pdf_text_and_decompressed_page_content() {
        let long_text = "x".repeat(MAX_TEXT_CHARS + 1);
        let bounded = extract_pdf_bytes(&pdf_bytes(Some(&long_text)), now());
        assert_eq!(bounded.status, ContentStatus::Indexed);
        assert!(bounded.truncated);
        assert_eq!(bounded.text_chars, MAX_TEXT_CHARS);
        assert_eq!(bounded.error_code, Some("text_limit"));

        let decompressed_bomb = "x".repeat(PDF_MAX_DECOMPRESSED_BYTES + 1);
        let bomb_pdf = pdf_bytes_with_compression(Some(&decompressed_bomb), true);
        assert!(bomb_pdf.len() < MAX_FILE_BYTES as usize);
        let bomb = extract_pdf_bytes(&bomb_pdf, now());
        assert_eq!(bomb.status, ContentStatus::ExtractError);
        assert_eq!(bomb.error_code, Some("extract_error"));
        assert!(bomb.text.is_empty());
    }

    #[test]
    fn bounds_pdf_object_and_page_structure_at_the_exact_limits() {
        assert!(!pdf_object_limit_exceeded(PDF_MAX_OBJECTS));
        assert!(pdf_object_limit_exceeded(PDF_MAX_OBJECTS + 1));
        assert!(!pdf_page_limit_exceeded(PDF_MAX_PAGES));
        assert!(pdf_page_limit_exceeded(PDF_MAX_PAGES + 1));
    }

    #[test]
    fn isolates_scanned_encrypted_corrupt_oversized_and_timed_out_pdfs() {
        let scanned = extract_pdf_bytes(&pdf_bytes(None), now());
        assert_eq!(scanned.status, ContentStatus::NoText);
        assert_eq!(scanned.error_code, Some("no_text"));

        let encrypted = extract_pdf_bytes(&encrypted_pdf_bytes("user"), now());
        assert_eq!(encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(encrypted.error_code, Some("unsupported_encrypted"));
        assert!(encrypted.text.is_empty());

        let empty_password = extract_pdf_bytes(&encrypted_pdf_bytes(""), now());
        assert_eq!(empty_password.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(empty_password.error_code, Some("unsupported_encrypted"));
        assert!(empty_password.text.is_empty());

        let corrupt = extract_pdf_bytes(b"%PDF-1.7\nnot a valid PDF", now());
        assert_eq!(corrupt.status, ContentStatus::ExtractError);
        assert_eq!(corrupt.error_code, Some("extract_error"));

        // A raw marker in malformed bytes is not proof of a valid encryption
        // dictionary and must not hide the corrupt-input classification.
        let corrupt_with_marker = extract_pdf_bytes(b"%PDF-1.7\n/Encrypt\nnot a valid PDF", now());
        assert_eq!(corrupt_with_marker.status, ContentStatus::ExtractError);
        assert_eq!(corrupt_with_marker.error_code, Some("extract_error"));

        let oversized = extract_pdf_bytes(&vec![b'x'; MAX_FILE_BYTES as usize + 1], now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.error_code, Some("file_too_large"));

        let timeout = extract_pdf_bytes(b"%PDF-1.7", Instant::now() - PROCESSING_LIMIT);
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));
    }

    #[test]
    fn pdf_file_boundary_failures_keep_the_pdf_extractor_version() {
        let oversized = extract_file(Path::new("report.pdf"), MAX_FILE_BYTES + 1, now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.extractor_version, PDF_EXTRACTOR_VERSION);

        let timeout = extract_file(
            Path::new("report.pdf"),
            0,
            Instant::now() - PROCESSING_LIMIT,
        );
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.extractor_version, PDF_EXTRACTOR_VERSION);
    }

    #[test]
    fn decodes_utf8_utf16_and_empty_text() {
        let english = extract_bytes("hello world".as_bytes(), now());
        assert_eq!(english.text, "hello world");
        assert_eq!(english.encoding, Some("utf8"));

        let korean = extract_bytes("안녕하세요 devbox".as_bytes(), now());
        assert_eq!(korean.status, ContentStatus::Indexed);
        assert!(korean.text.contains("안녕하세요"));

        let little = extract_bytes(&utf16("한글 UTF-16", true), now());
        assert_eq!(little.status, ContentStatus::Indexed);
        assert_eq!(little.encoding, Some("utf16-le"));
        assert_eq!(little.text, "한글 UTF-16");

        let big = extract_bytes(&utf16("UTF-16 BE", false), now());
        assert_eq!(big.status, ContentStatus::Indexed);
        assert_eq!(big.encoding, Some("utf16-be"));
        assert_eq!(big.text, "UTF-16 BE");

        let mut no_bom = utf16("UTF-16 without BOM", true);
        no_bom.drain(..2);
        let no_bom_record = extract_bytes(&no_bom, now());
        assert_eq!(no_bom_record.status, ContentStatus::Indexed);
        assert_eq!(no_bom_record.encoding, Some("utf16-le"));
        assert_eq!(no_bom_record.text, "UTF-16 without BOM");

        let empty = extract_bytes(b"", now());
        assert_eq!(empty.status, ContentStatus::Indexed);
        assert_eq!(empty.text_chars, 0);
    }

    #[test]
    fn rejects_binary_and_invalid_utf16() {
        let binary = extract_bytes(&[0, 1, 2, 3, 4], now());
        assert_eq!(binary.status, ContentStatus::UnsupportedEncoding);
        assert_eq!(binary.error_code, Some("unsupported_encoding"));

        let invalid = extract_bytes(&[0xFF, 0xFE, 0x00], now());
        assert_eq!(invalid.status, ContentStatus::UnsupportedEncoding);
    }

    #[test]
    fn truncates_by_unicode_scalar_count_not_bytes() {
        let input = "가😊".repeat(MAX_TEXT_CHARS / 2 + 1);
        let record = extract_bytes(input.as_bytes(), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert!(record.truncated);
        assert_eq!(record.text_chars, MAX_TEXT_CHARS);
        assert_eq!(record.error_code, Some("text_limit"));
        assert!(record.text.is_char_boundary(record.text.len()));
    }

    #[test]
    fn returns_fixed_large_file_and_timeout_statuses() {
        let large = extract_bytes(&vec![b'x'; (MAX_FILE_BYTES + 1) as usize], now());
        assert_eq!(large.status, ContentStatus::TooLarge);
        assert_eq!(large.text, "");
        assert_eq!(large.error_code, Some("file_too_large"));
        let timeout = extract_bytes(b"small", Instant::now() - PROCESSING_LIMIT);
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));
    }

    #[test]
    fn detects_same_size_metadata_change_with_fingerprints() {
        let initial = FileFingerprint {
            len: 12,
            modified: Some(SystemTime::UNIX_EPOCH),
        };
        let rewritten = FileFingerprint {
            len: 12,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
        };
        assert!(rewritten.changed_from(initial, 12));
        assert!(!initial.changed_from(initial, 12));
        assert!(initial.changed_from(initial, 11));
    }

    #[test]
    fn skips_credential_named_files_without_reading() {
        assert!(is_sensitive_filename(Path::new(".env")));
        assert!(is_sensitive_filename(Path::new("credentials.json")));
        assert!(is_sensitive_filename(Path::new("server.key")));
        assert!(!is_sensitive_filename(Path::new("src/secret_manager.rs")));
    }

    #[test]
    fn redacts_common_credential_values_but_preserves_normal_text() {
        let snippet = "Authorization: Bearer abc123 [review] password=letmein token: yaml-secret";
        let redacted = redact_snippet(snippet);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("letmein"));
        assert!(!redacted.contains("yaml-secret"));
        assert!(redacted.contains("[review]"));
        let json = redact_snippet(r#"{"token":"json-secret","note":"keep"}"#);
        assert!(!json.contains("json-secret"));
        assert!(json.contains("keep"));
        assert_eq!(redact_snippet("quarterly review"), "quarterly review");
        assert_eq!(
            redact_snippet("-----BEGIN PRIVATE KEY-----"),
            "[REDACTED PRIVATE KEY]"
        );
    }

    #[test]
    fn redacts_known_tokens_without_assignment_keys() {
        // Assemble fixtures at runtime so repository secret scanners do not
        // mistake deliberately fake credentials for committed live values.
        let github = ["ghp", "_1234567890abcdefghijkl"].concat();
        let aws = ["AK", "IA1234567890ABCDEF"].concat();
        let jwt = [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "abcdefghijklmnop",
        ]
        .join(".");
        let snippet = format!("use {github} and {aws} then {jwt}");
        let redacted = redact_snippet(&snippet);
        assert!(!redacted.contains(&github));
        assert!(!redacted.contains(&aws));
        assert!(!redacted.contains(&jwt));
        assert_eq!(redacted.matches("[REDACTED TOKEN]").count(), 3);
    }

    #[test]
    fn bounds_unusually_long_snippets_on_character_boundary() {
        let snippet = "가".repeat(MAX_SNIPPET_CHARS + 100);
        let redacted = redact_snippet(&snippet);
        assert_eq!(redacted.chars().count(), MAX_SNIPPET_CHARS);
        assert!(redacted.ends_with('…'));
    }

    fn xls_fixture() -> Vec<u8> {
        let encoded = include_str!("../../fixtures/biff5_write.xls.b64");
        let mut output = Vec::new();
        let mut buffer = 0_u32;
        let mut bits = 0_u8;
        for byte in encoded.bytes() {
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                b'\r' | b'\n' | b' ' | b'\t' => continue,
                _ => panic!("invalid fixture base64"),
            };
            buffer = (buffer << 6) | u32::from(value);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                output.push((buffer >> bits) as u8);
                if bits == 0 {
                    buffer = 0;
                } else {
                    buffer &= (1_u32 << bits) - 1;
                }
            }
        }
        output
    }

    fn push_biff_record(stream: &mut Vec<u8>, typ: u16, payload: &[u8]) -> usize {
        assert!(payload.len() <= u16::MAX as usize);
        let offset = stream.len();
        stream.extend(typ.to_le_bytes());
        stream.extend((payload.len() as u16).to_le_bytes());
        stream.extend(payload);
        offset
    }
}
