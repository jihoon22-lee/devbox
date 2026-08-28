//! Bounded, read-only inspection of the SQLite databases owned by devbox apps.
//!
//! The UI never supplies a filesystem path.  A database is resolved from the
//! reviewed catalog and the platform local-data root, then opened read-only.
//! The connection has query-only mode, a SQLite authorizer, a progress
//! deadline, and bounded row/cell/result limits.  Values are sanitized before
//! they cross the command boundary because a diagnostic query is not a secret
//! exfiltration escape hatch.

use rusqlite::hooks::{AuthAction, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::CStr;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::fs::{self, Metadata};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::catalog::{Catalog, CatalogApp};

pub const MAX_QUERY_BYTES: usize = 16 * 1024;
pub const MAX_QUERY_ID_BYTES: usize = 128;
pub const MAX_DATABASES: usize = 64;
pub const MAX_SCHEMA_OBJECTS: usize = 128;
pub const MAX_COLUMNS: usize = 64;
pub const MAX_ROWS: usize = 1_000;
pub const MAX_CELL_BYTES: usize = 64 * 1024;
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;

// SQLite's process-wide defaults are intentionally much larger than a
// diagnostic preview needs.  These per-connection limits keep expressions,
// temporary values, and generated VDBE programs bounded before SQLite starts
// evaluating an untrusted SELECT.
const SQLITE_MAX_EXPRESSION_DEPTH: i32 = 128;
const SQLITE_MAX_COMPOUND_SELECTS: i32 = 32;
const SQLITE_MAX_VDBE_OPS: i32 = 100_000;
const SQLITE_MAX_FUNCTION_ARGS: i32 = 32;
// SQLite's bundled build has a 50,000-byte compile-time LIKE ceiling, so keep
// the diagnostic connection setting below it instead of relying on clamping.
const SQLITE_MAX_LIKE_PATTERN_BYTES: i32 = 48 * 1024;
const SQLITE_MAX_VARIABLES: i32 = 999;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataInspectorSnapshot {
    pub catalog_revision: Option<u64>,
    pub databases: Vec<DataDatabaseInfo>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataDatabaseInfo {
    pub app_id: String,
    pub display_name: String,
    pub identifier: String,
    /// `available`, `missing`, `unsafe-path`, or `unreadable`.
    pub state: String,
    pub revision: Option<String>,
    pub byte_length: Option<u64>,
    pub schema_version: Option<u64>,
    pub tables: Vec<DataSchemaObject>,
    pub views: Vec<DataSchemaObject>,
    /// `ok`, `failed`, `timed-out`, or `unavailable`.
    pub integrity: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataSchemaObject {
    pub name: String,
    pub row_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DataQueryRequest {
    pub app_id: String,
    pub sql: String,
    pub query_id: String,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DataQueryResult {
    pub preview_id: String,
    pub query_id: String,
    pub app_id: String,
    pub database_revision: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub result_bytes: usize,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DataExport {
    pub filename: String,
    pub mime_type: String,
    pub format: ExportFormat,
    pub content: String,
    pub byte_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFailure {
    InvalidRequest,
    UnknownDatabase,
    DatabaseMissing,
    UnsafePath,
    DatabaseUnavailable,
    QueryRejected,
    TimedOut,
    Cancelled,
    ResultTooLarge,
    Stale,
}

impl QueryFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "진단 요청이 올바르지 않습니다.",
            Self::UnknownDatabase => "선택한 devbox 데이터베이스를 찾을 수 없습니다.",
            Self::DatabaseMissing => "선택한 앱의 데이터베이스가 없습니다.",
            Self::UnsafePath => "데이터베이스 경로를 안전하게 확인할 수 없습니다.",
            Self::DatabaseUnavailable => "데이터베이스를 읽기 전용으로 열 수 없습니다.",
            Self::QueryRejected => "읽기 전용 조회만 허용됩니다.",
            Self::TimedOut => "조회 시간이 제한을 초과했습니다. 범위를 줄여 다시 시도하세요.",
            Self::Cancelled => "조회가 취소되었습니다.",
            Self::ResultTooLarge => "조회 결과가 허용된 크기를 초과했습니다.",
            Self::Stale => "데이터베이스가 바뀌었습니다. 최신 진단 결과를 다시 확인하세요.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatabasePathState {
    Unsafe,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DatabaseFingerprint {
    byte_length: u64,
    modified_ns: u128,
    // Size/mtime alone can remain unchanged when a path is atomically
    // replaced. Include the platform file identity in stale checks whenever
    // the OS exposes one.
    file_identity: Option<(u64, u64)>,
}

impl DatabaseFingerprint {
    fn revision(self, app_id: &str) -> String {
        let mut digest = Sha256::new();
        digest.update(b"devbox-data-revision-v1\0");
        digest.update(app_id.as_bytes());
        digest.update([0]);
        digest.update(self.byte_length.to_le_bytes());
        digest.update(self.modified_ns.to_le_bytes());
        if let Some((first, second)) = self.file_identity {
            digest.update([1]);
            digest.update(first.to_le_bytes());
            digest.update(second.to_le_bytes());
        } else {
            digest.update([0]);
        }
        format!("{:x}", digest.finalize())
    }
}

#[derive(Debug)]
struct ResolvedDatabase {
    app_id: String,
    path: PathBuf,
    fingerprint: DatabaseFingerprint,
}

#[derive(Debug)]
struct RawQueryResult {
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    row_count: usize,
    result_bytes: usize,
    truncated: bool,
    elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct QueryBudget {
    started: Instant,
    deadline: Instant,
}

impl QueryBudget {
    fn new(timeout: Duration) -> Self {
        let started = Instant::now();
        Self {
            started,
            deadline: started + timeout,
        }
    }

    fn expired(self) -> bool {
        Instant::now() >= self.deadline
    }

    fn elapsed_ms(self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
}

/// Discover all catalog-known `data.db` files without creating directories or
/// following links. Missing entries are returned so the UI can distinguish an
/// uninstalled app from a read failure.
pub fn inspect_databases(
    catalog: &Catalog,
    data_root: &Path,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<DataInspectorSnapshot, QueryFailure> {
    if !data_root.is_absolute() || data_root.to_string_lossy().len() > 4096 {
        return Err(QueryFailure::UnsafePath);
    }
    let mut databases = Vec::new();
    for app in catalog.apps.iter().take(MAX_DATABASES) {
        if cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(QueryFailure::Cancelled);
        }
        databases.push(inspect_one_database(
            app,
            data_root,
            cancel.as_deref(),
            cancel.clone(),
        ));
        if cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return Err(QueryFailure::Cancelled);
        }
    }
    Ok(DataInspectorSnapshot {
        catalog_revision: catalog.catalog_revision,
        databases,
    })
}

fn inspect_one_database(
    app: &CatalogApp,
    data_root: &Path,
    cancel: Option<&AtomicBool>,
    cancel_shared: Option<Arc<AtomicBool>>,
) -> DataDatabaseInfo {
    let path = data_root.join(&app.identifier).join("data.db");
    let base = base_info(app);
    let metadata = match safe_database_metadata(data_root, &app.identifier, &path) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => {
            return DataDatabaseInfo {
                state: "missing".to_string(),
                warning: None,
                ..base
            }
        }
        Err(DatabasePathState::Unsafe) => {
            return DataDatabaseInfo {
                state: "unsafe-path".to_string(),
                warning: Some("symlink/reparse point 또는 안전하지 않은 경로입니다.".to_string()),
                ..base
            }
        }
        Err(DatabasePathState::Unreadable) => {
            return DataDatabaseInfo {
                state: "unreadable".to_string(),
                warning: Some("데이터베이스 메타데이터를 읽을 수 없습니다.".to_string()),
                ..base
            }
        } // `safe_database_metadata` represents a missing final file as
          // `Ok(None)`, so no separate missing error state is needed here.
    };

    if metadata.len() > MAX_DATABASE_BYTES {
        return DataDatabaseInfo {
            state: "unreadable".to_string(),
            byte_length: Some(metadata.len()),
            warning: Some("데이터베이스가 허용된 진단 크기를 초과했습니다.".to_string()),
            ..base
        };
    }

    let expected_fingerprint = fingerprint(&metadata);
    let revision = Some(expected_fingerprint.revision(&app.id));
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return DataDatabaseInfo {
            state: "unreadable".to_string(),
            revision,
            byte_length: Some(metadata.len()),
            warning: Some("진단이 취소되었습니다.".to_string()),
            ..base
        };
    }

    match inspect_database_file(&path, app, expected_fingerprint, cancel, cancel_shared) {
        Ok(mut info) => {
            info.revision = revision;
            info.byte_length = Some(metadata.len());
            info
        }
        Err(QueryFailure::Cancelled) => DataDatabaseInfo {
            state: "unreadable".to_string(),
            revision,
            byte_length: Some(metadata.len()),
            warning: Some("진단이 취소되었습니다.".to_string()),
            ..base
        },
        Err(QueryFailure::TimedOut) => DataDatabaseInfo {
            state: "available".to_string(),
            revision,
            byte_length: Some(metadata.len()),
            integrity: "timed-out".to_string(),
            warning: Some("schema 또는 integrity 확인이 시간 제한을 초과했습니다.".to_string()),
            ..base
        },
        Err(_) => DataDatabaseInfo {
            state: "unreadable".to_string(),
            revision,
            byte_length: Some(metadata.len()),
            warning: Some("데이터베이스를 안전하게 읽을 수 없습니다.".to_string()),
            ..base
        },
    }
}

fn base_info(app: &CatalogApp) -> DataDatabaseInfo {
    DataDatabaseInfo {
        app_id: app.id.clone(),
        display_name: sanitize_identifier(&app.display_name),
        identifier: sanitize_identifier(&app.identifier),
        state: "missing".to_string(),
        revision: None,
        byte_length: None,
        schema_version: None,
        tables: Vec::new(),
        views: Vec::new(),
        integrity: "unavailable".to_string(),
        warning: None,
    }
}

fn inspect_database_file(
    path: &Path,
    app: &CatalogApp,
    expected_fingerprint: DatabaseFingerprint,
    cancel: Option<&AtomicBool>,
    cancel_shared: Option<Arc<AtomicBool>>,
) -> Result<DataDatabaseInfo, QueryFailure> {
    let conn = open_read_only(path, Some(expected_fingerprint))?;
    let budget = QueryBudget::new(QUERY_TIMEOUT);
    let query_only: i64 = conn
        .pragma_query_value(None, "query_only", |row| row.get(0))
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    if query_only != 1 {
        return Err(QueryFailure::DatabaseUnavailable);
    }
    let schema_version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .ok()
        .and_then(|value| u64::try_from(value).ok());
    install_progress_handler(&conn, budget, cancel_shared);

    // Integrity check is a read-only SQLite pragma. It runs before the
    // authorizer is installed; all user SQL below sees the stricter hook.
    let integrity = if budget.expired() || cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        return Err(if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            QueryFailure::Cancelled
        } else {
            QueryFailure::TimedOut
        });
    } else {
        match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
            Ok(value) if value.eq_ignore_ascii_case("ok") => "ok",
            Ok(_) => "failed",
            Err(_) if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) => {
                return Err(QueryFailure::Cancelled)
            }
            Err(_) if budget.expired() => return Err(QueryFailure::TimedOut),
            Err(_) => "unavailable",
        }
    };

    install_read_only_authorizer(&conn);
    let mut objects = Vec::new();
    let mut statement = conn
        .prepare(
            "SELECT type, name FROM sqlite_schema \
             WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' \
             ORDER BY name LIMIT 128",
        )
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    let mut rows = statement
        .query([])
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    while let Some(row) = rows
        .next()
        .map_err(|_| classify_interruption(budget, cancel))?
    {
        if budget.expired() {
            return Err(QueryFailure::TimedOut);
        }
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(QueryFailure::Cancelled);
        }
        let kind: String = row.get(0).map_err(|_| QueryFailure::DatabaseUnavailable)?;
        let name: String = row.get(1).map_err(|_| QueryFailure::DatabaseUnavailable)?;
        let safe_name = sanitize_identifier(&name);
        objects.push((kind, safe_name, name));
        if objects.len() >= MAX_SCHEMA_OBJECTS {
            break;
        }
    }
    drop(rows);
    drop(statement);

    let mut tables = Vec::new();
    let mut views = Vec::new();
    for (kind, safe_name, source_name) in objects {
        let row_count = if kind == "table" {
            count_rows(&conn, &source_name, budget, cancel)?
        } else {
            None
        };
        let object = DataSchemaObject {
            name: safe_name,
            row_count,
        };
        if kind == "table" {
            tables.push(object);
        } else {
            views.push(object);
        }
    }
    Ok(DataDatabaseInfo {
        app_id: app.id.clone(),
        display_name: sanitize_identifier(&app.display_name),
        identifier: sanitize_identifier(&app.identifier),
        state: "available".to_string(),
        revision: None,
        byte_length: None,
        schema_version,
        tables,
        views,
        integrity: integrity.to_string(),
        warning: None,
    })
}

fn count_rows(
    conn: &Connection,
    table_name: &str,
    budget: QueryBudget,
    cancel: Option<&AtomicBool>,
) -> Result<Option<u64>, QueryFailure> {
    if !valid_sql_identifier(table_name) {
        return Ok(None);
    }
    if budget.expired() {
        return Err(QueryFailure::TimedOut);
    }
    let quoted = quote_identifier(table_name);
    let sql = format!("SELECT COUNT(*) FROM {quoted}");
    match conn.query_row(&sql, [], |row| row.get::<_, i64>(0)) {
        Ok(value) => Ok(u64::try_from(value).ok()),
        Err(_) if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) => {
            Err(QueryFailure::Cancelled)
        }
        Err(_) if budget.expired() => Err(QueryFailure::TimedOut),
        Err(_) => Ok(None),
    }
}

