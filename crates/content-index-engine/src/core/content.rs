//! Bounded, offline text extraction for the Everything+ content index.
//!
//! Plain text/source/Markdown, PDF, DOCX, legacy XLS, XLSX, and ODS are
//! intentionally separate extractor formats. The PDF path uses lopdf only for
//! text objects, DOCX uses bounded ZIP/XML readers, and spreadsheet paths use
//! calamine's pure-Rust readers; none renders pages, runs OCR, follows external
//! resources, evaluates formulas, or executes document content.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use calamine::{Data, DataRef, Dimensions, Ods, OdsError, Reader, Xls, XlsError, Xlsx, XlsxError};
use quick_xml::events::Event;
use quick_xml::Reader as XmlReader;
use zip::read::ZipArchive;

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
/// Bumped only when DOCX package admission or WordprocessingML text rules
/// change. DOCX remains independent from spreadsheet OOXML because its part
/// relationships and text semantics have a separate review/rollback boundary.
pub const DOCX_EXTRACTOR_VERSION: &str = "docx-v1";
pub const DOCX_MAX_ZIP_ENTRIES: usize = 4_096;
pub const DOCX_MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
pub const DOCX_MAX_ZIP_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
pub const DOCX_MAX_XML_DEPTH: usize = 128;
pub const DOCX_MAX_XML_EVENTS: usize = 1_000_000;
pub const DOCX_MAX_XML_SOURCE_BUDGET: usize = 8_000_000;
pub const DOCX_MAX_RELATIONSHIPS: usize = 4_096;
const DOCX_MAX_CONTENT_TYPES_BYTES: u64 = 2 * 1024 * 1024;
const DOCX_MAX_PACKAGE_RELATIONSHIPS_BYTES: u64 = 1024 * 1024;
const DOCX_MAX_DOCUMENT_RELATIONSHIPS_BYTES: u64 = 4 * 1024 * 1024;
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
/// Bumped only when XLSX parsing or cell-to-text normalization rules change.
pub const XLSX_EXTRACTOR_VERSION: &str = "xlsx-v1";
pub const XLSX_MAX_SHEETS: usize = 256;
pub const XLSX_MAX_ROWS: u32 = 1_048_576;
pub const XLSX_MAX_COLUMNS: u32 = 16_384;
pub const XLSX_MAX_CELLS: usize = 4_000_000;
pub const XLSX_MAX_ZIP_ENTRIES: usize = 4_096;
pub const XLSX_MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
pub const XLSX_MAX_ZIP_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
pub const XLSX_MAX_SHARED_STRINGS: usize = 1_000_000;
pub const XLSX_MAX_SHARED_STRING_CHARS: usize = 8_000_000;
pub const XLSX_MAX_SHARED_STRING_XML_BYTES: u64 = 16 * 1024 * 1024;
pub const XLSX_MAX_XML_DEPTH: usize = 128;
pub const XLSX_MAX_XML_EVENTS: usize = 1_000_000;
pub const XLSX_MAX_XML_TEXT_CHARS: usize = 8_000_000;
pub const XLSX_MAX_RELATIONSHIPS: usize = 4_096;
const XLSX_MAX_PACKAGE_RELATIONSHIPS_BYTES: u64 = 1024 * 1024;
const XLSX_MAX_WORKBOOK_RELATIONSHIPS_BYTES: u64 = 4 * 1024 * 1024;
const XLSX_MAX_WORKBOOK_XML_BYTES: u64 = 8 * 1024 * 1024;
const XLSX_MAX_STYLES_XML_BYTES: u64 = 8 * 1024 * 1024;
/// Bumped only when ODS parsing or cell-to-text normalization rules change.
pub const ODS_EXTRACTOR_VERSION: &str = "ods-v1";
pub const ODS_MAX_SHEETS: usize = 256;
pub const ODS_MAX_ROWS: u32 = 1_048_576;
pub const ODS_MAX_COLUMNS: u32 = 16_384;
pub const ODS_MAX_CELLS: usize = 4_000_000;
pub const ODS_MAX_ZIP_ENTRIES: usize = 4_096;
pub const ODS_MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
pub const ODS_MAX_ZIP_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
pub const ODS_MAX_XML_DEPTH: usize = 128;
pub const ODS_MAX_XML_EVENTS: usize = 1_000_000;
pub const ODS_MAX_EXPANDED_TEXT_CHARS: usize = 16_000_000;
pub const ODS_MAX_ESTIMATED_MEMORY_BYTES: usize = 256 * 1024 * 1024;
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
const ZIP_EOCD_MIN_BYTES: usize = 22;
const ZIP_MAX_COMMENT_BYTES: usize = u16::MAX as usize;
const ZIP64_LOCATOR_BYTES: usize = 20;
const ZIP64_EOCD_MIN_BYTES: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZipEnvelopeFailure {
    Invalid,
    EntryLimit,
}

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

/// DOCX is the only Word container accepted by this extractor. Macro-enabled
/// DOCM and legacy binary DOC files retain their explicit non-support status.
pub fn is_docx_ext(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("docx")
}

pub fn is_docx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_docx_ext)
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

pub fn is_xlsx_ext(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("xlsx")
}

pub fn is_xlsx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_xlsx_ext)
}

pub fn is_ods_ext(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("ods")
}

pub fn is_ods_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(is_ods_ext)
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
    is_text_ext(ext)
        || is_pdf_ext(ext)
        || is_docx_ext(ext)
        || is_xls_ext(ext)
        || is_xlsx_ext(ext)
        || is_ods_ext(ext)
}

/// Extract one file while enforcing byte, character, encoding, race, and time
/// bounds.  No error returned by this function contains the source path.
pub fn extract_file(path: &Path, expected_size: u64, started: Instant) -> ContentRecord {
    let extractor_version = if is_pdf_path(path) {
        PDF_EXTRACTOR_VERSION
    } else if is_docx_path(path) {
        DOCX_EXTRACTOR_VERSION
    } else if is_xls_path(path) {
        XLS_EXTRACTOR_VERSION
    } else if is_xlsx_path(path) {
        XLSX_EXTRACTOR_VERSION
    } else if is_ods_path(path) {
        ODS_EXTRACTOR_VERSION
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
    } else if is_docx_path(path) {
        extract_docx_bytes(&bytes, started)
    } else if is_xls_path(path) {
        extract_xls_bytes(&bytes, started)
    } else if is_xlsx_path(path) {
        extract_xlsx_bytes(&bytes, started)
    } else if is_ods_path(path) {
        extract_ods_bytes(&bytes, started)
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

/// Extracts searchable WordprocessingML text from the canonical DOCX main part.
/// The package is admitted entirely in memory and no relationship target is
/// opened. Field instructions, macros, images, styles, embedded objects, and
/// non-main parts are deliberately outside this text-only contract.
pub fn extract_docx_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    if office_is_encrypted(bytes) {
        return docx_failure_record(DocxFailure::UnsupportedEncrypted);
    }

    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_docx_package(bytes, started)
    }));
    match parsed {
        Ok(Ok(record)) => record,
        Ok(Err(failure)) => docx_failure_record(failure),
        Err(_) => ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "extract_error",
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocxFailure {
    Timeout,
    UnsupportedEncrypted,
    UnsupportedEncoding,
    Extract(&'static str),
}

fn docx_failure_record(failure: DocxFailure) -> ContentRecord {
    match failure {
        DocxFailure::Timeout => ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        ),
        DocxFailure::UnsupportedEncrypted => ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncrypted,
            "unsupported_encrypted",
        ),
        DocxFailure::UnsupportedEncoding => ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncoding,
            "unsupported_encoding",
        ),
        DocxFailure::Extract(error_code) => ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            error_code,
        ),
    }
}

fn extract_docx_package(bytes: &[u8], started: Instant) -> Result<ContentRecord, DocxFailure> {
    validate_zip_envelope(bytes, DOCX_MAX_ZIP_ENTRIES).map_err(|failure| match failure {
        ZipEnvelopeFailure::Invalid => DocxFailure::Extract("extract_error"),
        ZipEnvelopeFailure::EntryLimit => DocxFailure::Extract("zip_limit"),
    })?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| DocxFailure::Extract("extract_error"))?;
    if archive.len() > DOCX_MAX_ZIP_ENTRIES {
        return Err(DocxFailure::Extract("zip_limit"));
    }

    let mut total_uncompressed = 0u64;
    let mut seen_names = HashSet::with_capacity(archive.len());
    let mut content_types_entry = None;
    let mut package_relationships_entry = None;
    let mut document_entry = None;
    let mut document_relationships_entry = None;
    for index in 0..archive.len() {
        if timed_out(started) {
            return Err(DocxFailure::Timeout);
        }
        let file = archive
            .by_index_raw(index)
            .map_err(|_| DocxFailure::Extract("extract_error"))?;
        if file.encrypted() {
            return Err(DocxFailure::UnsupportedEncrypted);
        }
        if file.name_raw().contains(&0)
            || file.name().contains('\\')
            || file.enclosed_name().is_none()
        {
            return Err(DocxFailure::Extract("zip_path"));
        }
        let size = file.size();
        if size > DOCX_MAX_ZIP_ENTRY_BYTES {
            return Err(DocxFailure::Extract("zip_limit"));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > DOCX_MAX_ZIP_UNCOMPRESSED_BYTES {
            return Err(DocxFailure::Extract("zip_limit"));
        }
        let lower_name = file.name().to_ascii_lowercase();
        if !seen_names.insert(lower_name.clone()) {
            return Err(DocxFailure::Extract("zip_path"));
        }
        match lower_name.as_str() {
            "[content_types].xml" => {
                require_docx_part_size(size, DOCX_MAX_CONTENT_TYPES_BYTES)?;
                content_types_entry = Some(index);
            }
            "_rels/.rels" => {
                require_docx_part_size(size, DOCX_MAX_PACKAGE_RELATIONSHIPS_BYTES)?;
                package_relationships_entry = Some(index);
            }
            "word/document.xml" => {
                require_docx_part_size(size, DOCX_MAX_ZIP_ENTRY_BYTES)?;
                document_entry = Some(index);
            }
            "word/_rels/document.xml.rels" => {
                require_docx_part_size(size, DOCX_MAX_DOCUMENT_RELATIONSHIPS_BYTES)?;
                document_relationships_entry = Some(index);
            }
            _ => {}
        }
    }

    let content_types_entry = content_types_entry.ok_or(DocxFailure::Extract("extract_error"))?;
    let package_relationships_entry =
        package_relationships_entry.ok_or(DocxFailure::Extract("extract_error"))?;
    let document_entry = document_entry.ok_or(DocxFailure::Extract("extract_error"))?;

    let content_types = read_docx_part(
        &mut archive,
        content_types_entry,
        started,
        DOCX_MAX_CONTENT_TYPES_BYTES,
    )?;
    scan_docx_content_types(&content_types, started)?;

    let package_relationships = read_docx_part(
        &mut archive,
        package_relationships_entry,
        started,
        DOCX_MAX_PACKAGE_RELATIONSHIPS_BYTES,
    )?;
    scan_docx_relationships(&package_relationships, true, started)?;

    if let Some(index) = document_relationships_entry {
        let relationships = read_docx_part(
            &mut archive,
            index,
            started,
            DOCX_MAX_DOCUMENT_RELATIONSHIPS_BYTES,
        )?;
        scan_docx_relationships(&relationships, false, started)?;
    }

    let document = read_docx_part(
        &mut archive,
        document_entry,
        started,
        DOCX_MAX_ZIP_ENTRY_BYTES,
    )?;
    scan_docx_document(&document, started)
}

fn require_docx_part_size(size: u64, limit: u64) -> Result<(), DocxFailure> {
    if size > limit {
        Err(DocxFailure::Extract("zip_limit"))
    } else {
        Ok(())
    }
}

fn read_docx_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
    started: Instant,
    max_bytes: u64,
) -> Result<Vec<u8>, DocxFailure> {
    let file = archive
        .by_index(index)
        .map_err(|_| DocxFailure::Extract("extract_error"))?;
    read_docx_entry(file, started, max_bytes)
}

fn read_docx_entry<R: Read>(
    mut reader: R,
    started: Instant,
    max_bytes: u64,
) -> Result<Vec<u8>, DocxFailure> {
    let mut output = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if timed_out(started) {
            return Err(DocxFailure::Timeout);
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|_| DocxFailure::Extract("extract_error"))?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) as u64 > max_bytes {
            return Err(DocxFailure::Extract("zip_limit"));
        }
        output.extend_from_slice(&chunk[..read]);
    }
    Ok(output)
}

