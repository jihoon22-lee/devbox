//! Native, offline project-definition import.
//!
//! The importer reads only two files directly beneath a user-selected project
//! root: `package.json` and `Cargo.toml`.  It never invokes npm, Cargo, a
//! shell, a network client, or a dotenv loader.  Imported commands are stable
//! package/Cargo invocations, while environment values are deliberately not
//! copied.  The command layer re-reads the same files and compares `revision`
//! before saving, so a preview cannot silently apply stale source data.

use crate::core::models::JobKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, BTreeSet};
use std::fs::{self, File, Metadata};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROJECT_IMPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROJECT_ROOT_BYTES: usize = 4_096;
pub const MAX_SOURCE_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_ITEMS: usize = 128;
pub const MAX_ITEM_NAME_BYTES: usize = 128;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_ENV_KEYS: usize = 64;
/// SHA-256 is represented as lower-case hexadecimal in the IPC contract.
pub const MAX_REVISION_BYTES: usize = 64;
pub const MAX_DEFINITION_JSON_BYTES: usize = 512 * 1024;
pub const PROJECT_IMPORT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OPERATION_ID_BYTES: usize = 64;
const MAX_ACTIVE_OPERATIONS: usize = 4;

const PACKAGE_JSON: &str = "package.json";
const CARGO_TOML: &str = "Cargo.toml";
const DISABLED_IMPORT_CRON: &str = "0 0 1 1 *";

/// Errors intentionally contain no source path, command text, or file
/// contents.  The command layer maps all variants to the same fixed public
/// error code so local usernames and project paths do not cross the IPC error
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectImportError {
    InvalidRoot,
    SourceUnavailable,
    SourceTooLarge,
    UnsafeSource,
    InvalidJson,
    InvalidToml,
    InvalidSourceEntry,
    TooManyItems,
    StaleSource,
    Cancelled,
    TimedOut,
    DuplicateOperation,
    OperationBusy,
}

impl std::fmt::Display for ProjectImportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRoot => "project-import-invalid-root",
            Self::SourceUnavailable => "project-import-source-unavailable",
            Self::SourceTooLarge => "project-import-source-too-large",
            Self::UnsafeSource => "project-import-unsafe-source",
            Self::InvalidJson => "project-import-invalid-json",
            Self::InvalidToml => "project-import-invalid-toml",
            Self::InvalidSourceEntry => "project-import-invalid-entry",
            Self::TooManyItems => "project-import-too-many-items",
            Self::StaleSource => "project-import-stale",
            Self::Cancelled => "project-import-cancelled",
            Self::TimedOut => "project-import-timeout",
            Self::DuplicateOperation => "project-import-duplicate",
            Self::OperationBusy => "project-import-busy",
        })
    }
}

impl std::error::Error for ProjectImportError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImportFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectImportSource {
    PackageScript,
    CargoTarget,
}

