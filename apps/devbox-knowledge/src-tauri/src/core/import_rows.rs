//! Authoritative rows only. Call on private, unselected destination generations.
//! Receipts are logical-source based, so a later snapshot cannot resurrect a
//! destination deletion or overwrite a newer edit. Source conflicts are reported.
use rusqlite::{
    params, params_from_iter,
    types::{Value, ValueRef},
    Connection, OptionalExtension,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Notes,
    Activity,
    Search,
}
impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Activity => "activity",
            Self::Search => "search",
        }
    }
    pub fn identifier(self) -> &'static str {
        match self {
            Self::Notes => "com.devbox.knowledgebase",
            Self::Activity => "com.devbox.lifelog",
            Self::Search => "com.devbox.everythingplus",
        }
    }
}
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub imported: u64,
    pub repeated: u64,
    pub conflicts: u64,
    pub retired: u64,
    pub reserved_root_ids: u64,
}
const MAX_ROWS: usize = 500_000;
const MAX_TOTAL: usize = 128 * 1024 * 1024;
const MAX_CELL: usize = 1024 * 1024;
fn sql<T>(value: rusqlite::Result<T>) -> Result<T, String> {
    value.map_err(|_| "import_database_invalid".into())
}
fn text(value: &Value) -> Result<&str, String> {
    if let Value::Text(s) = value {
        Ok(s)
    } else {
        Err("import_row_invalid".into())
    }
}
fn int(value: &Value) -> Result<i64, String> {
    if let Value::Integer(i) = value {
        Ok(*i)
    } else {
        Err("import_row_invalid".into())
    }
}
fn positive(value: &Value) -> Result<i64, String> {
    let id = int(value)?;
    if id <= 0 {
        return Err("import_row_invalid".into());
    }
    Ok(id)
}
fn digest(row: &[Value]) -> String {
    let mut hash = Sha256::new();
    for value in row {
        match value {
            Value::Null => hash.update([0]),
            Value::Integer(i) => {
                hash.update([1]);
                hash.update(i.to_be_bytes());
            }
            Value::Text(s) => {
                hash.update([2]);
                hash.update((s.len() as u64).to_be_bytes());
                hash.update(s.as_bytes());
            }
            _ => unreachable!("walk only admits integer, text and null"),
        }
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
struct Budget {
    cancelled: Arc<AtomicBool>,
    started: Instant,
    rows: usize,
    bytes: usize,
}
impl Budget {
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err("import_cancelled".into());
        }
        if self.started.elapsed() > Duration::from_secs(30) {
            return Err("import_timed_out".into());
        }
        Ok(())
    }
}
// Table/column identifiers below are closed source literals, never IPC arguments.
fn schema(conn: &Connection, table: &str, columns: &str) -> Result<(), String> {
    let kind: Option<String> = sql(conn
        .query_row(
            "SELECT type FROM sqlite_master WHERE name=?1",
            [table],
            |r| r.get(0),
        )
        .optional())?;
    if kind.as_deref() != Some("table") {
        return Err("import_schema_unsupported".into());
    }
    let mut statement = sql(conn.prepare(&format!("PRAGMA table_info({table})")))?;
    let names = sql(statement.query_map([], |r| r.get::<_, String>(1)))?;
    let names = sql(names.collect::<rusqlite::Result<Vec<_>>>())?;
    if names.join(",") != columns {
        return Err("import_schema_unsupported".into());
    }
    Ok(())
}
fn inventory(connection: &Connection, source: Source, destination: bool) -> Result<(), String> {
    let allowed: &[&str] = match source {
        Source::Notes => &[
            "settings",
            "note_templates",
            "docs",
            "docs_fts",
            "docs_fts_data",
            "docs_fts_idx",
            "docs_fts_docsize",
            "docs_fts_config",
            "doc_link_keys",
            "wikilinks",
        ],
        Source::Activity => &["settings", "sessions", "knowledge_draft_history"],
        Source::Search => &[
            "roots",
            "files",
            "files_fts",
            "files_fts_data",
            "files_fts_idx",
            "files_fts_docsize",
            "files_fts_config",
            "file_content",
            "file_content_fts",
            "file_content_fts_data",
            "file_content_fts_idx",
            "file_content_fts_docsize",
            "file_content_fts_config",
            "meta",
            "saved_queries",
        ],
    };
    let mut statement = sql(connection.prepare(
        "SELECT name FROM sqlite_master WHERE type IN ('table','view') ORDER BY name LIMIT 65",
    ))?;
    let rows = sql(statement.query_map([], |r| r.get::<_, String>(0)))?;
    for (index, name) in rows.enumerate() {
        let name = sql(name)?;
        if index >= 64
            || (!allowed.contains(&name.as_str())
                && !name.starts_with("sqlite_")
                && !(destination && name == "knowledge_import_rows_v1"))
        {
            return Err("import_schema_unsupported".into());
        }
    }
    Ok(())
}