/// Execute one preview query against an already resolved catalog database.
/// The returned rows are safe to retain for an explicit JSON/CSV export.
pub fn preview_query(
    catalog: &Catalog,
    data_root: &Path,
    request: &DataQueryRequest,
    cancel: Arc<AtomicBool>,
) -> Result<(DataQueryResult, String), QueryFailure> {
    validate_query_request(request)?;
    let app = catalog
        .apps
        .iter()
        .find(|app| app.id == request.app_id)
        .ok_or(QueryFailure::UnknownDatabase)?;
    let resolved = resolve_database(catalog, data_root, &app.id)?;
    let current_revision = resolved.fingerprint.revision(&resolved.app_id);
    if request
        .expected_revision
        .as_deref()
        .is_some_and(|expected| expected != current_revision)
    {
        return Err(QueryFailure::Stale);
    }
    let raw = execute_query(
        &resolved.path,
        &app.id,
        &request.sql,
        cancel,
        QUERY_TIMEOUT,
        Some(resolved.fingerprint),
    )?;
    let after = current_database_fingerprint(&resolved.path).ok_or(QueryFailure::Stale)?;
    if after != resolved.fingerprint {
        return Err(QueryFailure::Stale);
    }
    let preview_id = opaque_id("query", &request.query_id);
    Ok((
        DataQueryResult {
            preview_id: preview_id.clone(),
            query_id: request.query_id.clone(),
            app_id: request.app_id.clone(),
            database_revision: current_revision.clone(),
            columns: raw.columns,
            rows: raw.rows,
            row_count: raw.row_count,
            result_bytes: raw.result_bytes,
            truncated: raw.truncated,
            elapsed_ms: raw.elapsed_ms,
        },
        current_revision,
    ))
}