fn scan_docx_content_types(xml: &[u8], started: Instant) -> Result<(), DocxFailure> {
    const TRANSITIONAL_MAIN: &[u8] =
        b"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
    const MACRO_ENABLED_MAIN: &[u8] = b"application/vnd.ms-word.document.macroEnabled.main+xml";

    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut source_budget = 0usize;
    let mut main_document_types = 0usize;
    let mut saw_types_root = false;
    loop {
        if timed_out(started) {
            return Err(DocxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| DocxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > DOCX_MAX_XML_EVENTS {
            return Err(DocxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > DOCX_MAX_XML_DEPTH {
                    return Err(DocxFailure::Extract("xml_limit"));
                }
                if depth == 1 {
                    if element.local_name().as_ref() != b"Types" || saw_types_root {
                        return Err(DocxFailure::Extract("extract_error"));
                    }
                    saw_types_root = true;
                }
                source_budget = source_budget.saturating_add(docx_attribute_bytes(&element)?);
                if element.local_name().as_ref() == b"Override" {
                    let part = docx_attribute_value(&element, b"PartName")?
                        .ok_or(DocxFailure::Extract("extract_error"))?;
                    let content_type = docx_attribute_value(&element, b"ContentType")?
                        .ok_or(DocxFailure::Extract("extract_error"))?;
                    if part == b"/word/document.xml" {
                        if content_type.as_slice() == TRANSITIONAL_MAIN {
                            main_document_types = main_document_types.saturating_add(1);
                        } else if content_type.as_slice() == MACRO_ENABLED_MAIN {
                            // Macro-enabled content is intentionally excluded
                            // even when it is mislabeled with a .docx suffix.
                            return Err(DocxFailure::Extract("unsupported_document"));
                        } else {
                            return Err(DocxFailure::Extract("extract_error"));
                        }
                    }
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                source_budget = source_budget.saturating_add(docx_xml_text_chars(&value)?);
            }
            Event::CData(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                source_budget = source_budget.saturating_add(docx_xml_text_chars(&value)?);
            }
            Event::GeneralRef(reference) => {
                let reference: &[u8] = reference.as_ref();
                decode_docx_reference(reference).ok_or(DocxFailure::Extract("extract_error"))?;
                source_budget = source_budget.saturating_add(1);
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(DocxFailure::Extract("extract_error"));
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(DocxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
        if source_budget > DOCX_MAX_XML_SOURCE_BUDGET {
            return Err(DocxFailure::Extract("xml_limit"));
        }
    }
    if depth != 0 || !saw_types_root || main_document_types != 1 {
        return Err(DocxFailure::Extract("extract_error"));
    }
    Ok(())
}

fn scan_docx_relationships(
    xml: &[u8],
    package_root: bool,
    started: Instant,
) -> Result<(), DocxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut source_budget = 0usize;
    let mut relationships = 0usize;
    let mut office_documents = 0usize;
    let mut saw_relationships_root = false;
    let mut ids = HashSet::new();
    loop {
        if timed_out(started) {
            return Err(DocxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| DocxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > DOCX_MAX_XML_EVENTS {
            return Err(DocxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > DOCX_MAX_XML_DEPTH {
                    return Err(DocxFailure::Extract("xml_limit"));
                }
                if depth == 1 {
                    if element.local_name().as_ref() != b"Relationships" || saw_relationships_root {
                        return Err(DocxFailure::Extract("extract_error"));
                    }
                    saw_relationships_root = true;
                }
                source_budget = source_budget.saturating_add(docx_attribute_bytes(&element)?);
                if element.local_name().as_ref() == b"Relationship" {
                    relationships = relationships.saturating_add(1);
                    if relationships > DOCX_MAX_RELATIONSHIPS {
                        return Err(DocxFailure::Extract("xml_limit"));
                    }
                    let id = docx_attribute_value(&element, b"Id")?
                        .ok_or(DocxFailure::Extract("extract_error"))?;
                    if id.is_empty() || !ids.insert(id) {
                        return Err(DocxFailure::Extract("extract_error"));
                    }
                    let relation_type = docx_attribute_value(&element, b"Type")?
                        .ok_or(DocxFailure::Extract("extract_error"))?;
                    let target = docx_attribute_value(&element, b"Target")?
                        .ok_or(DocxFailure::Extract("extract_error"))?;
                    let target_mode = docx_attribute_value(&element, b"TargetMode")?;
                    let is_external = target_mode
                        .as_deref()
                        .is_some_and(|mode| mode.eq_ignore_ascii_case(b"external"));
                    let is_internal = target_mode
                        .as_deref()
                        .is_none_or(|mode| mode.eq_ignore_ascii_case(b"internal"));
                    let safe_internal_target = if package_root {
                        xlsx_relationship_target_is_safe(&target)
                    } else {
                        docx_document_relationship_target_is_safe(&target)
                    };
                    if (!is_external && !is_internal)
                        || (package_root && is_external)
                        || (!is_external && !safe_internal_target)
                    {
                        return Err(DocxFailure::Extract("external_relationship"));
                    }
                    let is_office_document = matches!(
                        relation_type.as_slice(),
                        b"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"
                            | b"http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument"
                    );
                    if package_root
                        && relation_type.ends_with(b"/relationships/officeDocument")
                        && !is_office_document
                    {
                        return Err(DocxFailure::Extract("external_relationship"));
                    }
                    if package_root && is_office_document {
                        office_documents = office_documents.saturating_add(1);
                        let normalized = target.strip_prefix(b"/").unwrap_or(&target);
                        if normalized != b"word/document.xml" || office_documents > 1 {
                            return Err(DocxFailure::Extract("external_relationship"));
                        }
                    }
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                source_budget = source_budget.saturating_add(docx_xml_text_chars(&value)?);
            }
            Event::CData(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                source_budget = source_budget.saturating_add(docx_xml_text_chars(&value)?);
            }
            Event::GeneralRef(reference) => {
                let reference: &[u8] = reference.as_ref();
                decode_docx_reference(reference).ok_or(DocxFailure::Extract("extract_error"))?;
                source_budget = source_budget.saturating_add(1);
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(DocxFailure::Extract("extract_error"));
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(DocxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
        if source_budget > DOCX_MAX_XML_SOURCE_BUDGET {
            return Err(DocxFailure::Extract("xml_limit"));
        }
    }
    if depth != 0 || !saw_relationships_root || (package_root && office_documents != 1) {
        return Err(DocxFailure::Extract("extract_error"));
    }
    Ok(())
}

fn scan_docx_document(xml: &[u8], started: Instant) -> Result<ContentRecord, DocxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut source_budget = 0usize;
    let mut text_depth = None;
    let mut body_depth = None;
    let mut body_count = 0usize;
    let mut saw_document = false;
    let mut output = DocxTextAccumulator::default();

    loop {
        if timed_out(started) {
            return Err(DocxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| DocxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > DOCX_MAX_XML_EVENTS {
            return Err(DocxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > DOCX_MAX_XML_DEPTH {
                    return Err(DocxFailure::Extract("xml_limit"));
                }
                source_budget = source_budget.saturating_add(docx_attribute_bytes(&element)?);
                let name = element.local_name();
                if depth == 1 {
                    if name.as_ref() != b"document" || saw_document {
                        return Err(DocxFailure::Extract("extract_error"));
                    }
                    saw_document = true;
                }
                match name.as_ref() {
                    b"body" => {
                        if depth != 2 || body_depth.is_some() || body_count != 0 {
                            return Err(DocxFailure::Extract("extract_error"));
                        }
                        body_depth = Some(depth);
                        body_count = 1;
                    }
                    b"t" => {
                        if body_depth.is_some() && text_depth.is_some() {
                            return Err(DocxFailure::Extract("extract_error"));
                        }
                        if body_depth.is_some() {
                            text_depth = Some(depth);
                        }
                    }
                    b"tab" if body_depth.is_some() => output.queue_separator('\t'),
                    b"br" | b"cr" if body_depth.is_some() => output.queue_separator('\n'),
                    _ => {}
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                let value_chars = docx_xml_text_chars(&value)?;
                source_budget = source_budget.saturating_add(value_chars);
                if text_depth.is_some() {
                    output.push_text(&value, started)?;
                }
            }
            Event::CData(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| DocxFailure::UnsupportedEncoding)?;
                let value_chars = docx_xml_text_chars(&value)?;
                source_budget = source_budget.saturating_add(value_chars);
                if text_depth.is_some() {
                    output.push_text(&value, started)?;
                }
            }
            Event::GeneralRef(reference) => {
                source_budget = source_budget.saturating_add(1);
                let reference: &[u8] = reference.as_ref();
                let decoded = decode_docx_reference(reference)
                    .ok_or(DocxFailure::Extract("extract_error"))?;
                if text_depth.is_some() {
                    let mut encoded = [0u8; 4];
                    output.push_text(decoded.encode_utf8(&mut encoded), started)?;
                }
            }
            Event::End(element) => {
                if depth == 0 {
                    return Err(DocxFailure::Extract("extract_error"));
                }
                let name = element.local_name();
                if name.as_ref() == b"t" && text_depth == Some(depth) {
                    text_depth = None;
                }
                if name.as_ref() == b"p" && body_depth.is_some() {
                    output.queue_separator('\n');
                }
                if name.as_ref() == b"body" && body_depth == Some(depth) {
                    body_depth = None;
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(DocxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
        if source_budget > DOCX_MAX_XML_SOURCE_BUDGET {
            return Err(DocxFailure::Extract("xml_limit"));
        }
    }

    if depth != 0
        || text_depth.is_some()
        || body_depth.is_some()
        || body_count != 1
        || !saw_document
    {
        return Err(DocxFailure::Extract("extract_error"));
    }
    if !output
        .text
        .chars()
        .any(|character| !character.is_whitespace())
    {
        return Ok(ContentRecord::failure_for(
            DOCX_EXTRACTOR_VERSION,
            ContentStatus::NoText,
            "no_text",
        ));
    }
    Ok(ContentRecord {
        text: output.text,
        status: ContentStatus::Indexed,
        extractor_version: DOCX_EXTRACTOR_VERSION,
        encoding: Some("docx"),
        truncated: output.truncated,
        error_code: output.truncated.then_some("text_limit"),
        text_chars: output.text_chars,
    })
}

fn docx_attribute_bytes(element: &quick_xml::events::BytesStart<'_>) -> Result<usize, DocxFailure> {
    let mut bytes = 0usize;
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| DocxFailure::Extract("extract_error"))?;
        bytes = bytes.saturating_add(attribute.value.len());
    }
    Ok(bytes)
}

fn docx_attribute_value(
    element: &quick_xml::events::BytesStart<'_>,
    wanted: &[u8],
) -> Result<Option<Vec<u8>>, DocxFailure> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| DocxFailure::Extract("extract_error"))?;
        let key = attribute.key.as_ref();
        let key = key.rsplit(|byte| *byte == b':').next().unwrap_or(key);
        if key == wanted {
            return Ok(Some(attribute.value.into_owned()));
        }
    }
    Ok(None)
}

fn decode_docx_reference(reference: &[u8]) -> Option<char> {
    let character = match reference {
        b"amp" => '&',
        b"lt" => '<',
        b"gt" => '>',
        b"apos" => '\'',
        b"quot" => '"',
        value if value.starts_with(b"#x") || value.starts_with(b"#X") => {
            let digits = value.get(2..)?;
            if digits.is_empty() || digits.len() > 6 {
                return None;
            }
            char::from_u32(u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()?)?
        }
        value if value.starts_with(b"#") => {
            let digits = value.get(1..)?;
            if digits.is_empty() || digits.len() > 7 {
                return None;
            }
            char::from_u32(std::str::from_utf8(digits).ok()?.parse().ok()?)?
        }
        _ => return None,
    };
    is_xml_10_character(character).then_some(character)
}

fn docx_xml_text_chars(value: &str) -> Result<usize, DocxFailure> {
    let mut chars = 0usize;
    for character in value.chars() {
        if !is_xml_10_character(character) {
            return Err(DocxFailure::Extract("extract_error"));
        }
        chars = chars.saturating_add(1);
    }
    Ok(chars)
}

fn is_xml_10_character(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || matches!(character as u32, 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff)
}

/// Relationship targets are never opened, but normal Word documents can use
/// `../customXml/...` from `word/document.xml.rels`. Resolve those lexical
/// parent segments against the known `word/` base while refusing a target
/// that would escape the package root or introduce URI/path ambiguity.
fn docx_document_relationship_target_is_safe(target: &[u8]) -> bool {
    if target.starts_with(b"/") {
        return xlsx_relationship_target_is_safe(target);
    }
    let mut depth = 1usize;
    let mut ends_in_part = false;
    for part in target.split(|byte| *byte == b'/') {
        if part.is_empty() || part == b"." {
            return false;
        }
        if part == b".." {
            let Some(next_depth) = depth.checked_sub(1) else {
                return false;
            };
            depth = next_depth;
            ends_in_part = false;
            continue;
        }
        if part.iter().any(|byte| {
            byte.is_ascii_control() || matches!(*byte, b'\\' | b':' | b'?' | b'#' | b'%')
        }) {
            return false;
        }
        depth = depth.saturating_add(1);
        ends_in_part = true;
    }
    ends_in_part
}

#[derive(Debug, Default)]
struct DocxTextAccumulator {
    text: String,
    text_chars: usize,
    pending_separator: Option<char>,
    truncated: bool,
}

impl DocxTextAccumulator {
    fn queue_separator(&mut self, separator: char) {
        if self.text.is_empty() {
            return;
        }
        self.pending_separator = match (self.pending_separator, separator) {
            (Some('\n'), _) | (_, '\n') => Some('\n'),
            (Some('\t'), _) | (_, '\t') => Some('\t'),
            (_, value) => Some(value),
        };
    }

    fn push_text(&mut self, value: &str, started: Instant) -> Result<(), DocxFailure> {
        if value.is_empty() || self.truncated {
            return Ok(());
        }
        if let Some(separator) = self.pending_separator.take() {
            if self.text_chars == MAX_TEXT_CHARS {
                self.truncated = true;
                return Ok(());
            }
            self.text.push(separator);
            self.text_chars += 1;
        }
        for (index, character) in value.chars().enumerate() {
            if index.is_multiple_of(8192) && timed_out(started) {
                return Err(DocxFailure::Timeout);
            }
            if self.text_chars == MAX_TEXT_CHARS {
                self.truncated = true;
                break;
            }
            self.text.push(character);
            self.text_chars += 1;
        }
        Ok(())
    }
}

/// Extracts worksheet cell values from an Office Open XML workbook using
/// calamine's pure-Rust Xlsx reader. The reader is consumed through its
/// streaming cell API rather than `worksheet_range`, because the latter
/// materializes a dense range from an untrusted worksheet dimension. Formula
/// text is not evaluated; a cached value, when present, is simply parser data.
pub fn extract_xlsx_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    if office_is_encrypted(bytes) {
        return xlsx_failure_record(XlsxFailure::UnsupportedEncrypted);
    }

    if let Err(failure) = xlsx_preflight(bytes, started) {
        return xlsx_failure_record(failure);
    }

    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_xlsx_with_calamine(bytes, started)
    }));
    match parsed {
        Ok(Ok(record)) => record,
        Ok(Err(failure)) => xlsx_failure_record(failure),
        Err(_) => ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "extract_error",
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XlsxFailure {
    Timeout,
    UnsupportedEncrypted,
    UnsupportedEncoding,
    Extract(&'static str),
}

/// Office's password-protected OOXML files are CFB containers whose
/// `EncryptedPackage` stream contains the ZIP payload after decryption. They
/// are not ZIP archives that calamine can read, so identify them before ZIP
/// preflight and return the stable unsupported-encrypted status. Malformed CFB
/// input is intentionally treated as an ordinary corrupt OOXML candidate.
fn office_is_encrypted(bytes: &[u8]) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let container = cfb::CompoundFile::open(Cursor::new(bytes)).ok()?;
        Some(container.is_stream("/EncryptedPackage"))
    }))
    .ok()
    .flatten()
    .unwrap_or(false)
}

fn xlsx_failure_record(failure: XlsxFailure) -> ContentRecord {
    match failure {
        XlsxFailure::Timeout => ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        ),
        XlsxFailure::UnsupportedEncrypted => ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncrypted,
            "unsupported_encrypted",
        ),
        XlsxFailure::UnsupportedEncoding => ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncoding,
            "unsupported_encoding",
        ),
        XlsxFailure::Extract(error_code) => ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            error_code,
        ),
    }
}

fn extract_xlsx_with_calamine(
    bytes: &[u8],
    started: Instant,
) -> Result<ContentRecord, XlsxFailure> {
    let mut workbook = match Xlsx::new(Cursor::new(bytes)) {
        Ok(workbook) => workbook,
        Err(XlsxError::Password) => return Err(XlsxFailure::UnsupportedEncrypted),
        Err(_) => return Err(XlsxFailure::Extract("extract_error")),
    };
    if timed_out(started) {
        return Err(XlsxFailure::Timeout);
    }

    let sheet_names = workbook.sheet_names();
    if sheet_names.len() > XLSX_MAX_SHEETS {
        return Err(XlsxFailure::Extract("sheet_limit"));
    }

    let mut output = XlsTextAccumulator::default();
    let mut logical_cells = 0usize;
    let mut visited_cells = 0usize;

    for sheet_name in sheet_names {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        let mut reader = match workbook.worksheet_cells_reader(&sheet_name) {
            Ok(reader) => reader,
            Err(XlsxError::NotAWorksheet(_)) => continue,
            Err(XlsxError::Password) => return Err(XlsxFailure::UnsupportedEncrypted),
            Err(_) => return Err(XlsxFailure::Extract("extract_error")),
        };
        let dimensions = reader.dimensions();
        let dimension_cells = xlsx_dimension_cell_count(dimensions)?;
        logical_cells = logical_cells.saturating_add(dimension_cells);
        if logical_cells > XLSX_MAX_CELLS {
            return Err(XlsxFailure::Extract("cell_limit"));
        }

        // Cell coordinates restart at row zero for every worksheet. Keep a
        // sheet-local row state so the first value of a new sheet cannot be
        // mistaken for another cell in the final row of the previous sheet.
        let mut last_row = None;
        let mut row_has_value = false;

        loop {
            if timed_out(started) {
                return Err(XlsxFailure::Timeout);
            }
            let cell = match reader.next_cell() {
                Ok(Some(cell)) => cell,
                Ok(None) => break,
                Err(XlsxError::Password) => return Err(XlsxFailure::UnsupportedEncrypted),
                Err(_) => return Err(XlsxFailure::Extract("extract_error")),
            };
            visited_cells = visited_cells.saturating_add(1);
            if visited_cells > XLSX_MAX_CELLS {
                return Err(XlsxFailure::Extract("cell_limit"));
            }
            if visited_cells.is_multiple_of(8192) && timed_out(started) {
                return Err(XlsxFailure::Timeout);
            }

            let (row, column) = cell.get_position();
            if row >= XLSX_MAX_ROWS {
                return Err(XlsxFailure::Extract("row_limit"));
            }
            if column >= XLSX_MAX_COLUMNS {
                return Err(XlsxFailure::Extract("column_limit"));
            }
            let Some(value) = xlsx_cell_value(cell.get_value()) else {
                continue;
            };
            if value.contains('\0') {
                return Err(XlsxFailure::UnsupportedEncoding);
            }

            if last_row != Some(row) {
                if output.has_value {
                    output.push_separator('\n');
                }
                last_row = Some(row);
            } else if row_has_value {
                output.push_separator('\t');
            }
            row_has_value = true;
            match output.push_value(&value, started) {
                XlsAppendResult::Complete => {}
                XlsAppendResult::Truncated => {
                    return Ok(ContentRecord {
                        text: output.text,
                        status: ContentStatus::Indexed,
                        extractor_version: XLSX_EXTRACTOR_VERSION,
                        encoding: Some("xlsx"),
                        truncated: true,
                        error_code: Some("text_limit"),
                        text_chars: output.text_chars,
                    })
                }
                XlsAppendResult::Timeout => return Err(XlsxFailure::Timeout),
            }
        }
    }
    if timed_out(started) {
        return Err(XlsxFailure::Timeout);
    }
    if !output.has_value {
        return Ok(ContentRecord::failure_for(
            XLSX_EXTRACTOR_VERSION,
            ContentStatus::NoText,
            "no_text",
        ));
    }

    Ok(ContentRecord {
        text: output.text,
        status: ContentStatus::Indexed,
        extractor_version: XLSX_EXTRACTOR_VERSION,
        encoding: Some("xlsx"),
        truncated: false,
        error_code: None,
        text_chars: output.text_chars,
    })
}

/// Extracts worksheet cell values from an OpenDocument Spreadsheet using
/// calamine's native, pure-Rust Ods reader. ODS parsing is text-only: formulas
/// are never evaluated, and images, styles, macros, rendering, and external
/// resources are ignored. The preflight below is required because calamine's
/// ODS implementation materializes each worksheet into a dense range.
pub fn extract_ods_bytes(bytes: &[u8], started: Instant) -> ContentRecord {
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::TooLarge,
            "file_too_large",
        );
    }
    if timed_out(started) {
        return ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        );
    }
    if let Err(failure) = ods_preflight(bytes, started) {
        return ods_failure_record(failure);
    }

    let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        extract_ods_with_calamine(bytes, started)
    }));
    match parsed {
        Ok(Ok(record)) => record,
        Ok(Err(failure)) => ods_failure_record(failure),
        Err(_) => ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            "extract_error",
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OdsFailure {
    Timeout,
    UnsupportedEncrypted,
    UnsupportedEncoding,
    Extract(&'static str),
}

fn ods_failure_record(failure: OdsFailure) -> ContentRecord {
    match failure {
        OdsFailure::Timeout => ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::Timeout,
            "processing_timeout",
        ),
        OdsFailure::UnsupportedEncrypted => ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncrypted,
            "unsupported_encrypted",
        ),
        OdsFailure::UnsupportedEncoding => ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::UnsupportedEncoding,
            "unsupported_encoding",
        ),
        OdsFailure::Extract(error_code) => ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::ExtractError,
            error_code,
        ),
    }
}

