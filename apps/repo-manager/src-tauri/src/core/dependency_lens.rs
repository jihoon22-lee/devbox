//! Offline dependency inventory for one explicitly selected repository.
//!
//! The scanner reads only generated lockfiles and their local manifests. It
//! never invokes Cargo, pnpm, npm, uv, Gradle, a shell, or a build script. All
//! filesystem input, parsed nodes/edges, recursion, and published summaries
//! have explicit bounds so a repository cannot turn the read-only panel into
//! an unbounded parser or snapshot producer.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::Metadata;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEPENDENCY_LENS_ERROR: &str = "Dependency Lens 분석을 완료하지 못했습니다.";
pub const DEPENDENCY_SUMMARY_PRODUCER: &str = "repo-manager";
pub const DEPENDENCY_SUMMARY_VIEW: &str = "dependency-summary";
pub const DEPENDENCY_SUMMARY_VERSION: u32 = 1;

const MAX_SCAN_DEPTH: usize = 8;
const MAX_VISITED_DIRECTORIES: usize = 10_000;
const MAX_DIRECTORY_ENTRIES: usize = 10_000;
const MAX_INPUT_FILES: usize = 256;
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOTAL_INPUT_BYTES: usize = 24 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_PACKAGES: usize = 4_096;
const MAX_EDGES: usize = 16_384;
const MAX_PACKAGE_NAME_BYTES: usize = 256;
const MAX_VERSION_BYTES: usize = 128;
const MAX_PACKAGE_MANAGER_BYTES: usize = 256;
const MAX_RELATIVE_PATH_BYTES: usize = 4_096;
const MAX_SUMMARY_ENTRIES: usize = 256;
const MAX_SUMMARY_AGE_MS: u64 = 90 * 24 * 60 * 60 * 1_000;
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyEcosystem {
    Cargo,
    Pnpm,
    Npm,
    Python,
    Gradle,
}

