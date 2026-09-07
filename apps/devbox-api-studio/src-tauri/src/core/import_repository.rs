//! Destination-owned activation journal. SQLite records an accepted import
//! intent; browser storage and native files are activated separately with CAS,
//! explicit recovery and rollback before product features can start.
use super::import_model::{self, BrowserState, Documents, MergeSummary, Receipt, StoreKind, KINDS};
use data_migration::core::{
    migration::acquire_snapshot,
    migration_plan::{ImportPlan, Mapping, MigrationWorkspace},
};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
const MAX_FILE: usize = 20 * 1024 * 1024;
const MAX_BUNDLE: usize = 128 * 1024 * 1024;
const KNOWN_NATIVE: [(&str, StoreKind); 4] = [
    ("webhooks/fixtures.json", StoreKind::Fixtures),
    ("transforms/smart-workflows.json", StoreKind::Workflows),
    ("api/oauth/mcp-grants.json", StoreKind::OAuth),
    ("api/grpc/tls-credentials.json", StoreKind::GrpcTls),
];
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileChange {
    pub relative: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bundle {
    pub id: String,
    pub before_browser: BrowserState,
    pub after_browser: BrowserState,
    pub files: Vec<FileChange>,
    pub receipts: Vec<Receipt>,
    pub summary: MergeSummary,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Review {
    pub id: String,
    pub summary: MergeSummary,
    pub snapshot: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPatch {
    pub id: String,
    pub rollback: bool,
    pub before: BrowserState,
    pub after: BrowserState,
    pub summary: MergeSummary,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Prepared,
    NativeApplied,
    RollbackNativeApplied,
    Complete,
    Cancelled,
}
impl Phase {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "native-applied" => Ok(Self::NativeApplied),
            "rollback-native-applied" => Ok(Self::RollbackNativeApplied),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err("migration_journal_invalid".into()),
        }
    }
}
pub struct Repository {
    root: PathBuf,
    workspace: MigrationWorkspace,
    _activation_lock: fs::File,
    _single_owner: std::marker::PhantomData<std::cell::Cell<()>>,
}
fn sql<T>(value: rusqlite::Result<T>) -> Result<T, String> {
    value.map_err(|_| "migration_journal_unavailable".into())
}
fn valid_id(id: &str) -> bool {
    id.len() == 36 && uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let known = KNOWN_NATIVE.iter().any(|(name, _)| *name == relative);
    let profile = relative
        .strip_prefix("webhooks/service-profiles/")
        .and_then(|value| value.strip_suffix(".json"))
        .is_some_and(valid_id);
    if !known && !profile {
        return Err("migration_path_invalid".into());
    }
    Ok(root.join(relative))
}
fn ensure_directory(directory: &Path) -> Result<(), String> {
    if directory.exists() {
        return devbox_filesystem::ensure_no_links(directory)
            .map_err(|_| "migration_path_invalid".into());
    }
    ensure_directory(directory.parent().ok_or("migration_path_invalid")?)?;
    fs::create_dir(directory).map_err(|_| "migration_storage_unavailable")?;
    devbox_filesystem::ensure_no_links(directory).map_err(|_| "migration_path_invalid".into())
}
pub fn read_file(path: &Path, limit: usize) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("migration_storage_unavailable".into()),
        Ok(_) => {}
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "migration_path_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "migration_storage_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "migration_storage_unavailable")?;
    if bytes.len() > limit
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "migration_storage_unavailable")?
            != identity
    {
        return Err("migration_source_changed_or_large".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "migration_schema_invalid".into())
}
fn json_value(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|_| "migration_schema_invalid".into())
}
fn validate_browser(raw: &BrowserState) -> Result<(), String> {
    let expected: Vec<_> = KINDS.iter().filter_map(|kind| kind.browser_key()).collect();
    if raw.len() != expected.len()
        || raw.keys().any(|key| !expected.contains(&key.as_str()))
        || raw.values().flatten().map(String::len).sum::<usize>() > MAX_FILE
    {
        return Err("migration_browser_invalid".into());
    }
    Ok(())
}
// A plan maps store-qualified IDs: separate stores may legitimately use the
// same opaque ID, while the shared migration planner requires unique targets.
fn destination_key(receipt: &Receipt) -> String {
    import_model::fingerprint(&json!([receipt.source_store, receipt.destination_id]))
}
fn remove_owned_directory(directory: &Path) -> Result<(), String> {
    fn inspect(path: &Path, remaining: &mut usize) -> Result<(), String> {
        *remaining = remaining
            .checked_sub(1)
            .ok_or("migration_store_too_large")?;
        devbox_filesystem::ensure_no_links(path).map_err(|_| "migration_path_invalid")?;
        if path.is_dir() {
            for entry in fs::read_dir(path).map_err(|_| "migration_storage_unavailable")? {
                inspect(
                    &entry.map_err(|_| "migration_storage_unavailable")?.path(),
                    remaining,
                )?;
            }
        }
        Ok(())
    }
    inspect(directory, &mut 20_000)?;
    fs::remove_dir_all(directory).map_err(|_| "migration_storage_unavailable".into())
}
fn preserve_serialization(before: Option<&String>, after: &Value) -> Result<String, String> {
    if let Some(before) = before {
        if json_value(before)? == *after {
            return Ok(before.clone());
        }
    }
    serde_json::to_string(after).map_err(|_| "migration_schema_invalid".into())
}
impl Repository {
    pub fn open(root: &Path, installation: &str) -> Result<Self, String> {
        devbox_filesystem::ensure_no_links(root).map_err(|_| "migration_path_invalid")?;
        let imports = root.join("imports");
        ensure_directory(&imports)?;
        let lock_path = imports.join("activation.lock");
        if lock_path.exists() {
            devbox_filesystem::ensure_no_links(&lock_path).map_err(|_| "migration_path_invalid")?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| "migration_storage_unavailable")?;
        lock.try_lock().map_err(|_| "migration_busy")?;
        // Persistent lock sidecar is never removed or replaced while unlocked.
        let workspace = MigrationWorkspace::open(root, "api-studio", installation)?;
        let present = workspace.inspect(|connection| sql(connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='studio_import_meta_v1')", [], |row| row.get::<_, bool>(0))))?;
        if !present {
            workspace.update(|transaction| sql(transaction.execute_batch("CREATE TABLE studio_import_meta_v1 (singleton INTEGER PRIMARY KEY CHECK(singleton=1), schema_version INTEGER NOT NULL); INSERT INTO studio_import_meta_v1 VALUES(1,1); CREATE TABLE studio_imports_v1 (id TEXT PRIMARY KEY, phase TEXT NOT NULL, bundle TEXT NOT NULL); CREATE TABLE studio_import_receipts_v1 (activation_id TEXT NOT NULL REFERENCES studio_imports_v1(id), source_store TEXT NOT NULL, source_id TEXT NOT NULL, fingerprint TEXT NOT NULL, destination_id TEXT NOT NULL, PRIMARY KEY(activation_id,source_store,source_id,fingerprint));")))?;
        }
        let version: i64 = workspace.inspect(|connection| {
            sql(connection.query_row(
                "SELECT schema_version FROM studio_import_meta_v1 WHERE singleton=1",
                [],
                |row| row.get(0),
            ))
        })?;
        if version != 1 {
            return Err("migration_journal_future_schema".into());
        }
        Ok(Self {
            root: root.canonicalize().map_err(|_| "migration_path_invalid")?,
            workspace,
            _activation_lock: lock,
            _single_owner: std::marker::PhantomData,
        })
    }
    pub fn new_stage(&self) -> Result<(String, PathBuf), String> {
        let directory = self.root.join("imports/staging");
        ensure_directory(&directory)?;
        if self.pending()?.is_some() {
            return Err("migration_recovery_required".into());
        }
        // The exclusive activation lock excludes another importer/worker.
        // A new preview supersedes older previews. Accepted activations keep
        // their recovery bundle and completed ID receipts in the journal.
        let entries = fs::read_dir(&directory)
            .map_err(|_| "migration_storage_unavailable")?
            .take(33)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "migration_storage_unavailable")?;
        if entries.len() > 32 {
            return Err("migration_store_too_large".into());
        }
        for entry in entries {
            if !entry.file_name().to_str().is_some_and(valid_id) {
                return Err("migration_path_invalid".into());
            }
            remove_owned_directory(&entry.path())?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let stage = directory.join(&id);
        fs::create_dir(&stage).map_err(|_| "migration_storage_unavailable")?;
        Ok((id, stage))
    }
    pub fn stage(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("migration_plan_invalid".into());
        }
        let stage = self.root.join("imports/staging").join(id);
        devbox_filesystem::ensure_no_links(&stage).map_err(|_| "migration_plan_invalid")?;
        Ok(stage)
    }
    pub fn clear_export_copy(&self, id: &str) -> Result<(), String> {
        let stage = self.stage(id)?;
        let copy = stage.join("webview-copy");
        if fs::symlink_metadata(&copy).is_ok() {
            remove_owned_directory(&copy)?;
        }
        for name in [
            "worker-ticket.json",
            "worker-started",
            "worker-result.json",
            "worker-progress.json",
        ] {
            let file = stage.join(name);
            if fs::symlink_metadata(&file).is_ok() {
                devbox_filesystem::ensure_no_links(&file).map_err(|_| "migration_path_invalid")?;
                fs::remove_file(file).map_err(|_| "migration_storage_unavailable")?;
            }
        }
        Ok(())
    }
    pub fn pending(&self) -> Result<Option<String>, String> {
        self.workspace.inspect(|connection| {
            let mut statement = sql(connection.prepare("SELECT id FROM studio_imports_v1 WHERE phase NOT IN ('complete','cancelled') ORDER BY rowid DESC LIMIT 2"))?;
            let rows = sql(statement.query_map([], |row| row.get::<_, String>(0)))?;
            let ids: Vec<_> = rows.collect::<Result<_, _>>().map_err(|_| "migration_journal_unavailable")?;
            if ids.len() > 1 { return Err("migration_journal_invalid".into()); } Ok(ids.into_iter().next())
        })
    }
    fn prior_receipts(&self) -> Result<Vec<Receipt>, String> {
        self.workspace.inspect(|connection| {
            let mut statement = sql(connection.prepare("SELECT r.source_store,r.source_id,r.fingerprint,r.destination_id FROM studio_import_receipts_v1 r JOIN studio_imports_v1 i ON i.id=r.activation_id WHERE i.phase='complete' LIMIT 100001"))?;
            let rows = sql(statement.query_map([], |row| Ok(Receipt { source_store: row.get(0)?, source_id: row.get(1)?, fingerprint: row.get(2)?, destination_id: row.get(3)? })))?;
            let receipts: Vec<_> = rows.collect::<Result<_, _>>().map_err(|_| "migration_journal_unavailable")?;
            if receipts.len() > 100_000 { return Err("migration_journal_too_large".into()); } Ok(receipts)
        })
    }
    fn read_native(&self) -> Result<(Documents, BTreeMap<String, Option<String>>), String> {
        let mut documents = Documents::new();
        let mut files = BTreeMap::new();
        for (relative, kind) in KNOWN_NATIVE {
            let raw = read_file(&path(&self.root, relative)?, MAX_FILE)?;
            if let Some(raw) = &raw {
                documents.insert(kind, json_value(raw)?);
            }
            files.insert(relative.to_string(), raw);
        }
        let directory = self.root.join("webhooks/service-profiles");
        let mut profiles = Vec::new();
        if directory.exists() {
            devbox_filesystem::ensure_no_links(&directory).map_err(|_| "migration_path_invalid")?;
            let mut entries = fs::read_dir(&directory)
                .map_err(|_| "migration_storage_unavailable")?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "migration_storage_unavailable")?;
            if entries.len() > 256 {
                return Err("migration_store_too_large".into());
            }
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "migration_path_invalid")?;
                if !name.ends_with(".json") {
                    continue;
                }
                let relative = format!("webhooks/service-profiles/{name}");
                let raw = read_file(&path(&self.root, &relative)?, 8 * 1024 * 1024)?
                    .ok_or("migration_source_changed_or_large")?;
                let value = json_value(&raw)?;
                if value.get("id").and_then(Value::as_str) != name.strip_suffix(".json") {
                    return Err("migration_schema_invalid".into());
                }
                profiles.push(value);
                files.insert(relative, Some(raw));
            }
        }
        documents.insert(
            StoreKind::Profiles,
            json!({ "schemaVersion": 1, "profiles": profiles }),
        );
        Ok((documents, files))
    }
    pub fn prepare(
        &self,
        id: &str,
        sources: &Documents,
        browser: BrowserState,
        cancelled: &AtomicBool,
        api_owner: impl Fn(StoreKind, &[u8]) -> Result<(), String>,
    ) -> Result<Review, String> {
        if self.pending()?.is_some() {
            return Err("migration_recovery_required".into());
        }
        validate_browser(&browser)?;
        let (mut destination, native_before) = self.read_native()?;
        for kind in KINDS {
            if let Some(key) = kind.browser_key() {
                if let Some(raw) = browser.get(key).and_then(Option::as_ref) {
                    destination.insert(kind, json_value(raw)?);
                }
            }
        }
        let merged =
            import_model::merge(sources, &destination, &self.prior_receipts()?, api_owner)?;
        let mut after_browser = browser.clone();
        let mut native_after = native_before.clone();
        for (kind, value) in &merged.documents {
            if let Some(key) = kind.browser_key() {
                after_browser.insert(
                    key.into(),
                    Some(preserve_serialization(
                        browser.get(key).and_then(Option::as_ref),
                        value,
                    )?),
                );
            } else if *kind == StoreKind::Profiles {
                for profile in value
                    .get("profiles")
                    .and_then(Value::as_array)
                    .ok_or("migration_schema_invalid")?
                {
                    let id = profile
                        .get("id")
                        .and_then(Value::as_str)
                        .ok_or("migration_schema_invalid")?;
                    let relative = format!("webhooks/service-profiles/{id}.json");
                    path(&self.root, &relative)?;
                    native_after.insert(
                        relative.clone(),
                        Some(preserve_serialization(
                            native_before.get(&relative).and_then(Option::as_ref),
                            profile,
                        )?),
                    );
                }
            } else {
                let relative = KNOWN_NATIVE
                    .iter()
                    .find(|(_, candidate)| candidate == kind)
                    .map(|(relative, _)| *relative)
                    .ok_or("migration_schema_invalid")?;
                native_after.insert(
                    relative.into(),
                    Some(preserve_serialization(
                        native_before.get(relative).and_then(Option::as_ref),
                        value,
                    )?),
                );
            }
        }
        validate_browser(&after_browser)?;
        let files = native_after
            .into_iter()
            .filter_map(|(relative, after)| {
                let before = native_before.get(&relative).cloned().flatten();
                (before != after).then_some(FileChange {
                    relative,
                    before,
                    after,
                })
            })
            .collect();
        let bundle = Bundle {
            id: id.to_string(),
            before_browser: browser,
            after_browser,
            files,
            receipts: merged.receipts,
            summary: merged.summary.clone(),
        };
        let encoded = serde_json::to_string(&bundle).map_err(|_| "migration_plan_invalid")?;
        if encoded.len() > MAX_BUNDLE {
            return Err("migration_store_too_large".into());
        }
        let stage = self.stage(id)?;
        let source = stage.join("normalized.db");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source)
            .map_err(|_| "migration_plan_exists")?;
        let connection = sql(Connection::open(&source))?;
        sql(connection.execute_batch("PRAGMA user_version=1; CREATE TABLE activation_bundle_v1 (id TEXT PRIMARY KEY, body TEXT NOT NULL);"))?;
        sql(connection.execute(
            "INSERT INTO activation_bundle_v1 VALUES(?1,?2)",
            (id, encoded),
        ))?;
        drop(connection);
        let snapshot_stage = stage.join("snapshot");
        let snapshot = acquire_snapshot(&source, &snapshot_stage, &[1], cancelled)?;
        let mappings = bundle
            .receipts
            .iter()
            .map(|receipt| Mapping {
                source_id: import_model::receipt_key(receipt),
                destination_id: destination_key(receipt),
            })
            .collect();
        self.workspace
            .prepare(&snapshot_stage, "api-studio-legacy", mappings, vec![])?;
        Ok(Review {
            id: id.into(),
            summary: merged.summary,
            snapshot: snapshot.sha256,
        })
    }
    fn activation(&self, id: &str) -> Result<Option<(Phase, Bundle)>, String> {
        if !valid_id(id) {
            return Err("migration_plan_invalid".into());
        }
        self.workspace.inspect(|connection| {
            let row: Option<(String, String)> = sql(connection
                .query_row(
                    "SELECT phase,bundle FROM studio_imports_v1 WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional())?;
            row.map(|(phase, raw)| {
                Ok((
                    Phase::parse(&phase)?,
                    serde_json::from_str(&raw).map_err(|_| "migration_journal_invalid")?,
                ))
            })
            .transpose()
        })
    }
    fn set_phase(&self, id: &str, phase: &str) -> Result<(), String> {
        self.workspace.update(|transaction| {
            let count = sql(transaction.execute(
                "UPDATE studio_imports_v1 SET phase=?2 WHERE id=?1",
                (id, phase),
            ))?;
            if count != 1 {
                return Err("migration_journal_invalid".into());
            }
            Ok(())
        })
    }
    fn check_files(&self, bundle: &Bundle, allow_after: bool) -> Result<(), String> {
        for change in &bundle.files {
            let current = read_file(&path(&self.root, &change.relative)?, MAX_FILE)?;
            if current != change.before && !(allow_after && current == change.after) {
                return Err("migration_destination_changed".into());
            }
        }
        Ok(())
    }
    fn write_files(&self, bundle: &Bundle, rollback: bool) -> Result<(), String> {
        self.check_files(bundle, true)?;
        for change in &bundle.files {
            let path = path(&self.root, &change.relative)?;
            let current = read_file(&path, MAX_FILE)?;
            if current != change.before && current != change.after {
                return Err("migration_destination_changed".into());
            }
            let desired = if rollback {
                &change.before
            } else {
                &change.after
            };
            if &current == desired {
                continue;
            }
            if let Some(value) = desired {
                ensure_directory(path.parent().ok_or("migration_path_invalid")?)?;
                devbox_filesystem::atomic_write(&path, value.as_bytes())
                    .map_err(|_| "migration_storage_unavailable")?;
            } else {
                fs::remove_file(&path).map_err(|_| "migration_storage_unavailable")?;
            }
            if read_file(&path, MAX_FILE)? != *desired {
                return Err("migration_activation_failed".into());
            }
        }
        Ok(())
    }
    pub fn activate(
        &self,
        id: &str,
        browser: &BrowserState,
        cancelled: &AtomicBool,
    ) -> Result<BrowserPatch, String> {
        validate_browser(browser)?;
        if self.activation(id)?.is_none() {
            if self.pending()?.is_some() {
                return Err("migration_recovery_required".into());
            }
            let stage = self.stage(id)?.join("snapshot");
            let plan: ImportPlan = serde_json::from_str(
                &read_file(&stage.join("import-plan.json"), MAX_FILE)?
                    .ok_or("migration_plan_invalid")?,
            )
            .map_err(|_| "migration_plan_invalid")?;
            self.workspace.apply(&stage, &plan, cancelled, |source, transaction, mappings| {
                let pending: bool = sql(transaction.query_row("SELECT EXISTS(SELECT 1 FROM studio_imports_v1 WHERE phase NOT IN ('complete','cancelled'))", [], |row| row.get(0)))?;
                if pending { return Err("migration_recovery_required".into()); }
                let raw: String = sql(source.query_row("SELECT body FROM activation_bundle_v1 WHERE id=?1", [id], |row| row.get(0)))?;
                let bundle: Bundle = serde_json::from_str(&raw).map_err(|_| "migration_plan_invalid")?;
                if bundle.id != id || bundle.before_browser != *browser || mappings.len() != bundle.receipts.len() { return Err("migration_destination_changed".into()); }
                self.check_files(&bundle, false)?;
                for (mapping, receipt) in mappings.iter().zip(&bundle.receipts) {
                    if mapping.source_id != import_model::receipt_key(receipt) || mapping.destination_id != destination_key(receipt) { return Err("migration_plan_invalid".into()); }
                }
                sql(transaction.execute("INSERT INTO studio_imports_v1 VALUES(?1,'prepared',?2)", (id, raw)))?;
                for receipt in &bundle.receipts { sql(transaction.execute("INSERT INTO studio_import_receipts_v1 VALUES(?1,?2,?3,?4,?5)", (id, &receipt.source_store, &receipt.source_id, &receipt.fingerprint, &receipt.destination_id)))?; }
                Ok(())
            })?;
        }
        let (phase, bundle) = self.activation(id)?.ok_or("migration_journal_invalid")?;
        if matches!(phase, Phase::RollbackNativeApplied | Phase::Cancelled) {
            return Err("migration_rollback_in_progress".into());
        }
        if browser.iter().any(|(key, value)| {
            bundle.before_browser.get(key) != Some(value)
                && bundle.after_browser.get(key) != Some(value)
        }) {
            return Err("migration_destination_changed".into());
        }
        if phase != Phase::Complete {
            self.write_files(&bundle, false)?;
            self.set_phase(id, "native-applied")?;
        }
        Ok(BrowserPatch {
            id: id.into(),
            rollback: false,
            before: bundle.before_browser,
            after: bundle.after_browser,
            summary: bundle.summary,
        })
    }
    pub fn rollback(&self, id: &str) -> Result<BrowserPatch, String> {
        let (phase, bundle) = self.activation(id)?.ok_or("migration_journal_invalid")?;
        if phase == Phase::Complete {
            return Err("migration_already_complete".into());
        }
        if phase != Phase::Cancelled {
            self.write_files(&bundle, true)?;
            self.set_phase(id, "rollback-native-applied")?;
        }
        Ok(BrowserPatch {
            id: id.into(),
            rollback: true,
            before: bundle.after_browser,
            after: bundle.before_browser,
            summary: bundle.summary,
        })
    }
    pub fn acknowledge(
        &self,
        id: &str,
        browser: &BrowserState,
        rollback: bool,
    ) -> Result<(), String> {
        validate_browser(browser)?;
        let (phase, bundle) = self.activation(id)?.ok_or("migration_journal_invalid")?;
        let expected = if rollback {
            &bundle.before_browser
        } else {
            &bundle.after_browser
        };
        if browser != expected {
            return Err("migration_browser_not_applied".into());
        }
        let expected_phase = if rollback {
            Phase::RollbackNativeApplied
        } else {
            Phase::NativeApplied
        };
        let completed_phase = if rollback {
            Phase::Cancelled
        } else {
            Phase::Complete
        };
        if phase == completed_phase {
            return Ok(());
        }
        if phase != expected_phase {
            return Err("migration_phase_invalid".into());
        }
        for change in &bundle.files {
            let expected = if rollback {
                &change.before
            } else {
                &change.after
            };
            if read_file(&path(&self.root, &change.relative)?, MAX_FILE)? != *expected {
                return Err("migration_destination_changed".into());
            }
        }
        self.set_phase(id, if rollback { "cancelled" } else { "complete" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn browser() -> BrowserState {
        KINDS
            .iter()
            .filter_map(|kind| kind.browser_key().map(|key| (key.to_string(), None)))
            .collect()
    }
    fn sources() -> Documents {
        [(StoreKind::Collections, json!({ "version": 2, "collections": [{ "id": "c1", "name": "fixture", "folder": "", "saved_at": 1, "requiresSecretReview": false, "request": {"url":"https://fixture.test", "requiresSecretReview":false} }] })),
        (StoreKind::Fixtures, json!({ "schemaVersion": 1, "nextId": 8, "fixtures": [{ "id": "fixture-7", "method": "POST", "url": "/hook", "headers": [], "body": "fixture", "receivedAtMs": 1 }] }))].into()
    }
    #[test]
    fn restart_finishes_partial_activation_and_repeat_preserves_product_edits() {
        let root = tempfile::tempdir().unwrap();
        let before = browser();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let (id, _) = repo.new_stage().unwrap();
        let review = repo
            .prepare(
                &id,
                &sources(),
                before.clone(),
                &AtomicBool::new(false),
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(review.summary.added, 2);
        let patch = repo
            .activate(&id, &before, &AtomicBool::new(false))
            .unwrap();
        assert!(root.path().join("webhooks/fixtures.json").exists());
        assert!(repo.acknowledge(&id, &before, false).is_err());
        drop(repo);
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        assert_eq!(repo.pending().unwrap(), Some(id.clone()));
        let resumed = repo
            .activate(&id, &patch.after, &AtomicBool::new(false))
            .unwrap();
        repo.acknowledge(&id, &resumed.after, false).unwrap();
        assert!(repo.pending().unwrap().is_none());
        let mut edited = resumed.after;
        let mut collections = json_value(edited["apip-collections-v2"].as_ref().unwrap()).unwrap();
        collections["collections"][0]["request"]["url"] = json!("https://product-edit.test");
        edited.insert(
            "apip-collections-v2".into(),
            Some(serde_json::to_string(&collections).unwrap()),
        );
        let (repeat, _) = repo.new_stage().unwrap();
        let review = repo
            .prepare(
                &repeat,
                &sources(),
                edited.clone(),
                &AtomicBool::new(false),
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(review.summary.already_imported, 2);
        assert_eq!(review.summary.added, 0);
        let patch = repo
            .activate(&repeat, &edited, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(patch.after, edited);
        repo.acknowledge(&repeat, &patch.after, false).unwrap();
    }
    #[test]
    fn rollback_restores_only_recorded_values_and_cancelled_import_can_be_replanned() {
        let root = tempfile::tempdir().unwrap();
        let before = browser();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let (id, _) = repo.new_stage().unwrap();
        repo.prepare(
            &id,
            &sources(),
            before.clone(),
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .unwrap();
        let applied = repo
            .activate(&id, &before, &AtomicBool::new(false))
            .unwrap();
        drop(repo);
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let rollback = repo.rollback(&id).unwrap();
        assert_eq!(rollback.before, applied.after);
        assert_eq!(rollback.after, before);
        assert!(!root.path().join("webhooks/fixtures.json").exists());
        repo.acknowledge(&id, &rollback.after, true).unwrap();
        let (retry, _) = repo.new_stage().unwrap();
        let review = repo
            .prepare(
                &retry,
                &sources(),
                before.clone(),
                &AtomicBool::new(false),
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(review.summary.added, 2);
        let patch = repo
            .activate(&retry, &before, &AtomicBool::new(false))
            .unwrap();
        repo.acknowledge(&retry, &patch.after, false).unwrap();
    }
    #[test]
    fn stale_files_foreign_namespace_and_concurrent_owner_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let before = browser();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        assert!(Repository::open(root.path(), "fixture-install").is_err());
        let (id, _) = repo.new_stage().unwrap();
        repo.prepare(
            &id,
            &sources(),
            before.clone(),
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .unwrap();
        fs::create_dir(root.path().join("webhooks")).unwrap();
        fs::write(
            root.path().join("webhooks/fixtures.json"),
            "changed destination fixture",
        )
        .unwrap();
        assert!(repo
            .activate(&id, &before, &AtomicBool::new(false))
            .is_err());
        assert!(repo.pending().unwrap().is_none());
        assert_eq!(
            fs::read_to_string(root.path().join("webhooks/fixtures.json")).unwrap(),
            "changed destination fixture"
        );
        drop(repo);
        let foreign = Repository::open(root.path(), "other-install").unwrap();
        assert!(foreign
            .activate(&id, &before, &AtomicBool::new(false))
            .is_err());
        assert!(path(root.path(), "webhooks/service-profiles/../outside.json").is_err());
        assert!(path(root.path(), "../../outside").is_err());
    }
    #[test]
    fn separate_stores_can_import_the_same_id_and_pending_recovery_keeps_its_stage() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let mut source = sources();
        source.get_mut(&StoreKind::Collections).unwrap()["collections"][0]["id"] =
            json!("fixture-1");
        let (id, stage) = repo.new_stage().unwrap();
        repo.prepare(&id, &source, browser(), &AtomicBool::new(false), |_, _| {
            Ok(())
        })
        .unwrap();
        let patch = repo
            .activate(&id, &browser(), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(patch.summary.added, 2);
        assert!(repo.new_stage().is_err());
        assert!(stage.exists());
        repo.acknowledge(&id, &patch.after, false).unwrap();
        repo.new_stage().unwrap();
        assert!(!stage.exists());
        assert!(repo.pending().unwrap().is_none());
    }
    #[test]
    fn new_preview_purges_abandoned_raw_copies_and_invalidates_previous_plan() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let (id, stage) = repo.new_stage().unwrap();
        let raw = stage.join("webview-copy");
        fs::create_dir(&raw).unwrap();
        fs::write(raw.join("synthetic-raw"), "synthetic source only").unwrap();
        fs::write(stage.join("worker-ticket.json"), "synthetic nonce").unwrap();
        repo.clear_export_copy(&id).unwrap();
        assert!(!raw.exists());
        assert!(!stage.join("worker-ticket.json").exists());
        repo.prepare(
            &id,
            &sources(),
            browser(),
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .unwrap();
        let (_, next) = repo.new_stage().unwrap();
        assert!(next.exists());
        assert!(!stage.exists());
        assert!(repo
            .activate(&id, &browser(), &AtomicBool::new(false))
            .is_err());
        assert!(repo.pending().unwrap().is_none());
    }
    #[test]
    fn cancelled_or_tampered_plan_never_publishes_destination_data() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::open(root.path(), "fixture-install").unwrap();
        let (id, stage) = repo.new_stage().unwrap();
        repo.prepare(
            &id,
            &sources(),
            browser(),
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .unwrap();
        assert!(repo
            .activate(&id, &browser(), &AtomicBool::new(true))
            .is_err());
        assert!(repo.pending().unwrap().is_none());
        let snapshot = stage.join("snapshot");
        let database = snapshot.join("snapshot.db");
        let mut bytes = fs::read(&database).unwrap();
        bytes[0] ^= 1;
        fs::write(database, bytes).unwrap();
        assert!(repo
            .activate(&id, &browser(), &AtomicBool::new(false))
            .is_err());
        assert!(!root.path().join("webhooks/fixtures.json").exists());
        assert!(repo.pending().unwrap().is_none());
    }
}