impl ProjectImportSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PackageScript => "package-script",
            Self::CargoTarget => "cargo-target",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImportItem {
    /// Opaque, deterministic within the preview source.  It is not a path and
    /// is not accepted as a command or filesystem value.
    pub id: String,
    pub name: String,
    /// `new` or `conflict`; the command layer decorates this after checking
    /// current definitions by kind/name/cwd.
    pub status: String,
    pub command: String,
    pub kind: JobKind,
    pub source: ProjectImportSource,
    pub source_name: String,
    pub source_path: String,
    pub cwd: String,
    /// Names referenced by the source command only; values are never read.
    pub environment_keys: Vec<String>,
    pub requires_confirmation: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImportPlan {
    pub schema_version: u32,
    /// Canonical root displayed for the explicit user confirmation step.
    pub source_root: String,
    /// Non-secret source fingerprint used to reject a changed preview.
    pub revision: String,
    pub files: Vec<ProjectImportFile>,
    pub items: Vec<ProjectImportItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectImportApplyResult {
    pub created: u32,
    pub skipped_conflicts: u32,
}

/// Process-local guard for one preview/apply request. It is shared with the
/// explicit cancel command and checked between every bounded native step.
#[derive(Debug, Clone)]
pub struct ImportControl {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}

impl ImportControl {
    pub fn new(timeout: Duration) -> Self {
        let now = Instant::now();
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: now.checked_add(timeout).unwrap_or(now),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn check(&self) -> Result<(), ProjectImportError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(ProjectImportError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(ProjectImportError::TimedOut)
        } else {
            Ok(())
        }
    }
}

impl Default for ImportControl {
    fn default() -> Self {
        Self::new(PROJECT_IMPORT_TIMEOUT)
    }
}

/// Small process-local registry used to reject duplicate requests and route a
/// cancel action to the exact preview/apply operation. It never stores paths,
/// commands, source bytes, or environment values.
#[derive(Debug, Clone, Default)]
pub struct ImportOperationRegistry {
    active: Arc<Mutex<std::collections::HashMap<String, ImportControl>>>,
}

#[derive(Debug)]
pub struct ImportOperationGuard {
    registry: ImportOperationRegistry,
    id: String,
    control: ImportControl,
}

impl ImportOperationRegistry {
    pub fn begin(&self, operation_id: &str) -> Result<ImportOperationGuard, ProjectImportError> {
        validate_operation_id(operation_id)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| ProjectImportError::OperationBusy)?;
        if active.contains_key(operation_id) {
            return Err(ProjectImportError::DuplicateOperation);
        }
        if active.len() >= MAX_ACTIVE_OPERATIONS {
            return Err(ProjectImportError::OperationBusy);
        }
        let control = ImportControl::default();
        active.insert(operation_id.to_owned(), control.clone());
        Ok(ImportOperationGuard {
            registry: self.clone(),
            id: operation_id.to_owned(),
            control,
        })
    }

    pub fn cancel(&self, operation_id: &str) -> Result<bool, ProjectImportError> {
        validate_operation_id(operation_id)?;
        let active = self
            .active
            .lock()
            .map_err(|_| ProjectImportError::OperationBusy)?;
        if let Some(control) = active.get(operation_id) {
            control.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl ImportOperationGuard {
    pub fn control(&self) -> &ImportControl {
        &self.control
    }
}

impl Drop for ImportOperationGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.registry.active.lock() {
            if active
                .get(&self.id)
                .is_some_and(|control| Arc::ptr_eq(&control.cancelled, &self.control.cancelled))
            {
                active.remove(&self.id);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceSnapshot<'a> {
    package: Option<&'a [u8]>,
    cargo: Option<&'a [u8]>,
}

/// Read and parse a project without starting an external process.
pub fn preview_project(root: &Path) -> Result<ProjectImportPlan, ProjectImportError> {
    preview_project_with_control(root, &ImportControl::default())
}

pub fn preview_project_with_control(
    root: &Path,
    control: &ImportControl,
) -> Result<ProjectImportPlan, ProjectImportError> {
    control.check()?;
    let canonical_root = canonical_project_root(root)?;
    control.check()?;
    let root_identity = devbox_filesystem::filesystem_identity(&canonical_root, true)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let package = read_source_file(&canonical_root, PACKAGE_JSON, control)?;
    ensure_root_identity(&canonical_root, root_identity)?;
    let cargo = read_source_file(&canonical_root, CARGO_TOML, control)?;
    ensure_root_identity(&canonical_root, root_identity)?;
    let package_bytes = package.as_ref().map(|source| source.bytes.as_slice());
    let cargo_bytes = cargo.as_ref().map(|source| source.bytes.as_slice());
    let snapshot = SourceSnapshot {
        package: package_bytes,
        cargo: cargo_bytes,
    };

    ensure_root_identity(&canonical_root, root_identity)?;
    if snapshot.package.is_none() && snapshot.cargo.is_none() {
        return Err(ProjectImportError::SourceUnavailable);
    }

    let mut items = Vec::new();
    if let Some(bytes) = snapshot.package {
        items.extend(parse_package_scripts(bytes)?);
        control.check()?;
    }
    if let Some(bytes) = snapshot.cargo {
        items.extend(parse_cargo_targets(bytes)?);
        control.check()?;
    }
    if items.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    let source_root = safe_display_root(&canonical_root, root)?;
    for item in &mut items {
        item.cwd = source_root.clone();
    }

    let mut files = Vec::new();
    if let Some(source) = package.as_ref() {
        files.push(ProjectImportFile {
            path: PACKAGE_JSON.to_owned(),
            bytes: source.bytes.len() as u64,
        });
    }
    if let Some(source) = cargo.as_ref() {
        files.push(ProjectImportFile {
            path: CARGO_TOML.to_owned(),
            bytes: source.bytes.len() as u64,
        });
    }

    control.check()?;
    ensure_root_identity(&canonical_root, root_identity)?;
    Ok(ProjectImportPlan {
        schema_version: PROJECT_IMPORT_SCHEMA_VERSION,
        source_root,
        revision: source_revision_with_root(snapshot, Some(root_identity)),
        files,
        items,
    })
}

/// Verify that the source still yields exactly the same bounded revision as a
/// prior preview and return that one fresh parse for the actual insert
/// operation. Keeping verification and consumption together avoids a second
/// read window in which the source could change between two checks.
pub fn verify_preview_revision(
    root: &Path,
    expected_root: &str,
    expected_revision: &str,
) -> Result<ProjectImportPlan, ProjectImportError> {
    verify_preview_revision_with_control(
        root,
        expected_root,
        expected_revision,
        &ImportControl::default(),
    )
}

pub fn verify_preview_revision_with_control(
    root: &Path,
    expected_root: &str,
    expected_revision: &str,
    control: &ImportControl,
) -> Result<ProjectImportPlan, ProjectImportError> {
    control.check()?;
    if expected_root.is_empty()
        || expected_root.len() > MAX_PROJECT_ROOT_BYTES
        || expected_root.chars().any(char::is_control)
        || expected_revision.len() != MAX_REVISION_BYTES
        || !expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ProjectImportError::StaleSource);
    }
    let plan = preview_project_with_control(root, control)?;
    if plan.source_root != expected_root || plan.revision != expected_revision {
        return Err(ProjectImportError::StaleSource);
    }
    Ok(plan)
}

/// Existing definition imports may carry a working directory from another
/// machine.  Keep it as a reviewable absolute path, but reject traversal,
/// device aliases, controls, and oversized strings before it reaches storage.
pub fn validate_import_cwd(cwd: Option<&str>) -> bool {
    let Some(cwd) = cwd else {
        return true;
    };
    devbox_filesystem::parse_safe_project_path(cwd).is_some()
}

/// Parse only the `scripts` object.  The actual imported command is
/// `npm run -- <script>`, not the arbitrary script body; this avoids copying
/// inline tokens or credentials into the Run Manager database.  Script bodies
/// are inspected only for bounded environment *names* used in the confirmation
/// view.
pub fn parse_package_scripts(bytes: &[u8]) -> Result<Vec<ProjectImportItem>, ProjectImportError> {
    if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ProjectImportError::InvalidJson)?;
    let object = value.as_object().ok_or(ProjectImportError::InvalidJson)?;
    let Some(scripts) = object.get("scripts") else {
        return Ok(Vec::new());
    };
    let scripts = scripts
        .as_object()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    if scripts.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }

    scripts
        .iter()
        .map(|(name, value)| {
            validate_script_name(name)?;
            let body = value
                .as_str()
                .ok_or(ProjectImportError::InvalidSourceEntry)?;
            if body.is_empty() || body.len() > MAX_COMMAND_BYTES || body.contains('\0') {
                return Err(ProjectImportError::InvalidSourceEntry);
            }
            let command = format!("npm run -- {name}");
            let source_name = format!("scripts.{name}");
            let environment_keys = referenced_environment_keys(body);
            Ok(ProjectImportItem {
                id: format!("npm:script:{name}"),
                name: format!("npm · {name}"),
                status: "new".to_owned(),
                command,
                kind: JobKind::Job,
                source: ProjectImportSource::PackageScript,
                source_name,
                source_path: PACKAGE_JSON.to_owned(),
                cwd: String::new(),
                environment_keys,
                requires_confirmation: true,
                detail: "package.json script · 실행 명령과 환경변수를 확인한 뒤 활성화하세요"
                    .to_owned(),
            })
        })
        .collect()
}

/// Parse common local Cargo targets without invoking Cargo metadata.  Targets
/// are represented as safe Cargo argv-like command components, so a target
/// name can never add a shell operator to the imported command.
pub fn parse_cargo_targets(bytes: &[u8]) -> Result<Vec<ProjectImportItem>, ProjectImportError> {
    if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ProjectImportError::InvalidToml)?;
    let value: toml::Value = toml::from_str(text).map_err(|_| ProjectImportError::InvalidToml)?;
    let table = value.as_table().ok_or(ProjectImportError::InvalidToml)?;
    let Some(package) = table.get("package") else {
        // A virtual workspace has no directly runnable target.  It is a
        // valid source but cannot yield a safe single-package task.
        return Ok(Vec::new());
    };
    let package = package
        .as_table()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    validate_cargo_name(package_name)?;
    let autobins = match package.get("autobins") {
        Some(value) => value
            .as_bool()
            .ok_or(ProjectImportError::InvalidSourceEntry)?,
        None => true,
    };

    let mut items = Vec::new();
    let mut bin_names = BTreeSet::new();
    if let Some(bins) = table.get("bin") {
        for entry in bins.as_array().ok_or(ProjectImportError::InvalidToml)? {
            let entry = entry
                .as_table()
                .ok_or(ProjectImportError::InvalidSourceEntry)?;
            let name = cargo_bin_name(entry, package_name)?;
            validate_cargo_name(&name)?;
            if !bin_names.insert(name.clone()) {
                return Err(ProjectImportError::InvalidSourceEntry);
            }
            ensure_item_capacity(&items)?;
            items.push(cargo_item(
                "bin",
                &name,
                format!("cargo run --bin {name}"),
                "[[bin]]",
            )?);
        }
    }
    if bin_names.is_empty() && autobins {
        ensure_item_capacity(&items)?;
        items.push(cargo_item(
            "package",
            package_name,
            "cargo run".to_owned(),
            "[package]",
        )?);
    }

    if let Some(lib) = table.get("lib") {
        if !lib.is_table() {
            return Err(ProjectImportError::InvalidSourceEntry);
        }
        ensure_item_capacity(&items)?;
        items.push(cargo_item(
            "lib",
            package_name,
            "cargo test --lib".to_owned(),
            "[lib]",
        )?);
    }
    for (section, flag, label) in [
        ("example", "example", "[[example]]"),
        ("test", "test", "[[test]]"),
        ("bench", "bench", "[[bench]]"),
    ] {
        if let Some(entries) = table.get(section) {
            for entry in entries.as_array().ok_or(ProjectImportError::InvalidToml)? {
                let entry = entry
                    .as_table()
                    .ok_or(ProjectImportError::InvalidSourceEntry)?;
                let name = entry
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .ok_or(ProjectImportError::InvalidSourceEntry)?;
                validate_cargo_name(name)?;
                let command = match flag {
                    "example" => format!("cargo run --example {name}"),
                    "test" => format!("cargo test --test {name}"),
                    "bench" => format!("cargo bench --bench {name}"),
                    _ => unreachable!("fixed Cargo target flag"),
                };
                ensure_item_capacity(&items)?;
                items.push(cargo_item(flag, name, command, label)?);
            }
        }
    }
    if items.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    Ok(items)
}