impl DependencyEcosystem {
    fn key(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Python => "python",
            Self::Gradle => "gradle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencySourceStatus {
    Ready,
    MissingLockfile,
    StaleLockfile,
    Invalid,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencySource {
    pub ecosystem: DependencyEcosystem,
    /// Repository-relative manifest or lockfile path. Absolute paths never
    /// cross IPC or the integration snapshot boundary.
    pub path: String,
    pub status: DependencySourceStatus,
    pub manifest_count: usize,
    pub lockfile_count: usize,
    pub package_count: usize,
    pub direct_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyPackage {
    pub id: String,
    pub ecosystem: DependencyEcosystem,
    pub name: String,
    pub version: String,
    pub direct: bool,
    /// Resolved package node IDs only. A separate aggregate exposes the count
    /// of safe-but-ambiguous references which could not be resolved locally.
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateDependency {
    pub ecosystem: DependencyEcosystem,
    pub name: String,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyReport {
    pub revision: String,
    pub sources: Vec<DependencySource>,
    pub packages: Vec<DependencyPackage>,
    pub duplicates: Vec<DuplicateDependency>,
    pub package_count: usize,
    pub direct_count: usize,
    pub transitive_count: usize,
    pub unresolved_dependency_count: usize,
    pub missing_lockfile_count: usize,
    pub stale_lockfile_count: usize,
    pub unsupported_count: usize,
    pub invalid_count: usize,
    pub truncated: bool,
    pub summary_published: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DependencySummaryEcosystem {
    pub ecosystem: String,
    pub package_count: usize,
    pub direct_count: usize,
    pub duplicate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DependencySummaryEntry {
    pub project_id: String,
    pub revision: String,
    pub scanned_at_ms: u64,
    pub package_count: usize,
    pub direct_count: usize,
    pub transitive_count: usize,
    pub duplicate_count: usize,
    pub unresolved_dependency_count: usize,
    pub missing_lockfile_count: usize,
    pub stale_lockfile_count: usize,
    pub unsupported_count: usize,
    pub invalid_count: usize,
    pub truncated: bool,
    pub ecosystems: Vec<DependencySummaryEcosystem>,
}

#[derive(Debug, Clone)]
struct InputFile {
    relative: String,
    bytes: Vec<u8>,
    modified_ms: u64,
}

impl InputFile {
    fn name(&self) -> &str {
        Path::new(&self.relative)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
    }

    fn parent(&self) -> &Path {
        Path::new(&self.relative)
            .parent()
            .unwrap_or_else(|| Path::new(""))
    }

    fn text(&self) -> Result<&str, ParseFailure> {
        std::str::from_utf8(&self.bytes).map_err(|_| ParseFailure::Invalid)
    }
}

#[derive(Debug, Clone)]
struct InputProblem {
    relative: String,
    ecosystem: DependencyEcosystem,
}

#[derive(Debug, Default)]
struct InputDiscovery {
    files: Vec<InputFile>,
    problems: Vec<InputProblem>,
    truncated: bool,
}

#[derive(Debug, Clone)]
struct ParsedNode {
    ecosystem: DependencyEcosystem,
    name: String,
    version: String,
    direct: bool,
    dependencies: Vec<DependencyReference>,
}

#[derive(Debug)]
struct NodeManifest {
    direct_names: BTreeSet<String>,
    ecosystem: DependencyEcosystem,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyReference {
    name: String,
    version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseFailure {
    Invalid,
    Unsupported,
}

#[derive(Debug, Default)]
struct ParseResult {
    nodes: Vec<ParsedNode>,
}

struct DependencyCollection<'a> {
    nodes: &'a mut Vec<ParsedNode>,
    sources: &'a mut Vec<DependencySource>,
    truncated: &'a mut bool,
}

pub fn analyze_repository(root: &Path, budget: Duration) -> Result<DependencyReport, String> {
    let deadline = Instant::now() + budget;
    let discovery = discover_inputs(root, deadline)?;
    let revision = input_revision(&discovery.files, &discovery.problems);
    let mut nodes = Vec::new();
    let mut sources = Vec::new();
    let mut truncated = discovery.truncated;
    let mut unresolved_dependency_count = 0usize;

    let cargo_manifests = matching_files(&discovery.files, "Cargo.toml");
    let cargo_locks = matching_files(&discovery.files, "Cargo.lock");
    collect_toml_lock_sources(
        &cargo_locks,
        &cargo_manifests,
        DependencyEcosystem::Cargo,
        parse_cargo_lock,
        parse_cargo_direct_names,
        &mut DependencyCollection {
            nodes: &mut nodes,
            sources: &mut sources,
            truncated: &mut truncated,
        },
    );

    let package_manifests = matching_files(&discovery.files, "package.json");
    let pnpm_locks = matching_files(&discovery.files, "pnpm-lock.yaml");
    let npm_locks = matching_files(&discovery.files, "package-lock.json");
    let mut assigned_node_manifests = HashSet::new();
    collect_node_lock_sources(
        &pnpm_locks,
        &package_manifests,
        DependencyEcosystem::Pnpm,
        parse_pnpm_lock,
        &mut assigned_node_manifests,
        &mut DependencyCollection {
            nodes: &mut nodes,
            sources: &mut sources,
            truncated: &mut truncated,
        },
    );
    collect_node_lock_sources(
        &npm_locks,
        &package_manifests,
        DependencyEcosystem::Npm,
        parse_package_lock,
        &mut assigned_node_manifests,
        &mut DependencyCollection {
            nodes: &mut nodes,
            sources: &mut sources,
            truncated: &mut truncated,
        },
    );
    for (index, manifest) in package_manifests.iter().enumerate() {
        if !assigned_node_manifests.contains(&index) {
            match manifest.text().and_then(parse_node_manifest) {
                Ok(parsed) => sources.push(missing_source(parsed.ecosystem, manifest)),
                Err(_) => sources.push(DependencySource {
                    ecosystem: DependencyEcosystem::Npm,
                    path: manifest.relative.clone(),
                    status: DependencySourceStatus::Invalid,
                    manifest_count: 1,
                    lockfile_count: 0,
                    package_count: 0,
                    direct_count: 0,
                }),
            }
        }
    }

    let python_manifests = matching_files(&discovery.files, "pyproject.toml");
    let uv_locks = matching_files(&discovery.files, "uv.lock");
    collect_toml_lock_sources(
        &uv_locks,
        &python_manifests,
        DependencyEcosystem::Python,
        parse_uv_lock,
        parse_python_direct_names,
        &mut DependencyCollection {
            nodes: &mut nodes,
            sources: &mut sources,
            truncated: &mut truncated,
        },
    );

    let gradle_files = discovery
        .files
        .iter()
        .filter(|file| is_gradle_input(&file.relative))
        .collect::<Vec<_>>();
    if let Some(first) = gradle_files.first() {
        let manifest_count = gradle_files
            .iter()
            .filter(|file| is_dependency_manifest(&file.relative))
            .count();
        sources.push(DependencySource {
            ecosystem: DependencyEcosystem::Gradle,
            path: first.relative.clone(),
            status: DependencySourceStatus::Unsupported,
            manifest_count,
            lockfile_count: gradle_files.len().saturating_sub(manifest_count),
            package_count: 0,
            direct_count: 0,
        });
    }

    for problem in discovery.problems {
        let manifest_problem = is_dependency_manifest(&problem.relative);
        sources.push(DependencySource {
            ecosystem: problem.ecosystem,
            path: problem.relative,
            status: DependencySourceStatus::Invalid,
            manifest_count: if manifest_problem { 1 } else { 0 },
            lockfile_count: if manifest_problem { 0 } else { 1 },
            package_count: 0,
            direct_count: 0,
        });
    }

    if nodes.len() > MAX_PACKAGES {
        nodes.truncate(MAX_PACKAGES);
        truncated = true;
    }
    let packages = resolve_graph(nodes, &mut unresolved_dependency_count, &mut truncated);
    let duplicates = duplicate_versions(&packages);
    sources.sort_by(|left, right| {
        (left.ecosystem, left.path.as_str()).cmp(&(right.ecosystem, right.path.as_str()))
    });

    let package_count = packages.len();
    let direct_count = packages.iter().filter(|package| package.direct).count();
    let missing_lockfile_count = count_status(&sources, DependencySourceStatus::MissingLockfile);
    let stale_lockfile_count = count_status(&sources, DependencySourceStatus::StaleLockfile);
    let unsupported_count = count_status(&sources, DependencySourceStatus::Unsupported);
    let invalid_count = count_status(&sources, DependencySourceStatus::Invalid);
    Ok(DependencyReport {
        revision,
        sources,
        packages,
        duplicates,
        package_count,
        direct_count,
        transitive_count: package_count.saturating_sub(direct_count),
        unresolved_dependency_count,
        missing_lockfile_count,
        stale_lockfile_count,
        unsupported_count,
        invalid_count,
        truncated,
        summary_published: false,
    })
}

fn count_status(sources: &[DependencySource], status: DependencySourceStatus) -> usize {
    sources
        .iter()
        .filter(|source| source.status == status)
        .count()
}

fn matching_files<'a>(files: &'a [InputFile], name: &str) -> Vec<&'a InputFile> {
    files.iter().filter(|file| file.name() == name).collect()
}

fn missing_source(ecosystem: DependencyEcosystem, manifest: &InputFile) -> DependencySource {
    DependencySource {
        ecosystem,
        path: manifest.relative.clone(),
        status: DependencySourceStatus::MissingLockfile,
        manifest_count: 1,
        lockfile_count: 0,
        package_count: 0,
        direct_count: 0,
    }
}

fn collect_toml_lock_sources(
    locks: &[&InputFile],
    manifests: &[&InputFile],
    ecosystem: DependencyEcosystem,
    parse_lock: fn(&str, &BTreeSet<String>) -> Result<ParseResult, ParseFailure>,
    parse_direct: fn(&str) -> Result<BTreeSet<String>, ParseFailure>,
    output: &mut DependencyCollection<'_>,
) {
    let mut assigned = HashSet::new();
    for lock in locks {
        let assigned_manifests = manifests
            .iter()
            .enumerate()
            .filter(|(_, manifest)| {
                nearest_lock(manifest, locks)
                    .is_some_and(|candidate| std::ptr::eq(*lock, candidate))
            })
            .map(|(index, manifest)| {
                assigned.insert(index);
                *manifest
            })
            .collect::<Vec<_>>();
        let mut direct = BTreeSet::new();
        let mut manifest_invalid = false;
        for manifest in &assigned_manifests {
            match manifest.text().and_then(parse_direct) {
                Ok(names) => direct.extend(names),
                Err(_) => manifest_invalid = true,
            }
        }
        let stale = assigned_manifests
            .iter()
            .any(|manifest| manifest.modified_ms > lock.modified_ms);
        let parsed = if manifest_invalid {
            Err(ParseFailure::Invalid)
        } else {
            lock.text().and_then(|text| parse_lock(text, &direct))
        };
        append_parsed_source(
            ecosystem,
            lock,
            assigned_manifests.len(),
            stale,
            parsed,
            output,
        );
    }
    for (index, manifest) in manifests.iter().enumerate() {
        if !assigned.contains(&index) {
            output.sources.push(missing_source(ecosystem, manifest));
        }
    }
}

fn collect_node_lock_sources(
    locks: &[&InputFile],
    manifests: &[&InputFile],
    ecosystem: DependencyEcosystem,
    parse_lock: fn(&str, &BTreeSet<String>) -> Result<ParseResult, ParseFailure>,
    assigned: &mut HashSet<usize>,
    output: &mut DependencyCollection<'_>,
) {
    let all_locks = locks;
    for lock in locks {
        let assigned_manifests = manifests
            .iter()
            .enumerate()
            .filter(|(_, manifest)| {
                nearest_lock(manifest, all_locks)
                    .is_some_and(|candidate| std::ptr::eq(*lock, candidate))
            })
            .map(|(index, manifest)| {
                assigned.insert(index);
                *manifest
            })
            .collect::<Vec<_>>();
        let mut direct = BTreeSet::new();
        let mut manifest_invalid = false;
        for manifest in &assigned_manifests {
            match manifest.text().and_then(parse_node_direct_names) {
                Ok(names) => direct.extend(names),
                Err(_) => manifest_invalid = true,
            }
        }
        let stale = assigned_manifests
            .iter()
            .any(|manifest| manifest.modified_ms > lock.modified_ms);
        let parsed = if manifest_invalid {
            Err(ParseFailure::Invalid)
        } else {
            lock.text().and_then(|text| parse_lock(text, &direct))
        };
        append_parsed_source(
            ecosystem,
            lock,
            assigned_manifests.len(),
            stale,
            parsed,
            output,
        );
    }
}

fn nearest_lock<'a>(manifest: &InputFile, locks: &[&'a InputFile]) -> Option<&'a InputFile> {
    locks
        .iter()
        .copied()
        .filter(|lock| manifest.parent().starts_with(lock.parent()))
        .max_by_key(|lock| lock.parent().components().count())
}

fn append_parsed_source(
    ecosystem: DependencyEcosystem,
    lock: &InputFile,
    manifest_count: usize,
    stale: bool,
    parsed: Result<ParseResult, ParseFailure>,
    output: &mut DependencyCollection<'_>,
) {
    match parsed {
        Ok(mut result) => {
            let available = MAX_PACKAGES.saturating_sub(output.nodes.len());
            if result.nodes.len() > available {
                result.nodes.truncate(available);
                *output.truncated = true;
            }
            let package_count = result.nodes.len();
            let direct_count = result.nodes.iter().filter(|node| node.direct).count();
            output.nodes.extend(result.nodes);
            output.sources.push(DependencySource {
                ecosystem,
                path: lock.relative.clone(),
                status: if stale {
                    DependencySourceStatus::StaleLockfile
                } else {
                    DependencySourceStatus::Ready
                },
                manifest_count,
                lockfile_count: 1,
                package_count,
                direct_count,
            });
        }
        Err(failure) => output.sources.push(DependencySource {
            ecosystem,
            path: lock.relative.clone(),
            status: match failure {
                ParseFailure::Invalid => DependencySourceStatus::Invalid,
                ParseFailure::Unsupported => DependencySourceStatus::Unsupported,
            },
            manifest_count,
            lockfile_count: 1,
            package_count: 0,
            direct_count: 0,
        }),
    }
}

fn discover_inputs(root: &Path, deadline: Instant) -> Result<InputDiscovery, String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    devbox_filesystem::ensure_no_links(root).map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let mut discovery = InputDiscovery::default();
    let mut visited = 0usize;
    let mut total_bytes = 0usize;
    walk_inputs(
        root,
        root,
        0,
        deadline,
        &mut visited,
        &mut total_bytes,
        &mut discovery,
    )?;
    discovery
        .files
        .sort_by(|left, right| left.relative.cmp(&right.relative));
    discovery
        .problems
        .sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(discovery)
}

#[allow(clippy::too_many_arguments)]
fn walk_inputs(
    root: &Path,
    directory: &Path,
    depth: usize,
    deadline: Instant,
    visited: &mut usize,
    total_bytes: &mut usize,
    discovery: &mut InputDiscovery,
) -> Result<(), String> {
    if Instant::now() >= deadline {
        discovery.truncated = true;
        return Ok(());
    }
    if *visited >= MAX_VISITED_DIRECTORIES || depth > MAX_SCAN_DEPTH {
        discovery.truncated = true;
        return Ok(());
    }
    *visited += 1;
    if depth > 0 && directory.join(".git").exists() {
        return Ok(());
    }
    devbox_filesystem::ensure_no_links(directory).map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let directory_identity = devbox_filesystem::filesystem_identity(directory, true)
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let read_dir = match std::fs::read_dir(directory) {
        Ok(read_dir) => read_dir,
        Err(_) if depth == 0 => return Err(DEPENDENCY_LENS_ERROR.into()),
        Err(_) => {
            discovery.truncated = true;
            return Ok(());
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        if Instant::now() >= deadline || entries.len() >= MAX_DIRECTORY_ENTRIES {
            discovery.truncated = true;
            break;
        }
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => discovery.truncated = true,
        }
    }
    if devbox_filesystem::filesystem_identity(directory, true)
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?
        != directory_identity
    {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if Instant::now() >= deadline {
            discovery.truncated = true;
            break;
        }
        let path = entry.path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                discovery.truncated = true;
                continue;
            }
        };
        if is_link_metadata(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                discovery.truncated = true;
                continue;
            };
            if name == ".git" || devbox_filesystem::is_ignored_dir(name) {
                continue;
            }
            walk_inputs(
                root,
                &path,
                depth + 1,
                deadline,
                visited,
                total_bytes,
                discovery,
            )?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = match safe_relative(root, &path) {
            Ok(relative) => relative,
            Err(_) => {
                discovery.truncated = true;
                continue;
            }
        };
        let Some(ecosystem) = recognized_input(&relative) else {
            continue;
        };
        if discovery
            .files
            .len()
            .saturating_add(discovery.problems.len())
            >= MAX_INPUT_FILES
        {
            discovery.truncated = true;
            break;
        }
        let file_bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        if file_bytes > MAX_FILE_BYTES
            || total_bytes.saturating_add(file_bytes) > MAX_TOTAL_INPUT_BYTES
        {
            discovery.problems.push(InputProblem {
                relative,
                ecosystem,
            });
            discovery.truncated = true;
            continue;
        }
        match read_input_file(&path, relative.clone(), deadline) {
            Ok(file) => {
                *total_bytes = total_bytes.saturating_add(file.bytes.len());
                discovery.files.push(file);
            }
            Err(_) => {
                discovery.problems.push(InputProblem {
                    relative,
                    ecosystem,
                });
                discovery.truncated = true;
            }
        }
    }
    Ok(())
}

