//! Native, offline project-definition import.
//!
//! The importer reads the contents of only two files directly beneath a
//! user-selected project root: `package.json` and `Cargo.toml`.  Cargo target
//! auto-discovery uses bounded metadata-only checks of the standard target
//! layout; Rust source contents are never read.  It never invokes npm, Cargo,
//! a shell, a network client, or a dotenv loader.  Imported commands are stable
//! package/Cargo invocations, while environment values are deliberately not
//! copied.  The command layer re-reads the same files, repeats the metadata
//! snapshot, and compares `revision` before saving, so a preview cannot
//! silently apply stale source data.

use crate::core::models::JobKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet};
use std::fs::{self, Metadata};
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
const MAX_LAYOUT_DIRECTORY_ENTRIES: usize = MAX_ITEMS;
// Explicit and automatic discovery can observe the same target twice. Keep
// the pre-dedupe candidate set bounded while allowing those legitimate pairs
// to coalesce before enforcing the public 128-item result limit.
const MAX_LAYOUT_CANDIDATES: usize = MAX_ITEMS * 6;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CargoTargetKind {
    Lib,
    Bin,
    Example,
    Test,
    Bench,
}

impl CargoTargetKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
            Self::Example => "example",
            Self::Test => "test",
            Self::Bench => "bench",
        }
    }

    const fn source_section(self) -> &'static str {
        match self {
            Self::Lib => "[lib]",
            Self::Bin => "[[bin]]",
            Self::Example => "[[example]]",
            Self::Test => "[[test]]",
            Self::Bench => "[[bench]]",
        }
    }

    fn default_command(self, name: &str) -> String {
        match self {
            Self::Lib => "cargo test --lib".to_owned(),
            Self::Bin => format!("cargo run --bin {name}"),
            Self::Example => format!("cargo run --example {name}"),
            Self::Test => format!("cargo test --test {name}"),
            Self::Bench => format!("cargo bench --bench {name}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoEdition {
    E2015,
    E2018,
    E2021,
    E2024,
}

impl CargoEdition {
    fn parse(value: &str) -> Result<Self, ProjectImportError> {
        match value {
            "2015" => Ok(Self::E2015),
            "2018" => Ok(Self::E2018),
            "2021" => Ok(Self::E2021),
            "2024" => Ok(Self::E2024),
            _ => Err(ProjectImportError::InvalidSourceEntry),
        }
    }

    const fn automatic_default(self, has_manual_target: bool) -> bool {
        match self {
            Self::E2015 => !has_manual_target,
            Self::E2018 | Self::E2021 | Self::E2024 => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CargoAutoDiscovery {
    lib: bool,
    bins: bool,
    examples: bool,
    tests: bool,
    benches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SafeRelativePath {
    components: Vec<String>,
}

impl SafeRelativePath {
    fn display(&self) -> String {
        self.components.join("/")
    }

    fn join_to(&self, root: &Path) -> PathBuf {
        self.components
            .iter()
            .fold(root.to_path_buf(), |mut path, component| {
                path.push(component);
                path
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoExplicitTarget {
    kind: CargoTargetKind,
    name: String,
    path: SafeRelativePath,
    executable: bool,
    required_features: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CargoTargetOrigin {
    Auto,
    Explicit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoLayoutEntry {
    kind: CargoTargetKind,
    name: String,
    path: SafeRelativePath,
    origin: CargoTargetOrigin,
    fingerprint: SourceFileFingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CargoLayoutSnapshot {
    entries: Vec<CargoLayoutEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoManifest {
    package_name: String,
    default_run: Option<String>,
    auto: CargoAutoDiscovery,
    explicit_targets: Vec<CargoExplicitTarget>,
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
    let mut cargo_layout = None;
    if let Some(bytes) = snapshot.package {
        items.extend(parse_package_scripts(bytes)?);
        control.check()?;
    }
    if let Some(bytes) = snapshot.cargo {
        let (cargo_items, layout) =
            parse_cargo_targets_with_layout(&canonical_root, bytes, control, root_identity)?;
        items.extend(cargo_items);
        cargo_layout = Some(layout);
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
        revision: source_revision_with_layout(snapshot, Some(root_identity), cargo_layout.as_ref()),
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
pub fn normalize_import_cwd(cwd: Option<&str>) -> Result<Option<String>, ProjectImportError> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    devbox_filesystem::parse_safe_project_path(cwd)
        .map(|path| Some(path.into_string()))
        .ok_or(ProjectImportError::InvalidRoot)
}

pub fn validate_import_cwd(cwd: Option<&str>) -> bool {
    normalize_import_cwd(cwd).is_ok()
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

/// Parse only manifest-declared Cargo targets without invoking Cargo or
/// touching the project filesystem.  The project preview uses
/// `parse_cargo_targets_with_layout` below so automatic targets are resolved
/// from bounded metadata-only layout discovery.
pub fn parse_cargo_targets(bytes: &[u8]) -> Result<Vec<ProjectImportItem>, ProjectImportError> {
    let Some(manifest) = parse_cargo_manifest(bytes)? else {
        // A virtual workspace has no directly runnable target.  Workspace
        // members are intentionally outside the immediate-file import scope.
        return Ok(Vec::new());
    };
    explicit_cargo_items(&manifest)
}

fn parse_cargo_targets_with_layout(
    root: &Path,
    bytes: &[u8],
    control: &ImportControl,
    root_identity: devbox_filesystem::FilesystemIdentity,
) -> Result<(Vec<ProjectImportItem>, CargoLayoutSnapshot), ProjectImportError> {
    let Some(manifest) = parse_cargo_manifest(bytes)? else {
        return Ok((Vec::new(), CargoLayoutSnapshot::default()));
    };
    let layout = discover_cargo_layout(root, &manifest, control, root_identity)?;
    let items = merge_cargo_targets(&manifest, &layout)?;
    Ok((items, layout))
}

fn parse_cargo_manifest(bytes: &[u8]) -> Result<Option<CargoManifest>, ProjectImportError> {
    if bytes.len() as u64 > MAX_SOURCE_FILE_BYTES {
        return Err(ProjectImportError::SourceTooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ProjectImportError::InvalidToml)?;
    let value: toml::Value = toml::from_str(text).map_err(|_| ProjectImportError::InvalidToml)?;
    let table = value.as_table().ok_or(ProjectImportError::InvalidToml)?;
    let Some(package) = table.get("package") else {
        return Ok(None);
    };
    let package = package
        .as_table()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or(ProjectImportError::InvalidSourceEntry)?
        .to_owned();
    validate_cargo_name(&package_name)?;

    let edition = package
        .get("edition")
        .map(|value| {
            value
                .as_str()
                .ok_or(ProjectImportError::InvalidSourceEntry)
                .and_then(CargoEdition::parse)
        })
        .transpose()?
        .unwrap_or(CargoEdition::E2015);
    let default_run = package
        .get("default-run")
        .map(|value| {
            let name = value
                .as_str()
                .ok_or(ProjectImportError::InvalidSourceEntry)?
                .to_owned();
            validate_cargo_name(&name)?;
            Ok(name)
        })
        .transpose()?;

    let mut explicit_targets = Vec::new();
    if let Some(lib) = table.get("lib") {
        let entry = lib
            .as_table()
            .ok_or(ProjectImportError::InvalidSourceEntry)?;
        let name = entry
            .get("name")
            .map(|value| {
                value
                    .as_str()
                    .ok_or(ProjectImportError::InvalidSourceEntry)
                    .map(str::to_owned)
            })
            .transpose()?
            .unwrap_or_else(|| package_name.replace('-', "_"));
        validate_cargo_name(&name)?;
        let path = explicit_target_path(
            entry,
            inferred_target_path(CargoTargetKind::Lib, &name, &package_name)?,
        )?;
        add_explicit_target(
            &mut explicit_targets,
            CargoExplicitTarget {
                kind: CargoTargetKind::Lib,
                name,
                path,
                executable: false,
                required_features: parse_required_features(entry)?,
            },
        )?;
    }

    if let Some(bins) = table.get("bin") {
        for value in bins.as_array().ok_or(ProjectImportError::InvalidToml)? {
            let entry = value
                .as_table()
                .ok_or(ProjectImportError::InvalidSourceEntry)?;
            let name = cargo_bin_name(entry, &package_name)?;
            validate_cargo_name(&name)?;
            let path = explicit_target_path(
                entry,
                inferred_target_path(CargoTargetKind::Bin, &name, &package_name)?,
            )?;
            add_explicit_target(
                &mut explicit_targets,
                CargoExplicitTarget {
                    kind: CargoTargetKind::Bin,
                    name,
                    path,
                    executable: true,
                    required_features: parse_required_features(entry)?,
                },
            )?;
        }
    }

    for (section, kind) in [
        ("example", CargoTargetKind::Example),
        ("test", CargoTargetKind::Test),
        ("bench", CargoTargetKind::Bench),
    ] {
        if let Some(entries) = table.get(section) {
            for value in entries.as_array().ok_or(ProjectImportError::InvalidToml)? {
                let entry = value
                    .as_table()
                    .ok_or(ProjectImportError::InvalidSourceEntry)?;
                let name = entry
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .ok_or(ProjectImportError::InvalidSourceEntry)?
                    .to_owned();
                validate_cargo_name(&name)?;
                let path =
                    explicit_target_path(entry, inferred_target_path(kind, &name, &package_name)?)?;
                add_explicit_target(
                    &mut explicit_targets,
                    CargoExplicitTarget {
                        kind,
                        name,
                        path,
                        executable: target_is_executable(entry, kind)?,
                        required_features: parse_required_features(entry)?,
                    },
                )?;
            }
        }
    }

    let has_manual_target = !explicit_targets.is_empty();
    let automatic_default = edition.automatic_default(has_manual_target);
    let auto = CargoAutoDiscovery {
        lib: parse_auto_flag(package, "autolib", automatic_default)?,
        bins: parse_auto_flag(package, "autobins", automatic_default)?,
        examples: parse_auto_flag(package, "autoexamples", automatic_default)?,
        tests: parse_auto_flag(package, "autotests", automatic_default)?,
        benches: parse_auto_flag(package, "autobenches", automatic_default)?,
    };

    Ok(Some(CargoManifest {
        package_name,
        default_run,
        auto,
        explicit_targets,
    }))
}

fn explicit_cargo_items(
    manifest: &CargoManifest,
) -> Result<Vec<ProjectImportItem>, ProjectImportError> {
    let mut items = Vec::new();
    for target in &manifest.explicit_targets {
        if target.required_features
            || (target.kind == CargoTargetKind::Example && !target.executable)
        {
            continue;
        }
        ensure_item_capacity(&items)?;
        items.push(cargo_item(
            target.kind.as_str(),
            &target.name,
            target.kind.default_command(&target.name),
            target.kind.source_section(),
        )?);
    }
    items.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(items)
}

fn parse_auto_flag(
    package: &toml::value::Table,
    key: &str,
    default: bool,
) -> Result<bool, ProjectImportError> {
    package
        .get(key)
        .map(|value| {
            value
                .as_bool()
                .ok_or(ProjectImportError::InvalidSourceEntry)
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

fn add_explicit_target(
    targets: &mut Vec<CargoExplicitTarget>,
    target: CargoExplicitTarget,
) -> Result<(), ProjectImportError> {
    if targets.iter().any(|existing| {
        existing.kind == target.kind
            && (existing.name == target.name || existing.path == target.path)
    }) {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    if targets.len() >= MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    targets.push(target);
    Ok(())
}

fn explicit_target_path(
    entry: &toml::value::Table,
    inferred: SafeRelativePath,
) -> Result<SafeRelativePath, ProjectImportError> {
    entry
        .get("path")
        .map(|value| {
            value
                .as_str()
                .ok_or(ProjectImportError::InvalidSourceEntry)
                .and_then(parse_safe_relative_path)
        })
        .transpose()
        .map(|path| path.unwrap_or(inferred))
}

fn inferred_target_path(
    kind: CargoTargetKind,
    name: &str,
    package_name: &str,
) -> Result<SafeRelativePath, ProjectImportError> {
    let path = match kind {
        CargoTargetKind::Lib => "src/lib.rs".to_owned(),
        CargoTargetKind::Bin if name == package_name => "src/main.rs".to_owned(),
        CargoTargetKind::Bin => format!("src/bin/{name}.rs"),
        CargoTargetKind::Example => format!("examples/{name}.rs"),
        CargoTargetKind::Test => format!("tests/{name}.rs"),
        CargoTargetKind::Bench => format!("benches/{name}.rs"),
    };
    parse_safe_relative_path(&path)
}

fn parse_required_features(entry: &toml::value::Table) -> Result<bool, ProjectImportError> {
    let Some(value) = entry.get("required-features") else {
        return Ok(false);
    };
    let values = value
        .as_array()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    if values.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    for value in values {
        let feature = value
            .as_str()
            .ok_or(ProjectImportError::InvalidSourceEntry)?;
        if feature.is_empty()
            || feature.len() > MAX_ITEM_NAME_BYTES
            || feature.chars().any(char::is_control)
        {
            return Err(ProjectImportError::InvalidSourceEntry);
        }
    }
    Ok(!values.is_empty())
}

fn target_is_executable(
    entry: &toml::value::Table,
    kind: CargoTargetKind,
) -> Result<bool, ProjectImportError> {
    if kind != CargoTargetKind::Example {
        return Ok(true);
    }
    let Some(value) = entry.get("crate-type") else {
        return Ok(true);
    };
    let values = value
        .as_array()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    if values.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    let mut executable = false;
    for value in values {
        let crate_type = value
            .as_str()
            .ok_or(ProjectImportError::InvalidSourceEntry)?;
        if crate_type.is_empty()
            || crate_type.len() > MAX_ITEM_NAME_BYTES
            || crate_type.chars().any(char::is_control)
        {
            return Err(ProjectImportError::InvalidSourceEntry);
        }
        executable |= crate_type == "bin";
    }
    Ok(executable)
}

/// Cargo derives an explicit `[[bin]]` target name from its `path` when the
/// optional `name` field is omitted.  The path is metadata only and is never
/// opened or passed to a process by this parser.
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
    let path = parse_safe_relative_path(path)?;
    let leaf = path
        .components
        .last()
        .ok_or(ProjectImportError::InvalidSourceEntry)?;
    let name = leaf
        .rsplit_once('.')
        .map_or(leaf.as_str(), |(stem, _)| stem)
        .trim_end_matches('.')
        .to_owned();
    if name.is_empty() {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(name)
}

fn parse_safe_relative_path(value: &str) -> Result<SafeRelativePath, ProjectImportError> {
    if value.is_empty()
        || value.len() > MAX_PROJECT_ROOT_BYTES
        || value.chars().any(char::is_control)
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.as_bytes().get(1) == Some(&b':')
    {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    let components = value
        .split(['/', '\\'])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|component| {
            component.is_empty()
                || matches!(component.as_str(), "." | "..")
                || component.contains(':')
        })
    {
        return Err(ProjectImportError::InvalidSourceEntry);
    }
    Ok(SafeRelativePath { components })
}

fn append_relative_component(base: &SafeRelativePath, component: &str) -> SafeRelativePath {
    let mut components = base.components.clone();
    components.push(component.to_owned());
    SafeRelativePath { components }
}

fn fixed_relative_path(value: &str) -> SafeRelativePath {
    parse_safe_relative_path(value).expect("fixed Cargo layout path must be safe")
}

fn discover_cargo_layout(
    root: &Path,
    manifest: &CargoManifest,
    control: &ImportControl,
    expected_root_identity: devbox_filesystem::FilesystemIdentity,
) -> Result<CargoLayoutSnapshot, ProjectImportError> {
    let mut entries = Vec::new();
    for target in &manifest.explicit_targets {
        control.check()?;
        let Some(fingerprint) = inspect_target_file(root, &target.path, control, true)? else {
            return Err(ProjectImportError::SourceUnavailable);
        };
        entries.push(CargoLayoutEntry {
            kind: target.kind,
            name: target.name.clone(),
            path: target.path.clone(),
            origin: CargoTargetOrigin::Explicit,
            fingerprint,
        });
        ensure_root_identity(root, expected_root_identity)?;
    }

    if manifest.auto.lib {
        discover_auto_file(
            root,
            CargoTargetKind::Lib,
            &manifest.package_name.replace('-', "_"),
            &fixed_relative_path("src/lib.rs"),
            control,
            &mut entries,
        )?;
        ensure_root_identity(root, expected_root_identity)?;
    }
    if manifest.auto.bins {
        discover_auto_file(
            root,
            CargoTargetKind::Bin,
            &manifest.package_name,
            &fixed_relative_path("src/main.rs"),
            control,
            &mut entries,
        )?;
        scan_auto_directory(
            root,
            &fixed_relative_path("src/bin"),
            CargoTargetKind::Bin,
            control,
            &mut entries,
        )?;
        ensure_root_identity(root, expected_root_identity)?;
    }
    if manifest.auto.examples {
        scan_auto_directory(
            root,
            &fixed_relative_path("examples"),
            CargoTargetKind::Example,
            control,
            &mut entries,
        )?;
        ensure_root_identity(root, expected_root_identity)?;
    }
    if manifest.auto.tests {
        scan_auto_directory(
            root,
            &fixed_relative_path("tests"),
            CargoTargetKind::Test,
            control,
            &mut entries,
        )?;
        ensure_root_identity(root, expected_root_identity)?;
    }
    if manifest.auto.benches {
        scan_auto_directory(
            root,
            &fixed_relative_path("benches"),
            CargoTargetKind::Bench,
            control,
            &mut entries,
        )?;
        ensure_root_identity(root, expected_root_identity)?;
    }
    ensure_root_identity(root, expected_root_identity)?;
    coalesce_layout_entries(entries)
}

fn discover_auto_file(
    root: &Path,
    kind: CargoTargetKind,
    name: &str,
    path: &SafeRelativePath,
    control: &ImportControl,
    entries: &mut Vec<CargoLayoutEntry>,
) -> Result<(), ProjectImportError> {
    let Some(fingerprint) = inspect_target_file(root, path, control, false)? else {
        return Ok(());
    };
    add_layout_entry(
        entries,
        CargoLayoutEntry {
            kind,
            name: name.to_owned(),
            path: path.clone(),
            origin: CargoTargetOrigin::Auto,
            fingerprint,
        },
    )
}

fn scan_auto_directory(
    root: &Path,
    directory: &SafeRelativePath,
    kind: CargoTargetKind,
    control: &ImportControl,
    entries: &mut Vec<CargoLayoutEntry>,
) -> Result<(), ProjectImportError> {
    control.check()?;
    let directory_path = directory.join_to(root);
    let metadata = match fs::symlink_metadata(&directory_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(ProjectImportError::SourceUnavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ProjectImportError::UnsafeSource);
    }
    devbox_filesystem::ensure_no_links(&directory_path)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let before_identity = devbox_filesystem::filesystem_identity(&directory_path, true)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let read_dir =
        fs::read_dir(&directory_path).map_err(|_| ProjectImportError::SourceUnavailable)?;
    let mut seen_entries = 0usize;
    for entry in read_dir {
        control.check()?;
        seen_entries += 1;
        if seen_entries > MAX_LAYOUT_DIRECTORY_ENTRIES {
            return Err(ProjectImportError::TooManyItems);
        }
        let entry = entry.map_err(|_| ProjectImportError::SourceUnavailable)?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let child_path = append_relative_component(directory, file_name);
        let child_fs_path = child_path.join_to(root);
        let child_metadata = fs::symlink_metadata(&child_fs_path)
            .map_err(|_| ProjectImportError::SourceUnavailable)?;
        if child_metadata.file_type().is_symlink() {
            return Err(ProjectImportError::UnsafeSource);
        }
        devbox_filesystem::ensure_no_links(&child_fs_path)
            .map_err(|_| ProjectImportError::UnsafeSource)?;

        if child_metadata.is_file() {
            let Some(name) = file_name.strip_suffix(".rs") else {
                continue;
            };
            validate_cargo_name(name)?;
            let Some(fingerprint) = inspect_target_file(root, &child_path, control, true)? else {
                return Err(ProjectImportError::SourceUnavailable);
            };
            add_layout_entry(
                entries,
                CargoLayoutEntry {
                    kind,
                    name: name.to_owned(),
                    path: child_path,
                    origin: CargoTargetOrigin::Auto,
                    fingerprint,
                },
            )?;
        } else if child_metadata.is_dir() {
            let main_path = append_relative_component(&child_path, "main.rs");
            let Some(fingerprint) = inspect_target_file(root, &main_path, control, false)? else {
                continue;
            };
            validate_cargo_name(file_name)?;
            add_layout_entry(
                entries,
                CargoLayoutEntry {
                    kind,
                    name: file_name.to_owned(),
                    path: main_path,
                    origin: CargoTargetOrigin::Auto,
                    fingerprint,
                },
            )?;
        }
    }
    control.check()?;
    let after_identity = devbox_filesystem::filesystem_identity(&directory_path, true)
        .map_err(|_| ProjectImportError::StaleSource)?;
    if before_identity != after_identity {
        return Err(ProjectImportError::StaleSource);
    }
    Ok(())
}

fn inspect_target_file(
    root: &Path,
    path: &SafeRelativePath,
    control: &ImportControl,
    required: bool,
) -> Result<Option<SourceFileFingerprint>, ProjectImportError> {
    control.check()?;
    let filesystem_path = path.join_to(root);
    let metadata = match fs::symlink_metadata(&filesystem_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if required {
                return Err(ProjectImportError::SourceUnavailable);
            }
            return Ok(None);
        }
        Err(_) => return Err(ProjectImportError::SourceUnavailable),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProjectImportError::UnsafeSource);
    }
    devbox_filesystem::ensure_no_links(&filesystem_path)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let canonical = filesystem_path
        .canonicalize()
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if canonical.strip_prefix(root).is_err() {
        return Err(ProjectImportError::UnsafeSource);
    }
    let identity = devbox_filesystem::filesystem_identity(&filesystem_path, false)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    let fingerprint = source_file_fingerprint(&metadata, Some(identity));
    control.check()?;
    devbox_filesystem::ensure_no_links(&filesystem_path)
        .map_err(|_| ProjectImportError::StaleSource)?;
    let current_canonical = filesystem_path
        .canonicalize()
        .map_err(|_| ProjectImportError::StaleSource)?;
    if current_canonical.strip_prefix(root).is_err() {
        return Err(ProjectImportError::StaleSource);
    }
    let current_metadata =
        fs::symlink_metadata(&filesystem_path).map_err(|_| ProjectImportError::StaleSource)?;
    let current_identity = devbox_filesystem::filesystem_identity(&filesystem_path, false)
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if current_metadata.file_type().is_symlink()
        || source_file_fingerprint(&current_metadata, Some(current_identity)) != fingerprint
        || current_identity != identity
    {
        return Err(ProjectImportError::StaleSource);
    }
    control.check()?;
    Ok(Some(fingerprint))
}

fn add_layout_entry(
    entries: &mut Vec<CargoLayoutEntry>,
    entry: CargoLayoutEntry,
) -> Result<(), ProjectImportError> {
    if entries.len() >= MAX_LAYOUT_CANDIDATES {
        return Err(ProjectImportError::TooManyItems);
    }
    entries.push(entry);
    Ok(())
}

fn coalesce_layout_entries(
    entries: Vec<CargoLayoutEntry>,
) -> Result<CargoLayoutSnapshot, ProjectImportError> {
    let explicit_paths = entries
        .iter()
        .filter(|entry| entry.origin == CargoTargetOrigin::Explicit)
        .map(|entry| (entry.kind, entry.path.clone()))
        .collect::<BTreeSet<_>>();
    let mut by_key: BTreeMap<(CargoTargetKind, String), CargoLayoutEntry> = BTreeMap::new();
    for entry in entries {
        // Cargo's explicit target table is authoritative for a standard-layout
        // source path. In particular, an executable auto-example must not be
        // synthesized for a path that an explicit non-bin example renamed or
        // configured with a library crate type.
        if entry.origin == CargoTargetOrigin::Auto
            && explicit_paths.contains(&(entry.kind, entry.path.clone()))
        {
            continue;
        }
        let key = (entry.kind, entry.name.clone());
        if let Some(existing) = by_key.get(&key) {
            if existing.path != entry.path {
                return Err(ProjectImportError::InvalidSourceEntry);
            }
            if existing.fingerprint != entry.fingerprint {
                return Err(ProjectImportError::StaleSource);
            }
            if entry.origin > existing.origin {
                by_key.insert(key, entry);
            }
        } else {
            by_key.insert(key, entry);
        }
    }
    if by_key.len() > MAX_ITEMS {
        return Err(ProjectImportError::TooManyItems);
    }
    Ok(CargoLayoutSnapshot {
        entries: by_key.into_values().collect(),
    })
}

fn merge_cargo_targets(
    manifest: &CargoManifest,
    layout: &CargoLayoutSnapshot,
) -> Result<Vec<ProjectImportItem>, ProjectImportError> {
    let mut by_key: BTreeMap<(CargoTargetKind, String), &CargoLayoutEntry> = BTreeMap::new();
    for entry in &layout.entries {
        let key = (entry.kind, entry.name.clone());
        if let Some(existing) = by_key.get(&key) {
            if existing.path != entry.path {
                return Err(ProjectImportError::InvalidSourceEntry);
            }
        } else {
            by_key.insert(key, entry);
        }
    }

    let mut bin_names = BTreeSet::new();
    let mut items = Vec::new();
    for ((kind, name), entry) in by_key {
        if kind == CargoTargetKind::Bin {
            bin_names.insert(name.clone());
        }
        let explicit = manifest
            .explicit_targets
            .iter()
            .find(|target| target.kind == kind && target.name == name);
        if let Some(target) = explicit {
            if target.required_features
                || (target.kind == CargoTargetKind::Example && !target.executable)
            {
                continue;
            }
        }
        ensure_item_capacity(&items)?;
        items.push(cargo_item(
            kind.as_str(),
            &name,
            kind.default_command(&name),
            if entry.origin == CargoTargetOrigin::Explicit {
                kind.source_section()
            } else {
                "auto-discovered layout"
            },
        )?);
    }
    if let Some(default_run) = &manifest.default_run {
        if !bin_names.contains(default_run) {
            return Err(ProjectImportError::InvalidSourceEntry);
        }
    }
    items.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(items)
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
    object_identity: Option<devbox_filesystem::FilesystemIdentity>,
}

fn source_file_fingerprint(
    metadata: &Metadata,
    object_identity: Option<devbox_filesystem::FilesystemIdentity>,
) -> SourceFileFingerprint {
    SourceFileFingerprint {
        byte_length: metadata.len(),
        modified: metadata.modified().ok(),
        object_identity,
    }
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
    devbox_filesystem::ensure_no_links(root).map_err(|_| ProjectImportError::UnsafeSource)?;
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
    let expected_fingerprint = source_file_fingerprint(&metadata, None);
    devbox_filesystem::ensure_no_links(&path).map_err(|_| ProjectImportError::UnsafeSource)?;
    let canonical = path
        .canonicalize()
        .map_err(|_| ProjectImportError::UnsafeSource)?;
    if canonical.parent() != Some(root)
        || canonical.file_name().and_then(|name| name.to_str()) != Some(name)
    {
        return Err(ProjectImportError::UnsafeSource);
    }
    let (file, identity) = devbox_filesystem::open_filesystem_object(&canonical, false)
        .map_err(|_| ProjectImportError::SourceUnavailable)?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| ProjectImportError::SourceUnavailable)?;
    if !opened_metadata.is_file()
        || source_file_fingerprint(&opened_metadata, None) != expected_fingerprint
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
    if source_file_fingerprint(&handle_metadata, None) != expected_fingerprint {
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
        || source_file_fingerprint(&current_metadata, None) != expected_fingerprint
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

#[cfg(test)]
fn source_revision_with_root(
    snapshot: SourceSnapshot<'_>,
    root_identity: Option<devbox_filesystem::FilesystemIdentity>,
) -> String {
    source_revision_with_layout(snapshot, root_identity, None)
}

fn source_revision_with_layout(
    snapshot: SourceSnapshot<'_>,
    root_identity: Option<devbox_filesystem::FilesystemIdentity>,
    cargo_layout: Option<&CargoLayoutSnapshot>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"run-manager-project-import-v3");
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
    match cargo_layout {
        Some(layout) => {
            hasher.update([1]);
            let layout_revision = cargo_layout_revision(layout);
            hasher.update((layout_revision.len() as u64).to_le_bytes());
            hasher.update(layout_revision);
        }
        None => hasher.update([0]),
    }
    hex_digest(hasher.finalize())
}

fn cargo_layout_revision(layout: &CargoLayoutSnapshot) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"run-manager-cargo-layout-v1");
    for entry in &layout.entries {
        hasher.update([match entry.kind {
            CargoTargetKind::Lib => 0,
            CargoTargetKind::Bin => 1,
            CargoTargetKind::Example => 2,
            CargoTargetKind::Test => 3,
            CargoTargetKind::Bench => 4,
        }]);
        update_hashed_string(&mut hasher, &entry.name);
        update_hashed_string(&mut hasher, &entry.path.display());
        update_hashed_fingerprint(&mut hasher, entry.fingerprint);
    }
    hasher.finalize().to_vec()
}

fn update_hashed_string(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn update_hashed_fingerprint(hasher: &mut Sha256, fingerprint: SourceFileFingerprint) {
    hasher.update(fingerprint.byte_length.to_le_bytes());
    match fingerprint.modified {
        Some(modified) => match modified.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => {
                hasher.update([1]);
                hasher.update(duration.as_secs().to_le_bytes());
                hasher.update(duration.subsec_nanos().to_le_bytes());
            }
            Err(_) => hasher.update([0]),
        },
        None => hasher.update([0]),
    }
    match fingerprint.object_identity {
        Some(identity) => {
            hasher.update([1]);
            let mut identity_hasher = DefaultHasher::new();
            identity.hash(&mut identity_hasher);
            hasher.update(identity_hasher.finish().to_le_bytes());
        }
        None => hasher.update([0]),
    }
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
    fn definition_import_cwd_returns_the_validated_canonical_spelling() {
        assert_eq!(
            normalize_import_cwd(Some(" C:/Work/demo/ ")).unwrap(),
            Some("C:/Work/demo".to_owned())
        );
        assert_eq!(normalize_import_cwd(None).unwrap(), None);
        assert!(normalize_import_cwd(Some("relative/project")).is_err());
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

        let package_only = br#"[package]
name = "package-only"
version = "0.1.0"
"#;
        let items = parse_cargo_targets(package_only).unwrap();
        assert!(items.is_empty());
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

    fn write_project_file(root: &std::path::Path, relative: &str, contents: &[u8]) {
        let path = relative
            .split('/')
            .fold(root.to_path_buf(), |mut path, component| {
                path.push(component);
                path
            });
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn preview_discovers_bounded_standard_cargo_layout_without_reading_sources() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        for path in [
            "src/lib.rs",
            "src/main.rs",
            "src/bin/worker.rs",
            "src/bin/tools/main.rs",
            "examples/basic.rs",
            "examples/multi/main.rs",
            "tests/smoke.rs",
            "tests/multi/main.rs",
            "benches/throughput.rs",
            "benches/multi/main.rs",
        ] {
            // Invalid source bytes prove discovery uses metadata only.
            write_project_file(root.path(), path, b"\0not-rust");
        }

        let plan = preview_project(root.path()).unwrap();
        let commands = plan
            .items
            .iter()
            .map(|item| item.command.as_str())
            .collect::<BTreeSet<_>>();
        for command in [
            "cargo test --lib",
            "cargo run --bin demo",
            "cargo run --bin worker",
            "cargo run --bin tools",
            "cargo run --example basic",
            "cargo run --example multi",
            "cargo test --test smoke",
            "cargo test --test multi",
            "cargo bench --bench throughput",
            "cargo bench --bench multi",
        ] {
            assert!(commands.contains(command), "missing {command}");
        }
        assert!(!plan.items.iter().any(|item| item.command == "cargo run"));
    }

    #[test]
    fn cargo_auto_flags_and_edition_follow_cargo_defaults() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "legacy"
version = "0.1.0"
edition = "2015"

[[bin]]
name = "worker"
path = "src/bin/worker.rs"
"#,
        )
        .unwrap();
        write_project_file(root.path(), "src/main.rs", b"not-rust");
        write_project_file(root.path(), "src/bin/worker.rs", b"not-rust");
        let plan = preview_project(root.path()).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .map(|item| item.command.as_str())
                .collect::<Vec<_>>(),
            vec!["cargo run --bin worker"]
        );

        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "flags"
version = "0.1.0"
edition = "2021"
autolib = false
autobins = false
autoexamples = false
autotests = false
autobenches = false

[[bin]]
name = "worker"
path = "custom/worker.rs"
"#,
        )
        .unwrap();
        for path in [
            "src/lib.rs",
            "src/main.rs",
            "examples/basic.rs",
            "tests/smoke.rs",
            "benches/perf.rs",
            "custom/worker.rs",
        ] {
            write_project_file(root.path(), path, b"not-rust");
        }
        let plan = preview_project(root.path()).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].command, "cargo run --bin worker");
    }

    #[test]
    fn explicit_and_automatic_targets_merge_by_kind_name_and_path() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
default-run = "worker"

[[bin]]
name = "worker"
path = "src/bin/worker.rs"
"#,
        )
        .unwrap();
        write_project_file(root.path(), "src/main.rs", b"not-rust");
        write_project_file(root.path(), "src/bin/worker.rs", b"not-rust");
        write_project_file(root.path(), "src/bin/other.rs", b"not-rust");
        let plan = preview_project(root.path()).unwrap();
        let bin_commands = plan
            .items
            .iter()
            .filter(|item| item.command.starts_with("cargo run --bin"))
            .map(|item| item.command.as_str())
            .collect::<BTreeSet<_>>();
        let expected = [
            "cargo run --bin demo",
            "cargo run --bin other",
            "cargo run --bin worker",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(bin_commands, expected);
    }

    #[test]
    fn conflicting_explicit_and_automatic_target_paths_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "worker"
path = "custom/worker.rs"
"#,
        )
        .unwrap();
        write_project_file(root.path(), "custom/worker.rs", b"not-rust");
        write_project_file(root.path(), "src/bin/worker.rs", b"not-rust");
        assert_eq!(
            preview_project(root.path()),
            Err(ProjectImportError::InvalidSourceEntry)
        );
    }

    #[test]
    fn required_features_and_library_examples_are_not_execution_tasks() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "feature-bin"
path = "custom/feature.rs"
required-features = ["feature"]

[[example]]
name = "library-example"
path = "examples/library.rs"
crate-type = ["staticlib"]

[[test]]
name = "smoke"
path = "tests/smoke.rs"
"#,
        )
        .unwrap();
        for path in ["custom/feature.rs", "examples/library.rs", "tests/smoke.rs"] {
            write_project_file(root.path(), path, b"not-rust");
        }
        let plan = preview_project(root.path()).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .map(|item| item.command.as_str())
                .collect::<Vec<_>>(),
            vec!["cargo test --test smoke"]
        );
    }

    #[test]
    fn layout_changes_make_a_project_preview_stale() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
        )
        .unwrap();
        write_project_file(root.path(), "src/main.rs", b"not-rust");
        let plan = preview_project(root.path()).unwrap();
        assert!(!plan.revision.contains(root.path().to_str().unwrap()));
        write_project_file(root.path(), "src/bin/new.rs", b"not-rust");
        assert_eq!(
            verify_preview_revision(root.path(), &plan.source_root, &plan.revision),
            Err(ProjectImportError::StaleSource)
        );
    }

    #[test]
    fn explicit_target_files_are_required_but_auto_files_may_be_absent() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(CARGO_TOML),
            br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "missing"
path = "src/bin/missing.rs"
"#,
        )
        .unwrap();
        assert_eq!(
            preview_project(root.path()),
            Err(ProjectImportError::SourceUnavailable)
        );
    }

    #[test]
    fn symlinked_automatic_target_is_rejected_without_following_it() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let root = tempfile::tempdir().unwrap();
            std::fs::write(
                root.path().join(CARGO_TOML),
                br#"[package]
name = "demo"
version = "0.1.0"
edition = "2021"
"#,
            )
            .unwrap();
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::fs::create_dir_all(root.path().join("src")).unwrap();
            symlink(outside.path(), root.path().join("src/main.rs")).unwrap();
            assert_eq!(
                preview_project(root.path()),
                Err(ProjectImportError::UnsafeSource)
            );
        }
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
        let first_identity = devbox_filesystem::filesystem_identity(&first, false).unwrap();
        let second_identity = devbox_filesystem::filesystem_identity(&second, false).unwrap();
        let first =
            source_file_fingerprint(&std::fs::metadata(first).unwrap(), Some(first_identity));
        let second =
            source_file_fingerprint(&std::fs::metadata(second).unwrap(), Some(second_identity));
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