/// Return the current opaque revision for one catalog database. This is used
/// by export to reject a preview whose source changed after the user reviewed
/// it.
pub fn database_revision(
    catalog: &Catalog,
    data_root: &Path,
    app_id: &str,
) -> Result<String, QueryFailure> {
    let resolved = resolve_database(catalog, data_root, app_id)?;
    Ok(resolved.fingerprint.revision(app_id))
}

/// Return a bounded revision/state vector for every catalog database without
/// opening SQLite. Support-bundle export uses this cheap check to reject a
/// preview whose source appeared, disappeared, or changed while the user was
/// reviewing it.
pub fn database_state_revisions(
    catalog: &Catalog,
    data_root: &Path,
) -> Result<Vec<String>, QueryFailure> {
    if !data_root.is_absolute() || data_root.to_string_lossy().len() > 4096 {
        return Err(QueryFailure::UnsafePath);
    }
    let mut revisions = Vec::new();
    for app in catalog.apps.iter().take(MAX_DATABASES) {
        let path = data_root.join(&app.identifier).join("data.db");
        let entry = match safe_database_metadata(data_root, &app.identifier, &path) {
            Ok(Some(metadata)) if metadata.len() <= MAX_DATABASE_BYTES => format!(
                "{}:available:{}",
                app.id,
                fingerprint(&metadata).revision(&app.id)
            ),
            Ok(Some(_)) => format!("{}:unreadable:", app.id),
            Ok(None) => format!("{}:missing:", app.id),
            Err(DatabasePathState::Unsafe) => format!("{}:unsafe-path:", app.id),
            Err(DatabasePathState::Unreadable) => format!("{}:unreadable:", app.id),
        };
        revisions.push(entry);
    }
    Ok(revisions)
}

fn execute_query(
    path: &Path,
    app_id: &str,
    sql: &str,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
    expected_fingerprint: Option<DatabaseFingerprint>,
) -> Result<RawQueryResult, QueryFailure> {
    let conn = open_read_only(path, expected_fingerprint)?;
    install_read_only_authorizer(&conn);
    let budget = QueryBudget::new(timeout);
    install_progress_handler(&conn, budget, Some(cancel.clone()));
    let mut statement = conn.prepare(sql).map_err(|_| QueryFailure::QueryRejected)?;
    let column_count = statement.column_count();
    if column_count == 0 || column_count > MAX_COLUMNS {
        return Err(QueryFailure::QueryRejected);
    }
    let raw_columns = statement.column_names();
    let origins = column_origins(&conn, sql, column_count);
    let columns = raw_columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let origin = origins.as_ref().and_then(|items| items.get(index));
            sanitize_column_name(name, index, origin.and_then(Option::as_deref))
        })
        .collect::<Vec<_>>();
    let mut rows = statement.query([]).map_err(|_| {
        if cancel.load(Ordering::Relaxed) {
            QueryFailure::Cancelled
        } else if budget.expired() {
            QueryFailure::TimedOut
        } else {
            QueryFailure::QueryRejected
        }
    })?;
    let mut result = Vec::new();
    let mut result_bytes = 0usize;
    let mut truncated = false;
    while let Some(row) = rows
        .next()
        .map_err(|_| classify_interruption(budget, Some(&cancel)))?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(QueryFailure::Cancelled);
        }
        if budget.expired() {
            return Err(QueryFailure::TimedOut);
        }
        if result.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
        let mut output_row = Vec::with_capacity(column_count);
        for (index, column) in columns.iter().enumerate() {
            let value = row
                .get_ref(index)
                .map_err(|_| QueryFailure::DatabaseUnavailable)?;
            let origin = origins.as_ref().and_then(|items| items.get(index));
            output_row.push(value_to_json(
                value,
                column,
                app_id,
                origin.and_then(Option::as_deref),
            ));
        }
        let row_size = serde_json::to_vec(&output_row)
            .map(|bytes| bytes.len())
            .unwrap_or(MAX_CELL_BYTES);
        if row_size > MAX_RESULT_BYTES || result_bytes.saturating_add(row_size) > MAX_RESULT_BYTES {
            truncated = true;
            break;
        }
        result_bytes = result_bytes.saturating_add(row_size);
        result.push(output_row);
    }
    drop(rows);
    drop(statement);
    if cancel.load(Ordering::Relaxed) {
        return Err(QueryFailure::Cancelled);
    }
    if budget.expired() {
        return Err(QueryFailure::TimedOut);
    }
    Ok(RawQueryResult {
        columns,
        row_count: result.len(),
        rows: result,
        result_bytes,
        truncated,
        elapsed_ms: budget.elapsed_ms(),
    })
}

pub fn export_query(
    result: &DataQueryResult,
    format: ExportFormat,
) -> Result<DataExport, QueryFailure> {
    if result.rows.len() > MAX_ROWS || result.columns.len() > MAX_COLUMNS {
        return Err(QueryFailure::ResultTooLarge);
    }
    let (content, mime_type, extension) = match format {
        ExportFormat::Json => (
            serde_json::to_string_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "appId": result.app_id,
                "databaseRevision": result.database_revision,
                "columns": result.columns,
                "rows": result.rows,
                "rowCount": result.row_count,
                "truncated": result.truncated,
                "redactionVersion": REDACTION_VERSION,
            }))
            .map_err(|_| QueryFailure::ResultTooLarge)?,
            "application/json",
            "json",
        ),
        ExportFormat::Csv => (to_csv(result)?, "text/csv;charset=utf-8", "csv"),
    };
    let byte_count = content.len();
    if byte_count > MAX_RESULT_BYTES {
        return Err(QueryFailure::ResultTooLarge);
    }
    Ok(DataExport {
        filename: format!(
            "devbox-data-{}.{}",
            safe_filename(&result.app_id),
            extension
        ),
        mime_type: mime_type.to_string(),
        format,
        content,
        byte_count,
    })
}

fn to_csv(result: &DataQueryResult) -> Result<String, QueryFailure> {
    let mut out = String::new();
    for (index, column) in result.columns.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&csv_field(column, true));
    }
    out.push('\n');
    for row in result.rows.iter().take(MAX_ROWS) {
        for (index, value) in row.iter().enumerate().take(MAX_COLUMNS) {
            if index > 0 {
                out.push(',');
            }
            let text = match value {
                serde_json::Value::Null => String::new(),
                serde_json::Value::String(value) => value.clone(),
                other => serde_json::to_string(other).map_err(|_| QueryFailure::ResultTooLarge)?,
            };
            out.push_str(&csv_field(
                &text,
                matches!(value, serde_json::Value::String(_)),
            ));
        }
        out.push('\n');
        if out.len() > MAX_RESULT_BYTES {
            return Err(QueryFailure::ResultTooLarge);
        }
    }
    Ok(out)
}

fn csv_field(value: &str, formula_guard: bool) -> String {
    let safe_value = if formula_guard && starts_like_spreadsheet_formula(value) {
        // A leading apostrophe is retained as a literal marker by spreadsheet
        // programs and prevents `=`, `+`, `-`, and `@` text from becoming a
        // formula when a user opens the CSV. Numeric JSON values deliberately
        // skip this guard so their ordinary representation remains intact.
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if safe_value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe_value.replace('"', "\"\""))
    } else {
        safe_value
    }
}

