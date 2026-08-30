//! Bounded, local-only log inspection primitives for Log Lens.
//!
//! The core deliberately has no Tauri or UI dependency.  Sources are either
//! explicitly selected local files or one of the small, fixed adapters below;
//! log text stays in process memory unless a user explicitly asks for an
//! export.  This keeps the bootstrap useful in browser fixtures and makes the
//! safety and ordering rules testable on every platform.

mod buffer;
pub mod handoff;
mod lifecycle;
mod model;
mod parser;
pub mod saved_views;
mod sources;

pub use buffer::{BufferPush, RingBuffer};
pub use lifecycle::{CancellationToken, OperationRegistry};
pub use model::{
    validate_source_list, ContainerEngine, CoreError, FileCursor, FileIdentity, FilterSpec,
    LogFormat, LogLevel, LogRecord, LogSourceRef, ReadStatus, SavedView, SourceKind,
    SourceSnapshot, SourceSpec, SourceSummary, MAX_FIELDS, MAX_FIELD_BYTES, MAX_FILTER_BYTES,
    MAX_LINE_BYTES, MAX_RECORDS, MAX_SOURCES, MAX_SOURCE_BYTES,
};
pub use parser::{
    export_records, filter_records, merge_records, merge_records_with_stats, parse_bytes,
    parse_line, ExportedText, MergeBuffer, ParseBatch,
};
pub use sources::{adapter_argv, load_source, AdapterPlan, LoadContext};