/// Cargo derives an explicit `[[bin]]` target name from its `path` when the
/// optional `name` field is omitted.  Keep that derivation local and bounded;
/// the path is metadata only and is never opened or passed to a process.
fn cargo_bin_name(
    entry: &toml::value::Table,
    package_name: &str,
) -> Result<String, ProjectImportError> {
    if let Some(name) = entry.get("name") {
        return name
            .as_str()
            .map(str::to_owned)
            .ok_or(ProjectImportError::InvalidSourceEntry);
    }
    let Some(path) = entry.get("path") else {
        return Ok(package_name.to_owned());
    };
    let path = path
        .as_str()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    if path.is_empty()
        || path.len() > MAX_COMMAND_BYTES
        || path.chars().any(char::is_control)
        || path.starts_with('/')
        || path.starts_with('\\')
    {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    let mut components = path.split(['/', '\\']);
    if components.clone().any(|component| {
        component.is_empty() || matches!(component, "." | "..") || component.contains(':')
    }) {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    let leaf = components
        .next_back()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    let name = leaf
        .rsplit_once('.')
        .map_or(leaf, |(stem, _)| stem)
        .trim_end_matches('.')
        .to_owned();
    if name.is_empty() {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(name)
}

fn ensure_item_capacity(items: &[ProjectImportItem]) -> Result<(), ProjectImportError> {
    if items.len() >= MAX_ITEMS {
        Err(ProjectImportError::TooManyItems)
    } else {
        Ok(())
    }
}

fn cargo_item(
    target_kind: &str,
    name: &str,
    command: String,
    source_name: &str,
) -> Result<ProjectImportItem, ProjectImportError> {
    if command.len() > MAX_COMMAND_BYTES {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(ProjectImportItem {
        id: format!("cargo:{target_kind}:{name}"),
        name: format!("Cargo · {target_kind} · {name}"),
        status: "new".to_owned(),
        command,
        kind: JobKind::Job,
        source: ProjectImportSource::CargoTarget,
        source_name: name.to_owned(),
        source_path: CARGO_TOML.to_owned(),
        cwd: String::new(),
        environment_keys: Vec::new(),
        requires_confirmation: true,
        detail: format!("Cargo {source_name} target · 실행 전에 작업 디렉터리를 확인하세요"),
    })
}

fn validate_script_name(value: &str) -> Result<(), ProjectImportError> {
    if value.is_empty() || value.len() > MAX_ITEM_NAME_BYTES || value.contains('\0') {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'@' | b'/')
    }) {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(())
}

fn validate_cargo_name(value: &str) -> Result<(), ProjectImportError> {
    if value.is_empty()
        || value.len() > MAX_ITEM_NAME_BYTES
        || !value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(())
}

fn referenced_environment_keys(command: &str) -> Vec<String> {
    let mut keys = BTreeSet::new();
    let bytes = command.as_bytes();
    let mut index = 0;
    while index < bytes.len() && keys.len() < MAX_ENV_KEYS {
        let (start, end, next_index) = if bytes[index] == b'$' {
            if bytes.get(index + 1) == Some(&b'{') {
                let start = index + 2;
                let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'}')
                else {
                    index += 1;
                    continue;
                };
                let end = start + relative_end;
                (start, end, end + 1)
            } else {
                let start = index + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                if end == start {
                    index += 1;
                    continue;
                }
                (start, end, end)
            }
        } else if bytes[index] == b'%' {
            let start = index + 1;
            let Some(relative_end) = bytes[start..].iter().position(|byte| *byte == b'%') else {
                index += 1;
                continue;
            };
            let end = start + relative_end;
            if end == start {
                index += 1;
                continue;
            }
            (start, end, end + 1)
        } else {
            index += 1;
            continue;
        };
        if start < end {
            let candidate = &bytes[start..end];
            if candidate.len() <= MAX_ITEM_NAME_BYTES
                && candidate
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                && candidate
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
            {
                // Candidate bytes are restricted to ASCII above, so this
                // conversion cannot expose a malformed UTF-8 slice from a
                // command containing unrelated non-ASCII text.
                keys.insert(String::from_utf8_lossy(candidate).into_owned());
            }
        }
        index = next_index;
    }
    keys.into_iter().take(MAX_ENV_KEYS).collect()
}

#[derive(Debug, PartialEq, Eq)]
struct SourceBytes {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceFileFingerprint {
    byte_length: u64,
    modified: Option<std::time::SystemTime>,
    object_identity: Option<(u64, u64)>,
}

fn source_file_fingerprint(metadata: &Metadata) -> SourceFileFingerprint {
    SourceFileFingerprint {
        byte_length: metadata.len(),
        modified: metadata.modified().ok(),
        object_identity: source_file_identity(metadata),
    }
}

#[cfg(unix)]
fn source_file_identity(metadata: &Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn source_file_identity(metadata: &Metadata) -> Option<(u64, u64)> {
    use std::os::windows::fs::MetadataExt;
    Some((
        u64::from(metadata.volume_serial_number()),
        metadata.file_index(),
    ))
}

#[cfg(not(any(unix, windows)))]
fn source_file_identity(_metadata: &Metadata) -> Option<(u64, u64)> {
    None
}

fn canonical_project_root(root: &Path) -> Result<PathBuf, ProjectImportError> {
    let raw = root.to_str().ok_or(ProjectImportError::InvalidRoot)?;
    if raw.is_empty()
        || raw.len() > MAX_PROJECT_ROOT_BYTES
        || !root.is_absolute()
        || raw.chars().any(char::is_control)
    {
        return Err(ProjectImportError::InvalidRoot);
    }
    reject_link_components(root)?;
    let metadata = fs::symlink_metadata(root).map_err(|_| ProjectImportError::InvalidRoot)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ProjectImportError::InvalidRoot);
    }
    // Bind the user-selected spelling to the same directory object that will
    // be used after canonicalization. If a parent/root is swapped between the
    // link checks and `canonicalize`, fail closed instead of previewing an
    // unexpected directory.
    let requested_identity = devbox_filesystem::filesystem_identity(root, true)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let canonical = root
        .canonicalize()
        .map_err(|_| ProjectImportError::InvalidRoot)?;
    if !canonical.is_absolute() {
        return Err(ProjectImportError::InvalidRoot);
    }
    // The final component is opened without following a link on supported
    // platforms.  This also catches Windows reparse-point roots.
    let canonical_identity = devbox_filesystem::filesystem_identity(&canonical, true)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if requested_identity != canonical_identity {
        return Err(ProjectImportError::StaleSource);
    }
    Ok(canonical)
}

/// Windows `canonicalize` may return an extended-length (`\\?\\`) spelling
/// that the shared project-path validator intentionally rejects. Keep the
/// filesystem path extended internally, but expose only its equivalent safe
/// drive/UNC spelling as the persisted cwd. The requested spelling is a
/// fallback for platforms whose canonicalizer returns a non-displayable alias.
fn safe_display_root(canonical: &Path, requested: &Path) -> Result<String, ProjectImportError> {
    for candidate in [canonical, requested] {
        let text = candidate.to_str().ok_or(ProjectImportError::UnsafeSource)?;
        let display = text
            .strip_prefix("\\\\?\\UNC\\")
            .map(|rest| format!("\\\\{rest}"))
            .or_else(|| text.strip_prefix("\\\\?\\").map(str::to_owned))
            .unwrap_or_else(|| text.to_owned());
        if let Some(safe) = devbox_filesystem::parse_safe_project_path(&display) {
            return Ok(safe.into_string());
        }
    }
    Err(ProjectImportError::UnsafeSource)
}

fn ensure_root_identity(
    root: &Path,
    expected: devbox_filesystem::FilesystemIdentity,
) -> Result<(), ProjectImportError> {
    let actual = devbox_filesystem::filesystem_identity(root, true)
        .map_err(|_| ProjectImportError::StaleSource)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ProjectImportError::StaleSource)
    }
}