fn walk(
    conn: &Connection,
    table: &str,
    columns: &str,
    budget: &mut Budget,
    mut apply: impl FnMut(Vec<Value>) -> Result<(), String>,
) -> Result<(), String> {
    schema(conn, table, columns)?;
    let width = columns.split(',').count();
    let mut statement = sql(conn.prepare(&format!("SELECT {columns} FROM {table} ORDER BY 1")))?;
    let mut rows = sql(statement.query([]))?;
    while let Some(row) = sql(rows.next())? {
        budget.check()?;
        budget.rows += 1;
        if budget.rows > MAX_ROWS {
            return Err("import_limit_exceeded".into());
        }
        let mut values = Vec::with_capacity(width);
        for index in 0..width {
            let value = match sql(row.get_ref(index))? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(i) => Value::Integer(i),
                ValueRef::Text(bytes) if bytes.len() <= MAX_CELL => {
                    budget.bytes = budget
                        .bytes
                        .checked_add(bytes.len())
                        .ok_or("import_limit_exceeded")?;
                    if budget.bytes > MAX_TOTAL {
                        return Err("import_limit_exceeded".into());
                    }
                    Value::Text(
                        std::str::from_utf8(bytes)
                            .map_err(|_| "import_row_invalid")?
                            .into(),
                    )
                }
                _ => return Err("import_row_invalid".into()),
            };
            values.push(value);
        }
        apply(values)?;
    }
    Ok(())
}
struct Merge<'a> {
    destination: &'a Connection,
    source: Source,
    report: Report,
}
impl Merge<'_> {
    fn receipt(
        &mut self,
        table: &str,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<String>, String> {
        let value: Option<(String,String)>=sql(self.destination.query_row("SELECT fingerprint,destination_id FROM knowledge_import_rows_v1 WHERE source=?1 AND source_table=?2 AND source_id=?3", params![self.source.key(),table,key], |r| Ok((r.get(0)?,r.get(1)?))).optional())?;
        Ok(value.map(|(previous, id)| {
            if previous == fingerprint {
                self.report.repeated += 1;
            } else {
                self.report.conflicts += 1;
            }
            id
        }))
    }
    fn record(
        &self,
        table: &str,
        key: &str,
        fingerprint: &str,
        destination: &str,
    ) -> Result<(), String> {
        sql(self.destination.execute("INSERT INTO knowledge_import_rows_v1(source,source_table,source_id,fingerprint,destination_id) VALUES(?1,?2,?3,?4,?5)", params![self.source.key(),table,key,fingerprint,destination]))?;
        Ok(())
    }
    fn insert(&mut self, table: &str, columns: &str, values: &[Value]) -> Result<i64, String> {
        let limit = match table {
            "note_templates" => Some(knowledge_base_lib::component::IMPORT_TEMPLATE_LIMIT),
            "saved_queries" => Some(everything_plus_lib::component::IMPORT_SAVED_QUERY_LIMIT),
            _ => None,
        };
        if let Some(limit) = limit {
            let count: i64 = sql(self.destination.query_row(
                &format!("SELECT count(*) FROM {table}"),
                [],
                |r| r.get(0),
            ))?;
            if count < 0 || count as usize >= limit {
                return Err("import_limit_exceeded".into());
            }
        }
        let placeholders = vec!["?"; values.len()].join(",");
        sql(self.destination.execute(
            &format!("INSERT INTO {table}({columns}) VALUES({placeholders})"),
            params_from_iter(values),
        ))?;
        self.report.imported += 1;
        Ok(self.destination.last_insert_rowid())
    }
    fn settings(&mut self, row: Vec<Value>, fresh: bool) -> Result<(), String> {
        let key = text(&row[0])?;
        let value = text(&row[1])?;
        if key.len() > 256 || value.len() > MAX_CELL {
            return Err("import_row_invalid".into());
        }
        let fp = digest(&row);
        if self.receipt("settings", key, &fp)?.is_some() {
            return Ok(());
        }
        if key == "wikilink-schema"
            || key == "product_activity_collection_v1"
            || key.starts_with("devbox_knowledge_")
        {
            self.report.retired += 1;
        } else {
            let previous: Option<String> = sql(self
                .destination
                .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                    r.get(0)
                })
                .optional())?;
            if self.source == Source::Notes && key == "root" && fresh {
                everything_plus_lib::component::normalize_import_root(value)?;
                if value.is_empty() || value.len() > 32768 || value.chars().any(char::is_control) {
                    return Err("import_row_invalid".into());
                }
                sql(self.destination.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]))?;
                self.report.imported += 1;
            } else if previous.is_some() {
                self.report.conflicts += 1;
            } else {
                self.insert("settings", "key,value", &row)?;
            }
        }
        self.record("settings", key, &fp, key)
    }
    fn template(&mut self, mut row: Vec<Value>) -> Result<(), String> {
        let key = positive(&row[0])?.to_string();
        let fp = digest(&row);
        if self.receipt("note_templates", &key, &fp)?.is_some() {
            return Ok(());
        }
        let original = text(&row[1])?.to_owned();
        let content = text(&row[2])?;
        knowledge_base_lib::component::validate_import_template(&original, content)?;
        if original.is_empty()
            || original.len() > 128
            || content.len() > 65536
            || positive(&row[3])? > int(&row[4])?
        {
            return Err("import_row_invalid".into());
        }
        for attempt in 0..=1000 {
            let candidate = text(&row[1])?;
            let exists = sql(self
                .destination
                .prepare("SELECT 1 FROM note_templates WHERE name=?1 COLLATE NOCASE"))?
            .exists([candidate])
            .map_err(|_| "import_database_invalid")?;
            if !exists {
                break;
            }
            if attempt == 1000 {
                return Err("import_limit_exceeded".into());
            }
            if attempt == 0 {
                self.report.conflicts += 1;
            }
            let suffix = format!(" (legacy {key}-{})", attempt + 1);
            let mut end = original.len().min(128 - suffix.len());
            while !original.is_char_boundary(end) {
                end -= 1;
            }
            row[1] = Value::Text(format!("{}{suffix}", &original[..end]));
        }
        let id = self.insert(
            "note_templates",
            "name,content,created_ts,updated_ts",
            &row[1..],
        )?;
        self.record("note_templates", &key, &fp, &id.to_string())
    }
    fn session(&mut self, row: Vec<Value>) -> Result<(), String> {
        let key = positive(&row[0])?.to_string();
        let fp = digest(&row);
        if self.receipt("sessions", &key, &fp)?.is_some() {
            return Ok(());
        }
        if text(&row[1])?.len() > 4096
            || text(&row[2])?.len() > 65536
            || int(&row[3])? > int(&row[4])?
            || int(&row[5])? < 0
        {
            return Err("import_row_invalid".into());
        }
        let existing: Option<i64>=sql(self.destination.query_row("SELECT id FROM sessions WHERE app=?1 AND title=?2 AND start_ts=?3 AND end_ts=?4 AND duration_ms=?5",params_from_iter(&row[1..]),|r|r.get(0)).optional())?;
        let id = match existing {
            Some(id) => {
                self.report.repeated += 1;
                id
            }
            None => self.insert(
                "sessions",
                "app,title,start_ts,end_ts,duration_ms",
                &row[1..],
            )?,
        };
        self.record("sessions", &key, &fp, &id.to_string())
    }
    fn history(&mut self, mut row: Vec<Value>) -> Result<(), String> {
        let key = positive(&row[0])?.to_string();
        let fp = digest(&row);
        if self
            .receipt("knowledge_draft_history", &key, &fp)?
            .is_some()
        {
            return Ok(());
        }
        let existing: Option<i64> = sql(self
            .destination
            .query_row(
                "SELECT id FROM knowledge_draft_history WHERE handoff_id=?1",
                [text(&row[1])?],
                |r| r.get(0),
            )
            .optional())?;
        let id = match existing {
            Some(id) => {
                self.report.conflicts += 1;
                id
            }
            None => {
                if matches!(text(&row[3])?, "pending" | "sent") {
                    row[3] = Value::Text("expired".into());
                    row[7] = Value::Integer(int(&row[7])?.max(int(&row[8])?));
                    self.report.retired += 1;
                }
                self.insert("knowledge_draft_history","handoff_id,kind,status,summary_json,sources_json,created_ts,updated_ts,expires_ts,regenerated_from",&row[1..])?
            }
        };
        self.record("knowledge_draft_history", &key, &fp, &id.to_string())
    }
    fn allocate_root(&self) -> Result<i64, String> {
        let maximum: i64 =
            sql(self
                .destination
                .query_row("SELECT COALESCE(MAX(id),0) FROM roots", [], |r| r.get(0)))?;
        let recorded: String = sql(self.destination.query_row(
            "SELECT value FROM meta WHERE key='next_root_id'",
            [],
            |r| r.get(0),
        ))?;
        let id = recorded
            .parse::<i64>()
            .map_err(|_| "import_database_invalid")?
            .max(maximum.checked_add(1).ok_or("import_limit_exceeded")?);
        if id <= 0 {
            return Err("import_database_invalid".into());
        }
        let next = id.checked_add(1).ok_or("import_limit_exceeded")?;
        sql(self.destination.execute(
            "UPDATE meta SET value=?1 WHERE key='next_root_id'",
            [next.to_string()],
        ))?;
        Ok(id)
    }
    fn root(&mut self, row: Vec<Value>) -> Result<(), String> {
        let key = positive(&row[0])?.to_string();
        let fp = digest(&row);
        if self.receipt("roots", &key, &fp)?.is_some() {
            return Ok(());
        }
        let path = everything_plus_lib::component::normalize_import_root(text(&row[1])?)?;
        let content = int(&row[2])?;
        if !matches!(content, 0 | 1) {
            return Err("import_row_invalid".into());
        }
        let existing: Option<i64> = sql(self
            .destination
            .query_row("SELECT id FROM roots WHERE path=?1", [&path], |r| r.get(0))
            .optional())?;
        let id = match existing {
            Some(id) => {
                self.report.conflicts += 1;
                id
            }
            None => {
                let id = self.allocate_root()?;
                self.insert(
                    "roots",
                    "id,path,content",
                    &[
                        Value::Integer(id),
                        Value::Text(path),
                        Value::Integer(content),
                    ],
                )?;
                id
            }
        };
        self.record("roots", &key, &fp, &id.to_string())
    }
    fn query(&mut self, mut row: Vec<Value>) -> Result<(), String> {
        let key = positive(&row[0])?.to_string();
        let fp = digest(&row);
        if self.receipt("saved_queries", &key, &fp)?.is_some() {
            return Ok(());
        }
        let raw = text(&row[3])?;
        let source_root = everything_plus_lib::component::import_filter_root(raw)?;
        let destination_root = match source_root {
            None => None,
            Some(id) => {
                let key = id.to_string();
                let mapped:Option<String>=sql(self.destination.query_row("SELECT destination_id FROM knowledge_import_rows_v1 WHERE source='search' AND source_table='roots' AND source_id=?1",[&key],|r|r.get(0)).optional())?;
                Some(match mapped {
                    Some(value) => value
                        .parse::<i64>()
                        .map_err(|_| "import_database_invalid")?,
                    None => {
                        let id = self.allocate_root()?;
                        self.record("roots", &key, "missing", &id.to_string())?;
                        self.report.reserved_root_ids += 1;
                        id
                    }
                })
            }
        };
        row[3] = Value::Text(everything_plus_lib::component::remap_import_filter(
            raw,
            destination_root,
        )?);
        everything_plus_lib::component::validate_import_saved_query(
            text(&row[1])?,
            text(&row[2])?,
            int(&row[4])?,
            int(&row[5])?,
        )?;
        let id = self.insert(
            "saved_queries",
            "name,query,filter_json,created_at,updated_at",
            &row[1..],
        )?;
        self.record("saved_queries", &key, &fp, &id.to_string())
    }
}