fn starts_like_spreadsheet_formula(value: &str) -> bool {
    matches!(
        value.trim_start().chars().next(),
        Some('=' | '+' | '-' | '@')
    )
}

fn resolve_database(
    catalog: &Catalog,
    data_root: &Path,
    app_id: &str,
) -> Result<ResolvedDatabase, QueryFailure> {
    let app = catalog
        .apps
        .iter()
        .find(|app| app.id == app_id)
        .ok_or(QueryFailure::UnknownDatabase)?;
    let path = data_root.join(&app.identifier).join("data.db");
    let Some(metadata) =
        safe_database_metadata(data_root, &app.identifier, &path).map_err(|state| match state {
            DatabasePathState::Unsafe => QueryFailure::UnsafePath,
            DatabasePathState::Unreadable => QueryFailure::DatabaseUnavailable,
        })?
    else {
        return Err(QueryFailure::DatabaseMissing);
    };
    if metadata.len() > MAX_DATABASE_BYTES {
        return Err(QueryFailure::DatabaseUnavailable);
    }
    Ok(ResolvedDatabase {
        app_id: app.id.clone(),
        path,
        fingerprint: fingerprint(&metadata),
    })
}

fn validate_query_request(request: &DataQueryRequest) -> Result<(), QueryFailure> {
    if request.app_id.is_empty()
        || request.app_id.len() > 128
        || request.query_id.is_empty()
        || request.query_id.len() > MAX_QUERY_ID_BYTES
        || !request
            .query_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || request.sql.len() > MAX_QUERY_BYTES
        || request.sql.as_bytes().contains(&0)
    {
        return Err(QueryFailure::InvalidRequest);
    }
    let trimmed = request.sql.trim();
    if trimmed.is_empty()
        || trimmed.contains(';')
        || trimmed.contains("--")
        || trimmed.contains("/*")
        || trimmed.contains("*/")
        // SQLite exposes PRAGMA table-valued functions such as
        // `pragma_table_info()` through the SELECT grammar. They are not
        // ordinary app tables and can bypass the intended schema/query
        // boundary, so reject the family before SQLite prepares it.
        || trimmed.to_ascii_lowercase().contains("pragma_")
    {
        return Err(QueryFailure::QueryRejected);
    }
    let first_word = trimmed
        .split(|char: char| !char.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or_default()
        .to_ascii_uppercase();
    if !matches!(first_word.as_str(), "SELECT" | "WITH" | "EXPLAIN") {
        return Err(QueryFailure::QueryRejected);
    }
    Ok(())
}

fn open_read_only(
    path: &Path,
    expected_fingerprint: Option<DatabaseFingerprint>,
) -> Result<Connection, QueryFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|_| QueryFailure::DatabaseUnavailable)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(QueryFailure::UnsafePath);
    }
    if expected_fingerprint.is_some_and(|expected| fingerprint(&metadata) != expected) {
        return Err(QueryFailure::Stale);
    }
    validate_sidecars(path)?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    // `immutable=1` prevents SQLite from opening a sibling `-wal`, `-shm`, or
    // journal file after the path checks above. Diagnostics intentionally read
    // the last checkpointed database image; a live WAL is reported through the
    // ordinary revision/stale path on the next refresh.
    let conn = open_sqlite_connection(path, flags, expected_fingerprint)?;
    let opened_metadata = fs::symlink_metadata(path).map_err(|_| QueryFailure::Stale)?;
    if is_link_or_reparse(&opened_metadata)
        || !opened_metadata.is_file()
        || expected_fingerprint.is_some_and(|expected| fingerprint(&opened_metadata) != expected)
    {
        return Err(QueryFailure::Stale);
    }
    configure_sqlite_limits(&conn);
    conn.pragma_update(None, "query_only", true)
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    let query_only: i64 = conn
        .pragma_query_value(None, "query_only", |row| row.get(0))
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    if query_only != 1 {
        return Err(QueryFailure::DatabaseUnavailable);
    }
    Ok(conn)
}

#[cfg(unix)]
fn open_sqlite_connection(
    path: &Path,
    flags: OpenFlags,
    expected_fingerprint: Option<DatabaseFingerprint>,
) -> Result<Connection, QueryFailure> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::OpenOptionsExt;

    // Open the catalog-derived directory chain with directory descriptors,
    // then keep the final descriptor open while SQLite resolves the URI. This
    // closes the path-swap window between the lexical/symlink checks and
    // sqlite3_open_v2: replacing an already-open parent cannot redirect the
    // final descriptor, and SQLite never re-resolves the user-visible path.
    // SQLite's NOFOLLOW flag cannot be used on the proc-fd URI itself because
    // proc exposes descriptors as kernel-managed symlinks; every descriptor
    // below was opened with O_NOFOLLOW and its components are catalog-derived.
    let identifier = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or(QueryFailure::UnsafePath)?;
    if !valid_identifier(identifier) {
        return Err(QueryFailure::UnsafePath);
    }
    let data_root = path
        .parent()
        .and_then(Path::parent)
        .ok_or(QueryFailure::UnsafePath)?;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(data_root)
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    let identifier_name = CString::new(identifier).map_err(|_| QueryFailure::UnsafePath)?;
    let app_fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            identifier_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if app_fd < 0 {
        return Err(openat_failure());
    }
    let app = unsafe { std::fs::File::from_raw_fd(app_fd) };
    let database_name = CString::new("data.db").expect("static database name has no NUL");
    let database_fd = unsafe {
        libc::openat(
            app.as_raw_fd(),
            database_name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if database_fd < 0 {
        return Err(openat_failure());
    }
    let file = unsafe { std::fs::File::from_raw_fd(database_fd) };
    let metadata = file
        .metadata()
        .map_err(|_| QueryFailure::DatabaseUnavailable)?;
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(QueryFailure::UnsafePath);
    }
    if expected_fingerprint.is_some_and(|expected| fingerprint(&metadata) != expected) {
        return Err(QueryFailure::Stale);
    }
    let fd = file.as_raw_fd();
    let fd_path = if cfg!(target_os = "linux") {
        format!("/proc/self/fd/{fd}")
    } else {
        format!("/dev/fd/{fd}")
    };
    let uri = immutable_uri(Path::new(&fd_path)).ok_or(QueryFailure::UnsafePath)?;
    let flags = flags.difference(OpenFlags::SQLITE_OPEN_NOFOLLOW);
    let connection =
        Connection::open_with_flags(uri, flags).map_err(|_| QueryFailure::DatabaseUnavailable)?;
    drop(file);
    Ok(connection)
}

#[cfg(unix)]
fn openat_failure() -> QueryFailure {
    match std::io::Error::last_os_error().raw_os_error() {
        Some(error) if error == libc::ELOOP || error == libc::ENOTDIR => QueryFailure::UnsafePath,
        _ => QueryFailure::DatabaseUnavailable,
    }
}

#[cfg(not(unix))]
fn open_sqlite_connection(
    path: &Path,
    flags: OpenFlags,
    _expected_fingerprint: Option<DatabaseFingerprint>,
) -> Result<Connection, QueryFailure> {
    let uri = immutable_uri(path).ok_or(QueryFailure::UnsafePath)?;
    Connection::open_with_flags(uri, flags).map_err(|_| QueryFailure::DatabaseUnavailable)
}

fn validate_sidecars(path: &Path) -> Result<(), QueryFailure> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{}", path.to_string_lossy(), suffix));
        reject_link_components(&sidecar).map_err(|_| QueryFailure::UnsafePath)?;
        let Ok(metadata) = fs::symlink_metadata(&sidecar) else {
            continue;
        };
        if is_link_or_reparse(&metadata) || !metadata.is_file() {
            return Err(QueryFailure::UnsafePath);
        }
    }
    Ok(())
}