fn read_input_file(path: &Path, relative: String, deadline: Instant) -> Result<InputFile, String> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    let initial_modified = metadata.modified().ok();
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(DEPENDENCY_LENS_ERROR.into());
        }
        let count = file
            .read(&mut chunk)
            .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_FILE_BYTES {
            return Err(DEPENDENCY_LENS_ERROR.into());
        }
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    if devbox_filesystem::filesystem_identity(path, false)
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?
        != identity
    {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    let final_metadata = file
        .metadata()
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    if final_metadata.len() != metadata.len()
        || final_metadata.len() != bytes.len() as u64
        || final_metadata.modified().ok() != initial_modified
    {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    Ok(InputFile {
        relative,
        bytes,
        modified_ms: modified_ms(&metadata),
    })
}

fn modified_ms(metadata: &Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn safe_relative(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
    let value = relative
        .to_str()
        .ok_or_else(|| DEPENDENCY_LENS_ERROR.to_string())?
        .replace('\\', "/");
    if value.is_empty()
        || value.len() > MAX_RELATIVE_PATH_BYTES
        || value.chars().any(char::is_control)
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(DEPENDENCY_LENS_ERROR.into());
    }
    Ok(value)
}

fn is_link_metadata(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn recognized_input(relative: &str) -> Option<DependencyEcosystem> {
    let name = Path::new(relative).file_name()?.to_str()?;
    match name {
        "Cargo.toml" | "Cargo.lock" => Some(DependencyEcosystem::Cargo),
        "package.json" => Some(DependencyEcosystem::Npm),
        "pnpm-lock.yaml" => Some(DependencyEcosystem::Pnpm),
        "package-lock.json" => Some(DependencyEcosystem::Npm),
        "pyproject.toml" | "uv.lock" => Some(DependencyEcosystem::Python),
        "build.gradle"
        | "build.gradle.kts"
        | "settings.gradle"
        | "settings.gradle.kts"
        | "gradle.lockfile" => Some(DependencyEcosystem::Gradle),
        "libs.versions.toml"
            if Path::new(relative).parent().is_some_and(|parent| {
                parent.components().any(|part| part.as_os_str() == "gradle")
            }) =>
        {
            Some(DependencyEcosystem::Gradle)
        }
        _ => None,
    }
}

fn is_gradle_input(relative: &str) -> bool {
    recognized_input(relative) == Some(DependencyEcosystem::Gradle)
}

fn is_dependency_manifest(relative: &str) -> bool {
    matches!(
        Path::new(relative)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(
            "Cargo.toml"
                | "package.json"
                | "pyproject.toml"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "libs.versions.toml"
        )
    )
}

fn input_revision(files: &[InputFile], problems: &[InputProblem]) -> String {
    let mut digest = Sha256::new();
    for file in files {
        digest.update((file.relative.len() as u64).to_le_bytes());
        digest.update(file.relative.as_bytes());
        digest.update((file.bytes.len() as u64).to_le_bytes());
        digest.update(&file.bytes);
    }
    for problem in problems {
        digest.update(b"invalid-input\0");
        digest.update(problem.ecosystem.key().as_bytes());
        digest.update((problem.relative.len() as u64).to_le_bytes());
        digest.update(problem.relative.as_bytes());
    }
    format!("sha256:{}", encode_lower_hex(&digest.finalize()))
}

fn encode_lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn parse_cargo_lock(
    text: &str,
    direct_names: &BTreeSet<String>,
) -> Result<ParseResult, ParseFailure> {
    let document: toml::Value = toml::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    let version = document
        .get("version")
        .and_then(toml::Value::as_integer)
        .ok_or(ParseFailure::Invalid)?;
    if !matches!(version, 3 | 4) {
        return Err(ParseFailure::Unsupported);
    }
    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or(ParseFailure::Invalid)?;
    if packages.len() > MAX_PACKAGES {
        return Err(ParseFailure::Unsupported);
    }
    let mut nodes = Vec::with_capacity(packages.len());
    for package in packages {
        let table = package.as_table().ok_or(ParseFailure::Invalid)?;
        let name = checked_package_name(
            table
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or(ParseFailure::Invalid)?,
        )?;
        let version = checked_version_text(
            table
                .get("version")
                .and_then(toml::Value::as_str)
                .ok_or(ParseFailure::Invalid)?,
        )?;
        let dependencies = table
            .get("dependencies")
            .map(parse_cargo_dependency_array)
            .transpose()?
            .unwrap_or_default();
        nodes.push(ParsedNode {
            ecosystem: DependencyEcosystem::Cargo,
            direct: direct_names.contains(&name),
            name,
            version,
            dependencies,
        });
    }
    Ok(ParseResult { nodes })
}

fn parse_cargo_dependency_array(
    value: &toml::Value,
) -> Result<Vec<DependencyReference>, ParseFailure> {
    let values = value.as_array().ok_or(ParseFailure::Invalid)?;
    let mut dependencies = Vec::new();
    for value in values {
        let raw = value.as_str().ok_or(ParseFailure::Invalid)?;
        let mut parts = raw.split_whitespace();
        let name = checked_package_name(parts.next().ok_or(ParseFailure::Invalid)?)?;
        let version = parts
            .next()
            .filter(|part| part.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            .map(checked_version_text)
            .transpose()?;
        dependencies.push(DependencyReference { name, version });
    }
    Ok(dependencies)
}

fn parse_cargo_direct_names(text: &str) -> Result<BTreeSet<String>, ParseFailure> {
    let document: toml::Value = toml::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    let table = document.as_table().ok_or(ParseFailure::Invalid)?;
    let mut direct = BTreeSet::new();
    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
        collect_toml_dependency_table(table.get(key), &mut direct)?;
    }
    if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                collect_toml_dependency_table(target.get(key), &mut direct)?;
            }
        }
    }
    Ok(direct)
}

fn collect_toml_dependency_table(
    value: Option<&toml::Value>,
    direct: &mut BTreeSet<String>,
) -> Result<(), ParseFailure> {
    let Some(value) = value else {
        return Ok(());
    };
    let table = value.as_table().ok_or(ParseFailure::Invalid)?;
    for (key, value) in table {
        let actual = value
            .as_table()
            .and_then(|entry| entry.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(key);
        direct.insert(checked_package_name(actual)?);
    }
    Ok(())
}

fn parse_uv_lock(text: &str, direct_names: &BTreeSet<String>) -> Result<ParseResult, ParseFailure> {
    let document: toml::Value = toml::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    if document.get("version").and_then(toml::Value::as_integer) != Some(1) {
        return Err(ParseFailure::Unsupported);
    }
    let packages = document
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or(ParseFailure::Invalid)?;
    if packages.len() > MAX_PACKAGES {
        return Err(ParseFailure::Unsupported);
    }
    let mut nodes = Vec::with_capacity(packages.len());
    for package in packages {
        let table = package.as_table().ok_or(ParseFailure::Invalid)?;
        let name = normalize_python_name(
            table
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or(ParseFailure::Invalid)?,
        )?;
        let version = checked_version_text(
            table
                .get("version")
                .and_then(toml::Value::as_str)
                .ok_or(ParseFailure::Invalid)?,
        )?;
        let mut dependencies = Vec::new();
        if let Some(values) = table.get("dependencies") {
            for value in values.as_array().ok_or(ParseFailure::Invalid)? {
                let entry = value.as_table().ok_or(ParseFailure::Invalid)?;
                let dependency_name = normalize_python_name(
                    entry
                        .get("name")
                        .and_then(toml::Value::as_str)
                        .ok_or(ParseFailure::Invalid)?,
                )?;
                let dependency_version = entry
                    .get("version")
                    .and_then(toml::Value::as_str)
                    .map(checked_version_text)
                    .transpose()?;
                dependencies.push(DependencyReference {
                    name: dependency_name,
                    version: dependency_version,
                });
            }
        }
        nodes.push(ParsedNode {
            ecosystem: DependencyEcosystem::Python,
            direct: direct_names.contains(&name),
            name,
            version,
            dependencies,
        });
    }
    Ok(ParseResult { nodes })
}

fn parse_python_direct_names(text: &str) -> Result<BTreeSet<String>, ParseFailure> {
    let document: toml::Value = toml::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    let mut direct = BTreeSet::new();
    if let Some(project) = document.get("project").and_then(toml::Value::as_table) {
        collect_python_requirements(project.get("dependencies"), &mut direct)?;
        if let Some(groups) = project
            .get("optional-dependencies")
            .and_then(toml::Value::as_table)
        {
            for requirements in groups.values() {
                collect_python_requirements(Some(requirements), &mut direct)?;
            }
        }
    }
    if let Some(groups) = document
        .get("dependency-groups")
        .and_then(toml::Value::as_table)
    {
        for requirements in groups.values() {
            collect_python_requirements(Some(requirements), &mut direct)?;
        }
    }
    if let Some(uv) = document
        .get("tool")
        .and_then(toml::Value::as_table)
        .and_then(|tool| tool.get("uv"))
        .and_then(toml::Value::as_table)
    {
        collect_python_requirements(uv.get("dev-dependencies"), &mut direct)?;
    }
    Ok(direct)
}

fn collect_python_requirements(
    value: Option<&toml::Value>,
    direct: &mut BTreeSet<String>,
) -> Result<(), ParseFailure> {
    let Some(value) = value else {
        return Ok(());
    };
    for requirement in value.as_array().ok_or(ParseFailure::Invalid)? {
        if let Some(raw) = requirement.as_str() {
            direct.insert(parse_python_requirement_name(raw)?);
        } else if let Some(table) = requirement.as_table() {
            let name = table
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or(ParseFailure::Invalid)?;
            direct.insert(normalize_python_name(name)?);
        } else {
            return Err(ParseFailure::Invalid);
        }
    }
    Ok(())
}

fn parse_python_requirement_name(raw: &str) -> Result<String, ParseFailure> {
    let trimmed = raw.trim();
    let end = trimmed
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '_' | '.'))
        .map(|(index, _)| index)
        .unwrap_or(trimmed.len());
    normalize_python_name(&trimmed[..end])
}