fn read_source_file(
    root: &Path,
    name: &str,
    control: &ImportControl,
) -> Result<Option<SourceBytes>, ProjectImportError> {
    control.check()?;
    let path = root.join(name);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ProjectImportError::SourceUnavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProjectImportError::UnsafeSource);
    }
    if metadata.len() > MAX_SOURCE_FILE_BYTES {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let expected_fingerprint = source_file_fingerprint(&metadata);
    let canonical = path
        .canonicalize()
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if canonical.parent() != Some(root)
        || canonical.file_name().and_then(|name| name.to_str()) != Some(name)
    {
        return Err(ProjectImportError::UnsafeSource);
    }
    let identity = devbox_filesystem::filesystem_identity(&canonical, false)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let file = File::open(&canonical).map_err(|_| ProjectImportError::SourceUnavailable)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| ProjectImportError::SourceUnavailable)?;
    if !opened_metadata.is_file()
        || source_file_fingerprint(&opened_metadata) != expected_fingerprint
    {
        return Err(ProjectImportError::StaleSource);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    control.check()?;
    let mut bounded_file = file.take(MAX_SOURCE_FILE_BYTES + 1);
    bounded_file
        .read_to_end(&mut bytes)
        .map_err(|_| ProjectImportError::SourceUnavailable)?;
    if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let handle_metadata = bounded_file
        .get_ref()
        .metadata()
        .map_err(|_| ProjectImportError::StaleSource)?;
    if source_file_fingerprint(&handle_metadata) != expected_fingerprint {
        return Err(ProjectImportError::StaleSource);
    }
    let current_identity = devbox_filesystem::filesystem_identity(&canonical, false)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if identity != current_identity {
        return Err(ProjectImportError::StaleSource);
    }
    let current_metadata =
        fs::symlink_metadata(&canonical).map_err(|_| ProjectImportError::StaleSource)?;
    if current_metadata.file_type().is_symlink()
        || source_file_fingerprint(&current_metadata) != expected_fingerprint
    {
        return Err(ProjectImportError::StaleSource);
    }
    control.check()?;
    Ok(Some(SourceBytes { bytes }))
}