fn immutable_uri(path: &Path) -> Option<String> {
    let path = path.to_str()?.replace('\\', "/");
    let mut uri = String::from("file:");
    if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') {
        uri.push_str("///");
    }
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'_' | b'.' | b'~') {
            uri.push(byte as char);
        } else {
            uri.push('%');
            uri.push(hex_digit(byte >> 4));
            uri.push(hex_digit(byte & 0x0f));
        }
    }
    uri.push_str("?immutable=1");
    Some(uri)
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("hex nibble is always below 16"),
    }
}

fn install_read_only_authorizer(conn: &Connection) {
    conn.authorizer(Some(
        move |context: rusqlite::hooks::AuthContext<'_>| match context.action {
            AuthAction::Read {
                table_name,
                column_name,
            } if matches!(
                table_name.to_ascii_lowercase().as_str(),
                "sqlite_schema" | "sqlite_master" | "sqlite_temp_schema" | "sqlite_temp_master"
            ) && column_name.eq_ignore_ascii_case("sql") =>
            {
                Authorization::Deny
            }
            AuthAction::Read { column_name, .. } => {
                if is_sensitive_name(column_name) {
                    // Replacing the source value with NULL at prepare time
                    // prevents WHERE/ORDER/GROUP from becoming a side channel.
                    // Masking only returned cells would still expose row count
                    // or ordering changes for guessed secret values.
                    Authorization::Ignore
                } else {
                    Authorization::Allow
                }
            }
            AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
            AuthAction::Function { function_name } => {
                if is_dangerous_function(function_name) {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }
            // `PRAGMA query_only` is configured before the hook. Every user
            // pragma, including writable and file-attached pragmas, is denied.
            _ => Authorization::Deny,
        },
    ));
}

fn configure_sqlite_limits(conn: &Connection) {
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_CELL_BYTES as i32);
    conn.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, MAX_QUERY_BYTES as i32);
    conn.set_limit(Limit::SQLITE_LIMIT_COLUMN, MAX_COLUMNS as i32);
    conn.set_limit(Limit::SQLITE_LIMIT_EXPR_DEPTH, SQLITE_MAX_EXPRESSION_DEPTH);
    conn.set_limit(
        Limit::SQLITE_LIMIT_COMPOUND_SELECT,
        SQLITE_MAX_COMPOUND_SELECTS,
    );
    conn.set_limit(Limit::SQLITE_LIMIT_VDBE_OP, SQLITE_MAX_VDBE_OPS);
    conn.set_limit(Limit::SQLITE_LIMIT_FUNCTION_ARG, SQLITE_MAX_FUNCTION_ARGS);
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
    conn.set_limit(
        Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
        SQLITE_MAX_LIKE_PATTERN_BYTES,
    );
    conn.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, SQLITE_MAX_VARIABLES);
    // Auxiliary worker threads are unnecessary for a bounded preview and can
    // multiply the memory consumed by a large query plan.
    conn.set_limit(Limit::SQLITE_LIMIT_WORKER_THREADS, 0);
}

fn install_progress_handler(
    conn: &Connection,
    budget: QueryBudget,
    cancel: Option<Arc<AtomicBool>>,
) {
    conn.progress_handler(
        1_000,
        Some(move || {
            budget.expired()
                || cancel
                    .as_ref()
                    .is_some_and(|flag| flag.load(Ordering::Relaxed))
        }),
    );
}

fn classify_interruption(budget: QueryBudget, cancel: Option<&AtomicBool>) -> QueryFailure {
    if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
        QueryFailure::Cancelled
    } else if budget.expired() {
        QueryFailure::TimedOut
    } else {
        QueryFailure::DatabaseUnavailable
    }
}

fn is_dangerous_function(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "load_extension"
            | "fts3_tokenizer"
            | "readfile"
            | "writefile"
            | "zeroblob"
            | "randomblob"
            | "printf"
            | "format"
            | "group_concat"
            | "string_agg"
            | "json_group_array"
            | "json_group_object"
    ) || lower.starts_with("pragma_")
}

fn value_to_json(
    value: ValueRef<'_>,
    column: &str,
    app_id: &str,
    origin: Option<&str>,
) -> serde_json::Value {
    // SQLite exposes the origin column for direct projections (including
    // aliases) when column metadata is enabled. Expressions have no origin;
    // mask them wholesale because their output can be a transformed secret
    // even when the displayed alias looks harmless.
    if origin.is_none() || origin.is_some_and(is_sensitive_name) || is_sensitive_name(column) {
        return serde_json::Value::String(REDACTED_VALUE.to_string());
    }
    match value {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(value) => serde_json::Value::from(value),
        ValueRef::Real(value) if value.is_finite() => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(REDACTED_VALUE.to_string())),
        ValueRef::Real(_) => serde_json::Value::String(REDACTED_VALUE.to_string()),
        ValueRef::Text(value) if value.len() > MAX_CELL_BYTES => {
            serde_json::Value::String(format!("[text omitted: {} bytes]", value.len()))
        }
        ValueRef::Text(value) => {
            let text = String::from_utf8_lossy(value);
            serde_json::Value::String(sanitize_value_text(&text, app_id))
        }
        ValueRef::Blob(value) => serde_json::Value::String(format!(
            "[binary omitted: {} bytes]",
            value.len().min(MAX_CELL_BYTES)
        )),
    }
}

/// Return the source column for each result column. rusqlite intentionally
/// keeps the raw `sqlite3_stmt` private, while SQLite's metadata API is the
/// only reliable way to distinguish `token AS label` from a harmless direct
/// projection. A fresh connection has no other live statements here, but we
/// still match the prepared SQL before reading the metadata.
fn column_origins(
    conn: &Connection,
    sql: &str,
    column_count: usize,
) -> Option<Vec<Option<String>>> {
    let target_sql = sql.as_bytes();
    let db = unsafe { conn.handle() };
    let mut cursor = ptr::null_mut();
    let mut target = ptr::null_mut();
    loop {
        cursor = unsafe { rusqlite::ffi::sqlite3_next_stmt(db, cursor) };
        if cursor.is_null() {
            break;
        }
        let statement_sql = unsafe { rusqlite::ffi::sqlite3_sql(cursor) };
        if !statement_sql.is_null()
            && unsafe { CStr::from_ptr(statement_sql) }.to_bytes() == target_sql
        {
            target = cursor;
            break;
        }
    }
    if target.is_null() {
        return None;
    }
    let mut origins = Vec::with_capacity(column_count);
    for index in 0..column_count {
        let origin = unsafe {
            rusqlite::ffi::sqlite3_column_origin_name(target, index as std::os::raw::c_int)
        };
        let origin = if origin.is_null() {
            None
        } else {
            unsafe { CStr::from_ptr(origin) }
                .to_str()
                .ok()
                .map(ToOwned::to_owned)
        };
        origins.push(origin);
    }
    Some(origins)
}

fn sanitize_column_name(name: &str, index: usize, origin: Option<&str>) -> String {
    let safe = sanitize_identifier(name);
    // Do not let raw expression text or a malformed metadata name become an
    // SQL/query echo in the UI. Stable indexed labels also keep duplicate
    // aliases from colliding as React keys.
    let mut safe = if safe.is_empty() {
        format!("column_{}", index + 1)
    } else {
        safe
    };
    if origin.is_none() && safe != REDACTED_VALUE {
        safe = format!("column_{}", index + 1);
    }
    safe
}