fn normalize_python_name(value: &str) -> Result<String, ParseFailure> {
    let value = checked_package_name(value)?;
    let mut normalized = String::with_capacity(value.len());
    let mut separator = false;
    for ch in value.chars() {
        if matches!(ch, '-' | '_' | '.') {
            if !separator {
                normalized.push('-');
                separator = true;
            }
        } else {
            normalized.extend(ch.to_lowercase());
            separator = false;
        }
    }
    checked_package_name(&normalized)
}

fn parse_package_lock(
    text: &str,
    direct_names: &BTreeSet<String>,
) -> Result<ParseResult, ParseFailure> {
    let document: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    let version = document
        .get("lockfileVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ParseFailure::Invalid)?;
    if !matches!(version, 1..=3) {
        return Err(ParseFailure::Unsupported);
    }
    if let Some(packages) = document
        .get("packages")
        .and_then(serde_json::Value::as_object)
    {
        if packages.len() > MAX_PACKAGES + 1 {
            return Err(ParseFailure::Unsupported);
        }
        let root = packages.get("").and_then(serde_json::Value::as_object);
        let mut direct = direct_names.clone();
        if let Some(root) = root {
            direct.extend(json_dependency_names(root)?);
        }
        let mut nodes = Vec::new();
        for (location, value) in packages {
            if location.is_empty() {
                continue;
            }
            let entry = value.as_object().ok_or(ParseFailure::Invalid)?;
            let Some(version) = entry.get("version").and_then(serde_json::Value::as_str) else {
                continue; // local link/workspace entry
            };
            let name = entry
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(checked_package_name)
                .transpose()?
                .or_else(|| npm_name_from_location(location))
                .ok_or(ParseFailure::Invalid)?;
            let version = checked_version_text(version)?;
            let dependencies = json_dependency_names(entry).map(|names| {
                names
                    .into_iter()
                    .map(|name| DependencyReference {
                        name,
                        version: None,
                    })
                    .collect()
            })?;
            nodes.push(ParsedNode {
                ecosystem: DependencyEcosystem::Npm,
                direct: direct.contains(&name) && npm_is_root_location(location, &name),
                name,
                version,
                dependencies,
            });
        }
        return Ok(ParseResult { nodes });
    }

    let dependencies = document
        .get("dependencies")
        .and_then(serde_json::Value::as_object)
        .ok_or(ParseFailure::Invalid)?;
    let mut nodes = Vec::new();
    parse_package_lock_v1_dependencies(dependencies, true, 0, &mut nodes)?;
    Ok(ParseResult { nodes })
}

fn parse_package_lock_v1_dependencies(
    dependencies: &serde_json::Map<String, serde_json::Value>,
    direct: bool,
    depth: usize,
    nodes: &mut Vec<ParsedNode>,
) -> Result<(), ParseFailure> {
    if depth > 64 || nodes.len().saturating_add(dependencies.len()) > MAX_PACKAGES {
        return Err(ParseFailure::Unsupported);
    }
    for (name, value) in dependencies {
        let entry = value.as_object().ok_or(ParseFailure::Invalid)?;
        let name = checked_package_name(name)?;
        let version = checked_version_text(
            entry
                .get("version")
                .and_then(serde_json::Value::as_str)
                .ok_or(ParseFailure::Invalid)?,
        )?;
        let child_map = entry
            .get("dependencies")
            .and_then(serde_json::Value::as_object);
        let refs = child_map
            .map(|children| {
                children
                    .iter()
                    .map(|(child_name, child)| {
                        let child_version = child
                            .get("version")
                            .and_then(serde_json::Value::as_str)
                            .map(checked_version_text)
                            .transpose()?;
                        Ok(DependencyReference {
                            name: checked_package_name(child_name)?,
                            version: child_version,
                        })
                    })
                    .collect::<Result<Vec<_>, ParseFailure>>()
            })
            .transpose()?
            .unwrap_or_default();
        nodes.push(ParsedNode {
            ecosystem: DependencyEcosystem::Npm,
            name,
            version,
            direct,
            dependencies: refs,
        });
        if let Some(children) = child_map {
            parse_package_lock_v1_dependencies(children, false, depth + 1, nodes)?;
        }
    }
    Ok(())
}

fn json_dependency_names(
    entry: &serde_json::Map<String, serde_json::Value>,
) -> Result<BTreeSet<String>, ParseFailure> {
    let mut names = BTreeSet::new();
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(values) = entry.get(key) else {
            continue;
        };
        for name in values.as_object().ok_or(ParseFailure::Invalid)?.keys() {
            names.insert(checked_package_name(name)?);
        }
    }
    Ok(names)
}

fn npm_name_from_location(location: &str) -> Option<String> {
    let (_, tail) = location.rsplit_once("node_modules/")?;
    checked_package_name(tail).ok()
}

fn npm_is_root_location(location: &str, name: &str) -> bool {
    location == format!("node_modules/{name}")
}

fn parse_node_direct_names(text: &str) -> Result<BTreeSet<String>, ParseFailure> {
    Ok(parse_node_manifest(text)?.direct_names)
}

fn parse_node_manifest(text: &str) -> Result<NodeManifest, ParseFailure> {
    let document: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ParseFailure::Invalid)?;
    let object = document.as_object().ok_or(ParseFailure::Invalid)?;
    let ecosystem = match object.get("packageManager") {
        Some(value) => {
            let package_manager = value.as_str().ok_or(ParseFailure::Invalid)?;
            let package_manager = checked_bounded_text(package_manager, MAX_PACKAGE_MANAGER_BYTES)?;
            if package_manager.starts_with("pnpm@") {
                DependencyEcosystem::Pnpm
            } else {
                DependencyEcosystem::Npm
            }
        }
        None => DependencyEcosystem::Npm,
    };
    Ok(NodeManifest {
        direct_names: json_dependency_names(object)?,
        ecosystem,
    })
}