fn validate_operation_id(value: &str) -> Result<(), ProjectImportError> {
    if value.is_empty()
        || value.len() > MAX_OPERATION_ID_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(())
}

fn reject_link_components(path: &Path) -> Result<(), ProjectImportError> {
    let mut cursor = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => cursor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(ProjectImportError::InvalidRoot),
            Component::Normal(value) => cursor.push(value),
        }
        if cursor.as_os_str().is_empty() {
            continue;
        }
        if let Ok(metadata) = fs::symlink_metadata(&cursor) {
            if metadata.file_type().is_symlink() {
                return Err(ProjectImportError::UnsafeSource);
            }
        }
    }
    Ok(())
}

fn source_revision_with_root(
    snapshot: SourceSnapshot<'_>,
    root_identity: Option<devbox_filesystem::FilesystemIdentity>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"run-manager-project-import-v2");
    // FilesystemIdentity intentionally keeps its platform-specific fields
    // private. Hash it once with the standard library and feed only that
    // opaque value into the cryptographic revision, so paths never enter the
    // digest while a root replacement still changes the revision.
    match root_identity {
        Some(identity) => {
            hasher.update([1]);
            let mut identity_hasher = DefaultHasher::new();
            identity.hash(&mut identity_hasher);
            hasher.update(identity_hasher.finish().to_le_bytes());
        }
        None => hasher.update([0]),
    }
    for (name, bytes) in [
        (PACKAGE_JSON, snapshot.package),
        (CARGO_TOML, snapshot.cargo),
    ] {
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        match bytes {
            Some(bytes) => {
                hasher.update([1]);
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
            None => hasher.update([0]),
        }
    }
    hex_digest(hasher.finalize())
}