fn extract_ods_with_calamine(bytes: &[u8], started: Instant) -> Result<ContentRecord, OdsFailure> {
    let mut workbook = match Ods::new(Cursor::new(bytes)) {
        Ok(workbook) => workbook,
        Err(OdsError::Password) => return Err(OdsFailure::UnsupportedEncrypted),
        Err(_) => return Err(OdsFailure::Extract("extract_error")),
    };
    if timed_out(started) {
        return Err(OdsFailure::Timeout);
    }

    let sheet_names = workbook.sheet_names();
    if sheet_names.len() > ODS_MAX_SHEETS {
        return Err(OdsFailure::Extract("sheet_limit"));
    }

    let mut output = XlsTextAccumulator::default();
    let mut logical_cells = 0usize;
    for sheet_name in sheet_names {
        if timed_out(started) {
            return Err(OdsFailure::Timeout);
        }
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(OdsError::Password) => return Err(OdsFailure::UnsupportedEncrypted),
            Err(_) => return Err(OdsFailure::Extract("extract_error")),
        };
        let (rows, columns) = range.get_size();
        if rows > ODS_MAX_ROWS as usize {
            return Err(OdsFailure::Extract("row_limit"));
        }
        if columns > ODS_MAX_COLUMNS as usize {
            return Err(OdsFailure::Extract("column_limit"));
        }
        logical_cells = logical_cells.saturating_add(rows.saturating_mul(columns));
        if logical_cells > ODS_MAX_CELLS {
            return Err(OdsFailure::Extract("cell_limit"));
        }

        let mut visited_cells = 0usize;
        for row in range.rows() {
            let mut row_has_value = false;
            for cell in row {
                visited_cells = visited_cells.saturating_add(1);
                if visited_cells.is_multiple_of(8192) && timed_out(started) {
                    return Err(OdsFailure::Timeout);
                }
                if matches!(cell, Data::Empty) {
                    continue;
                }
                let value = cell.to_string();
                if value.is_empty() {
                    continue;
                }
                if value.contains('\0') {
                    return Err(OdsFailure::UnsupportedEncoding);
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
                        return Ok(ContentRecord {
                            text: output.text,
                            status: ContentStatus::Indexed,
                            extractor_version: ODS_EXTRACTOR_VERSION,
                            encoding: Some("ods"),
                            truncated: true,
                            error_code: Some("text_limit"),
                            text_chars: output.text_chars,
                        })
                    }
                    XlsAppendResult::Timeout => return Err(OdsFailure::Timeout),
                }
            }
        }
    }
    if timed_out(started) {
        return Err(OdsFailure::Timeout);
    }
    if !output.has_value {
        return Ok(ContentRecord::failure_for(
            ODS_EXTRACTOR_VERSION,
            ContentStatus::NoText,
            "no_text",
        ));
    }

    Ok(ContentRecord {
        text: output.text,
        status: ContentStatus::Indexed,
        extractor_version: ODS_EXTRACTOR_VERSION,
        encoding: Some("ods"),
        truncated: false,
        error_code: None,
        text_chars: output.text_chars,
    })
}

/// Inspect the ODS ZIP package and content.xml before calamine constructs its
/// dense in-memory ranges. The archive is handled entirely from the supplied
/// byte buffer; no member path, relationship, or external resource is opened.
fn ods_preflight(bytes: &[u8], started: Instant) -> Result<(), OdsFailure> {
    const MIME_TYPE: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";

    validate_zip_envelope(bytes, ODS_MAX_ZIP_ENTRIES).map_err(|failure| match failure {
        ZipEnvelopeFailure::Invalid => OdsFailure::Extract("extract_error"),
        ZipEnvelopeFailure::EntryLimit => OdsFailure::Extract("zip_limit"),
    })?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| OdsFailure::Extract("extract_error"))?;
    if archive.len() > ODS_MAX_ZIP_ENTRIES {
        return Err(OdsFailure::Extract("zip_limit"));
    }

    let mut total_uncompressed = 0u64;
    let mut seen_names = HashSet::with_capacity(archive.len());
    let mut mimetype_entry = None;
    let mut manifest_entry = None;
    let mut content_entry = None;
    for index in 0..archive.len() {
        if timed_out(started) {
            return Err(OdsFailure::Timeout);
        }
        let file = archive
            .by_index_raw(index)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        if file.encrypted() {
            return Err(OdsFailure::UnsupportedEncrypted);
        }
        if file.name_raw().contains(&0) || file.enclosed_name().is_none() {
            return Err(OdsFailure::Extract("zip_path"));
        }
        let size = file.size();
        if size > ODS_MAX_ZIP_ENTRY_BYTES {
            return Err(OdsFailure::Extract("zip_limit"));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > ODS_MAX_ZIP_UNCOMPRESSED_BYTES {
            return Err(OdsFailure::Extract("zip_limit"));
        }
        let name = file.name().replace('\\', "/");
        if !seen_names.insert(name.clone()) {
            return Err(OdsFailure::Extract("zip_path"));
        }
        match name.as_str() {
            "mimetype" => mimetype_entry = Some(index),
            "META-INF/manifest.xml" => manifest_entry = Some(index),
            "content.xml" => content_entry = Some(index),
            _ => {}
        }
    }

    let mimetype_entry = mimetype_entry.ok_or(OdsFailure::Extract("extract_error"))?;
    let manifest_entry = manifest_entry.ok_or(OdsFailure::Extract("extract_error"))?;
    let content_entry = content_entry.ok_or(OdsFailure::Extract("extract_error"))?;

    let mimetype = {
        let file = archive
            .by_index(mimetype_entry)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        read_ods_entry(file, started, MIME_TYPE.len() as u64)?
    };
    if mimetype != MIME_TYPE {
        return Err(OdsFailure::Extract("extract_error"));
    }

    let manifest = {
        let file = archive
            .by_index(manifest_entry)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        read_ods_entry(file, started, ODS_MAX_ZIP_ENTRY_BYTES)?
    };
    scan_ods_manifest(&manifest, started)?;

    let content = {
        let file = archive
            .by_index(content_entry)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        read_ods_entry(file, started, ODS_MAX_ZIP_ENTRY_BYTES)?
    };
    scan_ods_content(&content, started)?;
    Ok(())
}