pub const REDACTION_VERSION: &str = "v1";
const REDACTED_VALUE: &str = "[REDACTED]";
const REDACTED_PATH: &str = "[REDACTED_PATH]";

pub fn is_sensitive_name(name: &str) -> bool {
    // Normalize separators/casing so aliases such as `apiKey`,
    // `client-secret`, and `refreshToken` cannot evade the name policy.
    let normalized = name
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    [
        "secret",
        "token",
        "password",
        "passwd",
        "credential",
        "authorization",
        "cookie",
        "apikey",
        "accesskey",
        "privatekey",
        "clientsecret",
        "refreshtoken",
        "rawbody",
        // Usernames and email/login identifiers are personal data too. Treat
        // both compact and separated spellings (`user_name`, `user-name`) as
        // sensitive source columns so a direct SELECT cannot expose them.
        "user",
        "username",
        "userid",
        "login",
        "email",
    ]
    .iter()
    .any(|needle| {
        normalized
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    })
}

/// Redact credentials, auth headers, common token formats, and filesystem
/// paths from free-form log/text values. It intentionally errs on the side of
/// masking a value; the inspector is for shape and health, not raw payloads.
pub fn redact_text(input: &str, app_id: &str) -> String {
    let mut limit = input.len().min(MAX_CELL_BYTES);
    while !input.is_char_boundary(limit) {
        limit -= 1;
    }
    let mut output = input[..limit].to_string();
    if limit < input.len() {
        output.push_str("…[truncated]");
    }
    for key in [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "client_secret",
        "refresh_token",
        "username",
        "user_name",
        "user-name",
        "user",
        "login",
        "email",
        "e-mail",
    ] {
        output = redact_key_value(&output, key);
    }
    let mut words = Vec::new();
    for word in output.split_whitespace() {
        let clean = word.trim_matches(|char: char| "\"'`()[]{}<>.,;".contains(char));
        let lower = clean.to_ascii_lowercase();
        if looks_like_secret_token(&lower) || is_sensitive_value(clean) || looks_like_email(clean) {
            words.push(REDACTED_VALUE.to_string());
        } else if looks_like_path(clean) && clean != app_id {
            words.push(REDACTED_PATH.to_string());
        } else {
            words.push(word.to_string());
        }
    }
    words.join(" ")
}

fn looks_like_secret_token(lower: &str) -> bool {
    lower.starts_with("ghp_")
        || lower.starts_with("gho_")
        || lower.starts_with("ghs_")
        || lower.starts_with("ghu_")
        || lower.starts_with("ghr_")
        || lower.starts_with("github_pat_")
        || lower.starts_with("glpat-")
        || lower.starts_with("npm_")
        || lower.starts_with("pypi-")
        || lower.starts_with("sk-")
        || lower.starts_with("sk_live_")
        || lower.starts_with("xoxb-")
        || lower.starts_with("xoxp-")
        || lower.starts_with("xoxs-")
        || lower.starts_with("akia")
        || lower.starts_with("aiza")
        || lower.starts_with("ya29.")
        || (lower.starts_with("eyj") && lower.matches('.').count() >= 2)
}

fn redact_key_value(input: &str, key: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let key_lower = key.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find(&key_lower) {
        let start = cursor + relative;
        let before_ok = start == 0
            || !lower.as_bytes()[start - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[start - 1] != b'_'
                && lower.as_bytes()[start - 1] != b'-';
        let after = start + key_lower.len();
        let after_ok = after == lower.len()
            || !lower.as_bytes()[after].is_ascii_alphanumeric()
                && lower.as_bytes()[after] != b'_'
                && lower.as_bytes()[after] != b'-';
        if !before_ok || !after_ok {
            output.push_str(&input[cursor..after]);
            cursor = after;
            continue;
        }
        output.push_str(&input[cursor..after]);
        let mut end = after;
        // Preserve a quoted key's closing quote and the key/value separator,
        // but never copy bytes from the value itself.
        if start > 0
            && end < input.len()
            && matches!(input.as_bytes()[start - 1], b'"' | b'\'')
            && input.as_bytes()[end] == input.as_bytes()[start - 1]
        {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        while end < input.len() && matches!(input.as_bytes()[end], b' ' | b'\t') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        if end < input.len() && matches!(input.as_bytes()[end], b':' | b'=') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        } else {
            output.push(':');
        }
        while end < input.len() && matches!(input.as_bytes()[end], b' ' | b'\t') {
            output.push(input.as_bytes()[end] as char);
            end += 1;
        }
        let is_header = matches!(
            key_lower.as_str(),
            "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
        );
        if is_header {
            while end < input.len() && !matches!(input.as_bytes()[end], b'\n' | b'\r') {
                end += 1;
            }
            output.push_str(REDACTED_VALUE);
        } else if end < input.len() && matches!(input.as_bytes()[end], b'"' | b'\'') {
            let quote = input.as_bytes()[end];
            output.push(quote as char);
            end += 1;
            let mut closed = false;
            while end < input.len() {
                match input.as_bytes()[end] {
                    b'\\' if end + 1 < input.len() => end += 2,
                    value if value == quote => {
                        end += 1;
                        closed = true;
                        break;
                    }
                    _ => end += 1,
                }
            }
            output.push_str(REDACTED_VALUE);
            if closed {
                output.push(quote as char);
            }
        } else {
            while end < input.len()
                && !input.as_bytes()[end].is_ascii_whitespace()
                && !matches!(input.as_bytes()[end], b',' | b'}' | b']')
            {
                end += 1;
            }
            output.push_str(REDACTED_VALUE);
        }
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn looks_like_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .get(1..3)
            .is_some_and(|prefix| prefix == ":\\" || prefix == ":/")
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with(".\\")
        || value.starts_with("..\\")
        || value.contains('/')
        || value.contains('\\')
        || value.contains("\\Users\\")
        || value.contains("/home/")
}

fn is_sensitive_value(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "secret",
        "password",
        "passwd",
        "credential",
        "authorization",
        "cookie",
        "api_key",
        "apikey",
        "bearer",
        "token",
    ]
    .iter()
    .any(|needle| lower.contains(*needle))
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !value.chars().any(char::is_control)
}

fn sanitize_identifier(value: &str) -> String {
    let mut output = value
        .chars()
        .filter(|char| !char.is_control())
        .take(128)
        .collect::<String>();
    if is_sensitive_name(&output) || looks_like_path(&output) {
        output = REDACTED_VALUE.to_string();
    }
    output
}

fn safe_filename(value: &str) -> String {
    let output: String = value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_'))
        .take(64)
        .collect();
    if output.is_empty() {
        "query".to_string()
    } else {
        output
    }
}

fn valid_sql_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn opaque_id(kind: &str, input: &str) -> String {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    digest.update(input.as_bytes());
    digest.update(sequence.to_le_bytes());
    digest.update(now.to_le_bytes());
    format!("{kind}-{:x}", digest.finalize())
}

fn safe_database_metadata(
    data_root: &Path,
    identifier: &str,
    path: &Path,
) -> Result<Option<Metadata>, DatabasePathState> {
    if !valid_identifier(identifier)
        || !data_root.is_absolute()
        || path.to_string_lossy().len() > 4096
    {
        return Err(DatabasePathState::Unsafe);
    }
    reject_link_components(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(DatabasePathState::Unreadable),
    };
    if is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(DatabasePathState::Unsafe);
    }
    // SQLite may consult these sibling files in rollback/WAL mode. Reject
    // links and non-regular sidecars before opening so a diagnostic request
    // cannot be redirected outside the catalog-derived data root.
    validate_sidecars(path).map_err(|_| DatabasePathState::Unsafe)?;
    let canonical_root = fs::canonicalize(data_root).map_err(|_| DatabasePathState::Unreadable)?;
    let canonical_path = fs::canonicalize(path).map_err(|_| DatabasePathState::Unreadable)?;
    if !path_is_within(&canonical_root, &canonical_path) {
        return Err(DatabasePathState::Unsafe);
    }
    Ok(Some(metadata))
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.starts_with("com.devbox.")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

fn reject_link_components(candidate: &Path) -> Result<(), DatabasePathState> {
    let mut current = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => return Err(DatabasePathState::Unsafe),
            Component::Normal(value) => current.push(value),
        }
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => return Err(DatabasePathState::Unsafe),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(DatabasePathState::Unreadable),
        }
    }
    Ok(())
}