/// Convert a preview item into the disabled `JobInput` that the command layer
/// can save after revision/conflict checks.  The root is supplied separately
/// so parsed source text cannot smuggle a working directory into the plan.
pub fn imported_job_input(
    item: &ProjectImportItem,
    source_root: &str,
) -> crate::core::models::JobInput {
    crate::core::models::JobInput {
        name: item.name.clone(),
        command: item.command.clone(),
        cwd: Some(source_root.to_owned()),
        target_kind: crate::core::models::TargetKind::Windows,
        target_distro: None,
        environment: crate::core::models::EnvironmentUpdate::Keep,
        cron_expr: DISABLED_IMPORT_CRON.to_owned(),
        enabled: false,
        overlap_policy: crate::core::models::OverlapPolicy::Skip,
        catch_up: false,
    }
}

/// Opaque revision for the existing definition JSON import.  It is a stale
/// preview guard only, not an authenticity or secrecy primitive.
pub fn definition_revision(json: &str) -> Result<String, ProjectImportError> {
    if json.len() > MAX_DEFINITION_JSON_BYTES || json.contains('\0') {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"run-manager-definition-import-v2");
    hasher.update((json.len() as u64).to_le_bytes());
    hasher.update(json.as_bytes());
    Ok(hex_digest(hasher.finalize()))
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const PACKAGE: &[u8] = br#"{
        "name": "demo",
        "scripts": {
            "build": "vite build",
            "dev:local": "API_URL=$API_URL vite --token raw-secret-fixture"
        }
    }"#;

    const CARGO: &[u8] = br#"[package]