fn parse_pnpm_lock(
    text: &str,
    manifest_direct_names: &BTreeSet<String>,
) -> Result<ParseResult, ParseFailure> {
    if text.lines().any(|line| line.len() > MAX_LINE_BYTES) {
        return Err(ParseFailure::Invalid);
    }
    let version_line = text
        .lines()
        .find(|line| line.starts_with("lockfileVersion:"))
        .ok_or(ParseFailure::Invalid)?;
    let version = yaml_scalar(
        version_line
            .split_once(':')
            .map(|(_, value)| value)
            .ok_or(ParseFailure::Invalid)?,
    );
    let major = version
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or(ParseFailure::Invalid)?;
    if !(5..=9).contains(&major) {
        return Err(ParseFailure::Unsupported);
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Section {
        None,
        Importers,
        Packages,
        Snapshots,
        LegacyDirect,
    }

    let mut section = Section::None;
    let mut current_package: Option<(String, String)> = None;
    let mut dependency_indent: Option<usize> = None;
    let mut direct_names = manifest_direct_names.clone();
    let mut direct_versions = BTreeMap::<String, BTreeSet<String>>::new();
    let mut package_nodes = BTreeMap::<(String, String), Vec<DependencyReference>>::new();
    let mut pending_direct: Option<(usize, String)> = None;

    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 0 {
            section = match trimmed {
                "importers:" => Section::Importers,
                "packages:" => Section::Packages,
                "snapshots:" => Section::Snapshots,
                "dependencies:"
                | "devDependencies:"
                | "optionalDependencies:"
                | "peerDependencies:" => Section::LegacyDirect,
                _ => Section::None,
            };
            current_package = None;
            dependency_indent = None;
            pending_direct = None;
            continue;
        }

        match section {
            Section::Packages if indent == 2 => {
                let key = yaml_mapping_key(trimmed)
                    .or_else(|| yaml_mapping_pair(trimmed).map(|pair| pair.0))
                    .ok_or(ParseFailure::Invalid)?;
                if let Some((name, version)) = parse_pnpm_package_key(&key)? {
                    package_nodes
                        .entry((name.clone(), version.clone()))
                        .or_default();
                    current_package = Some((name, version));
                } else {
                    current_package = None;
                }
                dependency_indent = None;
            }
            Section::Packages if indent == 4 && is_resolved_dependency_group(trimmed) => {
                dependency_indent = Some(indent);
            }
            Section::Packages
                if dependency_indent.is_some_and(|group_indent| indent == group_indent + 2) =>
            {
                let (name, value) = yaml_mapping_pair(trimmed).ok_or(ParseFailure::Invalid)?;
                if let Some(package) = current_package.as_ref() {
                    let dependency = DependencyReference {
                        name: checked_package_name(&name)?,
                        version: pnpm_reference_version(&value)?,
                    };
                    package_nodes
                        .entry(package.clone())
                        .or_default()
                        .push(dependency);
                }
            }
            Section::Packages if indent <= 4 => {
                dependency_indent = None;
            }
            Section::Snapshots if indent == 2 => {
                let key = yaml_mapping_key(trimmed)
                    .or_else(|| yaml_mapping_pair(trimmed).map(|pair| pair.0))
                    .ok_or(ParseFailure::Invalid)?;
                current_package = parse_pnpm_package_key(&key)?;
                dependency_indent = None;
            }
            Section::Snapshots if indent == 4 && is_resolved_dependency_group(trimmed) => {
                dependency_indent = Some(indent);
            }
            Section::Snapshots
                if dependency_indent.is_some_and(|group_indent| indent == group_indent + 2) =>
            {
                let (name, value) = yaml_mapping_pair(trimmed).ok_or(ParseFailure::Invalid)?;
                if let Some(package) = current_package.as_ref() {
                    let dependency = DependencyReference {
                        name: checked_package_name(&name)?,
                        version: pnpm_reference_version(&value)?,
                    };
                    package_nodes
                        .entry(package.clone())
                        .or_default()
                        .push(dependency);
                }
            }
            Section::Snapshots if indent <= 4 => {
                dependency_indent = None;
            }
            Section::Importers if indent == 4 && is_dependency_group(trimmed) => {
                pending_direct = None;
                dependency_indent = Some(indent);
            }
            Section::Importers
                if dependency_indent.is_some_and(|group_indent| indent == group_indent + 2) =>
            {
                let pair = yaml_mapping_pair(trimmed);
                let key = yaml_mapping_key(trimmed)
                    .or_else(|| pair.as_ref().map(|pair| pair.0.clone()))
                    .ok_or(ParseFailure::Invalid)?;
                let name = checked_package_name(&key)?;
                direct_names.insert(name.clone());
                if let Some((_, value)) = pair {
                    record_pnpm_direct_version(&mut direct_versions, &name, &value);
                }
                pending_direct = Some((indent, name));
            }
            Section::Importers
                if pending_direct
                    .as_ref()
                    .is_some_and(|(package_indent, _)| indent == package_indent + 2) =>
            {
                if let Some((key, value)) = yaml_mapping_pair(trimmed) {
                    if key == "version" {
                        if let Some((_, name)) = pending_direct.as_ref() {
                            record_pnpm_direct_version(&mut direct_versions, name, &value);
                        }
                    }
                }
            }
            Section::Importers if indent <= 4 => {
                dependency_indent = None;
                pending_direct = None;
            }
            Section::Importers
                if pending_direct
                    .as_ref()
                    .is_some_and(|(package_indent, _)| indent <= *package_indent) =>
            {
                pending_direct = None;
            }
            Section::LegacyDirect if indent == 2 => {
                let pair = yaml_mapping_pair(trimmed);
                let key = yaml_mapping_key(trimmed)
                    .or_else(|| pair.as_ref().map(|pair| pair.0.clone()))
                    .ok_or(ParseFailure::Invalid)?;
                let name = checked_package_name(&key)?;
                direct_names.insert(name.clone());
                if let Some((_, value)) = pair {
                    record_pnpm_direct_version(&mut direct_versions, &name, &value);
                }
            }
            _ => {}
        }
    }

    if package_nodes.len() > MAX_PACKAGES {
        return Err(ParseFailure::Unsupported);
    }
    let nodes = package_nodes
        .into_iter()
        .map(|((name, version), dependencies)| ParsedNode {
            ecosystem: DependencyEcosystem::Pnpm,
            direct: direct_versions.get(&name).map_or_else(
                || direct_names.contains(&name),
                |versions| versions.contains(&version),
            ),
            name,
            version,
            dependencies,
        })
        .collect();
    Ok(ParseResult { nodes })
}

fn record_pnpm_direct_version(
    direct_versions: &mut BTreeMap<String, BTreeSet<String>>,
    name: &str,
    value: &str,
) {
    if let Ok(Some(version)) = pnpm_reference_version(value) {
        direct_versions
            .entry(name.to_string())
            .or_default()
            .insert(version);
    }
}

fn is_dependency_group(value: &str) -> bool {
    matches!(
        value,
        "dependencies:" | "devDependencies:" | "optionalDependencies:" | "peerDependencies:"
    )
}

fn is_resolved_dependency_group(value: &str) -> bool {
    matches!(value, "dependencies:" | "optionalDependencies:")
}

fn yaml_mapping_key(value: &str) -> Option<String> {
    value.strip_suffix(':').map(yaml_scalar)
}

fn yaml_mapping_pair(value: &str) -> Option<(String, String)> {
    let mut quote = None;
    for (index, ch) in value.char_indices() {
        match (quote, ch) {
            (None, '\'' | '"') => quote = Some(ch),
            (Some(expected), actual) if expected == actual => quote = None,
            (None, ':')
                if value[index + ch.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace) =>
            {
                let key = yaml_scalar(&value[..index]);
                let scalar = yaml_scalar(&value[index + 1..]);
                if key.is_empty() {
                    return None;
                }
                return Some((key, scalar));
            }
            _ => {}
        }
    }
    None
}

fn yaml_scalar(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('\'') && value.ends_with('\''))
            || (value.starts_with('"') && value.ends_with('"')))
    {
        value[1..value.len() - 1].replace("''", "'")
    } else {
        value.to_string()
    }
}

fn parse_pnpm_package_key(value: &str) -> Result<Option<(String, String)>, ParseFailure> {
    let legacy = value.starts_with('/');
    let value = value.trim_start_matches('/');
    if value.starts_with("file:") || value.starts_with("link:") {
        return Ok(None);
    }
    let without_peers = value.split('(').next().unwrap_or(value);
    let last_slash = without_peers.rfind('/');
    let last_at = without_peers.rfind('@');
    let at_pair = without_peers.rsplit_once('@');
    let protocol_at_pair = at_pair.is_some_and(|(_, version)| {
        ["file:", "link:", "workspace:"]
            .iter()
            .any(|prefix| version.starts_with(prefix))
    });
    let legacy_slash_format = legacy
        && last_slash.is_some()
        && !protocol_at_pair
        && (!without_peers.starts_with('@') || last_at < last_slash);
    let pair = if legacy_slash_format {
        without_peers.rsplit_once('/')
    } else {
        at_pair
    };
    let Some((name, raw_version)) = pair else {
        return Err(ParseFailure::Unsupported);
    };
    let version = if legacy_slash_format {
        raw_version.split('_').next().unwrap_or(raw_version)
    } else {
        raw_version
    };
    if name.is_empty() || version.is_empty() {
        return Err(ParseFailure::Invalid);
    }
    if ["file:", "link:", "workspace:"]
        .iter()
        .any(|prefix| version.starts_with(prefix))
    {
        return Ok(None);
    }
    Ok(Some((
        checked_package_name(name)?,
        checked_version_text(version)?,
    )))
}

fn pnpm_reference_version(value: &str) -> Result<Option<String>, ParseFailure> {
    let value = value.split('(').next().unwrap_or(value).trim();
    if value.is_empty()
        || value.starts_with("link:")
        || value.starts_with("workspace:")
        || value.starts_with("file:")
    {
        return Ok(None);
    }
    let value = value.split('_').next().unwrap_or(value);
    Ok(Some(checked_version_text(value)?))
}

fn checked_bounded_text(value: &str, max_bytes: usize) -> Result<&str, ParseFailure> {
    if value.is_empty()
        || value.len() > max_bytes
        || value != value.trim()
        || value.chars().any(char::is_control)
    {
        return Err(ParseFailure::Invalid);
    }
    Ok(value)
}

fn checked_package_name(value: &str) -> Result<String, ParseFailure> {
    let value = checked_bounded_text(value, MAX_PACKAGE_NAME_BYTES)?;
    if !value.is_ascii() {
        return Err(ParseFailure::Invalid);
    }
    let component_valid = |component: &str| {
        !component.is_empty()
            && !matches!(component, "." | "..")
            && component.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
            })
    };
    let valid = if let Some(scoped) = value.strip_prefix('@') {
        scoped.split_once('/').is_some_and(|(scope, package)| {
            component_valid(scope)
                && component_valid(package)
                && !package.contains('/')
                && !package.contains('@')
        })
    } else {
        component_valid(value) && !value.contains('/') && !value.contains('@')
    };
    if !valid {
        return Err(ParseFailure::Invalid);
    }
    Ok(value.to_string())
}

fn checked_version_text(value: &str) -> Result<String, ParseFailure> {
    let value = checked_bounded_text(value, MAX_VERSION_BYTES)?;
    if !value.is_ascii()
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+' | b'!')
        })
    {
        return Err(ParseFailure::Invalid);
    }
    Ok(value.to_string())
}

/// Reuse the lockfile parser's package-coordinate policy at the remote
/// enrichment boundary without exposing the parser's internal error type.
pub(super) fn validated_package_name(value: &str) -> Option<String> {
    checked_package_name(value).ok()
}

/// Remote metadata may suggest another version. It must satisfy the same
/// bounded, URI-rejecting policy as a version accepted from a local lockfile.
pub(super) fn validated_version_text(value: &str) -> Option<String> {
    checked_version_text(value).ok()
}

fn node_id(ecosystem: DependencyEcosystem, name: &str, version: &str) -> String {
    format!("{}:{name}@{version}", ecosystem.key())
}