/// Check a catalog-derived path without following a link/reparse component.
/// The target may not exist yet (for example, an app's `logs` directory), so
/// this deliberately validates all existing ancestors and lexical containment
/// rather than requiring a final `canonicalize`.
pub(crate) fn safe_derived_path(root: &Path, candidate: &Path) -> bool {
    root.is_absolute() && candidate.starts_with(root) && reject_link_components(candidate).is_ok()
}

fn path_is_within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_path(root) == normalize_windows_path(candidate)
            || normalize_windows_path(candidate)
                .starts_with(&format!("{}\\", normalize_windows_path(root)))
    }
    #[cfg(not(windows))]
    {
        candidate == root || candidate.starts_with(root)
    }
}

#[cfg(windows)]
fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(not(windows))]
pub(crate) fn is_link_or_reparse(metadata: &Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(crate) fn is_link_or_reparse(metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn fingerprint(metadata: &Metadata) -> DatabaseFingerprint {
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    DatabaseFingerprint {
        byte_length: metadata.len(),
        modified_ns,
        file_identity: file_identity(metadata),
    }
}

#[cfg(unix)]
fn file_identity(metadata: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn file_identity(metadata: &Metadata) -> Option<(u64, u64)> {
    use std::os::windows::fs::MetadataExt;
    Some((
        u64::from(metadata.volume_serial_number()),
        metadata.file_index(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_metadata: &Metadata) -> Option<(u64, u64)> {
    None
}

fn current_database_fingerprint(path: &Path) -> Option<DatabaseFingerprint> {
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| !is_link_or_reparse(metadata) && metadata.is_file())
        .map(|metadata| fingerprint(&metadata))
}

fn sanitize_value_text(value: &str, app_id: &str) -> String {
    let mut output = redact_text(value, app_id);
    if output.len() > MAX_CELL_BYTES {
        let mut limit = MAX_CELL_BYTES;
        while !output.is_char_boundary(limit) {
            limit -= 1;
        }
        output.truncate(limit);
        output.push_str("…[truncated]");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        root: PathBuf,
        db: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!("devbox-inspector-{id}"));
            let db_dir = root.join("com.devbox.testapp");
            fs::create_dir_all(&db_dir).unwrap();
            let db = db_dir.join("data.db");
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE events (id INTEGER, message TEXT, token TEXT, path TEXT, username TEXT, email TEXT);\
                 CREATE VIEW event_view AS SELECT id, message FROM events;\
                 INSERT INTO events VALUES (1, 'hello', 'secret-value', '/home/alice/project', 'alice', 'alice@example.com');",
            )
            .unwrap();
            drop(conn);
            Self { root, db }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            schema_version: 2,
            catalog_revision: Some(1),
            apps: vec![CatalogApp {
                id: "testapp".into(),
                display_name: "Test App".into(),
                product_name: "Test".into(),
                identifier: "com.devbox.testapp".into(),
                cargo_package: "testapp".into(),
                app_dir: "apps/testapp".into(),
                release: true,
                manager_visible: true,
                self_managed: false,
                accepts: vec![],
                produces: vec![],
                actions: vec![],
            }],
        }
    }

    fn request(sql: &str) -> DataQueryRequest {
        DataQueryRequest {
            app_id: "testapp".into(),
            sql: sql.into(),
            query_id: "test-query".into(),
            expected_revision: None,
        }
    }

    #[test]
    fn discovers_schema_without_returning_a_raw_path() {
        let fixture = Fixture::new();
        let snapshot = inspect_databases(&catalog(), &fixture.root, None).unwrap();
        let database = &snapshot.databases[0];
        assert_eq!(database.state, "available");
        assert_eq!(database.tables[0].name, "events");
        assert!(database.tables[0].row_count.is_some());
        assert!(!serde_json::to_string(database).unwrap().contains("data.db"));
    }

    #[test]
    fn write_attach_and_pragma_are_rejected() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let before = fs::read(&fixture.db).unwrap();
        for sql in [
            "UPDATE events SET id = 2",
            "ATTACH 'other.db' AS other",
            "PRAGMA journal_mode = WAL",
            "DELETE FROM events",
            "SELECT sql FROM sqlite_schema",
        ] {
            assert_eq!(
                preview_query(&catalog(), &fixture.root, &request(sql), cancel.clone())
                    .unwrap_err(),
                QueryFailure::QueryRejected
            );
        }
        let after = fs::read(&fixture.db).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn query_redacts_secret_path_and_auth_values() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let (result, _) = preview_query(
            &catalog(),
            &fixture.root,
            &request("SELECT message, token, path, username, email FROM events"),
            cancel,
        )
        .unwrap();
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("secret-value"));
        assert!(!json.contains("/home/alice"));
        assert!(!json.contains("alice@example.com"));
        assert!(!json.contains("\"alice\""));
        assert!(json.contains(REDACTED_VALUE));
        let redacted = redact_text("Authorization: Bearer top-secret /home/alice/x", "testapp");
        assert!(redacted.contains(REDACTED_VALUE));
        assert!(!redacted.contains("top-secret"));
    }

    #[test]
    fn aliases_and_expressions_cannot_bypass_source_redaction() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let (result, _) = preview_query(
            &catalog(),
            &fixture.root,
            &request(
                "SELECT message, token AS harmless_label, token || '-suffix' AS transformed FROM events",
            ),
            cancel,
        )
        .unwrap();
        assert_eq!(result.rows[0][0], serde_json::json!("hello"));
        assert_eq!(result.rows[0][1], serde_json::json!(REDACTED_VALUE));
        assert_eq!(result.rows[0][2], serde_json::json!(REDACTED_VALUE));
        // Expression text is never reflected as a column label. SQLite's
        // authorizer replaces the sensitive source with NULL, so its alias is
        // intentionally reduced to an indexed label as well.
        assert_eq!(result.columns[0], "message");
        assert_eq!(result.columns[1], "column_2");
        assert_eq!(result.columns[2], "column_3");
    }

    #[test]
    fn sensitive_columns_cannot_be_used_as_predicate_oracles() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let correct_guess = preview_query(
            &catalog(),
            &fixture.root,
            &request("SELECT message FROM events WHERE token = 'secret-value'"),
            cancel.clone(),
        )
        .unwrap()
        .0;
        let wrong_guess = preview_query(
            &catalog(),
            &fixture.root,
            &request("SELECT message FROM events WHERE token = 'wrong-value'"),
            cancel,
        )
        .unwrap()
        .0;
        assert_eq!(correct_guess.row_count, 0);
        assert_eq!(correct_guess.rows, wrong_guess.rows);
    }

    #[test]
    fn pragma_table_functions_and_large_value_functions_are_rejected() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        for sql in [
            "SELECT * FROM pragma_table_info('events')",
            "SELECT zeroblob(1000000000)",
            "SELECT randomblob(1000000000)",
        ] {
            assert_eq!(
                preview_query(&catalog(), &fixture.root, &request(sql), cancel.clone())
                    .unwrap_err(),
                QueryFailure::QueryRejected
            );
        }
    }

    #[test]
    fn cancelled_query_fails_closed() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            preview_query(
                &catalog(),
                &fixture.root,
                &request("SELECT * FROM events"),
                cancel,
            )
            .unwrap_err(),
            QueryFailure::Cancelled
        );
    }

    #[test]
    fn row_and_result_limits_are_explicit() {
        assert_eq!(MAX_ROWS, 1_000);
        assert_eq!(MAX_RESULT_BYTES, 1024 * 1024);
        assert_eq!(QUERY_TIMEOUT, Duration::from_secs(2));
        assert_eq!(MAX_DATABASE_BYTES, 512 * 1024 * 1024);
    }

    #[test]
    fn sqlite_connection_limits_are_applied_before_user_sql() {
        let connection = Connection::open_in_memory().unwrap();
        configure_sqlite_limits(&connection);
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_LENGTH),
            MAX_CELL_BYTES as i32
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_SQL_LENGTH),
            MAX_QUERY_BYTES as i32
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_COLUMN),
            MAX_COLUMNS as i32
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_EXPR_DEPTH),
            SQLITE_MAX_EXPRESSION_DEPTH
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_COMPOUND_SELECT),
            SQLITE_MAX_COMPOUND_SELECTS
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_VDBE_OP),
            SQLITE_MAX_VDBE_OPS
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_FUNCTION_ARG),
            SQLITE_MAX_FUNCTION_ARGS
        );
        assert_eq!(connection.limit(Limit::SQLITE_LIMIT_ATTACHED), 0);
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_LIKE_PATTERN_LENGTH),
            SQLITE_MAX_LIKE_PATTERN_BYTES
        );
        assert_eq!(
            connection.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER),
            SQLITE_MAX_VARIABLES
        );
        assert_eq!(connection.limit(Limit::SQLITE_LIMIT_WORKER_THREADS), 0);
    }

    #[test]
    fn row_limit_truncates_a_large_read_without_writing_during_preview() {
        let fixture = Fixture::new();
        let conn = Connection::open(&fixture.db).unwrap();
        let mut insert = conn
            .prepare("INSERT INTO events (id, message) VALUES (?1, ?2)")
            .unwrap();
        for id in 2..=(MAX_ROWS + 1) {
            insert.execute((id as i64, "bounded")).unwrap();
        }
        drop(insert);
        drop(conn);
        let before = fs::read(&fixture.db).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let (result, _) = preview_query(
            &catalog(),
            &fixture.root,
            &request("SELECT id FROM events ORDER BY id"),
            cancel,
        )
        .unwrap();
        assert_eq!(result.rows.len(), MAX_ROWS);
        assert!(result.truncated);
        assert_eq!(result.row_count, MAX_ROWS);
        assert_eq!(before, fs::read(&fixture.db).unwrap());
    }

    #[test]
    fn recursive_query_is_stopped_by_the_timeout_budget() {
        let fixture = Fixture::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let request = request(
            "WITH RECURSIVE numbers(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM numbers) SELECT count(*) FROM numbers",
        );
        let metadata = fs::metadata(&fixture.db).unwrap();
        assert_eq!(
            execute_query(
                &fixture.db,
                "testapp",
                &request.sql,
                cancel,
                Duration::from_millis(50),
                Some(fingerprint(&metadata)),
            )
            .unwrap_err(),
            QueryFailure::TimedOut
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_database_component_is_rejected() {
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        let link = fixture.root.join("com.devbox.linkapp");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert_eq!(
            safe_database_metadata(&fixture.root, "com.devbox.linkapp", &link.join("data.db"))
                .unwrap_err(),
            DatabasePathState::Unsafe
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_database_sidecar_is_rejected_before_open() {
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside-wal");
        fs::write(&outside, b"not a sqlite wal").unwrap();
        std::os::unix::fs::symlink(&outside, fixture.db.with_file_name("data.db-wal")).unwrap();
        assert_eq!(
            preview_query(
                &catalog(),
                &fixture.root,
                &request("SELECT message FROM events"),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap_err(),
            QueryFailure::UnsafePath
        );
    }

    #[cfg(unix)]
    #[test]
    fn opened_database_stays_bound_when_catalog_path_is_replaced() {
        let fixture = Fixture::new();
        let metadata = fs::metadata(&fixture.db).unwrap();
        let connection = open_read_only(&fixture.db, Some(fingerprint(&metadata))).unwrap();

        let moved = fixture.db.with_file_name("data.db-original");
        fs::rename(&fixture.db, &moved).unwrap();
        let replacement = Connection::open(&fixture.db).unwrap();
        replacement
            .execute_batch(
                "CREATE TABLE replacement (message TEXT); INSERT INTO replacement VALUES ('new');",
            )
            .unwrap();
        drop(replacement);
        let outside = fixture.root.join("late-wal");
        fs::write(&outside, b"not a sqlite wal").unwrap();
        std::os::unix::fs::symlink(&outside, fixture.db.with_file_name("data.db-wal")).unwrap();

        let value: String = connection
            .query_row("SELECT message FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "hello");
    }

    #[test]
    fn export_is_bounded_and_never_contains_query_text() {
        let result = DataQueryResult {
            preview_id: "query-id".into(),
            query_id: "opaque".into(),
            app_id: "testapp".into(),
            database_revision: "revision".into(),
            columns: vec!["message".into()],
            rows: vec![vec![serde_json::Value::String("ok".into())]],
            row_count: 1,
            result_bytes: 4,
            truncated: false,
            elapsed_ms: 1,
        };
        let export = export_query(&result, ExportFormat::Json).unwrap();
        assert!(export.content.contains("redactionVersion"));
        assert!(!export.content.contains("SELECT"));
    }

    #[test]
    fn csv_escapes_formula_like_text() {
        let result = DataQueryResult {
            preview_id: "query-id".into(),
            query_id: "opaque".into(),
            app_id: "testapp".into(),
            database_revision: "revision".into(),
            columns: vec!["name".into()],
            rows: vec![
                vec![serde_json::Value::String(
                    "=HYPERLINK(\"https://evil\")".into(),
                )],
                vec![serde_json::Value::String(" +SUM(A1:A2)".into())],
                vec![serde_json::Value::Number(serde_json::Number::from(-1))],
            ],
            row_count: 3,
            result_bytes: 64,
            truncated: false,
            elapsed_ms: 1,
        };
        let export = export_query(&result, ExportFormat::Csv).unwrap();
        assert!(export.content.contains("'=HYPERLINK"));
        assert!(export.content.contains("' +SUM"));
        assert!(export.content.contains("-1"));
        assert!(!export.content.contains("\n=HYPERLINK"));
    }

    #[test]
    fn name_and_username_redaction_normalize_common_variants() {
        assert!(is_sensitive_name("client-secret"));
        assert!(is_sensitive_name("refreshToken"));
        assert!(is_sensitive_name("user_name"));
        assert!(is_sensitive_name("email"));
        let redacted = redact_text(
            "username: alice email=alice@example.com user_name=bob",
            "testapp",
        );
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("bob"));

        let json = redact_text(
            r#"{"username":"alice","email":"alice@example.com","user_name":"bob","login":"carol"}"#,
            "testapp",
        );
        assert!(!json.contains("alice"));
        assert!(!json.contains("bob"));
        assert!(!json.contains("carol"));
        let escaped = redact_text(r#"{"username":"al\"ice","mode":"safe"}"#, "testapp");
        assert!(!escaped.contains("ice"));
        assert!(escaped.contains("mode"));
    }
}