name = "demo"
version = "0.1.0"

[[bin]]
name = "worker"
path = "src/bin/worker.rs"

[lib]
name = "demo"

[[test]]
name = "smoke"
path = "tests/smoke.rs"
"#;

    #[test]
    fn package_scripts_become_stable_commands_without_copying_bodies() {
        let items = parse_package_scripts(PACKAGE).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].command, "npm run -- build");
        assert_eq!(items[1].command, "npm run -- dev:local");
        assert_eq!(items[1].environment_keys, vec!["API_URL"]);
        assert!(!items[1].detail.contains("raw-secret-fixture"));
        assert!(!items[1].command.contains("raw-secret-fixture"));
        assert!(!items[0].detail.contains("vite build"));
    }

    #[test]
    fn environment_key_preview_handles_windows_and_adjacent_references() {
        let package = br#"{
            "scripts": {
                "check": "echo %API_URL% && echo $TOKEN${SECOND} %WIN_KEY%"
            }
        }"#;
        let items = parse_package_scripts(package).unwrap();
        assert_eq!(
            items[0].environment_keys,
            vec!["API_URL", "SECOND", "TOKEN", "WIN_KEY"]
        );
    }

    #[test]
    fn cargo_targets_are_bounded_and_never_execute_metadata() {
        let items = parse_cargo_targets(CARGO).unwrap();
        assert!(items
            .iter()
            .any(|item| item.command == "cargo run --bin worker"));
        assert!(items.iter().any(|item| item.command == "cargo test --lib"));
        assert!(items
            .iter()
            .any(|item| item.command == "cargo test --test smoke"));
        assert!(items.iter().all(|item| item.requires_confirmation));
    }

    #[test]
    fn cargo_import_respects_disabled_automatic_binary_discovery() {
        let library_only = br#"[package]
name = "library-only"
version = "0.1.0"
autobins = false

[lib]
name = "library_only"
"#;
        let items = parse_cargo_targets(library_only).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].command, "cargo test --lib");
        assert!(!items.iter().any(|item| item.command == "cargo run"));

        let invalid = br#"[package]
name = "demo"
version = "0.1.0"
autobins = "false"
"#;
        assert_eq!(
            parse_cargo_targets(invalid),
            Err(ProjectImportError::InvalidSourceEntry)
        );
    }

    #[test]
    fn cargo_import_derives_explicit_bin_name_from_relative_path() {
        let manifest = br#"[package]
name = "demo"
version = "0.1.0"
autobins = false

[[bin]]
path = "src/bin/worker.rs"
"#;
        let items = parse_cargo_targets(manifest).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].command, "cargo run --bin worker");

        let unsafe_path = br#"[package]
name = "demo"
version = "0.1.0"