/// Read the ZIP end records before `ZipArchive::new` allocates one metadata
/// object per central-directory entry. This keeps an attacker from using a
/// small file with hundreds of thousands of empty entries to bypass the
/// post-construction entry cap. Multi-disk containers are not valid document
/// packages and are rejected. ZIP64 is accepted when its locator and minimum
/// end record are present and in bounds.
fn validate_zip_envelope(bytes: &[u8], max_entries: usize) -> Result<(), ZipEnvelopeFailure> {
    if bytes.len() < ZIP_EOCD_MIN_BYTES {
        return Err(ZipEnvelopeFailure::Invalid);
    }
    let search_start = bytes
        .len()
        .saturating_sub(ZIP_EOCD_MIN_BYTES + ZIP_MAX_COMMENT_BYTES);
    let search_end = bytes.len() - ZIP_EOCD_MIN_BYTES;
    let mut candidates = (search_start..=search_end).rev().filter(|offset| {
        bytes.get(*offset..offset.saturating_add(4)) == Some(b"PK\x05\x06")
            && read_u16_le(bytes, offset.saturating_add(20)).is_some_and(|comment_bytes| {
                offset
                    .saturating_add(ZIP_EOCD_MIN_BYTES)
                    .saturating_add(comment_bytes as usize)
                    == bytes.len()
            })
    });
    let eocd = candidates.next().ok_or(ZipEnvelopeFailure::Invalid)?;
    // A second end record hidden in the first record's comment could make the
    // ZIP reader reject the last candidate and fall back to a different entry
    // count after this admission check. Document packages do not need nested
    // EOCD candidates, so reject the ambiguity instead of guessing.
    if candidates.next().is_some() {
        return Err(ZipEnvelopeFailure::Invalid);
    }

    let disk = read_u16_le(bytes, eocd + 4).ok_or(ZipEnvelopeFailure::Invalid)?;
    let central_disk = read_u16_le(bytes, eocd + 6).ok_or(ZipEnvelopeFailure::Invalid)?;
    let entries_on_disk = read_u16_le(bytes, eocd + 8).ok_or(ZipEnvelopeFailure::Invalid)?;
    let total_entries = read_u16_le(bytes, eocd + 10).ok_or(ZipEnvelopeFailure::Invalid)?;
    let central_size = read_u32_le(bytes, eocd + 12).ok_or(ZipEnvelopeFailure::Invalid)?;
    let central_offset = read_u32_le(bytes, eocd + 16).ok_or(ZipEnvelopeFailure::Invalid)?;
    if disk != 0 || central_disk != 0 {
        return Err(ZipEnvelopeFailure::Invalid);
    }

    let uses_zip64 = entries_on_disk == u16::MAX
        || total_entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX;
    let entries = if uses_zip64 {
        let locator = eocd
            .checked_sub(ZIP64_LOCATOR_BYTES)
            .ok_or(ZipEnvelopeFailure::Invalid)?;
        if bytes.get(locator..locator + 4) != Some(b"PK\x06\x07")
            || read_u32_le(bytes, locator + 4) != Some(0)
            || read_u32_le(bytes, locator + 16) != Some(1)
        {
            return Err(ZipEnvelopeFailure::Invalid);
        }
        let zip64_offset = read_u64_le(bytes, locator + 8)
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or(ZipEnvelopeFailure::Invalid)?;
        if zip64_offset.saturating_add(ZIP64_EOCD_MIN_BYTES) > locator
            || bytes.get(zip64_offset..zip64_offset + 4) != Some(b"PK\x06\x06")
            || read_u32_le(bytes, zip64_offset + 16) != Some(0)
            || read_u32_le(bytes, zip64_offset + 20) != Some(0)
        {
            return Err(ZipEnvelopeFailure::Invalid);
        }
        let record_size = read_u64_le(bytes, zip64_offset + 4)
            .and_then(|size| usize::try_from(size).ok())
            .ok_or(ZipEnvelopeFailure::Invalid)?;
        if record_size < ZIP64_EOCD_MIN_BYTES - 12
            || zip64_offset.saturating_add(12).saturating_add(record_size) > locator
        {
            return Err(ZipEnvelopeFailure::Invalid);
        }
        let entries_on_disk =
            read_u64_le(bytes, zip64_offset + 24).ok_or(ZipEnvelopeFailure::Invalid)?;
        let total_entries =
            read_u64_le(bytes, zip64_offset + 32).ok_or(ZipEnvelopeFailure::Invalid)?;
        if entries_on_disk != total_entries {
            return Err(ZipEnvelopeFailure::Invalid);
        }
        total_entries
    } else {
        if entries_on_disk != total_entries {
            return Err(ZipEnvelopeFailure::Invalid);
        }
        u64::from(total_entries)
    };
    if entries > max_entries as u64 {
        return Err(ZipEnvelopeFailure::EntryLimit);
    }
    Ok(())
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_u64_le(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn read_ods_entry<R: Read>(
    mut reader: R,
    started: Instant,
    max_bytes: u64,
) -> Result<Vec<u8>, OdsFailure> {
    let mut output = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if timed_out(started) {
            return Err(OdsFailure::Timeout);
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) as u64 > max_bytes {
            return Err(OdsFailure::Extract("zip_limit"));
        }
        output.extend_from_slice(&chunk[..read]);
    }
    Ok(output)
}

fn scan_ods_manifest(xml: &[u8], started: Instant) -> Result<(), OdsFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    loop {
        if timed_out(started) {
            return Err(OdsFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > ODS_MAX_XML_EVENTS {
            return Err(OdsFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > ODS_MAX_XML_DEPTH {
                    return Err(OdsFailure::Extract("xml_limit"));
                }
                if element.local_name().as_ref() == b"encryption-data" {
                    return Err(OdsFailure::UnsupportedEncrypted);
                }
            }
            Event::Empty(element) => {
                if element.local_name().as_ref() == b"encryption-data" {
                    return Err(OdsFailure::UnsupportedEncrypted);
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(OdsFailure::Extract("extract_error"));
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(OdsFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(OdsFailure::Extract("extract_error"));
    }
    Ok(())
}

/// Count the logical ODS dimensions represented by row/column repeat
/// attributes. This conservative physical-area bound mirrors the range that
/// calamine may materialize and rejects repeat bombs before parser allocation.
fn scan_ods_content(xml: &[u8], started: Instant) -> Result<(), OdsFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut sheet_count = 0usize;
    let mut logical_cells = 0usize;
    let mut source_chars = 0usize;
    let mut expanded_text_chars = 0usize;
    let mut estimated_memory_bytes = 0usize;
    let mut events = 0usize;
    let mut in_table = false;
    let mut in_row = false;
    let mut in_cell = false;
    let mut table_rows = 0u64;
    let mut row_width = 0u64;
    let mut row_repeats = 1u64;
    let mut row_value_chars = 0usize;
    let mut cell_repeats = 1u64;
    let mut cell_value_chars = 0usize;

    loop {
        if timed_out(started) {
            return Err(OdsFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| OdsFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > ODS_MAX_XML_EVENTS {
            return Err(OdsFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > ODS_MAX_XML_DEPTH {
                    return Err(OdsFailure::Extract("xml_limit"));
                }
                ods_count_attribute_chars(&element, &mut source_chars)?;
                match element.local_name().as_ref() {
                    b"table" => {
                        if in_table {
                            return Err(OdsFailure::Extract("extract_error"));
                        }
                        sheet_count = sheet_count.saturating_add(1);
                        if sheet_count > ODS_MAX_SHEETS {
                            return Err(OdsFailure::Extract("sheet_limit"));
                        }
                        in_table = true;
                        table_rows = 0;
                    }
                    b"table-row" if in_table => {
                        if in_row {
                            return Err(OdsFailure::Extract("extract_error"));
                        }
                        row_repeats = ods_repeat_attribute(
                            &element,
                            b"number-rows-repeated",
                            ODS_MAX_ROWS as u64,
                            "row_limit",
                        )?;
                        in_row = true;
                        row_width = 0;
                        row_value_chars = 0;
                    }
                    b"table-cell" | b"covered-table-cell" if in_row => {
                        if in_cell {
                            return Err(OdsFailure::Extract("extract_error"));
                        }
                        let repeats = ods_repeat_attribute(
                            &element,
                            b"number-columns-repeated",
                            ODS_MAX_COLUMNS as u64,
                            "column_limit",
                        )?;
                        row_width = row_width.saturating_add(repeats);
                        if row_width > ODS_MAX_COLUMNS as u64 {
                            return Err(OdsFailure::Extract("column_limit"));
                        }
                        in_cell = true;
                        cell_repeats = repeats;
                        cell_value_chars = ods_cell_value_chars(&element)?;
                    }
                    b"s" => {
                        let spaces = ods_repeat_attribute(
                            &element,
                            b"c",
                            ODS_MAX_SOURCE_TEXT_CHARS as u64,
                            "text_limit",
                        )?;
                        if in_cell {
                            cell_value_chars = cell_value_chars.saturating_add(spaces as usize);
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(element) => {
                ods_count_attribute_chars(&element, &mut source_chars)?;
                match element.local_name().as_ref() {
                    b"table" => {
                        sheet_count = sheet_count.saturating_add(1);
                        if sheet_count > ODS_MAX_SHEETS {
                            return Err(OdsFailure::Extract("sheet_limit"));
                        }
                    }
                    b"table-row" if in_table => {
                        let repeats = ods_repeat_attribute(
                            &element,
                            b"number-rows-repeated",
                            ODS_MAX_ROWS as u64,
                            "row_limit",
                        )?;
                        if repeats.saturating_mul(ODS_MAX_COLUMNS as u64) > ODS_MAX_CELLS as u64 {
                            return Err(OdsFailure::Extract("cell_limit"));
                        }
                    }
                    b"table-cell" | b"covered-table-cell" if in_row => {
                        let repeats = ods_repeat_attribute(
                            &element,
                            b"number-columns-repeated",
                            ODS_MAX_COLUMNS as u64,
                            "column_limit",
                        )?;
                        row_width = row_width.saturating_add(repeats);
                        if row_width > ODS_MAX_COLUMNS as u64 {
                            return Err(OdsFailure::Extract("column_limit"));
                        }
                        let value_chars = ods_cell_value_chars(&element)?;
                        row_value_chars = row_value_chars.saturating_add(
                            value_chars.saturating_mul(repeats.min(usize::MAX as u64) as usize),
                        );
                        if row_value_chars > ODS_MAX_EXPANDED_TEXT_CHARS {
                            return Err(OdsFailure::Extract("resource_limit"));
                        }
                    }
                    b"s" => {
                        let spaces = ods_repeat_attribute(
                            &element,
                            b"c",
                            ODS_MAX_SOURCE_TEXT_CHARS as u64,
                            "text_limit",
                        )?;
                        if in_cell {
                            cell_value_chars = cell_value_chars.saturating_add(spaces as usize);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| OdsFailure::UnsupportedEncoding)?;
                let value_chars = value.chars().count();
                source_chars = source_chars.saturating_add(value_chars);
                if source_chars > ODS_MAX_SOURCE_TEXT_CHARS {
                    return Err(OdsFailure::Extract("text_limit"));
                }
                if in_cell {
                    cell_value_chars = cell_value_chars.saturating_add(value_chars);
                }
            }
            Event::CData(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| OdsFailure::UnsupportedEncoding)?;
                let value_chars = value.chars().count();
                source_chars = source_chars.saturating_add(value_chars);
                if source_chars > ODS_MAX_SOURCE_TEXT_CHARS {
                    return Err(OdsFailure::Extract("text_limit"));
                }
                if in_cell {
                    cell_value_chars = cell_value_chars.saturating_add(value_chars);
                }
            }
            Event::GeneralRef(_) => {
                source_chars = source_chars.saturating_add(1);
                if source_chars > ODS_MAX_SOURCE_TEXT_CHARS {
                    return Err(OdsFailure::Extract("text_limit"));
                }
                if in_cell {
                    cell_value_chars = cell_value_chars.saturating_add(1);
                }
            }
            Event::End(element) => {
                if depth == 0 {
                    return Err(OdsFailure::Extract("extract_error"));
                }
                match element.local_name().as_ref() {
                    b"table-cell" | b"covered-table-cell" if in_cell => {
                        row_value_chars = row_value_chars.saturating_add(
                            cell_value_chars
                                .saturating_mul(cell_repeats.min(usize::MAX as u64) as usize),
                        );
                        if row_value_chars > ODS_MAX_EXPANDED_TEXT_CHARS {
                            return Err(OdsFailure::Extract("resource_limit"));
                        }
                        in_cell = false;
                        cell_repeats = 1;
                        cell_value_chars = 0;
                    }
                    b"table-row" if in_row => {
                        if in_cell {
                            return Err(OdsFailure::Extract("extract_error"));
                        }
                        let cells = row_repeats.saturating_mul(row_width);
                        table_rows = table_rows.saturating_add(row_repeats);
                        if table_rows > ODS_MAX_ROWS as u64 {
                            return Err(OdsFailure::Extract("row_limit"));
                        }
                        logical_cells =
                            logical_cells.saturating_add(cells.min(usize::MAX as u64) as usize);
                        if logical_cells > ODS_MAX_CELLS {
                            return Err(OdsFailure::Extract("cell_limit"));
                        }

                        let repeated_rows = row_repeats.min(usize::MAX as u64) as usize;
                        let expanded_row_chars = row_value_chars.saturating_mul(repeated_rows);
                        expanded_text_chars =
                            expanded_text_chars.saturating_add(expanded_row_chars);
                        if expanded_text_chars > ODS_MAX_EXPANDED_TEXT_CHARS {
                            return Err(OdsFailure::Extract("resource_limit"));
                        }

                        // calamine first builds one row-oriented Data/formula
                        // vector and then materializes each into a dense range.
                        // During each rebuild the old and new vectors overlap,
                        // so account for two Data and two String slots per
                        // logical cell rather than only the final ranges.
                        let parser_bytes_per_cell = std::mem::size_of::<Data>()
                            .saturating_mul(2)
                            .saturating_add(std::mem::size_of::<String>().saturating_mul(2));
                        let cell_storage_bytes = (cells.min(usize::MAX as u64) as usize)
                            .saturating_mul(parser_bytes_per_cell);
                        estimated_memory_bytes = estimated_memory_bytes
                            .saturating_add(cell_storage_bytes)
                            .saturating_add(expanded_row_chars.saturating_mul(4));
                        if estimated_memory_bytes > ODS_MAX_ESTIMATED_MEMORY_BYTES {
                            return Err(OdsFailure::Extract("resource_limit"));
                        }

                        in_row = false;
                        row_width = 0;
                        row_repeats = 1;
                        row_value_chars = 0;
                    }
                    b"table" if in_table => {
                        if in_row || in_cell {
                            return Err(OdsFailure::Extract("extract_error"));
                        }
                        in_table = false;
                        table_rows = 0;
                    }
                    _ => {}
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(OdsFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || in_table || in_row || in_cell {
        return Err(OdsFailure::Extract("extract_error"));
    }
    Ok(())
}

const ODS_MAX_SOURCE_TEXT_CHARS: usize = 8_000_000;

fn ods_cell_value_chars(element: &quick_xml::events::BytesStart<'_>) -> Result<usize, OdsFailure> {
    let mut value_chars = 0usize;
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| OdsFailure::Extract("extract_error"))?;
        let key = attribute.key.as_ref();
        let local_key = key.rsplit(|byte| *byte == b':').next().unwrap_or(key);
        if matches!(
            local_key,
            b"value"
                | b"string-value"
                | b"date-value"
                | b"time-value"
                | b"boolean-value"
                | b"formula"
        ) {
            value_chars = value_chars.saturating_add(attribute.value.len());
            if value_chars > ODS_MAX_EXPANDED_TEXT_CHARS {
                return Err(OdsFailure::Extract("resource_limit"));
            }
        }
    }
    Ok(value_chars)
}

fn ods_count_attribute_chars(
    element: &quick_xml::events::BytesStart<'_>,
    source_chars: &mut usize,
) -> Result<(), OdsFailure> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| OdsFailure::Extract("extract_error"))?;
        *source_chars = source_chars.saturating_add(attribute.value.len());
        if *source_chars > ODS_MAX_SOURCE_TEXT_CHARS {
            return Err(OdsFailure::Extract("text_limit"));
        }
    }
    Ok(())
}

fn ods_repeat_attribute(
    element: &quick_xml::events::BytesStart<'_>,
    wanted: &[u8],
    max: u64,
    limit_code: &'static str,
) -> Result<u64, OdsFailure> {
    let Some(value) = ods_attribute_value(element, wanted)? else {
        return Ok(1);
    };
    let value = std::str::from_utf8(&value).map_err(|_| OdsFailure::UnsupportedEncoding)?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| OdsFailure::Extract("extract_error"))?;
    if parsed == 0 {
        return Err(OdsFailure::Extract("extract_error"));
    }
    if parsed > max {
        return Err(OdsFailure::Extract(limit_code));
    }
    Ok(parsed)
}

fn ods_attribute_value(
    element: &quick_xml::events::BytesStart<'_>,
    wanted: &[u8],
) -> Result<Option<Vec<u8>>, OdsFailure> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| OdsFailure::Extract("extract_error"))?;
        let key = attribute.key.as_ref();
        let key = key.rsplit(|byte| *byte == b':').next().unwrap_or(key);
        if key == wanted {
            return Ok(Some(attribute.value.into_owned()));
        }
    }
    Ok(None)
}

fn xlsx_cell_value(value: &DataRef<'_>) -> Option<String> {
    let value = match value {
        DataRef::Empty => None,
        DataRef::Int(value) => Some(value.to_string()),
        DataRef::Float(value) => Some(value.to_string()),
        DataRef::String(value) => Some(value.clone()),
        DataRef::SharedString(value) => Some((*value).to_string()),
        DataRef::Bool(value) => Some(value.to_string()),
        DataRef::DateTime(value) => Some(value.to_string()),
        DataRef::DateTimeIso(value) => Some(value.clone()),
        DataRef::DurationIso(value) => Some(value.clone()),
        DataRef::Error(value) => Some(value.to_string()),
    }?;
    (!value.is_empty()).then_some(value)
}

fn xlsx_dimension_cell_count(dimensions: Dimensions) -> Result<usize, XlsxFailure> {
    if dimensions.end < dimensions.start {
        return Err(XlsxFailure::Extract("cell_limit"));
    }
    if dimensions.end.0 >= XLSX_MAX_ROWS {
        return Err(XlsxFailure::Extract("row_limit"));
    }
    if dimensions.end.1 >= XLSX_MAX_COLUMNS {
        return Err(XlsxFailure::Extract("column_limit"));
    }
    let rows = dimensions
        .end
        .0
        .saturating_sub(dimensions.start.0)
        .saturating_add(1) as usize;
    let columns = dimensions
        .end
        .1
        .saturating_sub(dimensions.start.1)
        .saturating_add(1) as usize;
    let cells = rows.saturating_mul(columns);
    if cells > XLSX_MAX_CELLS {
        return Err(XlsxFailure::Extract("cell_limit"));
    }
    Ok(cells)
}

/// Inspect the ZIP central directory and the bounded XML metadata needed by
/// calamine before constructing its workbook. In particular, calamine trusts
/// `sharedStrings.xml`'s `uniqueCount` for a `Vec::reserve`, so the declaration
/// must be checked before `Xlsx::new` can see the bytes. No archive entry is
/// opened as a path and no external relationship is followed.
fn xlsx_preflight(bytes: &[u8], started: Instant) -> Result<(), XlsxFailure> {
    validate_zip_envelope(bytes, XLSX_MAX_ZIP_ENTRIES).map_err(|failure| match failure {
        ZipEnvelopeFailure::Invalid => XlsxFailure::Extract("extract_error"),
        ZipEnvelopeFailure::EntryLimit => XlsxFailure::Extract("zip_limit"),
    })?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| XlsxFailure::Extract("extract_error"))?;
    if archive.len() > XLSX_MAX_ZIP_ENTRIES {
        return Err(XlsxFailure::Extract("zip_limit"));
    }

    let mut total_uncompressed = 0u64;
    let mut seen_names = HashSet::with_capacity(archive.len());
    let mut package_relationships_entry = None;
    let mut workbook_entry = None;
    let mut workbook_relationships_entry = None;
    let mut styles_entry = None;
    let mut shared_string_entries = Vec::new();
    let mut worksheet_entries = Vec::new();
    for index in 0..archive.len() {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        // `by_index` refuses to open an encrypted entry before exposing its
        // metadata. The raw view is metadata-only and lets us map that case
        // to the fixed unsupported-encrypted status without attempting to
        // decrypt or inflate attacker-controlled bytes.
        let file = archive
            .by_index_raw(index)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        if file.encrypted() {
            return Err(XlsxFailure::UnsupportedEncrypted);
        }
        if file.name_raw().contains(&0) || file.enclosed_name().is_none() {
            return Err(XlsxFailure::Extract("zip_path"));
        }
        let size = file.size();
        if size > XLSX_MAX_ZIP_ENTRY_BYTES {
            return Err(XlsxFailure::Extract("zip_limit"));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > XLSX_MAX_ZIP_UNCOMPRESSED_BYTES {
            return Err(XlsxFailure::Extract("zip_limit"));
        }
        let name = file.name().replace('\\', "/");
        let lower_name = name.to_ascii_lowercase();
        if !seen_names.insert(lower_name.clone()) {
            return Err(XlsxFailure::Extract("zip_path"));
        }
        match lower_name.as_str() {
            "_rels/.rels" => {
                require_xlsx_part_size(size, XLSX_MAX_PACKAGE_RELATIONSHIPS_BYTES)?;
                package_relationships_entry = Some(index);
            }
            "xl/workbook.xml" => {
                require_xlsx_part_size(size, XLSX_MAX_WORKBOOK_XML_BYTES)?;
                workbook_entry = Some(index);
            }
            "xl/_rels/workbook.xml.rels" => {
                require_xlsx_part_size(size, XLSX_MAX_WORKBOOK_RELATIONSHIPS_BYTES)?;
                workbook_relationships_entry = Some(index);
            }
            "xl/styles.xml" => {
                require_xlsx_part_size(size, XLSX_MAX_STYLES_XML_BYTES)?;
                styles_entry = Some(index);
            }
            "xl/sharedstrings.xml" => {
                require_xlsx_part_size(size, XLSX_MAX_SHARED_STRING_XML_BYTES)
                    .map_err(|_| XlsxFailure::Extract("shared_string_limit"))?;
                shared_string_entries.push(index);
            }
            _ if lower_name.starts_with("xl/worksheets/") && lower_name.ends_with(".xml") => {
                worksheet_entries.push(index);
            }
            _ => {}
        }
    }
    let package_relationships_entry =
        package_relationships_entry.ok_or(XlsxFailure::Extract("extract_error"))?;
    let workbook_entry = workbook_entry.ok_or(XlsxFailure::Extract("extract_error"))?;
    let workbook_relationships_entry =
        workbook_relationships_entry.ok_or(XlsxFailure::Extract("extract_error"))?;
    if shared_string_entries.len() > 1 {
        return Err(XlsxFailure::Extract("zip_path"));
    }
    if worksheet_entries.len() > XLSX_MAX_SHEETS {
        return Err(XlsxFailure::Extract("sheet_limit"));
    }

    let package_relationships = read_xlsx_part(
        &mut archive,
        package_relationships_entry,
        started,
        XLSX_MAX_PACKAGE_RELATIONSHIPS_BYTES,
    )?;
    scan_xlsx_relationships(&package_relationships, true, started)?;

    let workbook_relationships = read_xlsx_part(
        &mut archive,
        workbook_relationships_entry,
        started,
        XLSX_MAX_WORKBOOK_RELATIONSHIPS_BYTES,
    )?;
    scan_xlsx_relationships(&workbook_relationships, false, started)?;

    let workbook_xml = read_xlsx_part(
        &mut archive,
        workbook_entry,
        started,
        XLSX_MAX_WORKBOOK_XML_BYTES,
    )?;
    let workbook_sheets = scan_xlsx_xml_limits(&workbook_xml, Some(b"sheet"), started)?;
    if workbook_sheets > XLSX_MAX_SHEETS {
        return Err(XlsxFailure::Extract("sheet_limit"));
    }
    if let Some(index) = styles_entry {
        let styles = read_xlsx_part(&mut archive, index, started, XLSX_MAX_STYLES_XML_BYTES)?;
        scan_xlsx_xml_limits(&styles, None, started)?;
    }

    let mut shared_strings = 0usize;
    let mut shared_string_chars = 0usize;
    for index in shared_string_entries {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        let file = archive
            .by_index(index)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        let xml = read_zip_entry(file, started, XLSX_MAX_SHARED_STRING_XML_BYTES)?;
        let (count, chars) = scan_shared_strings(&xml, started)?;
        shared_strings = shared_strings.saturating_add(count);
        shared_string_chars = shared_string_chars.saturating_add(chars);
        if shared_strings > XLSX_MAX_SHARED_STRINGS
            || shared_string_chars > XLSX_MAX_SHARED_STRING_CHARS
        {
            return Err(XlsxFailure::Extract("shared_string_limit"));
        }
    }

    let mut logical_cells = 0usize;
    for index in worksheet_entries {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        let file = archive
            .by_index(index)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        let xml = read_zip_entry(file, started, XLSX_MAX_ZIP_ENTRY_BYTES)?;
        let sheet_cells = scan_worksheet_xml(&xml, started)?;
        logical_cells = logical_cells.saturating_add(sheet_cells);
        if logical_cells > XLSX_MAX_CELLS {
            return Err(XlsxFailure::Extract("cell_limit"));
        }
    }
    Ok(())
}

fn require_xlsx_part_size(size: u64, limit: u64) -> Result<(), XlsxFailure> {
    if size > limit {
        Err(XlsxFailure::Extract("zip_limit"))
    } else {
        Ok(())
    }
}

fn read_xlsx_part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
    started: Instant,
    max_bytes: u64,
) -> Result<Vec<u8>, XlsxFailure> {
    let file = archive
        .by_index(index)
        .map_err(|_| XlsxFailure::Extract("extract_error"))?;
    read_zip_entry(file, started, max_bytes)
}

fn read_zip_entry<R: Read>(
    mut reader: R,
    started: Instant,
    max_bytes: u64,
) -> Result<Vec<u8>, XlsxFailure> {
    let mut output = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        let read = reader
            .read(&mut chunk)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        if read == 0 {
            break;
        }
        if output.len().saturating_add(read) as u64 > max_bytes {
            return Err(XlsxFailure::Extract("zip_limit"));
        }
        output.extend_from_slice(&chunk[..read]);
    }
    Ok(output)
}

fn scan_xlsx_relationships(
    xml: &[u8],
    package_root: bool,
    started: Instant,
) -> Result<(), XlsxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut relationships = 0usize;
    let mut office_documents = 0usize;
    let mut ids = HashSet::new();
    loop {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > XLSX_MAX_XML_EVENTS {
            return Err(XlsxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > XLSX_MAX_XML_DEPTH {
                    return Err(XlsxFailure::Extract("xml_limit"));
                }
                if element.local_name().as_ref() == b"Relationship" {
                    relationships = relationships.saturating_add(1);
                    if relationships > XLSX_MAX_RELATIONSHIPS {
                        return Err(XlsxFailure::Extract("xml_limit"));
                    }
                    let id = attribute_value(&element, b"Id")?
                        .ok_or(XlsxFailure::Extract("extract_error"))?;
                    if id.is_empty() || !ids.insert(id) {
                        return Err(XlsxFailure::Extract("extract_error"));
                    }
                    let relation_type = attribute_value(&element, b"Type")?
                        .ok_or(XlsxFailure::Extract("extract_error"))?;
                    let target = attribute_value(&element, b"Target")?
                        .ok_or(XlsxFailure::Extract("extract_error"))?;
                    if attribute_value(&element, b"TargetMode")?
                        .is_some_and(|mode| !mode.as_slice().eq_ignore_ascii_case(b"internal"))
                    {
                        return Err(XlsxFailure::Extract("external_relationship"));
                    }
                    if !xlsx_relationship_target_is_safe(&target) {
                        return Err(XlsxFailure::Extract("external_relationship"));
                    }
                    if package_root && relation_type.ends_with(b"/relationships/officeDocument") {
                        office_documents = office_documents.saturating_add(1);
                        let normalized = target.strip_prefix(b"/").unwrap_or(&target);
                        if normalized != b"xl/workbook.xml" || office_documents > 1 {
                            return Err(XlsxFailure::Extract("external_relationship"));
                        }
                    }
                }
            }
            Event::End(_) => {
                if depth == 0 {
                    return Err(XlsxFailure::Extract("extract_error"));
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(XlsxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || (package_root && office_documents != 1) {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    Ok(())
}

fn xlsx_relationship_target_is_safe(target: &[u8]) -> bool {
    let target = target.strip_prefix(b"/").unwrap_or(target);
    !target.is_empty()
        && !target.iter().any(|byte| {
            byte.is_ascii_control() || matches!(*byte, b'\\' | b':' | b'?' | b'#' | b'%')
        })
        && target
            .split(|byte| *byte == b'/')
            .all(|part| !part.is_empty() && part != b"." && part != b"..")
}

fn scan_xlsx_xml_limits(
    xml: &[u8],
    counted_element: Option<&[u8]>,
    started: Instant,
) -> Result<usize, XlsxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut events = 0usize;
    let mut text_chars = 0usize;
    let mut counted = 0usize;
    loop {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > XLSX_MAX_XML_EVENTS {
            return Err(XlsxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > XLSX_MAX_XML_DEPTH {
                    return Err(XlsxFailure::Extract("xml_limit"));
                }
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|_| XlsxFailure::Extract("extract_error"))?;
                    text_chars = text_chars.saturating_add(attribute.value.len());
                }
                if counted_element.is_some_and(|wanted| element.local_name().as_ref() == wanted) {
                    counted = counted.saturating_add(1);
                }
            }
            Event::Text(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| XlsxFailure::UnsupportedEncoding)?;
                text_chars = text_chars.saturating_add(value.chars().count());
            }
            Event::CData(text) => {
                let value = text
                    .xml10_content()
                    .map_err(|_| XlsxFailure::UnsupportedEncoding)?;
                text_chars = text_chars.saturating_add(value.chars().count());
            }
            Event::GeneralRef(_) => text_chars = text_chars.saturating_add(1),
            Event::End(_) => {
                if depth == 0 {
                    return Err(XlsxFailure::Extract("extract_error"));
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(XlsxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
        if text_chars > XLSX_MAX_XML_TEXT_CHARS {
            return Err(XlsxFailure::Extract("xml_limit"));
        }
    }
    if depth != 0 {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    Ok(counted)
}

fn scan_shared_strings(xml: &[u8], started: Instant) -> Result<(usize, usize), XlsxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut shared_item_depth = 0usize;
    let mut count = 0usize;
    let mut chars = 0usize;
    let mut events = 0usize;
    let mut declared_unique_count = None;

    loop {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > XLSX_MAX_XML_EVENTS {
            return Err(XlsxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > XLSX_MAX_XML_DEPTH {
                    return Err(XlsxFailure::Extract("xml_limit"));
                }
                match element.local_name().as_ref() {
                    b"sst" => {
                        declared_unique_count = parse_unique_count(&element)?;
                    }
                    b"si" => {
                        count = count.saturating_add(1);
                        if count > XLSX_MAX_SHARED_STRINGS {
                            return Err(XlsxFailure::Extract("shared_string_limit"));
                        }
                        shared_item_depth = shared_item_depth.saturating_add(1);
                    }
                    _ => {}
                }
            }
            Event::Empty(element) => {
                if element.local_name().as_ref() == b"si" {
                    count = count.saturating_add(1);
                    if count > XLSX_MAX_SHARED_STRINGS {
                        return Err(XlsxFailure::Extract("shared_string_limit"));
                    }
                }
            }
            Event::Text(text) => {
                if shared_item_depth != 0 {
                    let value = text
                        .xml10_content()
                        .map_err(|_| XlsxFailure::UnsupportedEncoding)?;
                    chars = chars.saturating_add(value.chars().count());
                    if chars > XLSX_MAX_SHARED_STRING_CHARS {
                        return Err(XlsxFailure::Extract("shared_string_limit"));
                    }
                }
            }
            Event::CData(text) => {
                if shared_item_depth != 0 {
                    let value = text
                        .xml10_content()
                        .map_err(|_| XlsxFailure::UnsupportedEncoding)?;
                    chars = chars.saturating_add(value.chars().count());
                    if chars > XLSX_MAX_SHARED_STRING_CHARS {
                        return Err(XlsxFailure::Extract("shared_string_limit"));
                    }
                }
            }
            Event::End(element) => {
                if depth == 0 {
                    return Err(XlsxFailure::Extract("extract_error"));
                }
                if element.local_name().as_ref() == b"si" && shared_item_depth != 0 {
                    shared_item_depth -= 1;
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(XlsxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || shared_item_depth != 0 {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    if declared_unique_count.is_some_and(|declared| declared > XLSX_MAX_SHARED_STRINGS) {
        return Err(XlsxFailure::Extract("shared_string_limit"));
    }
    Ok((count, chars))
}

fn parse_unique_count(
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<Option<usize>, XlsxFailure> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| XlsxFailure::Extract("extract_error"))?;
        if attribute.key.as_ref().eq_ignore_ascii_case(b"uniqueCount") {
            let value = std::str::from_utf8(attribute.value.as_ref())
                .map_err(|_| XlsxFailure::UnsupportedEncoding)?;
            let count = value
                .parse::<usize>()
                .map_err(|_| XlsxFailure::Extract("extract_error"))?;
            return Ok(Some(count));
        }
    }
    Ok(None)
}

fn scan_worksheet_xml(xml: &[u8], started: Instant) -> Result<usize, XlsxFailure> {
    let mut reader = XmlReader::from_reader(Cursor::new(xml));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::with_capacity(1024);
    let mut depth = 0usize;
    let mut sheet_data_depth = 0usize;
    let mut current_row = 0u32;
    let mut current_column = 0u32;
    let mut row_records = 0usize;
    let mut cells = 0usize;
    let mut declared_cells = None;
    let mut events = 0usize;

    loop {
        if timed_out(started) {
            return Err(XlsxFailure::Timeout);
        }
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| XlsxFailure::Extract("extract_error"))?;
        events = events.saturating_add(1);
        if events > XLSX_MAX_XML_EVENTS {
            return Err(XlsxFailure::Extract("xml_limit"));
        }
        match event {
            Event::Start(element) => {
                depth = depth.saturating_add(1);
                if depth > XLSX_MAX_XML_DEPTH {
                    return Err(XlsxFailure::Extract("xml_limit"));
                }
                match element.local_name().as_ref() {
                    b"sheetData" => sheet_data_depth = sheet_data_depth.saturating_add(1),
                    b"dimension" => {
                        if let Some(reference) = attribute_value(&element, b"ref")? {
                            declared_cells = Some(parse_xlsx_dimension(&reference)?);
                        }
                    }
                    b"row" if sheet_data_depth != 0 => {
                        row_records = row_records.saturating_add(1);
                        if row_records > XLSX_MAX_ROWS as usize {
                            return Err(XlsxFailure::Extract("row_limit"));
                        }
                        current_column = 0;
                        current_row = parse_row_attribute(&element)?.unwrap_or(current_row);
                        if current_row >= XLSX_MAX_ROWS {
                            return Err(XlsxFailure::Extract("row_limit"));
                        }
                    }
                    b"c" if sheet_data_depth != 0 => {
                        cells = cells.saturating_add(1);
                        if cells > XLSX_MAX_CELLS {
                            return Err(XlsxFailure::Extract("cell_limit"));
                        }
                        let (row, column) = match attribute_value(&element, b"r")? {
                            Some(reference) => parse_cell_reference(&reference)?,
                            None => (current_row, current_column),
                        };
                        if row >= XLSX_MAX_ROWS {
                            return Err(XlsxFailure::Extract("row_limit"));
                        }
                        if column >= XLSX_MAX_COLUMNS {
                            return Err(XlsxFailure::Extract("column_limit"));
                        }
                        current_row = row;
                        current_column = column.saturating_add(1);
                    }
                    _ => {}
                }
            }
            Event::Empty(element) => match element.local_name().as_ref() {
                b"dimension" => {
                    if let Some(reference) = attribute_value(&element, b"ref")? {
                        declared_cells = Some(parse_xlsx_dimension(&reference)?);
                    }
                }
                b"row" if sheet_data_depth != 0 => {
                    row_records = row_records.saturating_add(1);
                    if row_records > XLSX_MAX_ROWS as usize {
                        return Err(XlsxFailure::Extract("row_limit"));
                    }
                }
                b"c" if sheet_data_depth != 0 => {
                    cells = cells.saturating_add(1);
                    if cells > XLSX_MAX_CELLS {
                        return Err(XlsxFailure::Extract("cell_limit"));
                    }
                    let (row, column) = match attribute_value(&element, b"r")? {
                        Some(reference) => parse_cell_reference(&reference)?,
                        None => (current_row, current_column),
                    };
                    if row >= XLSX_MAX_ROWS {
                        return Err(XlsxFailure::Extract("row_limit"));
                    }
                    if column >= XLSX_MAX_COLUMNS {
                        return Err(XlsxFailure::Extract("column_limit"));
                    }
                    current_column = column.saturating_add(1);
                }
                _ => {}
            },
            Event::End(element) => {
                if depth == 0 {
                    return Err(XlsxFailure::Extract("extract_error"));
                }
                if element.local_name().as_ref() == b"sheetData" && sheet_data_depth != 0 {
                    sheet_data_depth -= 1;
                }
                depth -= 1;
            }
            Event::DocType(_) => return Err(XlsxFailure::Extract("external_relationship")),
            Event::Eof => break,
            _ => {}
        }
    }
    if depth != 0 || sheet_data_depth != 0 {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    let logical_cells = declared_cells.unwrap_or(cells);
    if logical_cells > XLSX_MAX_CELLS {
        return Err(XlsxFailure::Extract("cell_limit"));
    }
    Ok(logical_cells)
}

fn attribute_value(
    element: &quick_xml::events::BytesStart<'_>,
    wanted: &[u8],
) -> Result<Option<Vec<u8>>, XlsxFailure> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| XlsxFailure::Extract("extract_error"))?;
        let key = attribute.key.as_ref();
        let key = key.rsplit(|byte| *byte == b':').next().unwrap_or(key);
        if key == wanted {
            return Ok(Some(attribute.value.into_owned()));
        }
    }
    Ok(None)
}

fn parse_row_attribute(
    element: &quick_xml::events::BytesStart<'_>,
) -> Result<Option<u32>, XlsxFailure> {
    let Some(value) = attribute_value(element, b"r")? else {
        return Ok(None);
    };
    let row = std::str::from_utf8(&value)
        .map_err(|_| XlsxFailure::UnsupportedEncoding)?
        .parse::<u32>()
        .map_err(|_| XlsxFailure::Extract("extract_error"))?;
    let row = row
        .checked_sub(1)
        .ok_or(XlsxFailure::Extract("row_limit"))?;
    Ok(Some(row))
}

fn parse_xlsx_dimension(reference: &[u8]) -> Result<usize, XlsxFailure> {
    let mut parts = reference.split(|byte| *byte == b':');
    let start = parse_cell_reference(parts.next().ok_or(XlsxFailure::Extract("extract_error"))?)?;
    let end = match parts.next() {
        Some(end) => parse_cell_reference(end)?,
        None => start,
    };
    if parts.next().is_some() || end < start {
        return Err(XlsxFailure::Extract("cell_limit"));
    }
    if end.0 >= XLSX_MAX_ROWS {
        return Err(XlsxFailure::Extract("row_limit"));
    }
    if end.1 >= XLSX_MAX_COLUMNS {
        return Err(XlsxFailure::Extract("column_limit"));
    }
    let rows = end.0.saturating_sub(start.0).saturating_add(1) as usize;
    let columns = end.1.saturating_sub(start.1).saturating_add(1) as usize;
    let cells = rows.saturating_mul(columns);
    if cells > XLSX_MAX_CELLS {
        return Err(XlsxFailure::Extract("cell_limit"));
    }
    Ok(cells)
}

fn parse_cell_reference(reference: &[u8]) -> Result<(u32, u32), XlsxFailure> {
    let mut index = 0usize;
    let mut column = 0u32;
    while index < reference.len() && reference[index].is_ascii_alphabetic() {
        let upper = reference[index].to_ascii_uppercase();
        column = column
            .checked_mul(26)
            .and_then(|value| value.checked_add(u32::from(upper - b'A' + 1)))
            .ok_or(XlsxFailure::Extract("column_limit"))?;
        index += 1;
    }
    if index == 0 || column == 0 {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    let row_start = index;
    let mut row = 0u32;
    while index < reference.len() && reference[index].is_ascii_digit() {
        row = row
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(reference[index] - b'0')))
            .ok_or(XlsxFailure::Extract("row_limit"))?;
        index += 1;
    }
    if index == row_start || index != reference.len() || row == 0 {
        return Err(XlsxFailure::Extract("extract_error"));
    }
    Ok((
        row.checked_sub(1)
            .ok_or(XlsxFailure::Extract("row_limit"))?,
        column
            .checked_sub(1)
            .ok_or(XlsxFailure::Extract("column_limit"))?,
    ))
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
pub(crate) fn xlsx_test_fixture_with(first_shared: &str) -> Vec<u8> {
    tests::xlsx_fixture_with(first_shared, "0000003", "A1:B2")
}

#[cfg(test)]
pub(crate) fn docx_test_fixture_with(text: &str) -> Vec<u8> {
    tests::docx_fixture_with_text(text)
}

#[cfg(test)]
pub(crate) fn ods_test_fixture_with(first_value: &str) -> Vec<u8> {
    tests::ods_fixture(first_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{
        dictionary, Document, EncryptionState, EncryptionVersion, Object, Permissions, Stream,
    };
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

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

    fn office_encrypted_bytes() -> Vec<u8> {
        let mut container = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        {
            let mut stream = container.create_stream("/EncryptedPackage").unwrap();
            stream.write_all(b"encrypted fixture").unwrap();
        }
        container.into_inner().into_inner()
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
    }

    #[test]
    fn recognizes_only_non_macro_docx_candidates_case_insensitively() {
        assert!(is_docx_ext("docx"));
        assert!(is_docx_ext(".DOCX"));
        assert!(is_docx_path(Path::new("report.DoCx")));
        assert!(is_content_candidate(Path::new("report.docx")));
        assert!(!is_docx_ext("doc"));
        assert!(!is_docx_ext("docm"));
        assert!(!is_content_candidate(Path::new("report.doc")));
        assert!(!is_content_candidate(Path::new("report.docm")));
    }

    #[test]
    fn recognizes_legacy_xls_candidates_without_treating_modern_formats_as_xls() {
        assert!(is_xls_ext("xls"));
        assert!(is_xls_ext(".XLS"));
        assert!(is_xls_path(Path::new("report.XlS")));
        assert!(is_content_candidate(Path::new("report.xls")));
        assert!(!is_xls_path(Path::new("report.xlsx")));
        assert!(is_xlsx_ext("xlsx"));
        assert!(is_xlsx_ext(".XLSX"));
        assert!(is_xlsx_path(Path::new("report.XlSx")));
        assert!(is_content_candidate(Path::new("report.xlsx")));
        assert!(!is_xlsx_ext("xlsm"));
        assert!(!is_content_candidate(Path::new("report.xlsm")));
        assert!(is_ods_ext("ods"));
        assert!(is_ods_ext(".ODS"));
        assert!(is_ods_path(Path::new("report.OdS")));
        assert!(is_content_candidate(Path::new("report.ods")));
        assert!(!is_ods_ext("odt"));
    }

    #[test]
    fn extracts_docx_korean_text_and_wordprocessingml_structure() {
        let document = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
  <w:p><w:r><w:t>안녕하세요 &amp; hello &#x1F642;</w:t><w:tab/><w:t>탭</w:t><w:br/><w:t>다음 줄</w:t></w:r></w:p>
  <w:p><w:r><w:instrText>DO_NOT_INDEX_FIELD_CODE</w:instrText><w:t>두 번째 문단</w:t></w:r></w:p>
</w:body></w:document>"#;
        let record = extract_docx_bytes(&docx_fixture_with_document(document), now());

        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.extractor_version, DOCX_EXTRACTOR_VERSION);
        assert_eq!(record.encoding, Some("docx"));
        assert_eq!(record.error_code, None);
        assert_eq!(
            record.text,
            "안녕하세요 & hello 🙂\t탭\n다음 줄\n두 번째 문단"
        );
        assert!(!record.text.contains("DO_NOT_INDEX_FIELD_CODE"));
    }

    #[test]
    fn isolates_empty_corrupt_oversized_timed_out_and_encrypted_docx_inputs() {
        let empty = extract_docx_bytes(&docx_fixture_with_text("   "), now());
        assert_eq!(empty.status, ContentStatus::NoText);
        assert_eq!(empty.error_code, Some("no_text"));

        let corrupt = extract_docx_bytes(b"not a DOCX package", now());
        assert_eq!(corrupt.status, ContentStatus::ExtractError);
        assert_eq!(corrupt.error_code, Some("extract_error"));
        assert!(corrupt.text.is_empty());

        let oversized = extract_docx_bytes(&vec![b'x'; MAX_FILE_BYTES as usize + 1], now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.error_code, Some("file_too_large"));

        let timeout = extract_docx_bytes(
            &docx_fixture_with_text("timeout"),
            Instant::now() - PROCESSING_LIMIT,
        );
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));

        let encrypted = extract_docx_bytes(
            &mark_zip_entry_encrypted(docx_fixture_with_text("protected")),
            now(),
        );
        assert_eq!(encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(encrypted.error_code, Some("unsupported_encrypted"));

        let office_encrypted = extract_docx_bytes(&office_encrypted_bytes(), now());
        assert_eq!(office_encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(office_encrypted.error_code, Some("unsupported_encrypted"));
    }

    #[test]
    fn indexes_docx_hyperlink_labels_without_following_external_targets() {
        let record = extract_docx_bytes(&docx_fixture_with_external_hyperlink(), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.text, "링크 레이블");
        assert!(!record.text.contains("example.invalid"));
    }

    #[test]
    fn rejects_docx_external_paths_macros_duplicate_parts_and_xml_expansion() {
        let external = extract_docx_bytes(
            &docx_fixture_with_package_target(
                "https://example.invalid/document.xml",
                Some("External"),
            ),
            now(),
        );
        assert_eq!(external.status, ContentStatus::ExtractError);
        assert_eq!(external.error_code, Some("external_relationship"));

        let unsafe_internal = extract_docx_bytes(
            &docx_fixture_with_package_target("../word/document.xml", None),
            now(),
        );
        assert_eq!(unsafe_internal.status, ContentStatus::ExtractError);
        assert_eq!(unsafe_internal.error_code, Some("external_relationship"));

        let escaping_document_target = extract_docx_bytes(
            &docx_fixture_with_document_relationship_target("../../outside.xml"),
            now(),
        );
        assert_eq!(escaping_document_target.status, ContentStatus::ExtractError);
        assert_eq!(
            escaping_document_target.error_code,
            Some("external_relationship")
        );

        let macro_enabled = extract_docx_bytes(
            &docx_fixture_with_content_type(
                "application/vnd.ms-word.document.macroEnabled.main+xml",
            ),
            now(),
        );
        assert_eq!(macro_enabled.status, ContentStatus::ExtractError);
        assert_eq!(macro_enabled.error_code, Some("unsupported_document"));

        let duplicate = extract_docx_bytes(&docx_fixture_with_duplicate_canonical_part(), now());
        assert_eq!(duplicate.status, ContentStatus::ExtractError);
        assert_eq!(duplicate.error_code, Some("zip_path"));

        let deep = extract_docx_bytes(&docx_fixture_with_depth(DOCX_MAX_XML_DEPTH + 1), now());
        assert_eq!(deep.status, ContentStatus::ExtractError);
        assert_eq!(deep.error_code, Some("xml_limit"));

        let doctype = extract_docx_bytes(&docx_fixture_with_doctype(), now());
        assert_eq!(doctype.status, ContentStatus::ExtractError);
        assert_eq!(doctype.error_code, Some("external_relationship"));

        let missing_body = extract_docx_bytes(
            &docx_fixture_with_document(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:t>outside body</w:t></w:document>"#,
            ),
            now(),
        );
        assert_eq!(missing_body.status, ContentStatus::ExtractError);
        assert_eq!(missing_body.error_code, Some("extract_error"));

        let invalid_control = extract_docx_bytes(
            &docx_fixture_with_document(
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>bad\u{1}text</w:t></w:r></w:p></w:body></w:document>",
            ),
            now(),
        );
        assert_eq!(invalid_control.status, ContentStatus::ExtractError);
        assert_eq!(invalid_control.error_code, Some("extract_error"));
    }

    #[test]
    fn bounds_docx_zip_metadata_xml_and_retained_text() {
        let entries = extract_docx_bytes(
            &with_declared_zip_entries(docx_fixture_with_text("bounded"), DOCX_MAX_ZIP_ENTRIES + 1),
            now(),
        );
        assert_eq!(entries.status, ContentStatus::ExtractError);
        assert_eq!(entries.error_code, Some("zip_limit"));

        let entry_size = extract_docx_bytes(
            &with_declared_zip_entry_size(
                docx_fixture_with_text("bounded"),
                DOCX_MAX_ZIP_ENTRY_BYTES + 1,
            ),
            now(),
        );
        assert_eq!(entry_size.status, ContentStatus::ExtractError);
        assert_eq!(entry_size.error_code, Some("zip_limit"));

        let long_text = "가".repeat(MAX_TEXT_CHARS + 1);
        let bounded = extract_docx_bytes(&docx_fixture_with_text(&long_text), now());
        assert_eq!(bounded.status, ContentStatus::Indexed);
        assert!(bounded.truncated);
        assert_eq!(bounded.text_chars, MAX_TEXT_CHARS);
        assert_eq!(bounded.error_code, Some("text_limit"));

        let compressed_source_bomb = extract_docx_bytes(
            &docx_fixture_with_text(&"x".repeat(DOCX_MAX_XML_SOURCE_BUDGET + 1)),
            now(),
        );
        assert_eq!(compressed_source_bomb.status, ContentStatus::ExtractError);
        assert_eq!(compressed_source_bomb.error_code, Some("xml_limit"));

        let oversized = extract_file(Path::new("report.docx"), MAX_FILE_BYTES + 1, now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.extractor_version, DOCX_EXTRACTOR_VERSION);
    }

    #[test]
    fn docx_snippets_redact_credentials_and_sensitive_names_are_not_read() {
        let credential = "Authorization: Bearer document-secret";
        let record = extract_docx_bytes(&docx_fixture_with_text(credential), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        let snippet = redact_snippet(&record.text);
        assert!(!snippet.contains("document-secret"));
        assert!(snippet.contains("[REDACTED]"));

        let sensitive = extract_file(Path::new("credentials.docx"), 0, now());
        assert_eq!(sensitive.status, ContentStatus::SkippedSensitive);
        assert_eq!(sensitive.extractor_version, DOCX_EXTRACTOR_VERSION);
        assert_eq!(sensitive.error_code, Some("sensitive_file"));
    }

    #[test]
    fn extracts_xlsx_shared_strings_and_typed_values_offline() {
        let record = extract_xlsx_bytes(&xlsx_fixture(), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.extractor_version, XLSX_EXTRACTOR_VERSION);
        assert_eq!(record.encoding, Some("xlsx"));
        assert_eq!(record.error_code, None);
        assert!(record.text.contains("shared alpha\tinline gamma\n42\ttrue"));
        assert!(record.text.contains("rich beta\tshared gamma"));
        assert!(!record.text.contains("SUM(40,2)"));
    }

    #[test]
    fn isolates_corrupt_oversized_timed_out_and_encrypted_xlsx_inputs() {
        let corrupt = extract_xlsx_bytes(b"not an XLSX workbook", now());
        assert_eq!(corrupt.status, ContentStatus::ExtractError);
        assert_eq!(corrupt.error_code, Some("extract_error"));
        assert!(corrupt.text.is_empty());

        let oversized = extract_xlsx_bytes(&vec![b'x'; MAX_FILE_BYTES as usize + 1], now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.error_code, Some("file_too_large"));

        let timeout = extract_xlsx_bytes(&xlsx_fixture(), Instant::now() - PROCESSING_LIMIT);
        assert_eq!(timeout.status, ContentStatus::Timeout);
        assert_eq!(timeout.error_code, Some("processing_timeout"));

        let encrypted = extract_xlsx_bytes(&mark_zip_entry_encrypted(xlsx_fixture()), now());
        assert_eq!(encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(encrypted.error_code, Some("unsupported_encrypted"));

        let office_encrypted = extract_xlsx_bytes(&office_encrypted_bytes(), now());
        assert_eq!(office_encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(office_encrypted.error_code, Some("unsupported_encrypted"));
    }

    #[test]
    fn rejects_xlsx_relationship_coordinate_and_package_bounds_before_parsing() {
        let shared_limit = extract_xlsx_bytes(
            &xlsx_fixture_with("shared alpha", "1000001", "A1:B2"),
            now(),
        );
        assert_eq!(shared_limit.status, ContentStatus::ExtractError);
        assert_eq!(shared_limit.error_code, Some("shared_string_limit"));

        let cell_limit = extract_xlsx_bytes(
            &xlsx_fixture_with("shared alpha", "0000003", "A1:XFD1048576"),
            now(),
        );
        assert_eq!(cell_limit.status, ContentStatus::ExtractError);
        assert_eq!(cell_limit.error_code, Some("cell_limit"));

        let external = extract_xlsx_bytes(
            &xlsx_fixture_with_package_target(
                "https://example.invalid/workbook.xml",
                Some("External"),
            ),
            now(),
        );
        assert_eq!(external.status, ContentStatus::ExtractError);
        assert_eq!(external.error_code, Some("external_relationship"));

        let duplicate = extract_xlsx_bytes(&xlsx_fixture_with_duplicate_canonical_part(), now());
        assert_eq!(duplicate.status, ContentStatus::ExtractError);
        assert_eq!(duplicate.error_code, Some("zip_path"));
    }

    #[test]
    fn bounds_xlsx_text_and_preserves_the_format_version_at_file_boundaries() {
        let long_value = "x".repeat(MAX_TEXT_CHARS + 1);
        let bounded =
            extract_xlsx_bytes(&xlsx_fixture_with(&long_value, "0000003", "A1:B2"), now());
        assert_eq!(bounded.status, ContentStatus::Indexed);
        assert!(bounded.truncated);
        assert_eq!(bounded.text_chars, MAX_TEXT_CHARS);
        assert_eq!(bounded.error_code, Some("text_limit"));

        let oversized = extract_file(Path::new("report.xlsx"), MAX_FILE_BYTES + 1, now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.extractor_version, XLSX_EXTRACTOR_VERSION);
    }

    #[test]
    fn extracts_ods_values_without_evaluating_formulas() {
        let record = extract_ods_bytes(&ods_fixture("shared alpha"), now());
        assert_eq!(record.status, ContentStatus::Indexed);
        assert_eq!(record.extractor_version, ODS_EXTRACTOR_VERSION);
        assert_eq!(record.encoding, Some("ods"));
        assert_eq!(record.error_code, None);
        assert!(record.text.contains("shared alpha\trich beta"));
        assert!(record.text.contains("42\ttrue"));
        assert!(record.text.contains("2026-08-27"));
        assert!(!record.text.contains("SUM(A1:A2)"));
    }

    #[test]
    fn isolates_corrupt_timed_out_and_encrypted_ods_inputs() {
        let corrupt = extract_ods_bytes(b"not an ODS workbook", now());
        assert_eq!(corrupt.status, ContentStatus::ExtractError);
        assert_eq!(corrupt.error_code, Some("extract_error"));

        let timeout = extract_ods_bytes(
            &ods_fixture("shared alpha"),
            Instant::now() - PROCESSING_LIMIT,
        );
        assert_eq!(timeout.status, ContentStatus::Timeout);

        let encrypted = extract_ods_bytes(
            &ods_fixture_with_manifest("<manifest:encryption-data/>"),
            now(),
        );
        assert_eq!(encrypted.status, ContentStatus::UnsupportedEncrypted);
        assert_eq!(encrypted.error_code, Some("unsupported_encrypted"));

        let marked_encrypted = extract_ods_bytes(
            &mark_zip_entry_encrypted(ods_fixture("shared alpha")),
            now(),
        );
        assert_eq!(marked_encrypted.status, ContentStatus::UnsupportedEncrypted);
    }

    #[test]
    fn rejects_ods_repeat_and_xml_expansion_before_calamine_allocation() {
        let row_limit =
            extract_ods_bytes(&ods_fixture_with_row_repeat(ODS_MAX_ROWS as u64 + 1), now());
        assert_eq!(row_limit.error_code, Some("row_limit"));

        let column_limit = extract_ods_bytes(
            &ods_fixture_with_column_repeat(ODS_MAX_COLUMNS as u64 + 1),
            now(),
        );
        assert_eq!(column_limit.error_code, Some("column_limit"));

        let cell_limit = extract_ods_bytes(
            &ods_fixture_with_repeats(ODS_MAX_ROWS as u64, ODS_MAX_COLUMNS as u64),
            now(),
        );
        assert_eq!(cell_limit.error_code, Some("cell_limit"));

        let repeated_value = "x".repeat(1_100);
        let clone_bomb = extract_ods_bytes(
            &ods_fixture_with_repeated_value(&repeated_value, 16_000),
            now(),
        );
        assert_eq!(clone_bomb.status, ContentStatus::ExtractError);
        assert_eq!(clone_bomb.error_code, Some("resource_limit"));

        let deep = extract_ods_bytes(&ods_fixture_with_depth(ODS_MAX_XML_DEPTH + 1), now());
        assert_eq!(deep.error_code, Some("xml_limit"));

        let doctype = extract_ods_bytes(&ods_fixture_with_doctype(), now());
        assert_eq!(doctype.error_code, Some("external_relationship"));
    }

    #[test]
    fn rejects_declared_zip_entry_bombs_before_archive_construction() {
        let xlsx = extract_xlsx_bytes(
            &with_declared_zip_entries(xlsx_fixture(), XLSX_MAX_ZIP_ENTRIES + 1),
            now(),
        );
        assert_eq!(xlsx.status, ContentStatus::ExtractError);
        assert_eq!(xlsx.error_code, Some("zip_limit"));

        let ods = extract_ods_bytes(
            &with_declared_zip_entries(ods_fixture("shared alpha"), ODS_MAX_ZIP_ENTRIES + 1),
            now(),
        );
        assert_eq!(ods.status, ContentStatus::ExtractError);
        assert_eq!(ods.error_code, Some("zip_limit"));

        let ambiguous = extract_xlsx_bytes(&with_ambiguous_zip_end_record(xlsx_fixture()), now());
        assert_eq!(ambiguous.status, ContentStatus::ExtractError);
        assert_eq!(ambiguous.error_code, Some("extract_error"));

        let oversized_entry = extract_xlsx_bytes(
            &with_declared_zip_entry_size(xlsx_fixture(), XLSX_MAX_ZIP_ENTRY_BYTES + 1),
            now(),
        );
        assert_eq!(oversized_entry.status, ContentStatus::ExtractError);
        assert_eq!(oversized_entry.error_code, Some("zip_limit"));
    }

    #[test]
    fn spreadsheet_snippets_redact_credentials_and_sensitive_names_are_not_read() {
        let credential = "Authorization: Bearer spreadsheet-secret";
        for record in [
            extract_xlsx_bytes(&xlsx_fixture_with(credential, "0000003", "A1:B2"), now()),
            extract_ods_bytes(&ods_fixture(credential), now()),
        ] {
            assert_eq!(record.status, ContentStatus::Indexed);
            let snippet = redact_snippet(&record.text);
            assert!(!snippet.contains("spreadsheet-secret"));
            assert!(snippet.contains("[REDACTED]"));
        }

        let xlsx = extract_file(Path::new("credentials.xlsx"), 0, now());
        let ods = extract_file(Path::new("credentials.ods"), 0, now());
        assert_eq!(xlsx.status, ContentStatus::SkippedSensitive);
        assert_eq!(xlsx.extractor_version, XLSX_EXTRACTOR_VERSION);
        assert_eq!(ods.status, ContentStatus::SkippedSensitive);
        assert_eq!(ods.extractor_version, ODS_EXTRACTOR_VERSION);
    }

    #[test]
    fn bounds_ods_text_and_preserves_the_format_version_at_file_boundaries() {
        let long_value = "x".repeat(MAX_TEXT_CHARS + 1);
        let bounded = extract_ods_bytes(&ods_fixture(&long_value), now());
        assert_eq!(bounded.status, ContentStatus::Indexed);
        assert!(bounded.truncated);
        assert_eq!(bounded.text_chars, MAX_TEXT_CHARS);

        let oversized = extract_file(Path::new("report.ods"), MAX_FILE_BYTES + 1, now());
        assert_eq!(oversized.status, ContentStatus::TooLarge);
        assert_eq!(oversized.extractor_version, ODS_EXTRACTOR_VERSION);
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

    pub(super) fn docx_fixture_with_text(text: &str) -> Vec<u8> {
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:body></w:document>"#,
            xml_escape(text),
        );
        docx_fixture_with_document(&document)
    }

    fn docx_fixture_with_document(document: &str) -> Vec<u8> {
        build_docx_fixture(
            document,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            "word/document.xml",
            None,
        )
    }

    fn docx_fixture_with_content_type(content_type: &str) -> Vec<u8> {
        build_docx_fixture(
            &docx_document("macro marker"),
            content_type,
            "word/document.xml",
            None,
        )
    }

    fn docx_fixture_with_package_target(target: &str, target_mode: Option<&str>) -> Vec<u8> {
        build_docx_fixture(
            &docx_document("relationship marker"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            target,
            target_mode,
        )
    }

    fn docx_fixture_with_depth(depth: usize) -> Vec<u8> {
        let nested = "<w:r>".repeat(depth);
        let close = "</w:r>".repeat(depth);
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p>{nested}<w:t>deep</w:t>{close}</w:p></w:body></w:document>"#,
        );
        docx_fixture_with_document(&document)
    }

    fn docx_fixture_with_doctype() -> Vec<u8> {
        let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE w:document SYSTEM "https://example.invalid/external.dtd">
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>blocked</w:t></w:r></w:p></w:body></w:document>"#;
        docx_fixture_with_document(document)
    }

    fn docx_fixture_with_duplicate_canonical_part() -> Vec<u8> {
        let mut writer =
            ZipWriter::new_append(Cursor::new(docx_fixture_with_text("duplicate"))).unwrap();
        write_zip_entry(&mut writer, "WORD/DOCUMENT.XML", b"duplicate");
        writer.finish().unwrap().into_inner()
    }

    fn docx_fixture_with_external_hyperlink() -> Vec<u8> {
        let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:hyperlink r:id="rIdExternal"><w:r><w:t>링크 레이블</w:t></w:r></w:hyperlink></w:p></w:body></w:document>"#;
        let mut writer =
            ZipWriter::new_append(Cursor::new(docx_fixture_with_document(document))).unwrap();
        write_zip_entry(
            &mut writer,
            "word/_rels/document.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdExternal" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.invalid/never-opened" TargetMode="External"/>
  <Relationship Id="rIdCustomXml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/>
</Relationships>"#,
        );
        writer.finish().unwrap().into_inner()
    }

    fn docx_fixture_with_document_relationship_target(target: &str) -> Vec<u8> {
        let mut writer =
            ZipWriter::new_append(Cursor::new(docx_fixture_with_text("relationship"))).unwrap();
        let relationships = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="{}"/>
</Relationships>"#,
            xml_escape(target),
        );
        write_zip_entry(
            &mut writer,
            "word/_rels/document.xml.rels",
            relationships.as_bytes(),
        );
        writer.finish().unwrap().into_inner()
    }

    fn docx_document(text: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{}</w:t></w:r></w:p></w:body></w:document>"#,
            xml_escape(text),
        )
    }

    fn build_docx_fixture(
        document: &str,
        content_type: &str,
        package_target: &str,
        target_mode: Option<&str>,
    ) -> Vec<u8> {
        let content_types = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="{}"/>
</Types>"#,
            xml_escape(content_type),
        );
        let target_mode = target_mode
            .map(|mode| format!(r#" TargetMode="{}""#, xml_escape(mode)))
            .unwrap_or_default();
        let package_relationships = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{}"{target_mode}/>
</Relationships>"#,
            xml_escape(package_target),
        );
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        write_zip_entry(&mut writer, "[Content_Types].xml", content_types.as_bytes());
        write_zip_entry(&mut writer, "_rels/.rels", package_relationships.as_bytes());
        write_zip_entry(&mut writer, "word/document.xml", document.as_bytes());
        writer.finish().unwrap().into_inner()
    }

    pub(super) fn xlsx_fixture() -> Vec<u8> {
        xlsx_fixture_with("shared alpha", "0000003", "A1:B2")
    }

    pub(super) fn xlsx_fixture_with(
        first_shared: &str,
        unique_count: &str,
        dimension: &str,
    ) -> Vec<u8> {
        build_xlsx_fixture(
            first_shared,
            unique_count,
            dimension,
            "xl/workbook.xml",
            None,
        )
    }

    fn xlsx_fixture_with_package_target(target: &str, target_mode: Option<&str>) -> Vec<u8> {
        build_xlsx_fixture("shared alpha", "0000003", "A1:B2", target, target_mode)
    }

    fn build_xlsx_fixture(
        first_shared: &str,
        unique_count: &str,
        dimension: &str,
        package_target: &str,
        target_mode: Option<&str>,
    ) -> Vec<u8> {
        let shared_strings = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="{}">
  <si><t>{}</t></si><si><r><t>rich beta</t></r></si><si><t>shared gamma</t></si>
</sst>"#,
            xml_escape(unique_count),
            xml_escape(first_shared),
        );
        let sheet_one = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <dimension ref="{}"/>
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="str"><v>inline gamma</v></c></row>
    <row r="2"><c r="A2" t="n"><f>SUM(40,2)</f><v>42</v></c><c r="B2" t="b"><v>1</v></c></row>
  </sheetData>
</worksheet>"#,
            xml_escape(dimension),
        );
        let sheet_two = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <dimension ref="A1:B1"/>
  <sheetData><row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="s"><v>2</v></c></row></sheetData>
</worksheet>"#;
        let target_mode = target_mode
            .map(|mode| format!(r#" TargetMode="{}""#, xml_escape(mode)))
            .unwrap_or_default();
        let package_relationships = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{}"{target_mode}/>
</Relationships>"#,
            xml_escape(package_target),
        );

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        write_zip_entry(
            &mut writer,
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#,
        );
        write_zip_entry(&mut writer, "_rels/.rels", package_relationships.as_bytes());
        write_zip_entry(
            &mut writer,
            "xl/workbook.xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets><sheet name="Summary" sheetId="1" r:id="rId1"/><sheet name="Detail" sheetId="2" r:id="rId2"/></sheets>
</workbook>"#,
        );
        write_zip_entry(
            &mut writer,
            "xl/_rels/workbook.xml.rels",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#,
        );
        write_zip_entry(
            &mut writer,
            "xl/sharedStrings.xml",
            shared_strings.as_bytes(),
        );
        write_zip_entry(
            &mut writer,
            "xl/worksheets/sheet1.xml",
            sheet_one.as_bytes(),
        );
        write_zip_entry(&mut writer, "xl/worksheets/sheet2.xml", sheet_two);
        writer.finish().unwrap().into_inner()
    }

    fn xlsx_fixture_with_duplicate_canonical_part() -> Vec<u8> {
        let mut writer = ZipWriter::new_append(Cursor::new(xlsx_fixture())).unwrap();
        write_zip_entry(&mut writer, "XL/WORKBOOK.XML", b"duplicate");
        writer.finish().unwrap().into_inner()
    }

    pub(super) fn ods_fixture(first_value: &str) -> Vec<u8> {
        ods_fixture_with_manifest_and_content("", &ods_content(&xml_escape(first_value), "", ""))
    }

    fn ods_fixture_with_manifest(marker: &str) -> Vec<u8> {
        ods_fixture_with_manifest_and_content(marker, &ods_content("shared alpha", "", ""))
    }

    fn ods_fixture_with_row_repeat(repeats: u64) -> Vec<u8> {
        ods_fixture_with_manifest_and_content(
            "",
            &ods_content(
                "shared alpha",
                &format!(" table:number-rows-repeated=\"{repeats}\""),
                "",
            ),
        )
    }

    fn ods_fixture_with_column_repeat(repeats: u64) -> Vec<u8> {
        ods_fixture_with_manifest_and_content(
            "",
            &ods_content(
                "shared alpha",
                "",
                &format!(" table:number-columns-repeated=\"{repeats}\""),
            ),
        )
    }

    fn ods_fixture_with_repeated_value(value: &str, repeats: u64) -> Vec<u8> {
        ods_fixture_with_manifest_and_content(
            "",
            &ods_content(
                &xml_escape(value),
                "",
                &format!(" table:number-columns-repeated=\"{repeats}\""),
            ),
        )
    }

    fn ods_fixture_with_repeats(row_repeats: u64, column_repeats: u64) -> Vec<u8> {
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:spreadsheet><table:table table:name="Summary"><table:table-row table:number-rows-repeated="{row_repeats}"><table:table-cell office:value-type="string" office:string-value="shared alpha" table:number-columns-repeated="{column_repeats}"/></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#,
        );
        ods_fixture_with_manifest_and_content("", &content)
    }

    fn ods_fixture_with_depth(depth: usize) -> Vec<u8> {
        let open = "<nested>".repeat(depth);
        let close = "</nested>".repeat(depth);
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet><table:table table:name="Summary"><table:table-row><table:table-cell office:value-type="string"><text:p>{open}deep{close}</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#,
        );
        ods_fixture_with_manifest_and_content("", &content)
    }

    fn ods_fixture_with_doctype() -> Vec<u8> {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE office:document-content SYSTEM "https://example.invalid/external.dtd">
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body><office:spreadsheet><table:table table:name="Summary"/></office:spreadsheet></office:body></office:document-content>"#;
        ods_fixture_with_manifest_and_content("", content)
    }

    fn ods_fixture_with_manifest_and_content(marker: &str, content: &str) -> Vec<u8> {
        let manifest = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">
  <manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>
  <manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml">{marker}</manifest:file-entry>
</manifest:manifest>"#,
        );
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        write_zip_entry(
            &mut writer,
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        );
        write_zip_entry(&mut writer, "META-INF/manifest.xml", manifest.as_bytes());
        write_zip_entry(&mut writer, "content.xml", content.as_bytes());
        writer.finish().unwrap().into_inner()
    }

    fn ods_content(first_value: &str, row_attributes: &str, cell_attributes: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
  <office:body><office:spreadsheet>
    <table:table table:name="Summary">
      <table:table-row{row_attributes}><table:table-cell office:value-type="string" office:string-value="{first_value}"{cell_attributes}/><table:table-cell office:value-type="string"><text:p>rich beta</text:p><text:p>line two</text:p></table:table-cell></table:table-row>
      <table:table-row><table:table-cell office:value-type="float" office:value="42"/><table:table-cell office:value-type="boolean" office:boolean-value="true"/></table:table-row>
      <table:table-row><table:table-cell office:value-type="date" office:date-value="2026-08-27"/><table:table-cell office:value-type="float" office:value="0" table:formula="of:=SUM(A1:A2)"/></table:table-row>
    </table:table>
    <table:table table:name="Detail"><table:table-row><table:table-cell office:value-type="string" office:string-value="shared gamma"/></table:table-row></table:table>
  </office:spreadsheet></office:body>
</office:document-content>"#,
        )
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn write_zip_entry(writer: &mut ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8]) {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }

    fn mark_zip_entry_encrypted(mut bytes: Vec<u8>) -> Vec<u8> {
        let mut offset = 0;
        while offset + 4 <= bytes.len() {
            if &bytes[offset..offset + 4] == b"PK\x01\x02" {
                bytes[offset + 8] |= 0x01;
            }
            offset += 1;
        }
        bytes
    }

    fn with_declared_zip_entries(mut bytes: Vec<u8>, entries: usize) -> Vec<u8> {
        let entries = u16::try_from(entries).unwrap();
        let eocd = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("fixture EOCD");
        bytes[eocd + 8..eocd + 10].copy_from_slice(&entries.to_le_bytes());
        bytes[eocd + 10..eocd + 12].copy_from_slice(&entries.to_le_bytes());
        bytes
    }

    fn with_ambiguous_zip_end_record(mut bytes: Vec<u8>) -> Vec<u8> {
        let eocd = bytes
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .expect("fixture EOCD");
        let fake = [
            b'P', b'K', 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        bytes[eocd + 20..eocd + 22].copy_from_slice(&(fake.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&fake);
        bytes
    }

    fn with_declared_zip_entry_size(mut bytes: Vec<u8>, size: u64) -> Vec<u8> {
        let size = u32::try_from(size).unwrap();
        let central = bytes
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .expect("fixture central directory");
        bytes[central + 24..central + 28].copy_from_slice(&size.to_le_bytes());
        bytes
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