pub fn verify_legacy_binding(connection: &Connection, path: &str) -> Result<(), String> {
    let fingerprint: String = sql(connection.query_row("SELECT fingerprint FROM knowledge_import_rows_v1 WHERE source='notes' AND source_table='settings' AND source_id='root'", [], |r| r.get(0)))?;
    if fingerprint != digest(&[Value::Text("root".into()), Value::Text(path.into())]) {
        return Err("vault_binding_invalid".into());
    }
    Ok(())
}

/// Configuration mutations must not reinterpret a future store's tables as
/// today's derived cache. This validation performs no migration or row writes.
pub fn validate_owned_store(connection: &Connection, source: Source) -> Result<(), String> {
    inventory(connection, source, true)?;
    let version: i64 = sql(connection.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
    if version != 0 {
        return Err("import_schema_unsupported".into());
    }
    Ok(())
}

/// Rollback fingerprints omit derived indexes while retaining all user-owned
/// rows and logical ID receipts. The newer generation is kept after rollback.
pub fn authoritative_fingerprint(
    connection: &Connection,
    source: Source,
    cancelled: Arc<AtomicBool>,
) -> Result<String, String> {
    inventory(connection, source, true)?;
    let version: i64 = sql(connection.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if version != 0 {
        return Err("import_schema_unsupported".into());
    }
    let mut budget = Budget {
        cancelled,
        started: Instant::now(),
        rows: 0,
        bytes: 0,
    };
    let mut hash = Sha256::new();
    let tables: &[(&str, &str)] = match source {
        Source::Notes => &[("settings","key,value"), ("note_templates","id,name,content,created_ts,updated_ts")],
        Source::Activity => &[("settings","key,value"), ("sessions","id,app,title,start_ts,end_ts,duration_ms"), ("knowledge_draft_history","id,handoff_id,kind,status,summary_json,sources_json,created_ts,updated_ts,expires_ts,regenerated_from")],
        Source::Search => &[("meta","key,value"), ("roots","id,path,content"), ("saved_queries","id,name,query,filter_json,created_at,updated_at")],
    };
    for (table, columns) in tables {
        hash.update(table.as_bytes());
        walk(connection, table, columns, &mut budget, |row| {
            if source == Source::Notes
                && *table == "settings"
                && text(&row[0])? == "wikilink-schema"
            {
                return Ok(());
            }
            if source == Source::Search
                && *table == "meta"
                && !matches!(text(&row[0])?, "schema_version" | "next_root_id")
            {
                return Ok(());
            }
            hash.update(digest(&row).as_bytes());
            Ok(())
        })?;
    }
    if sql(connection.prepare("SELECT 1 FROM sqlite_master WHERE name='knowledge_import_rows_v1'"))?
        .exists([])
        .map_err(|_| "import_database_invalid")?
    {
        schema(
            connection,
            "knowledge_import_rows_v1",
            "source,source_table,source_id,fingerprint,destination_id",
        )?;
        let mut statement = sql(connection.prepare("SELECT source,source_table,source_id,fingerprint,destination_id FROM knowledge_import_rows_v1 ORDER BY source,source_table,source_id"))?;
        let mut rows = sql(statement.query([]))?;
        let mut receipts = 0;
        while let Some(row) = sql(rows.next())? {
            budget.check()?;
            receipts += 1;
            if receipts > MAX_ROWS {
                return Err("import_limit_exceeded".into());
            }
            for index in 0..5 {
                let ValueRef::Text(value) = sql(row.get_ref(index))? else {
                    return Err("import_row_invalid".into());
                };
                if value.len() > 1024 {
                    return Err("import_row_invalid".into());
                }
                hash.update((value.len() as u64).to_be_bytes());
                hash.update(value);
            }
        }
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn merge(
    source_path: &Path,
    destination: &mut Connection,
    source: Source,
    fresh: bool,
    cancelled: Arc<AtomicBool>,
) -> Result<Report, String> {
    devbox_filesystem::ensure_no_links(source_path).map_err(|_| "import_path_invalid")?;
    let source_db = sql(Connection::open_with_flags(
        source_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    sql(source_db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN"))?;
    let version: i64 = sql(source_db.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if version != 0 {
        return Err("import_schema_unsupported".into());
    }
    inventory(&source_db, source, false)?;
    inventory(destination, source, true)?;
    let mut budget = Budget {
        cancelled,
        started: Instant::now(),
        rows: 0,
        bytes: 0,
    };
    budget.check()?;
    let cancel = budget.cancelled.clone();
    let started = budget.started;
    source_db.progress_handler(
        1000,
        Some(move || cancel.load(Ordering::Acquire) || started.elapsed() > Duration::from_secs(30)),
    );
    let transaction = sql(destination.transaction())?;
    sql(transaction.execute_batch("CREATE TABLE IF NOT EXISTS knowledge_import_rows_v1(source TEXT NOT NULL, source_table TEXT NOT NULL, source_id TEXT NOT NULL, fingerprint TEXT NOT NULL, destination_id TEXT NOT NULL, PRIMARY KEY(source,source_table,source_id));"))?;
    schema(
        &transaction,
        "knowledge_import_rows_v1",
        "source,source_table,source_id,fingerprint,destination_id",
    )?;
    let mut merger = Merge {
        destination: &transaction,
        source,
        report: Report::default(),
    };
    match source {
        Source::Notes => {
            walk(&source_db, "settings", "key,value", &mut budget, |r| {
                merger.settings(r, fresh)
            })?;
            walk(
                &source_db,
                "note_templates",
                "id,name,content,created_ts,updated_ts",
                &mut budget,
                |r| merger.template(r),
            )?;
        }
        Source::Activity => {
            life_log_lib::component::validate_import_history(&source_db)?;
            walk(&source_db, "settings", "key,value", &mut budget, |r| {
                merger.settings(r, fresh)
            })?;
            walk(
                &source_db,
                "sessions",
                "id,app,title,start_ts,end_ts,duration_ms",
                &mut budget,
                |r| merger.session(r),
            )?;
            walk(&source_db,"knowledge_draft_history","id,handoff_id,kind,status,summary_json,sources_json,created_ts,updated_ts,expires_ts,regenerated_from",&mut budget,|r|merger.history(r))?;
        }
        Source::Search => {
            schema(&source_db, "meta", "key,value")?;
            let version: String = sql(source_db.query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            ))?;
            if version != "2" {
                return Err("import_schema_unsupported".into());
            }
            walk(&source_db, "roots", "id,path,content", &mut budget, |r| {
                merger.root(r)
            })?;
            walk(
                &source_db,
                "saved_queries",
                "id,name,query,filter_json,created_at,updated_at",
                &mut budget,
                |r| merger.query(r),
            )?;
        }
    }
    budget.check()?;
    if source == Source::Activity {
        life_log_lib::component::validate_import_history(&transaction)?;
    }
    let report = merger.report;
    sql(transaction.commit())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create(path: &Path, source: Source, vault: &Path) {
        match source {
            Source::Notes => {
                knowledge_base_lib::component::create_empty_store(path, vault).unwrap()
            }
            Source::Activity => life_log_lib::component::create_empty_store(path).unwrap(),
            Source::Search => everything_plus_lib::component::create_empty_store(path).unwrap(),
        }
    }
    fn count(db: &Connection, table: &str) -> i64 {
        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn run(path: &Path, db: &mut Connection, source: Source, fresh: bool) -> Report {
        merge(path, db, source, fresh, Arc::new(AtomicBool::new(false))).unwrap()
    }
    #[test]
    fn notes_keep_vault_bytes_and_templates_without_copying_derived_index() {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().join("legacy-vault");
        std::fs::create_dir(&vault).unwrap();
        std::fs::write(vault.join("note.md"), "[[other]] ![](asset.png)").unwrap();
        std::fs::write(vault.join("asset.png"), [0, 1, 2, 255]).unwrap();
        let original = temp.path().join("source.db");
        let destination = temp.path().join("destination.db");
        create(&original, Source::Notes, &vault);
        create(&destination, Source::Notes, &temp.path().join("private"));
        let source = Connection::open(&original).unwrap();
        source.execute_batch("INSERT INTO note_templates VALUES(8,'Same','legacy template',1,2); INSERT INTO settings VALUES('capture','user preference'); INSERT INTO docs(path,title,body,modified_ts) VALUES('note.md','derived','cache',1);").unwrap();
        let mut db = Connection::open(&destination).unwrap();
        db.execute_batch("INSERT INTO note_templates VALUES(8,'Same','new product edit',1,3);")
            .unwrap();
        let report = run(&original, &mut db, Source::Notes, true);
        assert_eq!(report.conflicts, 1);
        assert_eq!(count(&db, "note_templates"), 2);
        assert_eq!(count(&db, "docs"), 0);
        assert_eq!(
            db.query_row("SELECT value FROM settings WHERE key='root'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            vault.to_str().unwrap()
        );
        assert_eq!(
            db.query_row("SELECT content FROM note_templates WHERE id=8", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "new product edit"
        );
        assert_eq!(
            std::fs::read(vault.join("note.md")).unwrap(),
            b"[[other]] ![](asset.png)"
        );
        assert_eq!(
            std::fs::read(vault.join("asset.png")).unwrap(),
            [0, 1, 2, 255]
        );
        db.execute("DELETE FROM note_templates WHERE id<>8", [])
            .unwrap();
        source
            .execute(
                "UPDATE note_templates SET content='changed source' WHERE id=8",
                [],
            )
            .unwrap();
        let report = run(&original, &mut db, Source::Notes, false);
        assert_eq!(report.conflicts, 1);
        assert_eq!(count(&db, "note_templates"), 1);
        assert_eq!(count(&source, "docs"), 1);
    }
    #[test]
    fn activity_keeps_privacy_and_history_but_never_restores_consent_or_pending_delivery() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("source.db");
        let dest = temp.path().join("dest.db");
        create(&original, Source::Activity, temp.path());
        create(&dest, Source::Activity, temp.path());
        let source = Connection::open(&original).unwrap();
        source.execute_batch("INSERT INTO settings VALUES('privacy_rules','{\"maskAllTitles\":true}'); INSERT INTO settings VALUES('product_activity_collection_v1','true'); INSERT INTO settings VALUES('devbox_knowledge_close_to_tray_v1','true'); INSERT INTO sessions VALUES(7,'synthetic.exe','',1,2,1);").unwrap();
        let history: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/legacy-activity-history.json"))
                .unwrap();
        source.execute("INSERT INTO knowledge_draft_history VALUES(3,'0123456789abcdef0123456789abcdef','knowledge-draft/v1','pending',?1,?2,1,2,3,NULL)", [history["summary"].to_string(), history["sources"].to_string()]).unwrap();
        let mut db = Connection::open(&dest).unwrap();
        let report = run(&original, &mut db, Source::Activity, true);
        assert_eq!(report.retired, 3);
        assert_eq!(count(&db, "settings"), 1);
        assert_eq!(count(&db, "sessions"), 1);
        assert_eq!(
            db.query_row("SELECT status FROM knowledge_draft_history", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "expired"
        );
        run(&original, &mut db, Source::Activity, false);
        assert_eq!(count(&db, "sessions"), 1);
        assert_eq!(count(&db, "knowledge_draft_history"), 1);
        assert_eq!(
            source
                .query_row("SELECT status FROM knowledge_draft_history", [], |r| r
                    .get::<_, String>(
                    0
                ))
                .unwrap(),
            "pending"
        );
    }
    #[test]
    fn saved_queries_map_live_and_deleted_roots_without_reusing_tombstones_or_erasing_last_good() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("source.db");
        let dest = temp.path().join("dest.db");
        create(&original, Source::Search, temp.path());
        create(&dest, Source::Search, temp.path());
        let source = Connection::open(&original).unwrap();
        source.execute_batch("INSERT INTO roots VALUES(4,'C:/legacy',1); INSERT INTO saved_queries VALUES(1,'live','needle','{\"sourceRootId\":4}',1,2); INSERT INTO saved_queries VALUES(2,'deleted','needle','{\"sourceRootId\":8}',1,2);").unwrap();
        let mut db = Connection::open(&dest).unwrap();
        db.execute_batch("INSERT INTO roots VALUES(4,'C:/existing',0); UPDATE meta SET value='5' WHERE key='next_root_id'; INSERT INTO files(id,path,name,size,modified_ts,root_id) VALUES(1,'C:/existing/last-good.md','last-good.md',1,1,4);").unwrap();
        let report = run(&original, &mut db, Source::Search, false);
        assert_eq!(report.reserved_root_ids, 1);
        assert_eq!(count(&db, "files"), 1);
        let ids: Vec<i64> = {
            let mut stmt = db
                .prepare("SELECT filter_json FROM saved_queries ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(|r| {
                    serde_json::from_str::<serde_json::Value>(&r.unwrap()).unwrap()["sourceRootId"]
                        .as_i64()
                        .unwrap()
                })
                .collect()
        };
        assert_eq!(ids, vec![5, 6]);
        assert_eq!(
            db.query_row("SELECT value FROM meta WHERE key='next_root_id'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "7"
        );
        db.execute("DELETE FROM roots WHERE id=5", []).unwrap();
        source
            .execute("INSERT INTO roots VALUES(8,'C:/reused-source-id',1)", [])
            .unwrap();
        let report = run(&original, &mut db, Source::Search, false);
        assert_eq!(report.conflicts, 1);
        assert_eq!(count(&db, "roots"), 1);
        assert_eq!(count(&db, "saved_queries"), 2);
        assert!(!db
            .prepare("SELECT 1 FROM roots WHERE id=6")
            .unwrap()
            .exists([])
            .unwrap());
    }
    #[test]
    fn malformed_future_cancelled_and_oversized_rows_leave_destination_transaction_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("source.db");
        let dest = temp.path().join("dest.db");
        create(&original, Source::Notes, temp.path());
        create(&dest, Source::Notes, temp.path());
        let source = Connection::open(&original).unwrap();
        let mut db = Connection::open(&dest).unwrap();
        source
            .execute_batch(
                "INSERT INTO settings VALUES('capture','must rollback'); PRAGMA user_version=9;",
            )
            .unwrap();
        assert!(merge(
            &original,
            &mut db,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        source.execute_batch("PRAGMA user_version=0;").unwrap();
        assert!(merge(
            &original,
            &mut db,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(true))
        )
        .is_err());
        source
            .execute(
                "INSERT INTO settings VALUES('oversize',?1)",
                ["x".repeat(MAX_CELL + 1)],
            )
            .unwrap();
        assert!(merge(
            &original,
            &mut db,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        source
            .execute("DELETE FROM settings WHERE key='oversize'", [])
            .unwrap();
        source
            .execute_batch("ALTER TABLE note_templates ADD COLUMN future TEXT;")
            .unwrap();
        assert!(merge(
            &original,
            &mut db,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        assert_eq!(count(&db, "settings"), 1);
        assert_eq!(count(&db, "note_templates"), 0);
        assert!(!db
            .prepare("SELECT 1 FROM sqlite_master WHERE name='knowledge_import_rows_v1'")
            .unwrap()
            .exists([])
            .unwrap());
        assert_eq!(count(&source, "settings"), 2);
    }
    #[test]
    fn unknown_authoritative_tables_and_invalid_bindings_are_not_silently_discarded() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.db");
        let target = temp.path().join("target.db");
        create(&source, Source::Notes, temp.path());
        create(&target, Source::Notes, temp.path());
        let legacy = Connection::open(&source).unwrap();
        let mut destination = Connection::open(&target).unwrap();
        legacy.execute_batch("CREATE TABLE future_user_documents(id INTEGER,body TEXT); INSERT INTO future_user_documents VALUES(1,'preserve');").unwrap();
        assert!(merge(
            &source,
            &mut destination,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        legacy.execute_batch("DROP TABLE future_user_documents; UPDATE settings SET value='../relative-vault' WHERE key='root';").unwrap();
        assert!(merge(
            &source,
            &mut destination,
            Source::Notes,
            true,
            Arc::new(AtomicBool::new(false))
        )
        .is_err());
        assert_eq!(
            destination
                .query_row("SELECT value FROM settings WHERE key='root'", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            temp.path().to_str().unwrap()
        );
    }
}