fn resolve_graph(
    nodes: Vec<ParsedNode>,
    unresolved: &mut usize,
    truncated: &mut bool,
) -> Vec<DependencyPackage> {
    let mut merged = BTreeMap::<(DependencyEcosystem, String, String), ParsedNode>::new();
    for node in nodes {
        let key = (node.ecosystem, node.name.clone(), node.version.clone());
        merged
            .entry(key)
            .and_modify(|existing| {
                existing.direct |= node.direct;
                existing.dependencies.extend(node.dependencies.clone());
                existing.dependencies.sort();
                existing.dependencies.dedup();
            })
            .or_insert(node);
    }
    let by_name = merged.keys().fold(
        HashMap::<(DependencyEcosystem, String), Vec<String>>::new(),
        |mut map, key| {
            map.entry((key.0, key.1.clone()))
                .or_default()
                .push(key.2.clone());
            map
        },
    );
    let mut examined_edge_count = 0usize;
    let mut packages = Vec::with_capacity(merged.len());
    for ((ecosystem, name, version), node) in merged {
        let mut dependencies = BTreeSet::new();
        let mut references = node.dependencies;
        references.sort();
        references.dedup();
        for reference in references {
            if examined_edge_count >= MAX_EDGES {
                *truncated = true;
                break;
            }
            examined_edge_count += 1;
            let candidates = by_name.get(&(ecosystem, reference.name.clone()));
            let resolved = match reference.version {
                Some(version)
                    if candidates.is_some_and(|values| {
                        values.iter().any(|candidate| candidate == &version)
                    }) =>
                {
                    Some(version)
                }
                Some(_) => None,
                None if candidates.is_some_and(|values| values.len() == 1) => {
                    candidates.and_then(|values| values.first()).cloned()
                }
                None => None,
            };
            if let Some(target_version) = resolved {
                dependencies.insert(node_id(ecosystem, &reference.name, &target_version));
            } else {
                *unresolved = unresolved.saturating_add(1);
            }
        }
        packages.push(DependencyPackage {
            id: node_id(ecosystem, &name, &version),
            ecosystem,
            name,
            version,
            direct: node.direct,
            dependencies: dependencies.into_iter().collect(),
        });
    }
    packages
}

fn duplicate_versions(packages: &[DependencyPackage]) -> Vec<DuplicateDependency> {
    let mut grouped = BTreeMap::<(DependencyEcosystem, String), BTreeSet<String>>::new();
    for package in packages {
        grouped
            .entry((package.ecosystem, package.name.clone()))
            .or_default()
            .insert(package.version.clone());
    }
    grouped
        .into_iter()
        .filter_map(|((ecosystem, name), versions)| {
            (versions.len() > 1).then(|| DuplicateDependency {
                ecosystem,
                name,
                versions: versions.into_iter().collect(),
            })
        })
        .collect()
}

pub fn dependency_summary_entry(
    canonical_project_key: &str,
    report: &DependencyReport,
    scanned_at_ms: u64,
) -> Result<DependencySummaryEntry, String> {
    let project_id = devbox_integration::opaque_identity("project", canonical_project_key)?;
    let mut ecosystems = Vec::new();
    for ecosystem in [
        DependencyEcosystem::Cargo,
        DependencyEcosystem::Pnpm,
        DependencyEcosystem::Npm,
        DependencyEcosystem::Python,
        DependencyEcosystem::Gradle,
    ] {
        let package_count = report
            .packages
            .iter()
            .filter(|package| package.ecosystem == ecosystem)
            .count();
        let direct_count = report
            .packages
            .iter()
            .filter(|package| package.ecosystem == ecosystem && package.direct)
            .count();
        let duplicate_count = report
            .duplicates
            .iter()
            .filter(|duplicate| duplicate.ecosystem == ecosystem)
            .count();
        let detected = report
            .sources
            .iter()
            .any(|source| source.ecosystem == ecosystem);
        if package_count > 0 || detected {
            ecosystems.push(DependencySummaryEcosystem {
                ecosystem: ecosystem.key().into(),
                package_count,
                direct_count,
                duplicate_count,
            });
        }
    }
    let entry = DependencySummaryEntry {
        project_id,
        revision: report.revision.clone(),
        scanned_at_ms,
        package_count: report.package_count,
        direct_count: report.direct_count,
        transitive_count: report.transitive_count,
        duplicate_count: report.duplicates.len(),
        unresolved_dependency_count: report.unresolved_dependency_count,
        missing_lockfile_count: report.missing_lockfile_count,
        stale_lockfile_count: report.stale_lockfile_count,
        unsupported_count: report.unsupported_count,
        invalid_count: report.invalid_count,
        truncated: report.truncated,
        ecosystems,
    };
    validate_summary_entry(&entry, scanned_at_ms)?;
    Ok(entry)
}