[[bin]]
path = "../worker.rs"
"#;
        assert_eq!(
            parse_cargo_targets(unsafe_path),
            Err(ProjectImportError::InvalidSourceEntry)
        );
    }

    #[test]
    fn unsafe_target_and_oversized_sources_fail_closed() {
        let unsafe_target = br#"[package]
name = "demo"
version = "0.1.0"
[[bin]]
name = "bad;run"
"#;
        assert_eq!(
            parse_cargo_targets(unsafe_target),
            Err(ProjectImportError::InvalidSourceEntry)
        );
        let malformed_package = br#"[package]
name = true
version = "0.1.0"
"#;
        assert_eq!(
            parse_cargo_targets(malformed_package),
            Err(ProjectImportError::InvalidSourceEntry)
        );
        let malformed_bin = br#"[package]
name = "demo"
version = "0.1.0"
[[bin]]
name = true
"#;
        assert_eq!(
            parse_cargo_targets(malformed_bin),
            Err(ProjectImportError::InvalidSourceEntry)
        );
        assert_eq!(
            parse_package_scripts(&vec![b'x'; MAX_SOURCE_FILE_BYTES as usize + 1]),
            Err(ProjectImportError::SourceTooLarge)
        );
    }

    #[test]
    fn revision_changes_with_source_bytes_and_has_no_path() {
        let first = source_revision_with_root(
            SourceSnapshot {
                package: Some(PACKAGE),
                cargo: Some(CARGO),
            },
            None,
        );
        let second = source_revision_with_root(
            SourceSnapshot {
                package: Some(b"{}"),
                cargo: Some(CARGO),
            },
            None,
        );
        assert_ne!(first, second);
        assert_eq!(first.len(), MAX_REVISION_BYTES);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn revision_includes_the_root_identity_without_exposing_a_path() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let first_identity =
            devbox_filesystem::filesystem_identity(first_root.path(), true).unwrap();
        let second_identity =
            devbox_filesystem::filesystem_identity(second_root.path(), true).unwrap();
        let first = source_revision_with_root(
            SourceSnapshot {
                package: Some(PACKAGE),
                cargo: None,
            },
            Some(first_identity),
        );
        let second = source_revision_with_root(
            SourceSnapshot {
                package: Some(PACKAGE),
                cargo: None,
            },
            Some(second_identity),
        );
        assert_ne!(first, second);
        assert!(!first.contains(first_root.path().to_str().unwrap()));
    }

    #[test]
    fn opened_source_fingerprint_distinguishes_same_sized_files() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        std::fs::write(&first, b"1234").unwrap();
        std::fs::write(&second, b"5678").unwrap();
        let first = source_file_fingerprint(&std::fs::metadata(first).unwrap());
        let second = source_file_fingerprint(&std::fs::metadata(second).unwrap());
        assert_eq!(first.byte_length, second.byte_length);
        assert_ne!(first.object_identity, second.object_identity);
        assert_ne!(first, second);
    }

    #[test]
    fn preview_revision_rejects_changed_local_source() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(PACKAGE_JSON), PACKAGE).unwrap();
        let plan = preview_project(root.path()).unwrap();
        std::fs::write(
            root.path().join(PACKAGE_JSON),
            br#"{"scripts":{"build":"changed"}}"#,
        )
        .unwrap();
        assert_eq!(
            verify_preview_revision(root.path(), &plan.source_root, &plan.revision),
            Err(ProjectImportError::StaleSource)
        );
    }

    #[test]
    fn symlinked_source_is_rejected() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::NamedTempFile::new().unwrap();
            symlink(outside.path(), root.path().join(PACKAGE_JSON)).unwrap();
            assert_eq!(
                read_source_file(root.path(), PACKAGE_JSON, &ImportControl::default()),
                Err(ProjectImportError::UnsafeSource)
            );
        }
    }

    #[test]
    fn operation_registry_rejects_duplicate_ids_and_routes_cancel() {
        let registry = ImportOperationRegistry::default();
        let operation = registry.begin("preview-1").unwrap();
        assert_eq!(
            registry.begin("preview-1").unwrap_err(),
            ProjectImportError::DuplicateOperation
        );
        assert!(registry.cancel("preview-1").unwrap());
        assert_eq!(
            operation.control().check(),
            Err(ProjectImportError::Cancelled)
        );
        drop(operation);
        assert!(!registry.cancel("preview-1").unwrap());
        assert!(registry.begin("preview-1").is_ok());
    }

    #[test]
    fn control_has_a_fixed_timeout_and_cancelled_state() {
        let timed_out = ImportControl::new(Duration::ZERO);
        assert_eq!(timed_out.check(), Err(ProjectImportError::TimedOut));
        let cancelled = ImportControl::new(PROJECT_IMPORT_TIMEOUT);
        cancelled.cancel();
        assert_eq!(cancelled.check(), Err(ProjectImportError::Cancelled));
    }
}
