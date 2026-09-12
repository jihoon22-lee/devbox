//! Immutable, bounded legacy Runtime acquisition. Only the native composition
//! chooses the fixed source and private stage; no renderer path is accepted.
use devbox_data_migration::core::migration::{acquire_snapshot, verify_snapshot, Snapshot};
use rusqlite::{types::Value, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub type Result<T> = std::result::Result<T, String>;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
pub(crate) const TABLES: &[&str] = &[
    "jobs",
    "workspace_task_sources",
    "workspace_tasks",
    "runs",
    "service_instances",
    "workspace_task_operations",
    "workspace_task_operation_runs",
    "workspace_task_control_receipts",
];
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportSummary {
    pub jobs: usize,
    pub services: usize,
    pub tasks: usize,
    pub runs: usize,
    pub active_history: usize,
    pub secrets_requiring_review: usize,
    pub log_bytes: u64,
    pub missing_logs: usize,
    pub orphan_log_directories: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreservedFile {
    relative: String,
    bytes: u64,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: u32,
    snapshot: Snapshot,
    summary: ImportSummary,
    files: Vec<PreservedFile>,
    directories: Vec<String>,
    // Original retention policy and per-stream manifest bytes are preserved.
    terminal_rows_per_job: u32,
    global_log_bytes: u64,
}
pub(crate) struct Table {
    pub name: &'static str,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}
impl Table {
    pub fn index(&self, column: &str) -> usize {
        self.columns
            .iter()
            .position(|value| value == column)
            .expect("validated domain schema")
    }
}
pub struct PreparedImport {
    stage: PathBuf,
    manifest: Manifest,
    pub(crate) tables: Vec<Table>,
}
fn fail<T>() -> Result<T> {
    Err("runtime_import_invalid".into())
}
fn sql<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|_| "runtime_import_invalid".into())
}
fn cancelled(flag: &AtomicBool) -> Result<()> {
    if flag.load(Ordering::Acquire) {
        Err("runtime_import_cancelled".into())
    } else {
        Ok(())
    }
}
pub(crate) fn text(value: &Value) -> Result<&str> {
    if let Value::Text(value) = value {
        Ok(value)
    } else {
        fail()
    }
}
fn columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = sql(connection.prepare(&format!("PRAGMA table_info({table})")))?;
    sql(statement
        .query_map([], |row| row.get(1))
        .and_then(|rows| rows.collect()))
}
fn tables(path: &Path, flag: &AtomicBool) -> Result<Vec<Table>> {
    let source = sql(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    sql(source.execute_batch("PRAGMA trusted_schema=OFF; BEGIN"))?;
    let version: String = sql(source.query_row(
        "SELECT value FROM meta WHERE key='schema_version'",
        [],
        |row| row.get(0),
    ))?;
    if version != crate::storage::SCHEMA_VERSION.to_string() {
        return Err("runtime_import_schema_unsupported".into());
    }
    let expected = crate::storage::import_schema()?;
    let violations: bool = sql(source.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
        [],
        |row| row.get(0),
    ))?;
    if violations {
        return fail();
    }
    let mut result = Vec::new();
    let mut row_count = 0usize;
    let mut bytes = 0u64;
    for &name in TABLES {
        cancelled(flag)?;
        let table_type: String = sql(source.query_row(
            "SELECT type FROM sqlite_master WHERE name=?",
            [name],
            |row| row.get(0),
        ))?;
        if table_type != "table" {
            return fail();
        }
        let columns = columns(&expected, name)?;
        // Order can differ after supported in-place schema migrations.
        let actual = self::columns(&source, name)?;
        if actual.len() != columns.len() || actual.iter().any(|column| !columns.contains(column)) {
            return fail();
        }
        let mut statement =
            sql(source.prepare(&format!("SELECT {} FROM {name}", columns.join(","))))?;
        let mut cursor = sql(statement.query([]))?;
        let mut rows = Vec::new();
        while let Some(row) = sql(cursor.next())? {
            cancelled(flag)?;
            row_count += 1;
            if row_count > MAX_ROWS {
                return Err("runtime_import_limit".into());
            }
            let values = (0..columns.len())
                .map(|index| sql(row.get::<_, Value>(index)))
                .collect::<Result<Vec<_>>>()?;
            for value in &values {
                bytes += match value {
                    Value::Blob(value) => value.len() as u64,
                    Value::Text(value) => value.len() as u64,
                    _ => 8,
                };
            }
            if bytes > MAX_BYTES / 2 {
                return Err("runtime_import_limit".into());
            }
            rows.push(values);
        }
        result.push(Table {
            name,
            columns,
            rows,
        });
    }
    Ok(result)
}
fn file_digest(
    path: &Path,
    destination: Option<&Path>,
    flag: &AtomicBool,
) -> Result<(u64, String)> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "runtime_import_source_changed")?;
    let (mut input, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "runtime_import_source_changed")?;
    let before = input
        .metadata()
        .map_err(|_| "runtime_import_source_changed")?;
    if !before.is_file() || before.len() > MAX_BYTES {
        return Err("runtime_import_limit".into());
    }
    let mut output = destination
        .map(|path| OpenOptions::new().write(true).create_new(true).open(path))
        .transpose()
        .map_err(|_| "runtime_import_destination_conflict")?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        cancelled(flag)?;
        let count = input
            .read(&mut buffer)
            .map_err(|_| "runtime_import_source_changed")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_BYTES {
            return Err("runtime_import_limit".into());
        }
        hash.update(&buffer[..count]);
        if let Some(output) = output.as_mut() {
            output
                .write_all(&buffer[..count])
                .map_err(|_| "runtime_import_write_failed")?;
        }
    }
    if let Some(output) = output {
        output
            .sync_all()
            .map_err(|_| "runtime_import_write_failed")?;
    }
    if before.len() != bytes
        || before.modified().ok()
            != input
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "runtime_import_source_changed")?
            != identity
    {
        return Err("runtime_import_source_changed".into());
    }
    Ok((
        bytes,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}
fn safe_file(relative: &str) -> bool {
    let parts = relative.split('/').collect::<Vec<_>>();
    parts.len() == 4
        && parts[0] == "logs"
        && parts[1] == "runs"
        && uuid::Uuid::parse_str(parts[2]).is_ok()
        && !parts[3].is_empty()
        && parts[3].len() <= 128
        && parts[3]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        && !matches!(parts[3], "." | "..")
}
impl PreparedImport {
    pub fn summary(&self) -> &ImportSummary {
        &self.manifest.summary
    }
    pub fn digest(&self) -> &str {
        &self.manifest.snapshot.sha256
    }
    pub fn acquire(source: &Path, stage: &Path, flag: &AtomicBool) -> Result<Self> {
        devbox_filesystem::ensure_no_links(source).map_err(|_| "runtime_import_source_changed")?;
        let (_source_root, root_identity) = devbox_filesystem::open_filesystem_object(source, true)
            .map_err(|_| "runtime_import_source_changed")?;
        let (_source_database, database_identity) =
            devbox_filesystem::open_filesystem_object(source.join("data.db"), false)
                .map_err(|_| "runtime_import_source_changed")?;
        for name in ["data.db", "data.db-wal", "data.db-shm"] {
            let path = source.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => devbox_filesystem::ensure_no_links(&path)
                    .map_err(|_| "runtime_import_source_changed")?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return fail(),
            }
        }
        let snapshot = acquire_snapshot(&source.join("data.db"), stage, &[0], flag)
            .map_err(|_| "runtime_import_snapshot_failed")?;
        let tables = tables(
            &verify_snapshot(stage, &snapshot).map_err(|_| "runtime_import_invalid")?,
            flag,
        )?;
        let jobs = &tables[0];
        let tasks = &tables[2];
        let runs = &tables[3];
        let mut summary = ImportSummary {
            jobs: 0,
            services: 0,
            tasks: tasks.rows.len(),
            runs: runs.rows.len(),
            active_history: 0,
            secrets_requiring_review: 0,
            log_bytes: 0,
            missing_logs: 0,
            orphan_log_directories: 0,
        };
        for row in &jobs.rows {
            match text(&row[jobs.index("kind")])? {
                "job" => summary.jobs += 1,
                "service" => summary.services += 1,
                _ => return fail(),
            }
            if row[jobs.index("env_ciphertext")] != Value::Null {
                summary.secrets_requiring_review += 1;
            }
        }
        let mut files = Vec::new();
        let mut identities = Vec::new();
        let mut directories = Vec::new();
        let mut known = BTreeMap::new();
        for row in &runs.rows {
            cancelled(flag)?;
            let id = text(&row[runs.index("id")])?;
            uuid::Uuid::parse_str(id).map_err(|_| "runtime_import_invalid")?;
            known.insert(id, ());
            if matches!(
                text(&row[runs.index("status")])?,
                "queued" | "starting" | "running" | "stopping"
            ) {
                summary.active_history += 1;
            }
            if row[runs.index("log_dir")] == Value::Null
                || row[runs.index("logs_deleted_at")] != Value::Null
            {
                continue;
            }
            let relative = format!("logs/runs/{id}");
            if text(&row[runs.index("log_dir")])? != relative {
                return fail();
            }
            let root = source.join(&relative);
            match fs::symlink_metadata(&root) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    summary.missing_logs += 1;
                    continue;
                }
                Err(_) => return fail(),
                Ok(_) => {}
            }
            devbox_filesystem::ensure_no_links(&root)
                .map_err(|_| "runtime_import_source_changed")?;
            let (handle, identity) = devbox_filesystem::open_filesystem_object(&root, true)
                .map_err(|_| "runtime_import_source_changed")?;
            let mut names = Vec::new();
            fs::create_dir_all(stage.join(&relative)).map_err(|_| "runtime_import_write_failed")?;
            for entry in fs::read_dir(&root).map_err(|_| "runtime_import_source_changed")? {
                let entry = entry.map_err(|_| "runtime_import_source_changed")?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "runtime_import_invalid")?;
                let relative = format!("{relative}/{name}");
                if !safe_file(&relative) || names.len() >= 64 {
                    return fail();
                }
                let (bytes, sha256) =
                    file_digest(&entry.path(), Some(&stage.join(&relative)), flag)?;
                summary.log_bytes += bytes;
                if summary.log_bytes > MAX_BYTES {
                    return Err("runtime_import_limit".into());
                }
                files.push(PreservedFile {
                    relative,
                    bytes,
                    sha256,
                });
                names.push(name);
            }
            directories.push(relative);
            names.sort();
            identities.push((root, handle, identity, names));
        }
        // A second pass detects rotation, mutation or replacement anywhere in
        // the acquisition, including manifests copied before the final segment.
        for file in &files {
            if file_digest(&source.join(&file.relative), None, flag)?
                != (file.bytes, file.sha256.clone())
            {
                return Err("runtime_import_source_changed".into());
            }
        }
        for (root, _handle, identity, names) in identities {
            if devbox_filesystem::filesystem_identity(&root, true)
                .map_err(|_| "runtime_import_source_changed")?
                != identity
            {
                return Err("runtime_import_source_changed".into());
            }
            let mut current = fs::read_dir(root)
                .map_err(|_| "runtime_import_source_changed")?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|_| "runtime_import_source_changed")?;
            current.sort();
            if names != current {
                return Err("runtime_import_source_changed".into());
            }
        }
        if source.join("logs/runs").exists() {
            devbox_filesystem::ensure_no_links(source.join("logs/runs"))
                .map_err(|_| "runtime_import_source_changed")?;
            for entry in fs::read_dir(source.join("logs/runs"))
                .map_err(|_| "runtime_import_source_changed")?
            {
                let entry = entry.map_err(|_| "runtime_import_source_changed")?;
                if !known.contains_key(entry.file_name().to_string_lossy().as_ref()) {
                    summary.orphan_log_directories += 1;
                }
            }
        }
        devbox_filesystem::ensure_no_links(source).map_err(|_| "runtime_import_source_changed")?;
        if devbox_filesystem::filesystem_identity(source, true)
            .map_err(|_| "runtime_import_source_changed")?
            != root_identity
            || devbox_filesystem::filesystem_identity(source.join("data.db"), false)
                .map_err(|_| "runtime_import_source_changed")?
                != database_identity
        {
            return Err("runtime_import_source_changed".into());
        }
        let manifest = Manifest {
            version: 1,
            snapshot,
            summary,
            files,
            directories,
            terminal_rows_per_job: 50,
            global_log_bytes: 200 * 1024 * 1024,
        };
        cancelled(flag)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.join("runtime-import.json"))
            .map_err(|_| "runtime_import_write_failed")?;
        output
            .write_all(&serde_json::to_vec(&manifest).map_err(|_| "runtime_import_invalid")?)
            .map_err(|_| "runtime_import_write_failed")?;
        output
            .sync_all()
            .map_err(|_| "runtime_import_write_failed")?;
        Ok(Self {
            stage: stage.into(),
            manifest,
            tables,
        })
    }
    pub fn reopen(stage: &Path, flag: &AtomicBool) -> Result<Self> {
        devbox_filesystem::ensure_no_links(stage).map_err(|_| "runtime_import_invalid")?;
        let path = stage.join("runtime-import.json");
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_import_invalid")?;
        if fs::metadata(&path)
            .map_err(|_| "runtime_import_invalid")?
            .len()
            > 32 * 1024 * 1024
        {
            return fail();
        }
        let manifest: Manifest =
            serde_json::from_reader(File::open(path).map_err(|_| "runtime_import_invalid")?)
                .map_err(|_| "runtime_import_invalid")?;
        if manifest.version != 1 || manifest.files.len() > MAX_ROWS * 64 {
            return fail();
        }
        let snapshot =
            verify_snapshot(stage, &manifest.snapshot).map_err(|_| "runtime_import_invalid")?;
        let tables = tables(&snapshot, flag)?;
        let result = Self {
            stage: stage.into(),
            manifest,
            tables,
        };
        result.verify_logs(flag)?;
        Ok(result)
    }
    pub(crate) fn verify_logs(&self, flag: &AtomicBool) -> Result<()> {
        if self.manifest.directories.len() > MAX_ROWS {
            return fail();
        }
        for directory in &self.manifest.directories {
            if !safe_file(&format!("{directory}/check")) {
                return fail();
            }
            let path = self.stage.join(directory);
            devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_import_invalid")?;
            if !path.is_dir() {
                return fail();
            }
        }
        let mut bytes = 0u64;
        for file in &self.manifest.files {
            if !safe_file(&file.relative) {
                return fail();
            }
            if file_digest(&self.stage.join(&file.relative), None, flag)?
                != (file.bytes, file.sha256.clone())
            {
                return fail();
            }
            bytes += file.bytes;
            if bytes > MAX_BYTES {
                return fail();
            }
        }
        if bytes != self.manifest.summary.log_bytes {
            return fail();
        }
        Ok(())
    }
    pub(crate) fn has_logs(&self, id: &str) -> bool {
        self.manifest
            .directories
            .contains(&format!("logs/runs/{id}"))
    }
    // Caller holds the destination retention/import lock through SQLite commit.
    pub(crate) fn publish_logs(&self, destination: &Path, flag: &AtomicBool) -> Result<()> {
        for row in &self.tables[3].rows {
            let id = text(&row[self.tables[3].index("id")])?;
            if !self.has_logs(id) {
                continue;
            }
            let relative = format!("logs/runs/{id}");
            let directory = destination.join(&relative);
            fs::create_dir_all(&directory).map_err(|_| "runtime_import_write_failed")?;
            devbox_filesystem::ensure_no_links(&directory)
                .map_err(|_| "runtime_import_destination_conflict")?;
            let prefix = format!("{relative}/");
            let files = self
                .manifest
                .files
                .iter()
                .filter(|file| file.relative.starts_with(&prefix))
                .collect::<Vec<_>>();
            for entry in
                fs::read_dir(&directory).map_err(|_| "runtime_import_destination_conflict")?
            {
                let entry = entry.map_err(|_| "runtime_import_destination_conflict")?;
                if !files
                    .iter()
                    .any(|file| destination.join(&file.relative) == entry.path())
                {
                    return Err("runtime_import_destination_conflict".into());
                }
            }
            for file in files {
                let target = destination.join(&file.relative);
                let actual = if target.exists() {
                    file_digest(&target, None, flag)?
                } else {
                    // A cancelled copy cannot leave a partial final log that
                    // would block retry. Scratch files stay outside logs/runs.
                    let temporary = self
                        .stage
                        .join(format!(".publish-{}", uuid::Uuid::new_v4()));
                    let copied =
                        file_digest(&self.stage.join(&file.relative), Some(&temporary), flag);
                    let actual = match copied {
                        Ok(actual) => actual,
                        Err(error) => {
                            let _ = fs::remove_file(&temporary);
                            return Err(error);
                        }
                    };
                    if actual != (file.bytes, file.sha256.clone()) {
                        let _ = fs::remove_file(&temporary);
                        return Err("runtime_import_invalid".into());
                    }
                    // A hard link publishes a complete file without replacing
                    // an unexpected existing target; both paths are local to
                    // the same retained Runtime store volume.
                    let published = fs::hard_link(&temporary, &target);
                    let _ = fs::remove_file(&temporary);
                    published.map_err(|_| "runtime_import_destination_conflict")?;
                    actual
                };
                if actual != (file.bytes, file.sha256.clone()) {
                    return Err("runtime_import_destination_conflict".into());
                }
            }
        }
        Ok(())
    }
}