pub fn publish_summary_in(
    integration_root: &Path,
    entry: DependencySummaryEntry,
    now_ms: u64,
) -> Result<(), String> {
    validate_summary_entry(&entry, now_ms)?;
    let mut views = match devbox_integration::read_snapshot_in(
        integration_root,
        DEPENDENCY_SUMMARY_PRODUCER,
        DEPENDENCY_SUMMARY_VERSION,
    )? {
        Some(envelope) => envelope.views()?,
        None => devbox_integration::SnapshotViews::new(),
    };
    let mut entries = match views.remove(DEPENDENCY_SUMMARY_VIEW) {
        Some(view) => {
            if view.schema_version != DEPENDENCY_SUMMARY_VERSION {
                return Err("dependency summary schema를 지원하지 않습니다".into());
            }
            let entries = view
                .entries
                .into_iter()
                .map(|value| {
                    let entry: DependencySummaryEntry = serde_json::from_value(value)
                        .map_err(|_| "dependency summary 형식이 올바르지 않습니다".to_string())?;
                    validate_summary_entry(&entry, now_ms)?;
                    Ok(entry)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let mut project_ids = HashSet::new();
            if entries
                .iter()
                .any(|candidate| !project_ids.insert(candidate.project_id.as_str()))
            {
                return Err("dependency summary 형식이 올바르지 않습니다".into());
            }
            entries
        }
        None => Vec::new(),
    };
    entries.retain(|existing| {
        existing.project_id != entry.project_id
            && now_ms.saturating_sub(existing.scanned_at_ms) <= MAX_SUMMARY_AGE_MS
    });
    entries.push(entry);
    entries.sort_by(|left, right| left.project_id.cmp(&right.project_id));
    if entries.len() > MAX_SUMMARY_ENTRIES {
        entries.sort_by_key(|candidate| candidate.scanned_at_ms);
        entries.drain(0..entries.len() - MAX_SUMMARY_ENTRIES);
        entries.sort_by(|left, right| left.project_id.cmp(&right.project_id));
    }
    let json_entries = entries
        .into_iter()
        .map(|entry| {
            serde_json::to_value(entry)
                .map_err(|_| "dependency summary를 직렬화할 수 없습니다".to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    views.insert(
        DEPENDENCY_SUMMARY_VIEW.into(),
        devbox_integration::SnapshotView {
            schema_version: DEPENDENCY_SUMMARY_VERSION,
            freshness_ms: 0,
            entries: json_entries,
        },
    );
    let envelope = devbox_integration::Envelope::with_views(
        DEPENDENCY_SUMMARY_PRODUCER,
        env!("CARGO_PKG_VERSION"),
        views,
    );
    devbox_integration::write_atomic(
        &envelope,
        &devbox_integration::snapshot_dir_in(
            integration_root,
            DEPENDENCY_SUMMARY_PRODUCER,
            DEPENDENCY_SUMMARY_VERSION,
        ),
    )
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn validate_summary_entry(entry: &DependencySummaryEntry, now_ms: u64) -> Result<(), String> {
    if entry.project_id.len() != "project-".len() + 64
        || !entry.project_id.starts_with("project-")
        || !entry.project_id["project-".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || entry.revision.len() != "sha256:".len() + 64
        || !entry.revision.starts_with("sha256:")
        || !entry.revision["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || entry.scanned_at_ms > now_ms.saturating_add(MAX_CLOCK_SKEW_MS)
        || entry.package_count > MAX_PACKAGES
        || entry.direct_count > entry.package_count
        || entry.transitive_count != entry.package_count.saturating_sub(entry.direct_count)
        || entry.duplicate_count > entry.package_count
        || entry.unresolved_dependency_count > MAX_EDGES
        || entry.missing_lockfile_count > MAX_INPUT_FILES
        || entry.stale_lockfile_count > MAX_INPUT_FILES
        || entry.unsupported_count > MAX_INPUT_FILES
        || entry.invalid_count > MAX_INPUT_FILES
        || entry.ecosystems.len() > 5
    {
        return Err("dependency summary 형식이 올바르지 않습니다".into());
    }
    let mut seen = HashSet::new();
    let mut package_total = 0usize;
    let mut direct_total = 0usize;
    let mut duplicate_total = 0usize;
    for ecosystem in &entry.ecosystems {
        if !matches!(
            ecosystem.ecosystem.as_str(),
            "cargo" | "pnpm" | "npm" | "python" | "gradle"
        ) || !seen.insert(ecosystem.ecosystem.as_str())
            || ecosystem.package_count > entry.package_count
            || ecosystem.direct_count > ecosystem.package_count
            || ecosystem.duplicate_count > ecosystem.package_count
        {
            return Err("dependency summary 형식이 올바르지 않습니다".into());
        }
        package_total = package_total
            .checked_add(ecosystem.package_count)
            .ok_or_else(|| "dependency summary 형식이 올바르지 않습니다".to_string())?;
        direct_total = direct_total
            .checked_add(ecosystem.direct_count)
            .ok_or_else(|| "dependency summary 형식이 올바르지 않습니다".to_string())?;
        duplicate_total = duplicate_total
            .checked_add(ecosystem.duplicate_count)
            .ok_or_else(|| "dependency summary 형식이 올바르지 않습니다".to_string())?;
    }
    if package_total != entry.package_count
        || direct_total != entry.direct_count
        || duplicate_total != entry.duplicate_count
    {
        return Err("dependency summary 형식이 올바르지 않습니다".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parses_cargo_lock_and_marks_manifest_dependencies_direct() {
        let direct = parse_cargo_direct_names(
            r#"[package]
name = "demo"
version = "0.1.0"

[dependencies]
serde = "1"
renamed = { package = "actual", version = "2" }
"#,
        )
        .unwrap();
        let parsed = parse_cargo_lock(
            r#"version = 4

[[package]]
name = "serde"
version = "1.0.0"
dependencies = ["actual 2.0.0"]

[[package]]
name = "actual"
version = "2.0.0"
"#,
            &direct,
        )
        .unwrap();
        assert_eq!(parsed.nodes.len(), 2);
        assert!(parsed.nodes.iter().all(|node| node.direct));
        assert_eq!(
            parsed.nodes[0].dependencies[0].version.as_deref(),
            Some("2.0.0")
        );
    }

    #[test]
    fn parses_pnpm_importers_packages_and_snapshot_edges() {
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      react:
        specifier: ^19
        version: 19.1.0
packages:
  react@19.1.0: {}
  scheduler@0.26.0: {}
snapshots:
  react@19.1.0:
    dependencies:
      scheduler: 0.26.0
  scheduler@0.26.0: {}
"#,
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(parsed.nodes.len(), 2);
        let react = parsed
            .nodes
            .iter()
            .find(|node| node.name == "react")
            .unwrap();
        assert!(react.direct);
        assert_eq!(react.dependencies[0].name, "scheduler");
        assert_eq!(react.dependencies[0].version.as_deref(), Some("0.26.0"));
    }

    #[test]
    fn ignores_pnpm_package_peer_ranges_and_uses_resolved_snapshot_edges() {
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: '9.0'
packages:
  plugin@1.0.0:
    peerDependencies:
      react: ^18.0.0 || ^19.0.0
  react@19.2.0: {}
snapshots:
  plugin@1.0.0:
    dependencies:
      react: 19.2.0
  react@19.2.0: {}
"#,
            &BTreeSet::new(),
        )
        .unwrap();
        let plugin = parsed
            .nodes
            .iter()
            .find(|node| node.name == "plugin")
            .unwrap();
        assert_eq!(plugin.dependencies.len(), 1);
        assert_eq!(plugin.dependencies[0].name, "react");
        assert_eq!(plugin.dependencies[0].version.as_deref(), Some("19.2.0"));
    }

    #[test]
    fn pnpm_importer_resolution_marks_only_the_selected_duplicate_version_direct() {
        let manifest_direct =
            parse_node_direct_names(r#"{ "dependencies": { "shared": "^2" } }"#).unwrap();
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      shared:
        specifier: ^2
        version: 2.0.0
packages:
  shared@1.0.0: {}
  shared@2.0.0: {}
snapshots:
  shared@1.0.0: {}
  shared@2.0.0: {}
"#,
            &manifest_direct,
        )
        .unwrap();
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.version == "1.0.0" && !node.direct));
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.version == "2.0.0" && node.direct));
    }

    #[test]
    fn parses_legacy_pnpm_slash_package_keys() {
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: 5.4
dependencies:
  '@scope/direct': 1.0.0
packages:
  /@scope/direct/1.0.0:
    dependencies:
      child: 2.0.0
  /child/2.0.0: {}
"#,
            &BTreeSet::new(),
        )
        .unwrap();
        let direct = parsed
            .nodes
            .iter()
            .find(|node| node.name == "@scope/direct")
            .unwrap();
        assert!(direct.direct);
        assert_eq!(direct.dependencies[0].name, "child");
        assert_eq!(direct.dependencies[0].version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn strips_legacy_pnpm_peer_suffixes_before_resolving_edges() {
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: 5.4
dependencies:
  root: 1.0.0_peer@3.0.0
packages:
  /root/1.0.0_peer@3.0.0:
    dependencies:
      child: 2.0.0_peer@3.0.0
  /child/2.0.0_peer@3.0.0: {}
"#,
            &BTreeSet::new(),
        )
        .unwrap();
        let root = parsed
            .nodes
            .iter()
            .find(|node| node.name == "root")
            .unwrap();
        assert_eq!(root.version, "1.0.0");
        assert_eq!(root.dependencies[0].version.as_deref(), Some("2.0.0"));
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.name == "child" && node.version == "2.0.0"));
    }

    #[test]
    fn parses_leading_slash_pnpm_at_keys_and_legacy_peer_direct_names() {
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: 6.0
peerDependencies:
  plain: 1.2.3
packages:
  /plain@1.2.3: {}
  /@scope/modern@2.0.0: {}
  /local@file:../local: {}
"#,
            &BTreeSet::new(),
        )
        .unwrap();
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.name == "plain" && node.version == "1.2.3" && node.direct));
        assert!(parsed.nodes.iter().any(|node| {
            node.name == "@scope/modern" && node.version == "2.0.0" && !node.direct
        }));
        assert_eq!(parsed.nodes.len(), 2);
    }

    #[test]
    fn parses_package_lock_without_exposing_registry_metadata() {
        let parsed = parse_package_lock(
            r#"{
  "lockfileVersion": 3,
  "packages": {
    "": { "dependencies": { "alpha": "^1" } },
    "node_modules/alpha": {
      "version": "1.2.0",
      "resolved": "https://user:secret@example.test/alpha.tgz",
      "dependencies": { "beta": "^2" }
    },
    "node_modules/beta": { "version": "2.0.0" }
  }
}"#,
            &BTreeSet::new(),
        )
        .unwrap();
        let debug = format!("{parsed:?}");
        assert!(!debug.contains("secret"));
        assert!(
            parsed
                .nodes
                .iter()
                .find(|node| node.name == "alpha")
                .unwrap()
                .direct
        );
    }

    #[test]
    fn package_manifests_extend_direct_names_without_marking_nested_versions_direct() {
        let direct = parse_node_direct_names(
            r#"{
  "dependencies": { "alpha": "^1" },
  "peerDependencies": { "bravo": "^2" }
}"#,
        )
        .unwrap();
        let parsed = parse_package_lock(
            r#"{
  "lockfileVersion": 3,
  "packages": {
    "": {},
    "node_modules/alpha": { "version": "1.0.0" },
    "node_modules/host/node_modules/alpha": { "version": "2.0.0" },
    "node_modules/bravo": { "version": "2.0.0" }
  }
}"#,
            &direct,
        )
        .unwrap();
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.name == "alpha" && node.version == "1.0.0" && node.direct));
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.name == "alpha" && node.version == "2.0.0" && !node.direct));
        assert!(parsed
            .nodes
            .iter()
            .any(|node| node.name == "bravo" && node.direct));
    }

    #[test]
    fn pnpm_uses_assigned_package_manifest_direct_names() {
        let direct =
            parse_node_direct_names(r#"{ "devDependencies": { "manifest-only": "1" } }"#).unwrap();
        let parsed = parse_pnpm_lock(
            r#"lockfileVersion: '9.0'
packages:
  manifest-only@1.0.0: {}
snapshots:
  manifest-only@1.0.0: {}
"#,
            &direct,
        )
        .unwrap();
        assert!(parsed.nodes[0].direct);
    }

    #[test]
    fn rejects_uri_shaped_versions_and_ambiguous_package_names() {
        let parsed = parse_package_lock(
            r#"{
  "lockfileVersion": 3,
  "packages": {
    "": {},
    "node_modules/private": {
      "version": "https://user:secret@example.test/private.tgz"
    }
  }
}"#,
            &BTreeSet::new(),
        );
        assert!(matches!(parsed, Err(ParseFailure::Invalid)));
        assert!(checked_package_name("@scope/package").is_ok());
        assert!(checked_package_name("name@version").is_err());
        assert!(checked_package_name("https://example.test/package").is_err());
        assert!(checked_version_text("1.0.0-rc.1+build").is_ok());
    }

    #[test]
    fn parses_uv_lock_and_normalizes_python_direct_names() {
        let direct = parse_python_direct_names(
            r#"[project]
dependencies = ["Typing_Extensions>=4", "httpx[http2]==1"]
"#,
        )
        .unwrap();
        let parsed = parse_uv_lock(
            r#"version = 1
[[package]]
name = "typing-extensions"
version = "4.15.0"
[[package]]
name = "httpx"
version = "1.0.0"
dependencies = [{ name = "typing_extensions" }]
"#,
            &direct,
        )
        .unwrap();
        assert!(parsed.nodes.iter().all(|node| node.direct));
        assert_eq!(parsed.nodes[1].dependencies[0].name, "typing-extensions");
    }

    #[test]
    fn repository_scan_reports_missing_stale_unsupported_and_duplicate_versions() {
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            r#"version = 4
[[package]]
name = "shared"
version = "1.0.0"
[[package]]
name = "shared"
version = "2.0.0"
"#,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\n[dependencies]\nshared='1'\n",
        )
        .unwrap();
        fs::create_dir(root.path().join("python")).unwrap();
        fs::write(
            root.path().join("python/pyproject.toml"),
            "[project]\nname='demo'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.path().join("build.gradle.kts"), "plugins {}\n").unwrap();

        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        assert_eq!(report.package_count, 2);
        assert_eq!(report.duplicates.len(), 1);
        assert_eq!(report.missing_lockfile_count, 1);
        assert_eq!(report.stale_lockfile_count, 1);
        assert_eq!(report.unsupported_count, 1);
    }

    #[test]
    fn recognizes_a_root_gradle_version_catalog_as_unsupported_input() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("gradle")).unwrap();
        fs::write(
            root.path().join("gradle/libs.versions.toml"),
            "[versions]\nkotlin = '2.0.0'\n",
        )
        .unwrap();

        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        assert_eq!(report.unsupported_count, 1);
        assert_eq!(report.sources[0].path, "gradle/libs.versions.toml");
        assert_eq!(report.sources[0].manifest_count, 1);
    }

    #[test]
    fn package_manager_field_classifies_missing_pnpm_locks_and_invalid_manifests() {
        let root = tempdir().unwrap();
        let pnpm = root.path().join("pnpm-project");
        let invalid = root.path().join("invalid-project");
        fs::create_dir(&pnpm).unwrap();
        fs::create_dir(&invalid).unwrap();
        fs::write(
            pnpm.join("package.json"),
            r#"{ "packageManager": "pnpm@9.15.0", "dependencies": {} }"#,
        )
        .unwrap();
        fs::write(invalid.join("package.json"), "{not-json").unwrap();

        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        assert!(report.sources.iter().any(|source| {
            source.path == "pnpm-project/package.json"
                && source.ecosystem == DependencyEcosystem::Pnpm
                && source.status == DependencySourceStatus::MissingLockfile
        }));
        assert!(report.sources.iter().any(|source| {
            source.path == "invalid-project/package.json"
                && source.status == DependencySourceStatus::Invalid
                && source.manifest_count == 1
                && source.lockfile_count == 0
        }));
        assert_eq!(report.missing_lockfile_count, 1);
        assert_eq!(report.invalid_count, 1);
    }

    #[test]
    fn oversized_input_is_reported_without_becoming_parser_input() {
        let root = tempdir().unwrap();
        let file = fs::File::create(root.path().join("Cargo.lock")).unwrap();
        file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();

        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        assert!(report.truncated);
        assert_eq!(report.invalid_count, 1);
        assert!(report.packages.is_empty());
    }

    #[test]
    fn exhausted_budget_returns_a_bounded_truncated_report() {
        let root = tempdir().unwrap();
        fs::write(
            root.path().join("Cargo.lock"),
            "version = 4\n[[package]]\nname='never-read'\nversion='1'\n",
        )
        .unwrap();

        let report = analyze_repository(root.path(), Duration::ZERO).unwrap();
        assert!(report.truncated);
        assert!(report.sources.is_empty());
        assert!(report.packages.is_empty());
    }

    #[test]
    fn unresolved_references_share_the_global_edge_budget() {
        let references = (0..MAX_EDGES + 7)
            .map(|index| DependencyReference {
                name: format!("missing-{index}"),
                version: None,
            })
            .collect();
        let mut unresolved = 0;
        let mut truncated = false;
        let packages = resolve_graph(
            vec![ParsedNode {
                ecosystem: DependencyEcosystem::Cargo,
                name: "root".into(),
                version: "1.0.0".into(),
                direct: true,
                dependencies: references,
            }],
            &mut unresolved,
            &mut truncated,
        );
        assert_eq!(packages.len(), 1);
        assert_eq!(unresolved, MAX_EDGES);
        assert!(truncated);
    }

    #[test]
    fn unreadable_or_oversized_inputs_count_toward_the_file_budget() {
        let root = tempdir().unwrap();
        for index in 0..MAX_INPUT_FILES + 5 {
            let directory = root.path().join(format!("project-{index:03}"));
            fs::create_dir(&directory).unwrap();
            let file = fs::File::create(directory.join("Cargo.lock")).unwrap();
            file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
        }

        let discovery =
            discover_inputs(root.path(), Instant::now() + Duration::from_secs(5)).unwrap();
        assert_eq!(
            discovery.files.len() + discovery.problems.len(),
            MAX_INPUT_FILES
        );
        assert!(discovery.truncated);
    }

    #[test]
    fn scans_the_checked_in_monorepo_as_an_offline_smoke_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let discovery = discover_inputs(&root, Instant::now() + Duration::from_secs(10)).unwrap();
        for manifest in matching_files(&discovery.files, "Cargo.toml") {
            let document: toml::Value = toml::from_str(manifest.text().unwrap()).unwrap();
            let table = document.as_table().unwrap();
            for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                let mut direct = BTreeSet::new();
                assert!(
                    collect_toml_dependency_table(table.get(key), &mut direct).is_ok(),
                    "invalid Cargo dependency table: {} {key}",
                    manifest.relative
                );
            }
            if let Some(targets) = table.get("target").and_then(toml::Value::as_table) {
                for (target_name, target) in targets {
                    let target = target.as_table().unwrap();
                    for key in ["dependencies", "dev-dependencies", "build-dependencies"] {
                        let mut direct = BTreeSet::new();
                        assert!(
                            collect_toml_dependency_table(target.get(key), &mut direct).is_ok(),
                            "invalid Cargo target dependency table: {} {target_name} {key}",
                            manifest.relative
                        );
                    }
                }
            }
            assert!(
                manifest.text().and_then(parse_cargo_direct_names).is_ok(),
                "invalid Cargo manifest: {}",
                manifest.relative
            );
        }
        for manifest in matching_files(&discovery.files, "package.json") {
            assert!(
                manifest.text().and_then(parse_node_direct_names).is_ok(),
                "invalid package manifest: {}",
                manifest.relative
            );
        }
        let report = analyze_repository(&root, Duration::from_secs(10)).unwrap();
        assert!(!report.truncated);
        assert!(
            report.package_count > 100,
            "packages={}, invalid={}, unsupported={}, sources={:?}",
            report.package_count,
            report.invalid_count,
            report.unsupported_count,
            report.sources
        );
        assert!(report
            .sources
            .iter()
            .any(|source| source.ecosystem == DependencyEcosystem::Cargo));
        assert!(report
            .sources
            .iter()
            .any(|source| source.ecosystem == DependencyEcosystem::Pnpm));
        assert_eq!(
            report.invalid_count, 0,
            "unexpected invalid sources: {:?}",
            report.sources
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_relative_paths_are_rejected_instead_of_lossily_colliding() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = tempdir().unwrap();
        let path = root
            .path()
            .join(OsString::from_vec(vec![b'C', b'a', b'r', b'g', b'o', 0xff]));
        assert!(safe_relative(root.path(), &path).is_err());
    }

    #[test]
    fn summary_contains_only_aggregate_package_information_and_round_trips() {
        let report = DependencyReport {
            revision: format!("sha256:{}", "a".repeat(64)),
            sources: vec![DependencySource {
                ecosystem: DependencyEcosystem::Cargo,
                path: "private/Cargo.lock".into(),
                status: DependencySourceStatus::Ready,
                manifest_count: 1,
                lockfile_count: 1,
                package_count: 1,
                direct_count: 1,
            }],
            packages: vec![DependencyPackage {
                id: "cargo:private-package@1.0.0".into(),
                ecosystem: DependencyEcosystem::Cargo,
                name: "private-package".into(),
                version: "1.0.0".into(),
                direct: true,
                dependencies: vec![],
            }],
            duplicates: vec![],
            package_count: 1,
            direct_count: 1,
            transitive_count: 0,
            unresolved_dependency_count: 0,
            missing_lockfile_count: 0,
            stale_lockfile_count: 0,
            unsupported_count: 0,
            invalid_count: 0,
            truncated: false,
            summary_published: false,
        };
        let entry = dependency_summary_entry("win:c:/private/repository", &report, 10).unwrap();
        let serialized = serde_json::to_string(&entry).unwrap();
        assert!(!serialized.contains("private-package"));
        assert!(!serialized.contains("private/repository"));
        assert!(!serialized.contains("Cargo.lock"));

        let integration_root = tempdir().unwrap();
        publish_summary_in(integration_root.path(), entry.clone(), 10).unwrap();
        let envelope = devbox_integration::read_snapshot_in(
            integration_root.path(),
            DEPENDENCY_SUMMARY_PRODUCER,
            DEPENDENCY_SUMMARY_VERSION,
        )
        .unwrap()
        .unwrap();
        let views = envelope.views().unwrap();
        assert_eq!(views[DEPENDENCY_SUMMARY_VIEW].entries.len(), 1);

        let replacement = DependencySummaryEntry {
            package_count: 2,
            direct_count: 1,
            transitive_count: 1,
            scanned_at_ms: 20,
            ecosystems: vec![DependencySummaryEcosystem {
                ecosystem: "cargo".into(),
                package_count: 2,
                direct_count: 1,
                duplicate_count: 0,
            }],
            ..entry.clone()
        };
        publish_summary_in(integration_root.path(), replacement.clone(), 20).unwrap();
        let envelope = devbox_integration::read_snapshot_in(
            integration_root.path(),
            DEPENDENCY_SUMMARY_PRODUCER,
            DEPENDENCY_SUMMARY_VERSION,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            envelope.views().unwrap()[DEPENDENCY_SUMMARY_VIEW]
                .entries
                .len(),
            1
        );

        let duplicate_root = tempdir().unwrap();
        let mut views = devbox_integration::SnapshotViews::new();
        views.insert(
            DEPENDENCY_SUMMARY_VIEW.into(),
            devbox_integration::SnapshotView {
                schema_version: DEPENDENCY_SUMMARY_VERSION,
                freshness_ms: 0,
                entries: vec![
                    serde_json::to_value(entry.clone()).unwrap(),
                    serde_json::to_value(entry).unwrap(),
                ],
            },
        );
        let envelope = devbox_integration::Envelope::with_views(
            DEPENDENCY_SUMMARY_PRODUCER,
            env!("CARGO_PKG_VERSION"),
            views,
        );
        devbox_integration::write_atomic(
            &envelope,
            &devbox_integration::snapshot_dir_in(
                duplicate_root.path(),
                DEPENDENCY_SUMMARY_PRODUCER,
                DEPENDENCY_SUMMARY_VERSION,
            ),
        )
        .unwrap();
        assert!(publish_summary_in(duplicate_root.path(), replacement, 20).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn scanner_does_not_follow_symlinked_manifests() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(
            outside.path().join("Cargo.toml"),
            "[package]\nname='outside'\n",
        )
        .unwrap();
        symlink(
            outside.path().join("Cargo.toml"),
            root.path().join("Cargo.toml"),
        )
        .unwrap();
        let report = analyze_repository(root.path(), Duration::from_secs(2)).unwrap();
        assert!(report.sources.is_empty());
        assert!(report.packages.is_empty());
    }
}
