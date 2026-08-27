use crate::core::{
    adapter_argv, export_records, filter_records, load_source, CoreError, ExportedText, FileCursor,
    FilterSpec, LogRecord, LogSourceRef, MergeBuffer, OperationRegistry, ReadStatus,
    SourceSnapshot, SourceSpec, SourceSummary,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Default)]
pub struct AppState {
    pub operations: Arc<OperationRegistry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelResponse {
    pub found: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesSnapshot {
    pub operation_id: String,
    pub generation: u64,
    pub sources: Vec<SourceSummary>,
    pub records: Vec<LogRecord>,
    pub cursors: Vec<Option<FileCursor>>,
    pub statuses: Vec<ReadStatus>,
    pub truncated: bool,
    pub dropped_records: usize,
    pub dropped_bytes: usize,
}

#[tauri::command]
pub fn summarize_source(source: SourceSpec) -> Result<SourceSummary, String> {
    source.summary().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn receive_log_source(reference: LogSourceRef) -> Result<SourceSpec, String> {
    reference.into_source().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn fixed_adapter(source: SourceSpec) -> Result<Option<crate::core::AdapterPlan>, String> {
    adapter_argv(&source).map_err(|error| error.to_string())
}

/// Load one bounded snapshot. `operationId` is caller-generated and opaque;
/// starting another generation cancels the previous one. The command returns
/// no raw path or process diagnostics in errors.
#[tauri::command]
pub async fn read_source(
    state: State<'_, AppState>,
    source: SourceSpec,
    cursor: Option<FileCursor>,
    sequence_start: u64,
    generation: u64,
    operation_id: String,
) -> Result<SourceSnapshot, String> {
    if sequence_start > 9_007_199_254_740_991_u64 || generation > 9_007_199_254_740_991_u64 {
        return Err(CoreError::InvalidInput.to_string());
    }
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.validate().is_err())
    {
        return Err(CoreError::InvalidInput.to_string());
    }
    let operations = Arc::clone(&state.operations);
    let token = operations
        .begin(&operation_id, generation)
        .map_err(|error| error.to_string())?;
    let task_operations = Arc::clone(&operations);
    let task_operation_id = operation_id.clone();
    let result = match tokio::task::spawn_blocking(move || {
        let context =
            crate::core::LoadContext::new(&task_operation_id, generation, &token, &task_operations);
        load_source(&source, cursor.as_ref(), sequence_start, &context)
            .map_err(|error| error.to_string())
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(CoreError::Io.to_string()),
    };
    operations.finish(&operation_id, generation);
    result
}

#[tauri::command]
pub async fn read_sources(
    state: State<'_, AppState>,
    sources: Vec<SourceSpec>,
    cursors: Vec<Option<FileCursor>>,
    sequence_starts: Vec<u64>,
    generation: u64,
    operation_id: String,
) -> Result<SourcesSnapshot, String> {
    if sources.len() != cursors.len()
        || sources.len() != sequence_starts.len()
        || sequence_starts
            .iter()
            .any(|value| *value > 9_007_199_254_740_991_u64)
        || generation > 9_007_199_254_740_991_u64
    {
        return Err(CoreError::InvalidSource.to_string());
    }
    if cursors
        .iter()
        .flatten()
        .any(|cursor| cursor.validate().is_err())
    {
        return Err(CoreError::InvalidInput.to_string());
    }
    // Validate every descriptor and reject duplicate identity before opening
    // any source. This keeps a bad descriptor from being reported as a
    // partial multi-source snapshot while still allowing an unavailable
    // (missing/permission-denied) source to be isolated below.
    if crate::core::validate_source_list(&sources).is_err() {
        return Err(CoreError::InvalidSource.to_string());
    }
    let operations = Arc::clone(&state.operations);
    let token = operations
        .begin(&operation_id, generation)
        .map_err(|error| error.to_string())?;
    let task_operations = Arc::clone(&operations);
    let task_operation_id = operation_id.clone();
    let result = match tokio::task::spawn_blocking(move || {
        let context =
            crate::core::LoadContext::new(&task_operation_id, generation, &token, &task_operations);
        let result = (|| {
            let mut merge_buffer = MergeBuffer::default();
            let mut source_summaries = Vec::with_capacity(sources.len());
            let mut next_cursors = Vec::with_capacity(sources.len());
            let mut statuses = Vec::with_capacity(sources.len());
            let mut truncated = false;
            let mut dropped_records = 0_usize;
            let mut dropped_bytes = 0_usize;
            for (index, source) in sources.iter().enumerate() {
                context.check()?;
                match load_source(
                    source,
                    cursors[index].as_ref(),
                    sequence_starts[index],
                    &context,
                ) {
                    Ok(snapshot) => {
                        source_summaries.push(snapshot.source);
                        next_cursors.push(snapshot.next_cursor);
                        statuses.push(snapshot.status);
                        truncated |= snapshot.truncated;
                        dropped_records = dropped_records.saturating_add(snapshot.dropped_records);
                        dropped_bytes = dropped_bytes.saturating_add(snapshot.dropped_bytes);
                        merge_buffer.extend(snapshot.records);
                    }
                    Err(CoreError::AdapterUnavailable | CoreError::Io) => {
                        source_summaries.push(source.summary()?);
                        next_cursors.push(None);
                        statuses.push(ReadStatus::Unavailable);
                    }
                    Err(error) => return Err(error),
                }
            }
            context.check()?;
            let (records, merge_dropped_records, merge_dropped_bytes) = merge_buffer.finish();
            Ok::<_, CoreError>(SourcesSnapshot {
                operation_id: task_operation_id.clone(),
                generation,
                sources: source_summaries,
                records,
                cursors: next_cursors,
                statuses,
                truncated: truncated || merge_dropped_records > 0,
                dropped_records: dropped_records.saturating_add(merge_dropped_records),
                dropped_bytes: dropped_bytes.saturating_add(merge_dropped_bytes),
            })
        })()
        .map_err(|error: CoreError| error.to_string());
        result
    })
    .await
    {
        Ok(result) => result,
        Err(_) => Err(CoreError::Io.to_string()),
    };
    operations.finish(&operation_id, generation);
    result
}

#[tauri::command]
pub fn cancel_read(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<CancelResponse, String> {
    state
        .operations
        .cancel(&operation_id)
        .map(|found| CancelResponse { found })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn filter_log_records(
    records: Vec<LogRecord>,
    filter: FilterSpec,
) -> Result<Vec<LogRecord>, String> {
    filter_records(&records, &filter).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_log_records(records: Vec<LogRecord>) -> Result<ExportedText, String> {
    export_records(&records).map_err(|error| error.to_string())
}
