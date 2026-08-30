//! Repo Manager command — 저장소 탐색·상태·worktree.

pub(crate) mod dependency_enrichment;

use crate::core::cleanup::{
    classify_preview, finalize_revision, parse_branch_records, parse_merged_branch_names,
    parse_worktree_records, parse_worktree_status, valid_cleanup_worktree_path, valid_ref_name,
    valid_revision, BranchCleanupEntry, CleanupItemResult, CleanupPreview, CleanupResult,
    ParsedBranch, ParsedWorktree, WorktreeCleanupEntry, GIT_CLEANUP_BUSY, GIT_CLEANUP_CANCELLED,
    GIT_CLEANUP_ERROR, GIT_CLEANUP_STATE_CHANGED, MAX_CLEANUP_BRANCHES, MAX_CLEANUP_OUTPUT_BYTES,
    MAX_CLEANUP_REF_BYTES, MAX_CLEANUP_SELECTIONS, MAX_CLEANUP_WORKTREES,
};
use crate::core::dependency_lens::{
    analyze_repository, dependency_summary_entry, now_epoch_ms, publish_summary_in,
    DependencyReport, DEPENDENCY_LENS_ERROR,
};
use crate::core::git::{parse_status, parse_worktrees, RepoSnapshot};
use crate::core::git_safety::{
    classify, parse_porcelain_v2, GitSafetySnapshot, GIT_SAFETY_ERROR, MAX_SAFETY_OUTPUT_BYTES,
};
use crate::core::history_diff::{
    parse_detail, parse_diff, parse_history, validate_commit_id, CommitDetail, DiffResult,
    HistoryResult, GIT_VIEW_ERROR, MAX_DETAIL_OUTPUT_BYTES, MAX_DIFF_OUTPUT_BYTES,
    MAX_HISTORY_LIMIT, MAX_HISTORY_OUTPUT_BYTES,
};
use crate::core::open_targets::{select_repo_open_targets, RepoOpenTarget};
use crate::core::remote_sync::{
    parse_remote_status, preflight_remote, RemoteAction, RemoteState, GIT_REMOTE_BUSY,
    GIT_REMOTE_CANCELLED, GIT_REMOTE_ERROR, GIT_REMOTE_STATE_CHANGED, MAX_REMOTE_BRANCH_BYTES,
};
use crate::core::stage_commit::{
    parse_status_changes, validate_change_path, validate_commit_message, ChangeEntry,
    GIT_MUTATION_ERROR, MAX_SELECTED_PATHS, MAX_STATUS_OUTPUT_BYTES,
};
use devbox_filesystem::{filesystem_identity, FilesystemIdentity};
use devbox_git::GitTarget;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub path: String,
    pub canonical_key: String,
    pub has_worktrees: bool,
}

/// 탐색 깊이 상한. 순환(예: Windows junction) 방어와 병목 방지 둘 다 겸한다 —
/// 재귀가 사이클을 타도 depth는 호출마다 증가하므로 결국 이 상한에서 멎는다.
const MAX_SCAN_DEPTH: usize = 12;
/// 방문 디렉터리 총량 상한. 얕지만 매우 넓은 트리(수만 개 형제 디렉터리)를 방어한다.
const MAX_VISITED_DIRS: usize = 20_000;
const MAX_REPOSITORY_PATH_BYTES: usize = 32_767;
const MAX_MUTATION_OUTPUT_BYTES: usize = 64 * 1024;
const MUTATION_TIMEOUT: Duration = Duration::from_secs(10);
const SAFETY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SAFETY_METADATA_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_REMOTE_STATUS_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_REMOTE_MARKER_OUTPUT_BYTES: usize = 4 * 1024;
const MAX_REMOTE_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_REMOTE_OPERATION_ID_BYTES: usize = 128;
const REMOTE_TIMEOUT: Duration = Duration::from_secs(30);
const CLEANUP_OBSERVATION_BUDGET: Duration = Duration::from_secs(30);
const CLEANUP_REVALIDATION_BUDGET: Duration = Duration::from_secs(15);
const CLEANUP_MUTATION_BUDGET: Duration = Duration::from_secs(120);
const MAX_WORKTREE_OUTPUT_BYTES: usize = 512 * 1024;
const GIT_STATUS_ERROR: &str = "Git 상태를 불러올 수 없습니다.";
const GIT_WORKTREE_ERROR: &str = "Git worktree 작업을 실행하지 못했습니다.";
const GIT_LOCAL_CANCELLED: &str = "Git 로컬 작업을 취소했습니다.";
const REMOTE_MARKERS: &[&str] = &[
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "rebase-merge",
    "rebase-apply",
];
static NEXT_INTERNAL_OPERATION: AtomicU64 = AtomicU64::new(0);

fn dependency_summary_write_lock() -> &'static Mutex<()> {
    static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

fn dependency_analysis_lock() -> &'static Mutex<()> {
    static ANALYSIS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ANALYSIS_LOCK.get_or_init(|| Mutex::new(()))
}

struct ActiveGitOperation {
    cancellation: Arc<AtomicBool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRepositoryOperation {
    operation_id: String,
}

#[derive(Default)]
struct ActiveGitOperations {
    by_id: HashMap<String, ActiveGitOperation>,
    by_repository: HashMap<FilesystemIdentity, ActiveRepositoryOperation>,
}

type ActiveGitOperationsState = Mutex<ActiveGitOperations>;

fn active_git_operations() -> &'static ActiveGitOperationsState {
    static ACTIVE: OnceLock<ActiveGitOperationsState> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(ActiveGitOperations::default()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryContext {
    worktree: PathBuf,
    worktree_identity: FilesystemIdentity,
    common_git_identity: FilesystemIdentity,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub repos: Vec<RepoEntry>,
    /// 깊이·방문 상한에 걸려 일부 디렉터리를 건너뛰었으면 true (조용한 누락 금지).
    pub truncated: bool,
}

/// root 아래 Git repository를 재귀 탐색한다 (canonical identity로 중복 제거).
/// node_modules·target·AppData 등 흔한 비-repo 디렉터리는 진입 전에 가지치기한다.
#[tauri::command]
pub fn scan_root(root: String) -> Result<ScanResult, String> {
    let mut repos = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    walk(
        Path::new(&root),
        &mut repos,
        0,
        &mut visited,
        &mut truncated,
    );
    // canonical key로 중복 제거
    let mut seen = std::collections::HashMap::new();
    for entry in repos {
        let key = entry.canonical_key.clone();
        seen.entry(key).or_insert(entry);
    }
    let mut out: Vec<RepoEntry> = seen.into_values().collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    crate::integration::replace_repositories(out.clone());
    Ok(ScanResult {
        repos: out,
        truncated,
    })
}

fn walk(
    dir: &Path,
    out: &mut Vec<RepoEntry>,
    depth: usize,
    visited: &mut usize,
    truncated: &mut bool,
) {
    if *visited >= MAX_VISITED_DIRS {
        *truncated = true;
        return;
    }
    *visited += 1;

    if dir.join(".git").exists() {
        if let Ok(entry) = repository_entry(dir) {
            out.push(entry);
        }
        return; // 중첩 repo는 건너뛴다
    }
    if depth >= MAX_SCAN_DEPTH {
        *truncated = true;
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if devbox_filesystem::is_ignored_dir(name) {
                continue;
            }
        }
        walk(&path, out, depth + 1, visited, truncated);
    }
}

fn host_path_spelling(path: &Path, error: &'static str) -> Result<String, String> {
    let value = path.to_str().ok_or_else(|| error.to_string())?;
    let folded = value.to_ascii_lowercase();
    if folded.starts_with(r"\\?\unc\") {
        return Ok(format!(r"\\{}", &value[8..]));
    }
    if folded.starts_with(r"\\?\") {
        let drive_path = &value[4..];
        let bytes = drive_path.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/')
        {
            return Ok(drive_path.to_owned());
        }
        return Err(error.to_string());
    }
    if folded.starts_with(r"\\.\") || folded.starts_with(r"\??\") {
        return Err(error.to_string());
    }
    Ok(value.to_owned())
}

fn git_target_for_path(cwd: &Path, error: &'static str) -> Result<GitTarget, String> {
    let cwd = host_path_spelling(cwd, error)?;
    GitTarget::from_project_path(&cwd).map_err(|_| error.to_string())
}

fn host_path_from_git(cwd: &Path, git_path: &str, error: &'static str) -> Result<PathBuf, String> {
    let target = git_target_for_path(cwd, error)?;
    target
        .host_path_from_git(git_path)
        .map(PathBuf::from)
        .map_err(|_| error.to_string())
}

fn git_path_from_host(cwd: &Path, host_path: &Path, error: &'static str) -> Result<String, String> {
    let target = git_target_for_path(cwd, error)?;
    let host_path = host_path_spelling(host_path, error)?;
    target
        .git_path_from_host(&host_path)
        .map_err(|_| error.to_string())
}

/// Execute a read-only Git query through the shared bounded runner. Its stderr,
/// timeout, argument, UTF-8, and stdout-cap failures are intentionally mapped
/// to one UI-safe error here.
fn run_git_bounded(args: &[String], cwd: &Path, max_bytes: usize) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_VIEW_ERROR)?;
    devbox_git::run_bounded_target(&args, &target, Duration::from_secs(5), max_bytes)
        .map_err(|_| GIT_VIEW_ERROR.to_string())
}

/// Bounded status reader for the mutable working-tree panel. Status output is
/// parsed as NUL-delimited porcelain records and all subprocess failures map
/// to the same UI-safe mutation error.
fn run_git_status_bounded(args: &[String], cwd: &Path) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_MUTATION_ERROR)?;
    devbox_git::run_bounded_target(&args, &target, MUTATION_TIMEOUT, MAX_STATUS_OUTPUT_BYTES)
        .map_err(|_| GIT_MUTATION_ERROR.to_string())
}

fn run_git_status_bounded_with_cancel(
    args: &[String],
    cwd: &Path,
    cancellation: &AtomicBool,
) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_MUTATION_ERROR)?;
    devbox_git::run_bounded_target_with_cancel(
        &args,
        &target,
        MUTATION_TIMEOUT,
        MAX_STATUS_OUTPUT_BYTES,
        cancellation,
    )
    .map_err(|error| {
        if error == "git_cancelled" {
            GIT_LOCAL_CANCELLED.to_string()
        } else {
            GIT_MUTATION_ERROR.to_string()
        }
    })
}

/// Fixed argv for the read-only Git safety snapshot. Porcelain-v2 branch
/// headers provide upstream/ahead/behind state without parsing human-facing
/// diagnostics, while `--` keeps the command from accepting a pathspec.
fn git_safety_status_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "status".to_string(),
        "--porcelain=v2".to_string(),
        "--branch".to_string(),
        "--untracked-files=all".to_string(),
        "-z".to_string(),
        "--".to_string(),
    ]
}

/// Fixed argv for locating the per-repository operation markers. No Git
/// mutation, remote operation, force option, reset, or clean command is
/// represented here.
fn git_safety_marker_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "rev-parse".to_string(),
        "--git-path".to_string(),
        "rebase-merge".to_string(),
        "--git-path".to_string(),
        "rebase-apply".to_string(),
        "--git-path".to_string(),
        "MERGE_HEAD".to_string(),
    ]
}

fn run_git_safety_bounded(args: &[String], cwd: &Path, max_bytes: usize) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_SAFETY_ERROR)?;
    devbox_git::run_bounded_target(&args, &target, SAFETY_TIMEOUT, max_bytes)
        .map_err(|_| GIT_SAFETY_ERROR.to_string())
}

/// Resolve the three fixed marker paths emitted by `git rev-parse`. The raw
/// paths remain command-internal and are never included in the result/error.
fn parse_safety_marker_paths(output: &str, cwd: &Path) -> Result<[std::path::PathBuf; 3], String> {
    if output.is_empty()
        || output.len() > MAX_SAFETY_METADATA_OUTPUT_BYTES
        || !output.ends_with('\n')
    {
        return Err(GIT_SAFETY_ERROR.to_string());
    }
    let lines = output
        .strip_suffix('\n')
        .unwrap_or_default()
        .split('\n')
        .collect::<Vec<_>>();
    if lines.len() != 3 {
        return Err(GIT_SAFETY_ERROR.to_string());
    }
    let mut paths = Vec::with_capacity(3);
    for (index, line) in lines.into_iter().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty()
            || line.len() > MAX_SAFETY_METADATA_OUTPUT_BYTES
            || line.chars().any(char::is_control)
        {
            return Err(GIT_SAFETY_ERROR.to_string());
        }
        let expected_name = ["rebase-merge", "rebase-apply", "MERGE_HEAD"][index];
        let resolved = host_path_from_git(cwd, line, GIT_SAFETY_ERROR)?;
        if resolved.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
            return Err(GIT_SAFETY_ERROR.to_string());
        }
        paths.push(resolved);
    }
    paths.try_into().map_err(|_| GIT_SAFETY_ERROR.to_string())
}

/// Treat only a missing marker as a normal negative result. Permission,
/// unmount, and other filesystem failures are reported as one fixed error so
/// an uncertain repository state cannot be presented as safe.
fn marker_present(path: &Path) -> Result<bool, String> {
    marker_present_with_error(path, GIT_SAFETY_ERROR)
}

/// Marker paths are state sentinels, so following a symlink would allow an
/// unrelated file to make a repository look mid-operation (or safe). Both the
/// read-only safety and remote surfaces use this fail-closed helper.
fn marker_present_with_error(path: &Path, error: &str) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(error.to_string());
            }
            Ok(true)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(_) => Err(error.to_string()),
    }
}

/// Execute a local stage/unstage/commit command. The shared runner closes
/// stdin/stderr, bounds stdout, kills a hung hook, and returns no Git path,
/// remote, credential, or OS diagnostic.
fn run_git_mutation(args: &[String], cwd: &Path) -> Result<(), String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_MUTATION_ERROR)?;
    devbox_git::run_mutating_target(&args, &target, MUTATION_TIMEOUT, MAX_MUTATION_OUTPUT_BYTES)
        .map(|_| ())
        .map_err(|_| GIT_MUTATION_ERROR.to_string())
}

fn run_git_mutation_with_cancel(
    args: &[String],
    cwd: &Path,
    cancellation: &AtomicBool,
) -> Result<(), String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_MUTATION_ERROR)?;
    devbox_git::run_mutating_target_with_cancel(
        &args,
        &target,
        MUTATION_TIMEOUT,
        MAX_MUTATION_OUTPUT_BYTES,
        cancellation,
    )
    .map(|_| ())
    .map_err(|error| {
        if error == "git_cancelled" {
            GIT_LOCAL_CANCELLED.to_string()
        } else {
            GIT_MUTATION_ERROR.to_string()
        }
    })
}

fn run_git_cleanup_bounded_with_timeout(
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    cancellation: Option<&AtomicBool>,
) -> Result<String, String> {
    if !cleanup_read_argv_allowed(args) {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_CLEANUP_ERROR)?;
    let result = match cancellation {
        Some(signal) => devbox_git::run_bounded_target_with_cancel(
            &args,
            &target,
            timeout,
            MAX_CLEANUP_OUTPUT_BYTES,
            signal,
        ),
        None => devbox_git::run_bounded_target(&args, &target, timeout, MAX_CLEANUP_OUTPUT_BYTES),
    };
    result.map_err(|error| {
        if error == "git_cancelled" {
            GIT_CLEANUP_CANCELLED.to_string()
        } else {
            GIT_CLEANUP_ERROR.to_string()
        }
    })
}

fn run_git_cleanup_mutation_with_cancel(
    args: &[String],
    cwd: &Path,
    cancellation: &AtomicBool,
    timeout: Duration,
) -> Result<(), String> {
    if !cleanup_mutation_argv_allowed(args) {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_CLEANUP_ERROR)?;
    devbox_git::run_mutating_target_with_cancel(
        &args,
        &target,
        timeout,
        MAX_MUTATION_OUTPUT_BYTES,
        cancellation,
    )
    .map(|_| ())
    .map_err(|error| {
        if error == "git_cancelled" {
            GIT_CLEANUP_CANCELLED.to_string()
        } else {
            GIT_CLEANUP_ERROR.to_string()
        }
    })
}

fn git_cleanup_branch_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "for-each-ref".to_string(),
        "--format=%(refname:strip=2)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(committerdate:unix)%00%(HEAD)%00".to_string(),
        "refs/heads".to_string(),
    ]
}

fn git_cleanup_merged_args(head: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "for-each-ref".to_string(),
        format!("--merged={head}"),
        "--format=%(refname:strip=2)".to_string(),
        "refs/heads".to_string(),
    ]
}

fn git_cleanup_worktree_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "worktree".to_string(),
        "list".to_string(),
        "--porcelain".to_string(),
        "-z".to_string(),
    ]
}

fn git_cleanup_status_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "--untracked-files=all".to_string(),
        "--ignored=matching".to_string(),
        "-z".to_string(),
        "--".to_string(),
    ]
}

fn git_cleanup_delete_branch_args(branch: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "branch".to_string(),
        "--delete".to_string(),
        "--".to_string(),
        branch.to_string(),
    ]
}

fn git_cleanup_remove_worktree_args(path: &Path) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "worktree".to_string(),
        "remove".to_string(),
        "--".to_string(),
        path.to_string_lossy().into_owned(),
    ]
}

fn valid_cleanup_object_id(value: &str) -> bool {
    (40..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_cleanup_worktree_arg(value: &str) -> bool {
    valid_cleanup_worktree_path(value)
}

/// Keep the cleanup read surface closed even if a future caller accidentally
/// passes an argument vector outside the audited fixed command set. Dynamic
/// values are admitted only in the exact slots documented by the parser.
fn cleanup_read_argv_allowed(args: &[String]) -> bool {
    let fixed = |expected: &[&str]| args.iter().map(String::as_str).eq(expected.iter().copied());
    if fixed(&[
        "--no-pager",
        "--no-optional-locks",
        "for-each-ref",
        "--format=%(refname:strip=2)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(committerdate:unix)%00%(HEAD)%00",
        "refs/heads",
    ]) || fixed(&[
        "--no-pager",
        "--no-optional-locks",
        "worktree",
        "list",
        "--porcelain",
        "-z",
    ]) || fixed(&[
        "--no-pager",
        "--no-optional-locks",
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
        "--ignored=matching",
        "-z",
        "--",
    ]) {
        return true;
    }
    args.len() == 6
        && args[0] == "--no-pager"
        && args[1] == "--no-optional-locks"
        && args[2] == "for-each-ref"
        && args[3]
            .strip_prefix("--merged=")
            .is_some_and(valid_cleanup_object_id)
        && args[4] == "--format=%(refname:strip=2)"
        && args[5] == "refs/heads"
}

fn cleanup_mutation_argv_allowed(args: &[String]) -> bool {
    if args.len() == 6
        && args[0] == "--no-pager"
        && args[1] == "--no-optional-locks"
        && args[2] == "branch"
        && args[3] == "--delete"
        && args[4] == "--"
    {
        return valid_ref_name(&args[5]);
    }
    args.len() == 6
        && args[0] == "--no-pager"
        && args[1] == "--no-optional-locks"
        && args[2] == "worktree"
        && args[3] == "remove"
        && args[4] == "--"
        && valid_cleanup_worktree_arg(&args[5])
}

#[derive(Debug)]
struct CleanupObservation {
    preview: CleanupPreview,
    worktree_identities: Vec<Option<FilesystemIdentity>>,
}

type CleanupRevalidationMetadata = (
    Vec<ParsedBranch>,
    Vec<ParsedWorktree>,
    HashSet<String>,
    Option<String>,
);

fn cleanup_now_unix() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| GIT_CLEANUP_ERROR.to_string())?;
    i64::try_from(duration.as_secs()).map_err(|_| GIT_CLEANUP_ERROR.to_string())
}

fn parse_host_worktree_records(
    output: &str,
    cwd: &Path,
    error: &'static str,
) -> Result<Vec<ParsedWorktree>, String> {
    let mut records = parse_worktree_records(output).map_err(|_| error.to_string())?;
    for record in &mut records {
        let host = host_path_from_git(cwd, &record.path, error)?;
        record.path = host.to_str().ok_or_else(|| error.to_string())?.to_owned();
    }
    Ok(records)
}

fn resolve_cleanup_worktree_path(
    parsed: &ParsedWorktree,
) -> Result<(PathBuf, FilesystemIdentity), String> {
    let path = PathBuf::from(&parsed.path);
    GitTarget::validate_host_absolute_path(&parsed.path)
        .map_err(|_| GIT_CLEANUP_ERROR.to_string())?;
    let identity = filesystem_identity(&path, true).map_err(|_| GIT_CLEANUP_ERROR.to_string())?;
    let canonical = path
        .canonicalize()
        .map_err(|_| GIT_CLEANUP_ERROR.to_string())?;
    if filesystem_identity(&canonical, true).map_err(|_| GIT_CLEANUP_ERROR.to_string())? != identity
    {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    Ok((canonical, identity))
}

fn cleanup_identity_revision(
    mut preview: CleanupPreview,
    identities: &[Option<FilesystemIdentity>],
    context: &RepositoryContext,
) -> CleanupPreview {
    let base = finalize_revision(preview.clone());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    base.revision.hash(&mut hasher);
    // The preview is also an approval for this exact repository context. A
    // worktree path can retain its filesystem identity while its `.git` file
    // is exchanged to point at another common Git directory; hashing only
    // worktree identities would let an equivalent-looking replacement reuse
    // the old revision.
    context.worktree_identity.hash(&mut hasher);
    context.common_git_identity.hash(&mut hasher);
    for identity in identities {
        format!("{identity:?}").hash(&mut hasher);
    }
    preview.revision = format!("cleanup-{:016x}", hasher.finish());
    preview
}

/// Collect one coherent read-only cleanup snapshot while the caller owns the
/// repository lock.  Inaccessible worktrees remain visible but are marked
/// blocked by `stateUnavailable`; they are never treated as clean.
fn cleanup_remaining_timeout(
    deadline: Instant,
    cancellation: Option<&AtomicBool>,
) -> Result<Duration, String> {
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(SAFETY_TIMEOUT))
        .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())
}

fn cleanup_remaining_mutation_timeout(
    deadline: Instant,
    cancellation: &AtomicBool,
) -> Result<Duration, String> {
    if cancellation.load(Ordering::Acquire) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .map(|remaining| remaining.min(MUTATION_TIMEOUT))
        .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())
}

fn collect_cleanup_observation(
    context: &RepositoryContext,
    cancellation: Option<&AtomicBool>,
) -> Result<CleanupObservation, String> {
    let deadline = Instant::now()
        .checked_add(CLEANUP_OBSERVATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
    let branch_text = run_git_cleanup_bounded_with_timeout(
        &git_cleanup_branch_args(),
        &context.worktree,
        cleanup_remaining_timeout(deadline, cancellation)?,
        cancellation,
    )?;
    let branches = parse_branch_records(&branch_text).map_err(|_| GIT_CLEANUP_ERROR.to_string())?;
    if branches.len() > MAX_CLEANUP_BRANCHES {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }

    let worktree_text = run_git_cleanup_bounded_with_timeout(
        &git_cleanup_worktree_args(),
        &context.worktree,
        cleanup_remaining_timeout(deadline, cancellation)?,
        cancellation,
    )?;
    let mut worktrees =
        parse_host_worktree_records(&worktree_text, &context.worktree, GIT_CLEANUP_ERROR)?;
    if worktrees.is_empty() || worktrees.len() > MAX_CLEANUP_WORKTREES {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }

    let mut statuses = Vec::with_capacity(worktrees.len());
    let mut identities = Vec::with_capacity(worktrees.len());
    let mut context_worktree_index = None;
    for worktree in &mut worktrees {
        // Filesystem identity/canonicalization is synchronous and cannot be
        // interrupted by the Git runner. Check the shared budget on both
        // sides so a large set of unavailable worktrees cannot continue
        // issuing status children after cancellation or expiry.
        cleanup_remaining_timeout(deadline, cancellation)?;
        if worktree.bare || worktree.prunable {
            statuses.push(None);
            identities.push(None);
            continue;
        }
        match resolve_cleanup_worktree_path(worktree) {
            Ok((canonical, identity)) => {
                worktree.path = host_path_spelling(&canonical, GIT_CLEANUP_ERROR)?;
                if identity == context.worktree_identity
                    && context_worktree_index.replace(statuses.len()).is_some()
                {
                    return Err(GIT_CLEANUP_ERROR.to_string());
                }
                cleanup_remaining_timeout(deadline, cancellation)?;
                let timeout = cleanup_remaining_timeout(deadline, cancellation)?;
                let status = match run_git_cleanup_bounded_with_timeout(
                    &git_cleanup_status_args(),
                    &canonical,
                    timeout,
                    cancellation,
                ) {
                    Ok(output) => parse_worktree_status(&output).ok(),
                    Err(error) if error == GIT_CLEANUP_CANCELLED => return Err(error),
                    Err(_) => None,
                };
                statuses.push(status);
                identities.push(Some(identity));
            }
            Err(_) => {
                statuses.push(None);
                identities.push(None);
            }
        }
    }

    // `worktree list` orders the primary worktree first, but the command may
    // be invoked for a linked worktree as well.  The selected repository's
    // HEAD is the merge base for the preview; using the list's first HEAD
    // would silently classify branches against another worktree's branch.
    let context_worktree_index =
        context_worktree_index.ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
    let current_head = worktrees
        .get(context_worktree_index)
        .and_then(|worktree| worktree.head.clone());
    let merged = if let Some(head) = current_head.as_deref() {
        let merged_text = run_git_cleanup_bounded_with_timeout(
            &git_cleanup_merged_args(head),
            &context.worktree,
            cleanup_remaining_timeout(deadline, cancellation)?,
            cancellation,
        )?;
        parse_merged_branch_names(&merged_text).map_err(|_| GIT_CLEANUP_ERROR.to_string())?
    } else {
        HashSet::new()
    };

    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    let now = cleanup_now_unix()?;
    let mut preview =
        classify_preview(current_head, &branches, &worktrees, &merged, &statuses, now);
    for (index, identity) in identities.iter().enumerate() {
        if *identity == Some(context.worktree_identity) {
            if let Some(worktree) = preview.worktrees.get_mut(index) {
                worktree.blocked.push("currentWorktree".to_string());
                worktree.eligible = false;
            }
        }
    }
    Ok(CleanupObservation {
        preview: cleanup_identity_revision(preview, &identities, context),
        worktree_identities: identities,
    })
}

/// Read only the metadata needed to bind a cleanup mutation to the approved
/// preview.  This is intentionally a separate bounded snapshot from the
/// initial preview: Git refs and worktree registrations may change while the
/// confirmation dialog is open.  Every caller holds the per-repository lock,
/// and every read is cancellation-aware and covered by one total deadline.
fn read_cleanup_revalidation_metadata(
    context: &RepositoryContext,
    cancellation: &AtomicBool,
    parent_deadline: Instant,
) -> Result<CleanupRevalidationMetadata, String> {
    let local_deadline = Instant::now()
        .checked_add(CLEANUP_REVALIDATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let deadline = local_deadline.min(parent_deadline);
    let read = |args: Vec<String>, cwd: &Path| {
        run_git_cleanup_bounded_with_timeout(
            &args,
            cwd,
            cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
                if error == GIT_CLEANUP_CANCELLED {
                    error
                } else {
                    GIT_CLEANUP_STATE_CHANGED.to_string()
                }
            })?,
            Some(cancellation),
        )
        .map_err(|error| {
            if error == GIT_CLEANUP_CANCELLED {
                error
            } else {
                GIT_CLEANUP_STATE_CHANGED.to_string()
            }
        })
    };

    let branches = parse_branch_records(&read(git_cleanup_branch_args(), &context.worktree)?)
        .map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let worktrees = parse_host_worktree_records(
        &read(git_cleanup_worktree_args(), &context.worktree)?,
        &context.worktree,
        GIT_CLEANUP_STATE_CHANGED,
    )?;
    if worktrees.is_empty() || branches.len() > MAX_CLEANUP_BRANCHES {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }
    let mut context_worktree_index = None;
    for (index, worktree) in worktrees.iter().enumerate() {
        cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
            if error == GIT_CLEANUP_CANCELLED {
                error
            } else {
                GIT_CLEANUP_STATE_CHANGED.to_string()
            }
        })?;
        if worktree.bare || worktree.prunable {
            continue;
        }
        if let Ok((_, identity)) = resolve_cleanup_worktree_path(worktree) {
            if identity == context.worktree_identity
                && context_worktree_index.replace(index).is_some()
            {
                return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
            }
        }
    }
    let context_worktree_index =
        context_worktree_index.ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let current_head = worktrees
        .get(context_worktree_index)
        .and_then(|worktree| worktree.head.clone());
    let merged = if let Some(head) = current_head.as_deref() {
        parse_merged_branch_names(&read(git_cleanup_merged_args(head), &context.worktree)?)
            .map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?
    } else {
        HashSet::new()
    };
    Ok((branches, worktrees, merged, current_head))
}

fn cleanup_revalidate_context(
    context: &RepositoryContext,
    cancellation: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    if cancellation.load(Ordering::Acquire) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    let timeout = cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
        if error == GIT_CLEANUP_CANCELLED {
            error
        } else {
            GIT_CLEANUP_STATE_CHANGED.to_string()
        }
    })?;
    revalidate_repository_context_with_timeout_and_cancel(
        context,
        GIT_CLEANUP_STATE_CHANGED,
        timeout,
        Some(cancellation),
        Some(deadline),
    )?;
    // A filesystem metadata call (canonicalize/identity) cannot be forcibly
    // interrupted once the OS has entered it. Re-check the same operation
    // boundary after the context helper so a slow call cannot make a cleanup
    // continue past its deadline or swallow a racing cancellation.
    cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
        if error == GIT_CLEANUP_CANCELLED {
            error
        } else {
            GIT_CLEANUP_STATE_CHANGED.to_string()
        }
    })?;
    Ok(())
}

/// Re-check every branch field that influenced eligibility immediately before
/// `git branch --delete`.  Comparing the complete classified entry prevents a
/// branch from being replaced, checked out, unmerged, or otherwise changed
/// after the user approved the earlier preview.
fn cleanup_branch_still_safe(
    context: &RepositoryContext,
    expected: &BranchCleanupEntry,
    expected_current_head: Option<&str>,
    cancellation: &AtomicBool,
    deadline: Instant,
) -> Result<(), String> {
    cleanup_revalidate_context(context, cancellation, deadline)?;
    let (branches, worktrees, merged, current_head) =
        read_cleanup_revalidation_metadata(context, cancellation, deadline)?;
    if current_head.as_deref() != expected_current_head {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }
    let statuses = vec![None; worktrees.len()];
    let now = cleanup_now_unix().map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let preview = classify_preview(current_head, &branches, &worktrees, &merged, &statuses, now);
    let actual = preview
        .branches
        .iter()
        .find(|branch| branch.name == expected.name)
        .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    if actual != expected || !actual.eligible {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }
    cleanup_revalidate_context(context, cancellation, deadline)
}

fn git_remote_status_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "--branch".to_string(),
        "--untracked-files=all".to_string(),
        "--".to_string(),
    ]
}

fn git_remote_marker_args(marker: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "rev-parse".to_string(),
        "--git-path".to_string(),
        marker.to_string(),
    ]
}

/// Fetch only Git's default configured remote: the current branch's remote,
/// or `origin` when no branch remote is configured. This is deliberately not
/// `--all`. `--no-tags` keeps a routine refresh from changing the local tag
/// namespace; no remote URL or refspec is accepted from the frontend.
fn git_fetch_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "fetch".to_string(),
        "--no-tags".to_string(),
    ]
}

fn git_pull_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "pull".to_string(),
        "--ff-only".to_string(),
        "--no-rebase".to_string(),
    ]
}

fn git_remote_name_args(branch: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "config".to_string(),
        "--get".to_string(),
        format!("branch.{branch}.remote"),
    ]
}

fn parse_remote_name(output: &str) -> Result<String, String> {
    let value = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .ok_or_else(|| GIT_REMOTE_ERROR.to_string())?;
    if value.len() > MAX_REMOTE_BRANCH_BYTES
        || !valid_ref_path_fragment(value)
        || value.contains('@')
        || value.contains(':')
    {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    Ok(value.to_string())
}

/// Validate the path portion of a Git ref before constructing an exact push
/// refspec. This mirrors the safety-relevant `git check-ref-format` rules
/// without invoking another process or accepting a frontend-controlled ref.
fn valid_ref_path_fragment(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.ends_with('.')
        && !value.contains("//")
        && !value.contains("..")
        && !value.contains("@{")
        && value != "@"
        && !value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && value.split('/').all(|component| {
            !component.is_empty() && !component.starts_with('.') && !component.ends_with(".lock")
        })
}

fn git_push_args(remote: &str, upstream: &str) -> Result<Vec<String>, String> {
    let destination = upstream
        .strip_prefix(remote)
        .and_then(|value| value.strip_prefix('/'))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GIT_REMOTE_ERROR.to_string())?;
    if !valid_ref_path_fragment(remote)
        || !valid_ref_path_fragment(destination)
        || destination
            .chars()
            .any(|character| matches!(character, ':' | '@' | '\\'))
    {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    Ok(vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "push".to_string(),
        "--".to_string(),
        remote.to_string(),
        format!("HEAD:refs/heads/{destination}"),
    ])
}

fn exact_push_args(
    cwd: &Path,
    state: &RemoteState,
    cancellation: &AtomicBool,
) -> Result<(Vec<String>, String), String> {
    let branch = state
        .current_branch
        .as_deref()
        .ok_or_else(|| GIT_REMOTE_ERROR.to_string())?;
    let upstream = state
        .upstream
        .as_deref()
        .ok_or_else(|| GIT_REMOTE_ERROR.to_string())?;
    let output = run_git_remote_bounded(
        &git_remote_name_args(branch),
        cwd,
        MAX_REMOTE_BRANCH_BYTES + 2,
        Some(cancellation),
    )?;
    let remote = parse_remote_name(&output)?;
    let args = git_push_args(&remote, upstream)?;
    Ok((args, remote))
}

fn valid_remote_operation_id(operation_id: &str) -> bool {
    !operation_id.is_empty()
        && operation_id.len() <= MAX_REMOTE_OPERATION_ID_BYTES
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn active_git_operation_in_progress(
    repository: FilesystemIdentity,
    excluded_operation_id: Option<&str>,
    error: &'static str,
) -> Result<bool, String> {
    let active = active_git_operations()
        .lock()
        .map_err(|_| error.to_string())?;
    Ok(active
        .by_repository
        .get(&repository)
        .is_some_and(|operation| Some(operation.operation_id.as_str()) != excluded_operation_id))
}

#[derive(Debug)]
struct GitOperationGuard {
    operation_id: String,
    repository: Option<FilesystemIdentity>,
    cancellation: Arc<AtomicBool>,
}

impl GitOperationGuard {
    fn bind_repository(
        &mut self,
        repository: FilesystemIdentity,
        error: &'static str,
        busy_error: &'static str,
    ) -> Result<(), String> {
        if self.repository.is_some() {
            return Err(error.to_string());
        }
        let mut active = active_git_operations()
            .lock()
            .map_err(|_| error.to_string())?;
        if active.by_repository.contains_key(&repository) {
            return Err(busy_error.to_string());
        }
        active.by_repository.insert(
            repository,
            ActiveRepositoryOperation {
                operation_id: self.operation_id.clone(),
            },
        );
        self.repository = Some(repository);
        Ok(())
    }
}

impl Drop for GitOperationGuard {
    fn drop(&mut self) {
        finish_git_operation(&self.operation_id, self.repository);
    }
}

fn begin_git_operation(
    operation_id: &str,
    error: &'static str,
    busy_error: &'static str,
) -> Result<GitOperationGuard, String> {
    if !valid_remote_operation_id(operation_id) {
        return Err(error.to_string());
    }
    let mut active = active_git_operations()
        .lock()
        .map_err(|_| error.to_string())?;
    if active.by_id.contains_key(operation_id) {
        return Err(busy_error.to_string());
    }
    let cancellation = Arc::new(AtomicBool::new(false));
    active.by_id.insert(
        operation_id.to_owned(),
        ActiveGitOperation {
            cancellation: Arc::clone(&cancellation),
        },
    );
    Ok(GitOperationGuard {
        operation_id: operation_id.to_owned(),
        repository: None,
        cancellation,
    })
}

fn begin_internal_git_operation(error: &'static str) -> Result<GitOperationGuard, String> {
    let sequence = NEXT_INTERNAL_OPERATION.fetch_add(1, Ordering::Relaxed);
    let operation_id = format!("internal-{}-{sequence}", std::process::id());
    begin_git_operation(&operation_id, error, error)
}

fn finish_git_operation(operation_id: &str, repository: Option<FilesystemIdentity>) {
    if let Ok(mut active) = active_git_operations().lock() {
        active.by_id.remove(operation_id);
        if let Some(repository) = repository {
            if active
                .by_repository
                .get(&repository)
                .is_some_and(|value| value.operation_id == operation_id)
            {
                active.by_repository.remove(&repository);
            }
        }
    }
}

fn cancel_git_operation(operation_id: &str) -> bool {
    let Ok(active) = active_git_operations().lock() else {
        return false;
    };
    if let Some(operation) = active.by_id.get(operation_id) {
        operation.cancellation.store(true, Ordering::Release);
        return true;
    }
    false
}

async fn spawn_git_task<T, F>(join_error: &'static str, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| join_error.to_string())?
}

fn run_git_remote_bounded(
    args: &[String],
    cwd: &Path,
    max_bytes: usize,
    cancellation: Option<&AtomicBool>,
) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_REMOTE_ERROR)?;
    let result = match cancellation {
        Some(signal) => devbox_git::run_bounded_target_with_cancel(
            &args,
            &target,
            Duration::from_secs(5),
            max_bytes,
            signal,
        ),
        None => devbox_git::run_bounded_target(&args, &target, Duration::from_secs(5), max_bytes),
    };
    result.map_err(|error| {
        if error == "git_cancelled" {
            GIT_REMOTE_CANCELLED.to_string()
        } else {
            GIT_REMOTE_ERROR.to_string()
        }
    })
}

fn remote_marker_exists(
    cwd: &Path,
    marker: &str,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, String> {
    if !REMOTE_MARKERS.contains(&marker) {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    let expected_marker = marker;
    let output = run_git_remote_bounded(
        &git_remote_marker_args(marker),
        cwd,
        MAX_REMOTE_MARKER_OUTPUT_BYTES,
        cancellation,
    )?;
    let marker_path_text = output.trim();
    if marker_path_text.is_empty()
        || marker_path_text.len() > MAX_REMOTE_BRANCH_BYTES
        || marker_path_text.chars().any(char::is_control)
    {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    let marker_path = host_path_from_git(cwd, marker_path_text, GIT_REMOTE_ERROR)?;
    if marker_path.file_name().and_then(|name| name.to_str()) != Some(expected_marker) {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    marker_present_with_error(&marker_path, GIT_REMOTE_ERROR)
}

fn remote_operation_in_progress(
    context: &RepositoryContext,
    cancellation: Option<&AtomicBool>,
    excluded_operation_id: Option<&str>,
) -> Result<bool, String> {
    if active_git_operation_in_progress(
        context.common_git_identity,
        excluded_operation_id,
        GIT_REMOTE_ERROR,
    )? {
        return Ok(true);
    }
    for marker in REMOTE_MARKERS {
        if remote_marker_exists(&context.worktree, marker, cancellation)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_remote_state(
    context: &RepositoryContext,
    cancellation: Option<&AtomicBool>,
    excluded_operation_id: Option<&str>,
) -> Result<RemoteState, String> {
    let output = run_git_remote_bounded(
        &git_remote_status_args(),
        &context.worktree,
        MAX_REMOTE_STATUS_OUTPUT_BYTES,
        cancellation,
    )?;
    let in_progress = remote_operation_in_progress(context, cancellation, excluded_operation_id)?;
    parse_remote_status(&output, in_progress).map_err(|_| GIT_REMOTE_ERROR.to_string())
}

fn run_registered_remote_operation<F, G>(
    context: &RepositoryContext,
    action: RemoteAction,
    operation_id: &str,
    cancellation: &AtomicBool,
    before_final_recheck: F,
    before_spawn: G,
) -> Result<(), String>
where
    F: FnOnce(),
    G: FnOnce(),
{
    (|| {
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        let state = read_remote_state(context, Some(cancellation), Some(operation_id))?;
        preflight_remote(&state, action).map_err(|reason| reason.message())?;
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        // The first status read is only an admission check. Re-read the native
        // state while the per-repository operation lock is held immediately
        // before spawning fetch/pull/push, and refuse the mutation if another
        // writer changed any safety-relevant field in the meantime.
        before_final_recheck();
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        revalidate_repository_context(context, GIT_REMOTE_STATE_CHANGED)?;
        let final_state = read_remote_state(context, Some(cancellation), Some(operation_id))?;
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        if final_state != state {
            return Err(GIT_REMOTE_STATE_CHANGED.to_string());
        }
        preflight_remote(&final_state, action).map_err(|reason| reason.message())?;
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        let (mut args, configured_remote) = match action {
            RemoteAction::Fetch => (git_fetch_args(), None),
            RemoteAction::Pull => (git_pull_args(), None),
            RemoteAction::Push => {
                let (args, remote) =
                    exact_push_args(&context.worktree, &final_state, cancellation)?;
                // The branch/upstream snapshot used to construct the exact
                // refspec must still be current after reading branch config.
                let post_config_state =
                    read_remote_state(context, Some(cancellation), Some(operation_id))?;
                if post_config_state != final_state {
                    return Err(GIT_REMOTE_STATE_CHANGED.to_string());
                }
                (args, Some(remote))
            }
        };
        before_spawn();
        if cancellation.load(Ordering::Acquire) {
            return Err(GIT_REMOTE_CANCELLED.to_string());
        }
        revalidate_repository_context(context, GIT_REMOTE_STATE_CHANGED)?;
        let pre_spawn_state = read_remote_state(context, Some(cancellation), Some(operation_id))?;
        if pre_spawn_state != final_state {
            return Err(GIT_REMOTE_STATE_CHANGED.to_string());
        }
        if let Some(expected_remote) = configured_remote {
            let (confirmed_args, confirmed_remote) =
                exact_push_args(&context.worktree, &pre_spawn_state, cancellation)?;
            if confirmed_remote != expected_remote || confirmed_args != args {
                return Err(GIT_REMOTE_STATE_CHANGED.to_string());
            }
            args = confirmed_args;
        }
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        let target = git_target_for_path(&context.worktree, GIT_REMOTE_ERROR)?;
        devbox_git::run_mutating_target_with_cancel(
            &args,
            &target,
            REMOTE_TIMEOUT,
            MAX_REMOTE_OUTPUT_BYTES,
            cancellation,
        )
        .map(|_| ())
        .map_err(|error| {
            if error == "git_cancelled" {
                GIT_REMOTE_CANCELLED.to_string()
            } else {
                GIT_REMOTE_ERROR.to_string()
            }
        })
    })()
}

#[cfg(test)]
fn run_remote_operation_with_hook<F>(
    cwd: &Path,
    action: RemoteAction,
    operation_id: &str,
    before_final_recheck: F,
) -> Result<(), String>
where
    F: FnOnce(),
{
    run_remote_operation_with_hooks(cwd, action, operation_id, before_final_recheck, || {})
}

#[cfg(test)]
fn run_remote_operation_with_hooks<F, G>(
    cwd: &Path,
    action: RemoteAction,
    operation_id: &str,
    before_final_recheck: F,
    before_spawn: G,
) -> Result<(), String>
where
    F: FnOnce(),
    G: FnOnce(),
{
    let context = repository_context_for_worktree(cwd.to_path_buf(), GIT_REMOTE_ERROR)?;
    let mut operation = begin_git_operation(operation_id, GIT_REMOTE_ERROR, GIT_REMOTE_BUSY)?;
    operation.bind_repository(
        context.common_git_identity,
        GIT_REMOTE_ERROR,
        GIT_REMOTE_BUSY,
    )?;
    run_registered_remote_operation(
        &context,
        action,
        operation_id,
        operation.cancellation.as_ref(),
        before_final_recheck,
        before_spawn,
    )
}

fn git_common_dir_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "rev-parse".to_string(),
        "--git-common-dir".to_string(),
    ]
}

fn repository_context_for_worktree(
    worktree: PathBuf,
    error: &'static str,
) -> Result<RepositoryContext, String> {
    repository_context_for_worktree_with_options(
        worktree,
        error,
        Duration::from_secs(5),
        None,
        None,
    )
}

/// Keep cancellation/deadline checks around the synchronous filesystem part
/// of repository identity validation. The cleanup command already runs this
/// whole operation on Tauri's bounded `spawn_blocking` pool; nested ad-hoc
/// threads would be unable to stop a blocked OS call and could leak workers.
fn repository_context_boundary(
    cancellation: Option<&AtomicBool>,
    deadline: Option<Instant>,
    error: &'static str,
) -> Result<(), String> {
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    if deadline.is_some_and(|value| Instant::now() >= value) {
        return Err(error.to_string());
    }
    Ok(())
}

fn repository_context_filesystem_error(
    cancellation: Option<&AtomicBool>,
    error: &'static str,
) -> String {
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        GIT_CLEANUP_CANCELLED.to_string()
    } else {
        error.to_string()
    }
}

fn repository_context_for_worktree_with_options(
    worktree: PathBuf,
    error: &'static str,
    timeout: Duration,
    cancellation: Option<&AtomicBool>,
    deadline: Option<Instant>,
) -> Result<RepositoryContext, String> {
    repository_context_boundary(cancellation, deadline, error)?;
    let worktree_identity = filesystem_identity(&worktree, true)
        .map_err(|_| repository_context_filesystem_error(cancellation, error))?;
    repository_context_boundary(cancellation, deadline, error)?;
    let args = git_common_dir_args();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(&worktree, error)?;
    let output = match cancellation {
        Some(signal) => devbox_git::run_bounded_target_with_cancel(
            &args,
            &target,
            timeout,
            MAX_REPOSITORY_PATH_BYTES + 2,
            signal,
        ),
        None => {
            devbox_git::run_bounded_target(&args, &target, timeout, MAX_REPOSITORY_PATH_BYTES + 2)
        }
    }
    .map_err(|runner_error| {
        if cancellation.is_some() && runner_error == "git_cancelled" {
            GIT_CLEANUP_CANCELLED.to_string()
        } else {
            error.to_string()
        }
    })?;
    repository_context_boundary(cancellation, deadline, error)?;
    let value = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .ok_or_else(|| error.to_string())?;
    if value.is_empty()
        || value.len() > MAX_REPOSITORY_PATH_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(error.to_string());
    }
    let common = host_path_from_git(&worktree, value, error)?;
    let common = common
        .canonicalize()
        .map_err(|_| repository_context_filesystem_error(cancellation, error))?;
    repository_context_boundary(cancellation, deadline, error)?;
    if !common.is_dir() {
        return Err(error.to_string());
    }
    repository_context_boundary(cancellation, deadline, error)?;
    let common_git_identity = filesystem_identity(&common, true)
        .map_err(|_| repository_context_filesystem_error(cancellation, error))?;
    repository_context_boundary(cancellation, deadline, error)?;
    // The worktree may have been exchanged while `rev-parse` ran. Compare the
    // exact directory object again before returning an operation authority.
    let current_worktree_identity = filesystem_identity(&worktree, true)
        .map_err(|_| repository_context_filesystem_error(cancellation, error))?;
    repository_context_boundary(cancellation, deadline, error)?;
    if current_worktree_identity != worktree_identity {
        return Err(error.to_string());
    }
    Ok(RepositoryContext {
        worktree,
        worktree_identity,
        common_git_identity,
    })
}

fn validated_repository_context(
    path: &str,
    error: &'static str,
) -> Result<RepositoryContext, String> {
    let entry = validated_repository(path).map_err(|_| error.to_string())?;
    let worktree = Path::new(&entry.path)
        .canonicalize()
        .map_err(|_| error.to_string())?;
    repository_context_for_worktree(worktree, error)
}

/// Cleanup owns an operation cancellation token from the first blocking
/// validation step. Keep the synchronous canonicalize/metadata calls inside
/// the same bounded worker and check the shared operation boundary before and
/// after each call; no per-call thread is created because a detached worker
/// cannot safely interrupt an OS filesystem syscall.
fn cleanup_validated_repository_context(
    path: &str,
    error: &'static str,
    cancellation: &AtomicBool,
    deadline: Instant,
) -> Result<RepositoryContext, String> {
    repository_context_boundary(Some(cancellation), Some(deadline), error)?;
    if !valid_repository_path_syntax(path) {
        return Err(error.to_string());
    }
    let worktree = Path::new(path)
        .canonicalize()
        .map_err(|_| repository_context_filesystem_error(Some(cancellation), error))?;
    repository_context_boundary(Some(cancellation), Some(deadline), error)?;
    if !worktree.is_dir() || !worktree.join(".git").exists() {
        return Err(error.to_string());
    }
    repository_context_boundary(Some(cancellation), Some(deadline), error)?;
    let timeout = cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
        if error == GIT_CLEANUP_CANCELLED {
            error
        } else {
            GIT_CLEANUP_ERROR.to_string()
        }
    })?;
    repository_context_for_worktree_with_options(
        worktree,
        error,
        timeout,
        Some(cancellation),
        Some(deadline),
    )
}

fn revalidate_repository_context(
    expected: &RepositoryContext,
    error: &'static str,
) -> Result<(), String> {
    revalidate_repository_context_with_timeout(expected, error, Duration::from_secs(5))
}

fn revalidate_repository_context_with_timeout(
    expected: &RepositoryContext,
    error: &'static str,
    timeout: Duration,
) -> Result<(), String> {
    revalidate_repository_context_with_timeout_and_cancel(expected, error, timeout, None, None)
}

fn revalidate_repository_context_with_timeout_and_cancel(
    expected: &RepositoryContext,
    error: &'static str,
    timeout: Duration,
    cancellation: Option<&AtomicBool>,
    deadline: Option<Instant>,
) -> Result<(), String> {
    let current = repository_context_for_worktree_with_options(
        expected.worktree.clone(),
        error,
        timeout,
        cancellation,
        deadline,
    )?;
    if current.worktree_identity != expected.worktree_identity
        || current.common_git_identity != expected.common_git_identity
    {
        return Err(error.to_string());
    }
    Ok(())
}

fn validated_git_path(path: &str) -> Result<std::path::PathBuf, String> {
    let entry = validated_repository(path).map_err(|_| GIT_VIEW_ERROR.to_string())?;
    Path::new(&entry.path)
        .canonicalize()
        .map_err(|_| GIT_VIEW_ERROR.to_string())
}

fn git_history_args(limit: usize) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "log".to_string(),
        "--topo-order".to_string(),
        format!("--max-count={}", limit.saturating_add(1)),
        "--date=iso-strict".to_string(),
        "--encoding=UTF-8".to_string(),
        "--no-decorate".to_string(),
        "--format=%H%x00%P%x00%aI%x00%an%x00%ae%x00%s%x00".to_string(),
    ]
}

fn git_detail_args(commit_id: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "show".to_string(),
        "--no-patch".to_string(),
        "--no-ext-diff".to_string(),
        "--no-textconv".to_string(),
        "--no-color".to_string(),
        "--date=iso-strict".to_string(),
        "--encoding=UTF-8".to_string(),
        "--format=%H%x00%P%x00%aI%x00%an%x00%ae%x00%s%x00%b%x00".to_string(),
        commit_id.to_string(),
        "--".to_string(),
    ]
}

fn git_working_tree_diff_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "diff".to_string(),
        "HEAD".to_string(),
        "--no-ext-diff".to_string(),
        "--no-textconv".to_string(),
        "--no-color".to_string(),
        "--no-renames".to_string(),
        "--unified=3".to_string(),
        "--".to_string(),
    ]
}

fn git_commit_diff_args(commit_id: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "-c".to_string(),
        "core.quotePath=false".to_string(),
        "show".to_string(),
        "--format=".to_string(),
        "--no-ext-diff".to_string(),
        "--no-textconv".to_string(),
        "--no-color".to_string(),
        "--no-renames".to_string(),
        "-m".to_string(),
        "--unified=3".to_string(),
        commit_id.to_string(),
        "--".to_string(),
    ]
}

fn git_status_changes_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "--untracked-files=all".to_string(),
        "-z".to_string(),
        "--".to_string(),
    ]
}

fn git_stage_args(paths: &[String]) -> Vec<String> {
    let mut args = vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "--literal-pathspecs".to_string(),
        "add".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    args
}

fn git_unstage_args_for_head(paths: &[String], has_head: bool) -> Vec<String> {
    let mut args = vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "--literal-pathspecs".to_string(),
    ];
    if has_head {
        args.push("restore".to_string());
        args.push("--staged".to_string());
    } else {
        // An unborn repository has no HEAD for `restore --staged` to read.
        // `rm --cached` removes only the selected index entries and leaves
        // their worktree files untouched, which is the same unstage intent.
        args.push("rm".to_string());
        args.push("--cached".to_string());
        args.push("--ignore-unmatch".to_string());
    }
    args.push("--".to_string());
    args.extend(paths.iter().cloned());
    args
}

fn git_head_args() -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "rev-parse".to_string(),
        "--verify".to_string(),
        "HEAD".to_string(),
    ]
}

fn repository_has_head(cwd: &Path, cancellation: &AtomicBool) -> Result<bool, String> {
    let args = git_head_args();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let target = git_target_for_path(cwd, GIT_MUTATION_ERROR)?;
    match devbox_git::run_bounded_target_with_cancel(
        &args,
        &target,
        MUTATION_TIMEOUT,
        128,
        cancellation,
    ) {
        Ok(_) => Ok(true),
        Err(error) if error == "git_failed" => Ok(false),
        Err(_) => Err(GIT_MUTATION_ERROR.to_string()),
    }
}

fn git_commit_args(message: &str) -> Vec<String> {
    vec![
        "--no-pager".to_string(),
        "--no-optional-locks".to_string(),
        "commit".to_string(),
        "--message".to_string(),
        message.to_string(),
        "--".to_string(),
    ]
}

fn validated_selected_paths(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() || paths.len() > MAX_SELECTED_PATHS {
        return Err(GIT_MUTATION_ERROR.to_string());
    }
    let mut seen = HashSet::with_capacity(paths.len());
    let mut validated = Vec::with_capacity(paths.len());
    for path in paths {
        let path = validate_change_path(path)?;
        if seen.insert(path.clone()) {
            validated.push(path);
        }
    }
    if validated.is_empty() {
        return Err(GIT_MUTATION_ERROR.to_string());
    }
    Ok(validated)
}

/// Confirm that every requested path still belongs to the corresponding side
/// of the current status snapshot. This keeps a stale or hand-crafted IPC
/// request from turning the selected-file operation into an arbitrary path
/// operation, and rejects the whole batch before Git can partially apply it.
fn resolve_current_selection(
    cwd: &Path,
    paths: &[String],
    require_staged: bool,
    cancellation: &AtomicBool,
) -> Result<Vec<String>, String> {
    let text = run_git_status_bounded_with_cancel(&git_status_changes_args(), cwd, cancellation)?;
    let changes = parse_status_changes(&text)?;
    let available = changes
        .into_iter()
        .filter(|change| {
            if require_staged {
                change.staged
            } else {
                change.unstaged
            }
        })
        .map(|change| (change.path.clone(), change))
        .collect::<HashMap<_, _>>();
    let mut expanded = Vec::with_capacity(paths.len().saturating_mul(2));
    let mut seen = HashSet::with_capacity(paths.len().saturating_mul(2));
    for path in paths {
        let change = available
            .get(path)
            .ok_or_else(|| GIT_MUTATION_ERROR.to_string())?;
        if seen.insert(change.path.clone()) {
            expanded.push(change.path.clone());
        }
        // Porcelain -z reports the new side first. A selected rename must
        // include its old path as a literal pathspec as well, otherwise Git
        // can stage/unstage only half of the rename and leave a deletion
        // behind. Copies intentionally retain their unchanged source.
        if change.kind == "renamed" {
            let old_path = change
                .old_path
                .as_ref()
                .ok_or_else(|| GIT_MUTATION_ERROR.to_string())?;
            if seen.insert(old_path.clone()) {
                expanded.push(old_path.clone());
            }
        }
    }
    Ok(expanded)
}

#[tauri::command]
pub async fn repo_status(path: String) -> Result<RepoSnapshot, String> {
    spawn_git_task(GIT_STATUS_ERROR, move || {
        let worktree = validated_git_path(&path).map_err(|_| GIT_STATUS_ERROR.to_string())?;
        let args = [
            "--no-pager",
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--branch",
            "--",
        ];
        let cwd = host_path_spelling(&worktree, GIT_STATUS_ERROR)?;
        let target = git_target_for_path(&worktree, GIT_STATUS_ERROR)?;
        let status = devbox_git::run_bounded_target(
            &args,
            &target,
            Duration::from_secs(5),
            MAX_STATUS_OUTPUT_BYTES,
        )
        .map_err(|_| GIT_STATUS_ERROR.to_string())?;
        Ok(parse_status(&cwd, &status))
    })
    .await
}

/// Read-only request for the selected repository's bounded Git safety state.
/// No remote, branch, recovery, or destructive action is represented by this
/// request.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepoPreflightRequest {
    pub path: String,
}

/// Detect local state that a remote operation should not silently override.
/// The status and marker reads are independently bounded and every failure is
/// mapped to the same redacted error. This command never changes repository
/// files, refs, index state, remotes, or credentials.
#[tauri::command]
pub async fn repo_preflight(request: RepoPreflightRequest) -> Result<GitSafetySnapshot, String> {
    spawn_git_task(GIT_SAFETY_ERROR, move || {
        let path = validated_git_path(&request.path).map_err(|_| GIT_SAFETY_ERROR.to_string())?;
        let status_text =
            run_git_safety_bounded(&git_safety_status_args(), &path, MAX_SAFETY_OUTPUT_BYTES)?;
        let parsed = parse_porcelain_v2(&status_text)?;
        let marker_text = run_git_safety_bounded(
            &git_safety_marker_args(),
            &path,
            MAX_SAFETY_METADATA_OUTPUT_BYTES,
        )?;
        let [rebase_merge, rebase_apply, merge_head] =
            parse_safety_marker_paths(&marker_text, &path)?;
        let rebase_in_progress = marker_present(&rebase_merge)? || marker_present(&rebase_apply)?;
        let merge_in_progress = marker_present(&merge_head)?;
        Ok(classify(parsed, rebase_in_progress, merge_in_progress))
    })
    .await
}

#[tauri::command]
pub async fn worktrees(path: String) -> Result<Vec<String>, String> {
    spawn_git_task(GIT_WORKTREE_ERROR, move || {
        let worktree = validated_git_path(&path).map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
        let args = [
            "--no-pager",
            "--no-optional-locks",
            "worktree",
            "list",
            "--porcelain",
        ];
        let target = git_target_for_path(&worktree, GIT_WORKTREE_ERROR)?;
        let out = devbox_git::run_bounded_target(
            &args,
            &target,
            Duration::from_secs(5),
            MAX_WORKTREE_OUTPUT_BYTES,
        )
        .map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
        let paths = parse_worktrees(&out)
            .into_iter()
            .map(|path| {
                target
                    .host_path_from_git(&path)
                    .map_err(|_| GIT_WORKTREE_ERROR.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let entries = paths
            .iter()
            .filter_map(|path| repository_entry(Path::new(path)).ok())
            .collect::<Vec<_>>();
        crate::integration::add_worktree_repositories(entries);
        Ok(paths)
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreate {
    pub path: String,
}

#[tauri::command]
pub async fn create_worktree(
    repo_path: String,
    branch: String,
    target_dir: String,
) -> Result<WorktreeCreate, String> {
    spawn_git_task(GIT_WORKTREE_ERROR, move || {
        if branch.len() > MAX_REMOTE_BRANCH_BYTES
            || branch.starts_with('-')
            || !valid_ref_path_fragment(&branch)
        {
            return Err(GIT_WORKTREE_ERROR.to_string());
        }
        let (target, target_parent_identity) = validated_new_worktree_target(&target_dir)?;
        let context = validated_repository_context(&repo_path, GIT_WORKTREE_ERROR)?;
        let mut operation = begin_internal_git_operation(GIT_WORKTREE_ERROR)?;
        operation.bind_repository(
            context.common_git_identity,
            GIT_WORKTREE_ERROR,
            GIT_WORKTREE_ERROR,
        )?;
        revalidate_repository_context(&context, GIT_WORKTREE_ERROR)?;
        let target_parent = target
            .parent()
            .ok_or_else(|| GIT_WORKTREE_ERROR.to_string())?;
        if filesystem_identity(target_parent, true).map_err(|_| GIT_WORKTREE_ERROR.to_string())?
            != target_parent_identity
            || target
                .try_exists()
                .map_err(|_| GIT_WORKTREE_ERROR.to_string())?
        {
            return Err(GIT_WORKTREE_ERROR.to_string());
        }
        let target_arg = git_path_from_host(&context.worktree, &target, GIT_WORKTREE_ERROR)?;
        let result_path = host_path_spelling(&target, GIT_WORKTREE_ERROR)?;
        let args = vec![
            "--no-pager".to_string(),
            "--no-optional-locks".to_string(),
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            branch,
            "--".to_string(),
            target_arg,
        ];
        run_git_mutation(&args, &context.worktree)?;
        Ok(WorktreeCreate { path: result_path })
    })
    .await
}

/// remove 전 uncommitted/untracked 검사. 없으면 true.
#[tauri::command]
pub async fn worktree_clean(path: String) -> Result<bool, String> {
    spawn_git_task(GIT_WORKTREE_ERROR, move || {
        let worktree = validated_git_path(&path).map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
        let status = run_git_status_bounded(&git_status_changes_args(), &worktree)
            .map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
        Ok(status.is_empty())
    })
    .await
}

/// Read-only cleanup preview.  The preview lists every local branch and
/// worktree, but marks only conservative merged/stale or linked-worktree
/// candidates as eligible.  No branch, ref, index, or worktree is changed.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupPreviewRequest {
    pub path: String,
    /// Frontend-owned opaque ID so an unmounted panel can cancel a long
    /// metadata/status observation before its 30-second total budget elapses.
    pub operation_id: String,
}

/// Explicit cleanup selection.  `branch_names` and `worktree_paths` must be
/// copied from the latest preview; the backend matches them against a fresh
/// snapshot and rejects stale or hand-crafted targets before Git runs.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CleanupRequest {
    pub path: String,
    pub branch_names: Vec<String>,
    pub worktree_paths: Vec<String>,
    pub preview_revision: String,
    pub operation_id: String,
}

fn cleanup_selection_path_is_bounded(value: &str) -> bool {
    valid_cleanup_worktree_arg(value)
}

fn cleanup_selection_branch_is_bounded(value: &str) -> bool {
    value.len() <= MAX_CLEANUP_REF_BYTES && valid_ref_name(value)
}

fn selected_worktree_index(
    requested: &str,
    observation: &CleanupObservation,
) -> Result<usize, String> {
    if !cleanup_selection_path_is_bounded(requested) {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    let exact = observation
        .preview
        .worktrees
        .iter()
        .enumerate()
        .filter_map(|(index, worktree)| (worktree.path == requested).then_some(index))
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    // The preview returns the canonical display path.  An explicit path alias
    // is accepted only when its final filesystem identity is exactly one of
    // the preview objects; a symlink/reparse point fails closed.
    let requested_identity = filesystem_identity(Path::new(requested), true)
        .map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let matches = observation
        .worktree_identities
        .iter()
        .enumerate()
        .filter_map(|(index, identity)| (*identity == Some(requested_identity)).then_some(index))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches[0])
    } else {
        Err(GIT_CLEANUP_STATE_CHANGED.to_string())
    }
}

fn cleanup_result_item(
    kind: &str,
    target: &str,
    outcome: &str,
    reason: Option<&str>,
) -> CleanupItemResult {
    CleanupItemResult {
        kind: kind.to_string(),
        target: target.to_string(),
        outcome: outcome.to_string(),
        reason: reason.map(str::to_string),
    }
}

fn cleanup_worktree_still_safe(
    context: &RepositoryContext,
    expected: &WorktreeCleanupEntry,
    expected_identity: FilesystemIdentity,
    expected_current_head: Option<&str>,
    cancellation: &AtomicBool,
    parent_deadline: Instant,
) -> Result<(), String> {
    let local_deadline = Instant::now()
        .checked_add(CLEANUP_REVALIDATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let deadline = local_deadline.min(parent_deadline);
    cleanup_revalidate_context(context, cancellation, deadline)?;
    let ensure_budget = || {
        cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
            if error == GIT_CLEANUP_CANCELLED {
                error
            } else {
                GIT_CLEANUP_STATE_CHANGED.to_string()
            }
        })
    };
    ensure_budget()?;
    let expected_path = Path::new(&expected.path);
    let (canonical, identity) = resolve_cleanup_worktree_path(&ParsedWorktree {
        path: expected_path.to_string_lossy().into_owned(),
        head: None,
        branch: None,
        locked: false,
        prunable: false,
        bare: false,
    })
    .map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    ensure_budget()?;
    if host_path_spelling(&canonical, GIT_CLEANUP_STATE_CHANGED)? != expected.path
        || identity != expected_identity
    {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }

    let status_text = run_git_cleanup_bounded_with_timeout(
        &git_cleanup_status_args(),
        &canonical,
        cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
            if error == GIT_CLEANUP_CANCELLED {
                error
            } else {
                GIT_CLEANUP_STATE_CHANGED.to_string()
            }
        })?,
        Some(cancellation),
    )
    .map_err(|error| {
        if error == GIT_CLEANUP_CANCELLED {
            error
        } else {
            GIT_CLEANUP_STATE_CHANGED.to_string()
        }
    })?;
    let status =
        parse_worktree_status(&status_text).map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    if status.dirty || status.untracked || status.ignored {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }

    let records_text = run_git_cleanup_bounded_with_timeout(
        &git_cleanup_worktree_args(),
        &context.worktree,
        cleanup_remaining_timeout(deadline, Some(cancellation)).map_err(|error| {
            if error == GIT_CLEANUP_CANCELLED {
                error
            } else {
                GIT_CLEANUP_STATE_CHANGED.to_string()
            }
        })?,
        Some(cancellation),
    )
    .map_err(|error| {
        if error == GIT_CLEANUP_CANCELLED {
            error
        } else {
            GIT_CLEANUP_STATE_CHANGED.to_string()
        }
    })?;
    let records =
        parse_host_worktree_records(&records_text, &context.worktree, GIT_CLEANUP_STATE_CHANGED)?;
    let mut matching = None;
    let mut context_worktree_head = None;
    let mut context_worktree_count = 0usize;
    for (index, record) in records.iter().enumerate() {
        ensure_budget()?;
        if let Ok((record_path, record_identity)) = resolve_cleanup_worktree_path(record) {
            if record_identity == context.worktree_identity {
                context_worktree_count = context_worktree_count.saturating_add(1);
                context_worktree_head = record.head.clone();
            }
            if record_path == canonical && matching.replace((index, record)).is_some() {
                return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
            }
        }
    }
    let Some((index, record)) = matching else {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    };
    if context_worktree_count != 1
        || context_worktree_head.as_deref() != expected_current_head
        || (index == 0) != expected.is_main
        || record.head != expected.head
        || record.branch != expected.branch
        || record.bare != expected.bare
        || record.locked != expected.locked
        || record.prunable != expected.prunable
        || index == 0
        || record.bare
        || record.locked
        || record.prunable
    {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }
    cleanup_revalidate_context(context, cancellation, deadline)?;
    // Keep the selected worktree identity as the last filesystem observation
    // before the remove child is spawned. This does not make an OS-level
    // rename race atomic, but closes the window opened by the metadata read
    // and ensures a replacement cannot be carried forward from an earlier
    // status check.
    ensure_budget()?;
    let (final_canonical, final_identity) = resolve_cleanup_worktree_path(&ParsedWorktree {
        path: expected_path.to_string_lossy().into_owned(),
        head: None,
        branch: None,
        locked: false,
        prunable: false,
        bare: false,
    })
    .map_err(|_| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    if host_path_spelling(&final_canonical, GIT_CLEANUP_STATE_CHANGED)? != expected.path
        || final_identity != expected_identity
    {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }
    Ok(())
}

enum CleanupAction {
    Branch {
        expected: BranchCleanupEntry,
        current_head: Option<String>,
    },
    Worktree {
        expected: WorktreeCleanupEntry,
        identity: FilesystemIdentity,
        current_head: Option<String>,
    },
}

fn run_cleanup_request(
    request: CleanupRequest,
    operation: GitOperationGuard,
) -> Result<CleanupResult, String> {
    let mut operation = operation;
    if operation.cancellation.load(Ordering::Acquire) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    let context_validation_deadline = Instant::now()
        .checked_add(CLEANUP_OBSERVATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
    let context = cleanup_validated_repository_context(
        &request.path,
        GIT_CLEANUP_ERROR,
        operation.cancellation.as_ref(),
        context_validation_deadline,
    )?;
    if operation.cancellation.load(Ordering::Acquire) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    operation.bind_repository(
        context.common_git_identity,
        GIT_CLEANUP_ERROR,
        GIT_CLEANUP_BUSY,
    )?;
    let context_deadline = Instant::now()
        .checked_add(CLEANUP_REVALIDATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
    let context_timeout =
        cleanup_remaining_timeout(context_deadline, Some(operation.cancellation.as_ref()))
            .map_err(|error| {
                if error == GIT_CLEANUP_CANCELLED {
                    error
                } else {
                    GIT_CLEANUP_ERROR.to_string()
                }
            })?;
    revalidate_repository_context_with_timeout_and_cancel(
        &context,
        GIT_CLEANUP_ERROR,
        context_timeout,
        Some(operation.cancellation.as_ref()),
        Some(context_deadline),
    )?;
    if operation.cancellation.load(Ordering::Acquire) {
        return Err(GIT_CLEANUP_CANCELLED.to_string());
    }
    let observation = collect_cleanup_observation(&context, Some(operation.cancellation.as_ref()))?;
    if observation.preview.revision != request.preview_revision {
        return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
    }

    let mut results = Vec::new();
    let mut actions = Vec::new();
    let mut selected_branches = HashSet::with_capacity(request.branch_names.len());
    for name in &request.branch_names {
        if !cleanup_selection_branch_is_bounded(name) || !selected_branches.insert(name.clone()) {
            return Err(GIT_CLEANUP_ERROR.to_string());
        }
        let Some(branch) = observation
            .preview
            .branches
            .iter()
            .find(|branch| branch.name == *name)
        else {
            return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
        };
        if !branch.eligible {
            let reason = branch
                .blocked
                .first()
                .map(String::as_str)
                .unwrap_or("notCandidate");
            results.push(cleanup_result_item("branch", name, "blocked", Some(reason)));
        } else {
            actions.push(CleanupAction::Branch {
                expected: branch.clone(),
                current_head: observation.preview.current_head.clone(),
            });
        }
    }

    let mut selected_worktrees = HashSet::with_capacity(request.worktree_paths.len());
    for requested in &request.worktree_paths {
        if !selected_worktrees.insert(requested.clone()) {
            return Err(GIT_CLEANUP_ERROR.to_string());
        }
        let index = selected_worktree_index(requested, &observation)?;
        let worktree = &observation.preview.worktrees[index];
        if !worktree.eligible {
            let reason = worktree
                .blocked
                .first()
                .map(String::as_str)
                .unwrap_or("notCandidate");
            results.push(cleanup_result_item(
                "worktree",
                &worktree.path,
                "blocked",
                Some(reason),
            ));
        } else {
            let identity = observation
                .worktree_identities
                .get(index)
                .and_then(|identity| *identity)
                .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
            actions.push(CleanupAction::Worktree {
                expected: worktree.clone(),
                identity,
                current_head: observation.preview.current_head.clone(),
            });
        }
    }

    // Never partially apply an explicit selection that contains a blocked
    // target.  The result tells the UI which precondition stopped the batch.
    if !results.is_empty() {
        return Ok(CleanupResult {
            preview_revision: observation.preview.revision,
            attempted: 0,
            removed: 0,
            items: results,
        });
    }

    let mutation_deadline = Instant::now()
        .checked_add(CLEANUP_MUTATION_BUDGET)
        .ok_or_else(|| GIT_CLEANUP_STATE_CHANGED.to_string())?;
    let mut attempted = 0u32;
    let mut removed = 0u32;
    for action in actions {
        cleanup_remaining_mutation_timeout(mutation_deadline, operation.cancellation.as_ref())?;
        cleanup_revalidate_context(&context, operation.cancellation.as_ref(), mutation_deadline)?;
        match action {
            CleanupAction::Branch {
                expected,
                current_head,
            } => {
                cleanup_branch_still_safe(
                    &context,
                    &expected,
                    current_head.as_deref(),
                    operation.cancellation.as_ref(),
                    mutation_deadline,
                )?;
                let timeout = cleanup_remaining_mutation_timeout(
                    mutation_deadline,
                    operation.cancellation.as_ref(),
                )?;
                let name = expected.name;
                attempted = attempted.saturating_add(1);
                let result = run_git_cleanup_mutation_with_cancel(
                    &git_cleanup_delete_branch_args(&name),
                    &context.worktree,
                    operation.cancellation.as_ref(),
                    timeout,
                );
                match result {
                    Ok(()) => {
                        removed = removed.saturating_add(1);
                        results.push(cleanup_result_item("branch", &name, "removed", None));
                    }
                    Err(error) if error == GIT_CLEANUP_CANCELLED => return Err(error),
                    Err(_) => {
                        results.push(cleanup_result_item(
                            "branch",
                            &name,
                            "failed",
                            Some("gitFailed"),
                        ));
                        break;
                    }
                }
            }
            CleanupAction::Worktree {
                expected,
                identity,
                current_head,
            } => {
                cleanup_worktree_still_safe(
                    &context,
                    &expected,
                    identity,
                    current_head.as_deref(),
                    operation.cancellation.as_ref(),
                    mutation_deadline,
                )?;
                let timeout = cleanup_remaining_mutation_timeout(
                    mutation_deadline,
                    operation.cancellation.as_ref(),
                )?;
                let path = PathBuf::from(&expected.path);
                let git_path = PathBuf::from(git_path_from_host(
                    &context.worktree,
                    &path,
                    GIT_CLEANUP_STATE_CHANGED,
                )?);
                attempted = attempted.saturating_add(1);
                let result = run_git_cleanup_mutation_with_cancel(
                    &git_cleanup_remove_worktree_args(&git_path),
                    &context.worktree,
                    operation.cancellation.as_ref(),
                    timeout,
                );
                match result {
                    Ok(()) => {
                        removed = removed.saturating_add(1);
                        let target = path.to_string_lossy().into_owned();
                        results.push(cleanup_result_item("worktree", &target, "removed", None));
                    }
                    Err(error) if error == GIT_CLEANUP_CANCELLED => return Err(error),
                    Err(_) => {
                        let target = path.to_string_lossy().into_owned();
                        results.push(cleanup_result_item(
                            "worktree",
                            &target,
                            "failed",
                            Some("gitFailed"),
                        ));
                        break;
                    }
                }
            }
        }
    }
    Ok(CleanupResult {
        preview_revision: request.preview_revision,
        attempted,
        removed,
        items: results,
    })
}

#[tauri::command]
pub async fn repo_cleanup_preview(
    request: CleanupPreviewRequest,
) -> Result<CleanupPreview, String> {
    let operation =
        begin_git_operation(&request.operation_id, GIT_CLEANUP_ERROR, GIT_CLEANUP_BUSY)?;
    spawn_git_task(GIT_CLEANUP_ERROR, move || {
        let mut operation = operation;
        if operation.cancellation.load(Ordering::Acquire) {
            return Err(GIT_CLEANUP_CANCELLED.to_string());
        }
        let context_validation_deadline = Instant::now()
            .checked_add(CLEANUP_OBSERVATION_BUDGET)
            .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
        let context = cleanup_validated_repository_context(
            &request.path,
            GIT_CLEANUP_ERROR,
            operation.cancellation.as_ref(),
            context_validation_deadline,
        )?;
        if operation.cancellation.load(Ordering::Acquire) {
            return Err(GIT_CLEANUP_CANCELLED.to_string());
        }
        operation.bind_repository(
            context.common_git_identity,
            GIT_CLEANUP_ERROR,
            GIT_CLEANUP_BUSY,
        )?;
        let context_deadline = Instant::now()
            .checked_add(CLEANUP_REVALIDATION_BUDGET)
            .ok_or_else(|| GIT_CLEANUP_ERROR.to_string())?;
        let context_timeout =
            cleanup_remaining_timeout(context_deadline, Some(operation.cancellation.as_ref()))
                .map_err(|error| {
                    if error == GIT_CLEANUP_CANCELLED {
                        error
                    } else {
                        GIT_CLEANUP_ERROR.to_string()
                    }
                })?;
        revalidate_repository_context_with_timeout_and_cancel(
            &context,
            GIT_CLEANUP_ERROR,
            context_timeout,
            Some(operation.cancellation.as_ref()),
            Some(context_deadline),
        )?;
        if operation.cancellation.load(Ordering::Acquire) {
            return Err(GIT_CLEANUP_CANCELLED.to_string());
        }
        Ok(collect_cleanup_observation(&context, Some(operation.cancellation.as_ref()))?.preview)
    })
    .await
}

#[tauri::command]
pub async fn repo_cleanup(request: CleanupRequest) -> Result<CleanupResult, String> {
    if request.branch_names.len() > MAX_CLEANUP_SELECTIONS
        || request.worktree_paths.len() > MAX_CLEANUP_SELECTIONS
        || (request.branch_names.is_empty() && request.worktree_paths.is_empty())
        || request
            .branch_names
            .len()
            .saturating_add(request.worktree_paths.len())
            > MAX_CLEANUP_SELECTIONS
        || request
            .branch_names
            .iter()
            .any(|name| !cleanup_selection_branch_is_bounded(name))
        || request
            .worktree_paths
            .iter()
            .any(|path| !cleanup_selection_path_is_bounded(path))
        || !valid_revision(&request.preview_revision)
    {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    let operation =
        begin_git_operation(&request.operation_id, GIT_CLEANUP_ERROR, GIT_CLEANUP_BUSY)?;
    spawn_git_task(GIT_CLEANUP_ERROR, move || {
        run_cleanup_request(request, operation)
    })
    .await
}

#[tauri::command]
pub fn repo_cleanup_cancel(request: RemoteCancelRequest) -> Result<bool, String> {
    if !valid_remote_operation_id(&request.operation_id) {
        return Err(GIT_CLEANUP_ERROR.to_string());
    }
    Ok(cancel_git_operation(&request.operation_id))
}

/// Read-only history request. `limit` is intentionally part of the typed
/// request so a caller cannot turn the history panel into an unbounded log
/// exporter.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    pub path: String,
    pub limit: usize,
}

/// Read-only commit metadata request. Only hexadecimal object IDs are
/// accepted; arbitrary rev expressions and pathspecs never reach Git.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitDetailRequest {
    pub path: String,
    pub commit_id: String,
}

/// `commit_id = null` means `HEAD` versus the current index/worktree. A value
/// means the selected commit's patch. Stage/unstage/commit/remote actions are
/// deliberately not represented by this request.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffRequest {
    pub path: String,
    pub commit_id: Option<String>,
}

/// An explicit, read-only dependency inventory for one already selected
/// repository. The command accepts no package-manager command, glob, or
/// format override and therefore cannot widen the scanner's fixed allowlist.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyInventoryRequest {
    pub path: String,
}

#[tauri::command]
pub async fn dependency_inventory(
    request: DependencyInventoryRequest,
) -> Result<DependencyReport, String> {
    spawn_git_task(DEPENDENCY_LENS_ERROR, move || {
        let _analysis = dependency_analysis_lock()
            .try_lock()
            .map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
        let context = validated_repository_context(&request.path, DEPENDENCY_LENS_ERROR)?;
        let repository =
            repository_entry(&context.worktree).map_err(|_| DEPENDENCY_LENS_ERROR.to_string())?;
        let mut report = analyze_repository(&context.worktree, Duration::from_secs(10))?;
        revalidate_repository_context(&context, DEPENDENCY_LENS_ERROR)?;

        // Publishing is derived-state best effort: a corrupt/unsafe snapshot
        // must not hide the successfully parsed local inventory, and it must
        // never be overwritten with a partial replacement.
        let now_ms = now_epoch_ms();
        let published = dependency_summary_entry(&repository.canonical_key, &report, now_ms)
            .and_then(|entry| {
                let _write = dependency_summary_write_lock()
                    .lock()
                    .map_err(|_| "dependency summary writer를 사용할 수 없습니다".to_string())?;
                publish_summary_in(&devbox_integration::integration_root(), entry, now_ms)
            })
            .is_ok();
        report.summary_published = published;
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn repo_history(request: HistoryRequest) -> Result<HistoryResult, String> {
    if !(1..=MAX_HISTORY_LIMIT).contains(&request.limit) {
        return Err(GIT_VIEW_ERROR.to_string());
    }
    spawn_git_task(GIT_VIEW_ERROR, move || {
        let path = validated_git_path(&request.path)?;
        let text = run_git_bounded(
            &git_history_args(request.limit),
            &path,
            MAX_HISTORY_OUTPUT_BYTES,
        )?;
        parse_history(&text, request.limit)
    })
    .await
}

#[tauri::command]
pub async fn repo_commit_detail(request: CommitDetailRequest) -> Result<CommitDetail, String> {
    let commit_id = validate_commit_id(&request.commit_id)?;
    spawn_git_task(GIT_VIEW_ERROR, move || {
        let path = validated_git_path(&request.path)?;
        let text = run_git_bounded(&git_detail_args(&commit_id), &path, MAX_DETAIL_OUTPUT_BYTES)?;
        parse_detail(&text)
    })
    .await
}

#[tauri::command]
pub async fn repo_diff(request: DiffRequest) -> Result<DiffResult, String> {
    let (args, scope, commit_id) = match request.commit_id {
        Some(value) => {
            let commit_id = validate_commit_id(&value)?;
            (git_commit_diff_args(&commit_id), "commit", Some(commit_id))
        }
        None => (git_working_tree_diff_args(), "workingTree", None),
    };
    spawn_git_task(GIT_VIEW_ERROR, move || {
        let path = validated_git_path(&request.path)?;
        let text = run_git_bounded(&args, &path, MAX_DIFF_OUTPUT_BYTES)?;
        parse_diff(&text, scope, commit_id, false)
    })
    .await
}

/// Request for the file-level staged/unstaged working-tree view.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepoChangesRequest {
    pub path: String,
}

/// Explicit selected paths to stage. Git receives only validated repository
/// relative paths after `--`; it never receives a frontend absolute path as a
/// pathspec.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StagePathsRequest {
    pub path: String,
    pub paths: Vec<String>,
    pub operation_id: String,
}

/// Explicit selected paths to unstage from the index.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnstagePathsRequest {
    pub path: String,
    pub paths: Vec<String>,
    pub operation_id: String,
}

/// Explicit commit request. The command commits the current index only; it
/// never adds all files implicitly and never stores credential material.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitRequest {
    pub path: String,
    pub message: String,
    pub operation_id: String,
}

#[tauri::command]
pub async fn repo_changes(request: RepoChangesRequest) -> Result<Vec<ChangeEntry>, String> {
    spawn_git_task(GIT_MUTATION_ERROR, move || {
        let path = validated_git_path(&request.path).map_err(|_| GIT_MUTATION_ERROR.to_string())?;
        let text = run_git_status_bounded(&git_status_changes_args(), &path)?;
        parse_status_changes(&text)
    })
    .await
}

#[tauri::command]
pub async fn repo_stage(request: StagePathsRequest) -> Result<(), String> {
    let paths = validated_selected_paths(&request.paths)?;
    let operation = begin_git_operation(
        &request.operation_id,
        GIT_MUTATION_ERROR,
        GIT_MUTATION_ERROR,
    )?;
    spawn_git_task(GIT_MUTATION_ERROR, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, GIT_MUTATION_ERROR)?;
        operation.bind_repository(
            context.common_git_identity,
            GIT_MUTATION_ERROR,
            GIT_MUTATION_ERROR,
        )?;
        let paths = resolve_current_selection(
            &context.worktree,
            &paths,
            false,
            operation.cancellation.as_ref(),
        )?;
        revalidate_repository_context(&context, GIT_MUTATION_ERROR)?;
        run_git_mutation_with_cancel(
            &git_stage_args(&paths),
            &context.worktree,
            operation.cancellation.as_ref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn repo_unstage(request: UnstagePathsRequest) -> Result<(), String> {
    let paths = validated_selected_paths(&request.paths)?;
    let operation = begin_git_operation(
        &request.operation_id,
        GIT_MUTATION_ERROR,
        GIT_MUTATION_ERROR,
    )?;
    spawn_git_task(GIT_MUTATION_ERROR, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, GIT_MUTATION_ERROR)?;
        operation.bind_repository(
            context.common_git_identity,
            GIT_MUTATION_ERROR,
            GIT_MUTATION_ERROR,
        )?;
        let paths = resolve_current_selection(
            &context.worktree,
            &paths,
            true,
            operation.cancellation.as_ref(),
        )?;
        let has_head = repository_has_head(&context.worktree, operation.cancellation.as_ref())?;
        revalidate_repository_context(&context, GIT_MUTATION_ERROR)?;
        run_git_mutation_with_cancel(
            &git_unstage_args_for_head(&paths, has_head),
            &context.worktree,
            operation.cancellation.as_ref(),
        )
    })
    .await
}

#[tauri::command]
pub async fn repo_commit(request: CommitRequest) -> Result<(), String> {
    let message = validate_commit_message(&request.message)?;
    let operation = begin_git_operation(
        &request.operation_id,
        GIT_MUTATION_ERROR,
        GIT_MUTATION_ERROR,
    )?;
    spawn_git_task(GIT_MUTATION_ERROR, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request.path, GIT_MUTATION_ERROR)?;
        operation.bind_repository(
            context.common_git_identity,
            GIT_MUTATION_ERROR,
            GIT_MUTATION_ERROR,
        )?;
        revalidate_repository_context(&context, GIT_MUTATION_ERROR)?;
        run_git_mutation_with_cancel(
            &git_commit_args(&message),
            &context.worktree,
            operation.cancellation.as_ref(),
        )
    })
    .await
}

/// Remote operations intentionally accept only a validated repository path.
/// Remote names, refspecs, URLs, credentials, and force flags are all owned by
/// Git configuration and are never supplied by the frontend.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteSyncRequest {
    pub path: String,
}

/// A mutating remote request carries a frontend-generated opaque operation ID.
/// The ID is never used as a path, refspec, or Git argument; it only lets the
/// cancel command address the exact in-flight native operation after a view or
/// repository has unmounted.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteOperationRequest {
    pub path: String,
    pub operation_id: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoteCancelRequest {
    pub operation_id: String,
}

async fn run_remote_request(
    request: RemoteOperationRequest,
    action: RemoteAction,
) -> Result<(), String> {
    // Register the opaque ID before the first await. An immediate UI cancel or
    // unmount therefore cannot race ahead of filesystem validation and lose
    // the cancellation signal.
    let operation = begin_git_operation(&request.operation_id, GIT_REMOTE_ERROR, GIT_REMOTE_BUSY)?;
    let operation_id = request.operation_id;
    let request_path = request.path;
    spawn_git_task(GIT_REMOTE_ERROR, move || {
        let mut operation = operation;
        let context = validated_repository_context(&request_path, GIT_REMOTE_ERROR)?;
        operation.bind_repository(
            context.common_git_identity,
            GIT_REMOTE_ERROR,
            GIT_REMOTE_BUSY,
        )?;
        run_registered_remote_operation(
            &context,
            action,
            &operation_id,
            operation.cancellation.as_ref(),
            || {},
            || {},
        )
    })
    .await
}

#[tauri::command]
pub async fn repo_remote_status(request: RemoteSyncRequest) -> Result<RemoteState, String> {
    spawn_git_task(GIT_REMOTE_ERROR, move || {
        let context = validated_repository_context(&request.path, GIT_REMOTE_ERROR)?;
        read_remote_state(&context, None, None)
    })
    .await
}

#[tauri::command]
pub async fn repo_fetch(request: RemoteOperationRequest) -> Result<(), String> {
    run_remote_request(request, RemoteAction::Fetch).await
}

#[tauri::command]
pub async fn repo_pull(request: RemoteOperationRequest) -> Result<(), String> {
    run_remote_request(request, RemoteAction::Pull).await
}

#[tauri::command]
pub async fn repo_push(request: RemoteOperationRequest) -> Result<(), String> {
    run_remote_request(request, RemoteAction::Push).await
}

/// Cancel an in-flight fetch/pull/push operation for this repository. The
/// operation remains owned by its original command until the child exits, so
/// a caller can safely ignore the result and rely on the command's fixed
/// cancellation error. No Git command is run by this handler.
#[tauri::command]
pub fn repo_remote_cancel(request: RemoteCancelRequest) -> Result<bool, String> {
    // Cancellation is addressed only by the opaque ID. It deliberately does
    // not re-canonicalize or touch the repository path, so unmount/deletion
    // during Git shutdown cannot make lookup fail.
    if !valid_remote_operation_id(&request.operation_id) {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    Ok(cancel_git_operation(&request.operation_id))
}

/// Cancel an in-flight selected stage/unstage/commit operation. The shared ID
/// registry also prevents a local and remote operation from reusing one ID or
/// mutating the same common Git directory concurrently.
#[tauri::command]
pub fn repo_local_cancel(request: RemoteCancelRequest) -> Result<bool, String> {
    if !valid_remote_operation_id(&request.operation_id) {
        return Err(GIT_MUTATION_ERROR.to_string());
    }
    Ok(cancel_git_operation(&request.operation_id))
}

fn available_open_targets() -> Vec<RepoOpenTarget> {
    select_repo_open_targets(
        "repo-manager",
        devbox_launch::installed_targets("path"),
        devbox_launch::installed_targets("workspace"),
    )
}

/// Catalog capability와 실제 설치 executable의 교집합만 반환한다. executable
/// 경로는 frontend에 노출하지 않는다.
#[tauri::command]
pub fn open_targets() -> Vec<RepoOpenTarget> {
    available_open_targets()
}

fn repository_entry(path: &Path) -> Result<RepoEntry, &'static str> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "repository를 찾을 수 없습니다")?;
    if !canonical.is_dir() || !canonical.join(".git").exists() {
        return Err("repository를 찾을 수 없습니다");
    }

    let display_path = path.to_string_lossy().into_owned();
    let canonical_key = devbox_wsl::path::canonical_project_key(Some(&display_path), None)
        .unwrap_or_else(|_| canonical.to_string_lossy().into_owned());
    Ok(RepoEntry {
        path: display_path,
        canonical_key,
        has_worktrees: canonical.join(".git").join("worktrees").is_dir(),
    })
}

fn validated_repository(path: &str) -> Result<RepoEntry, &'static str> {
    let raw = Path::new(path);
    if !valid_repository_path_syntax(path) {
        return Err("repository 경로가 올바르지 않습니다");
    }
    repository_entry(raw)
}

fn valid_repository_path_syntax(path: &str) -> bool {
    let raw = Path::new(path);
    if path.is_empty()
        || path.len() > MAX_REPOSITORY_PATH_BYTES
        || path.chars().any(char::is_control)
        || !raw.is_absolute()
        || is_device_path(path)
        || path
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "." | ".."))
    {
        return false;
    }
    true
}

fn validated_new_worktree_target(value: &str) -> Result<(PathBuf, FilesystemIdentity), String> {
    if !valid_repository_path_syntax(value) {
        return Err(GIT_WORKTREE_ERROR.to_string());
    }
    let raw = Path::new(value);
    let name = raw
        .file_name()
        .ok_or_else(|| GIT_WORKTREE_ERROR.to_string())?;
    let parent = raw
        .parent()
        .ok_or_else(|| GIT_WORKTREE_ERROR.to_string())?
        .canonicalize()
        .map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
    if !parent.is_dir() {
        return Err(GIT_WORKTREE_ERROR.to_string());
    }
    let parent_identity =
        filesystem_identity(&parent, true).map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
    let target = parent.join(name);
    if target
        .try_exists()
        .map_err(|_| GIT_WORKTREE_ERROR.to_string())?
    {
        return Err(GIT_WORKTREE_ERROR.to_string());
    }
    Ok((target, parent_identity))
}

/// Windows device namespaces are not ordinary repository paths. Reject their
/// spelling on every platform so an inbound string cannot change meaning when
/// it crosses the WSL/Windows boundary (`\\\\?\\`, `\\\\.\\`, `\\??\\`).
fn is_device_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.starts_with("//?/")
        || normalized.starts_with("//./")
        || normalized.starts_with("/??/")
}

/// Inbound Path를 임의 등록하거나 Git 명령을 실행하지 않고, 기존 목록 선택 또는
/// frontend 등록 초안에 쓸 검증된 metadata로만 변환한다.
#[tauri::command]
pub fn prepare_inbound_repository(path: String) -> Result<RepoEntry, String> {
    validated_repository(&path).map_err(str::to_string)
}

#[tauri::command]
pub fn open_in(app_id: String, path: String) -> Result<(), String> {
    let app_id = app_id.to_lowercase();
    let target = available_open_targets()
        .into_iter()
        .find(|target| target.id == app_id)
        .ok_or_else(|| "사용 가능한 대상 앱이 아닙니다".to_string())?;
    let path = validated_repository(&path).map_err(str::to_string)?.path;
    let req = target.request(path);
    devbox_launch::launch_open(&target.id, &req).map(|_| ())
}

/// 사용자가 명시적으로 복사를 선택한 순간에만 현재 Git repository 경로를 반환한다.
#[tauri::command]
pub fn repository_copy_path(path: String) -> Result<String, String> {
    validated_repository(&path)
        .map(|entry| entry.path)
        .map_err(str::to_string)
}

/// 현재도 유효한 Git repository만 OS file manager로 연다. opener 상세 오류와 raw path는
/// frontend error에 반향하지 않는다.
#[tauri::command]
pub fn open_repository_folder(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let repository = validated_repository(&path).map_err(str::to_string)?;
    app.opener()
        .open_path(repository.path, None::<&str>)
        .map_err(|_| "repository 폴더를 열 수 없습니다".to_string())
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn validated_repository_accepts_only_existing_absolute_git_directories() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let path = tmp.path().to_string_lossy().into_owned();

        let entry = validated_repository(&path).unwrap();
        assert_eq!(entry.path, path);
        assert!(!entry.canonical_key.is_empty());
        assert_eq!(
            validated_repository("relative/repository"),
            Err("repository 경로가 올바르지 않습니다")
        );
        assert_eq!(
            validated_repository(&format!("{path}/child/../repository")),
            Err("repository 경로가 올바르지 않습니다")
        );

        let non_repo = tempfile::tempdir().unwrap();
        assert_eq!(
            validated_repository(&non_repo.path().to_string_lossy()),
            Err("repository를 찾을 수 없습니다")
        );

        let secret = "repo-path-secret-must-not-appear";
        let error = validated_repository(&format!("{path}/{secret}"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "repository를 찾을 수 없습니다");
        assert!(!error.contains(secret));
    }

    #[test]
    fn rejects_windows_device_namespace_spellings_before_filesystem_lookup() {
        for path in [
            r"\\?\C:\projects\repo",
            r"\\.\PIPE\devbox",
            r"\??\C:\projects\repo",
        ] {
            assert!(is_device_path(path));
            assert_eq!(
                validated_repository(path),
                Err("repository 경로가 올바르지 않습니다")
            );
        }
    }

    #[test]
    fn trusted_canonical_windows_spelling_preserves_wsl_target_identity() {
        let unc = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\jihoon\Projects\DevBox");
        assert_eq!(
            host_path_spelling(unc, GIT_VIEW_ERROR).unwrap(),
            r"\\wsl.localhost\Ubuntu\home\jihoon\Projects\DevBox"
        );
        assert_eq!(
            git_target_for_path(unc, GIT_VIEW_ERROR).unwrap(),
            GitTarget::Wsl {
                distro: "Ubuntu".into(),
                cwd: "/home/jihoon/Projects/DevBox".into(),
            }
        );
        assert_eq!(
            host_path_spelling(Path::new(r"\\?\E:\Projects\DevBox"), GIT_VIEW_ERROR).unwrap(),
            r"E:\Projects\DevBox"
        );
        assert!(host_path_spelling(
            Path::new(r"\\?\Volume{00000000-0000-0000-0000-000000000000}\repo"),
            GIT_VIEW_ERROR,
        )
        .is_err());
    }

    #[test]
    fn read_only_history_detail_and_dirty_diff_smoke_use_real_git_output() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        for (key, value) in [("user.email", "smoke@example.test"), ("user.name", "Smoke")] {
            assert!(Command::new("git")
                .args(["config", key, value])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("fixture.txt"), "before\n").unwrap();
        fs::create_dir(repo.join("folder b")).unwrap();
        fs::write(repo.join("folder b/foo bar.txt"), "space before\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "fixture.txt", "folder b/foo bar.txt"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "--quiet", "-m", "history fixture"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());

        let path = repo.to_string_lossy().into_owned();
        let history = tauri::async_runtime::block_on(repo_history(HistoryRequest {
            path: path.clone(),
            limit: 5,
        }))
        .unwrap();
        assert_eq!(history.entries.len(), 1);
        assert!(!history.has_more);
        let commit_id = history.entries[0].id.clone();

        let detail = tauri::async_runtime::block_on(repo_commit_detail(CommitDetailRequest {
            path: path.clone(),
            commit_id: commit_id.clone(),
        }))
        .unwrap();
        assert_eq!(detail.id, commit_id);
        assert_eq!(detail.subject, "history fixture");

        fs::write(repo.join("fixture.txt"), "before\nafter\n").unwrap();
        fs::write(
            repo.join("folder b/foo bar.txt"),
            "space before\nspace after\n",
        )
        .unwrap();
        let working = tauri::async_runtime::block_on(repo_diff(DiffRequest {
            path: path.clone(),
            commit_id: None,
        }))
        .unwrap();
        assert_eq!(working.scope, "workingTree");
        assert_eq!(working.files.len(), 2);
        assert!(working
            .files
            .iter()
            .any(|file| { file.path == "fixture.txt" && file.patch.contains("+after") }));
        assert!(working.files.iter().any(|file| {
            file.path == "folder b/foo bar.txt" && file.patch.contains("+space after")
        }));

        let commit = tauri::async_runtime::block_on(repo_diff(DiffRequest {
            path,
            commit_id: Some(commit_id),
        }))
        .unwrap();
        assert_eq!(commit.scope, "commit");
        assert_eq!(commit.files.len(), 2);
        assert!(commit
            .files
            .iter()
            .any(|file| { file.path == "fixture.txt" && file.patch.contains("+before") }));
        assert!(commit.files.iter().any(|file| {
            file.path == "folder b/foo bar.txt" && file.patch.contains("+space before")
        }));
    }

    #[test]
    fn selected_stage_unstage_and_commit_preserve_unselected_changes_and_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        for (key, value) in [
            ("user.email", "stage-commit@example.test"),
            ("user.name", "Stage Commit Fixture"),
        ] {
            assert!(Command::new("git")
                .args(["config", key, value])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        let credential_store = repo.join("credential-store");
        let helper = format!("store --file={}", credential_store.to_string_lossy());
        assert!(Command::new("git")
            .args(["config", "credential.helper", &helper])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());

        fs::write(repo.join("selected.txt"), "selected\n").unwrap();
        fs::write(repo.join("left-unstaged.txt"), "left\n").unwrap();
        let path = repo.to_string_lossy().into_owned();

        let initial =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path: path.clone() }))
                .unwrap();
        assert_eq!(initial.len(), 2);
        assert!(initial
            .iter()
            .all(|change| change.unstaged && !change.staged));

        tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: path.clone(),
            paths: vec!["selected.txt".to_string()],
            operation_id: "stage-selected-initial".to_string(),
        }))
        .unwrap();
        let after_stage =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path: path.clone() }))
                .unwrap();
        let selected = after_stage
            .iter()
            .find(|change| change.path == "selected.txt")
            .unwrap();
        assert!(selected.staged);
        assert!(
            after_stage
                .iter()
                .find(|change| change.path == "left-unstaged.txt")
                .unwrap()
                .unstaged
        );

        tauri::async_runtime::block_on(repo_unstage(UnstagePathsRequest {
            path: path.clone(),
            paths: vec!["selected.txt".to_string()],
            operation_id: "unstage-selected-initial".to_string(),
        }))
        .unwrap();
        let after_unstage =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path: path.clone() }))
                .unwrap();
        let selected = after_unstage
            .iter()
            .find(|change| change.path == "selected.txt")
            .unwrap();
        assert!(!selected.staged && selected.unstaged);

        tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: path.clone(),
            paths: vec!["selected.txt".to_string()],
            operation_id: "stage-selected-commit".to_string(),
        }))
        .unwrap();
        tauri::async_runtime::block_on(repo_commit(CommitRequest {
            path: path.clone(),
            message: "Commit selected\nfixture".to_string(),
            operation_id: "commit-selected".to_string(),
        }))
        .unwrap();

        let after_commit =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path: path.clone() }))
                .unwrap();
        assert_eq!(after_commit.len(), 1);
        assert_eq!(after_commit[0].path, "left-unstaged.txt");
        assert!(after_commit[0].unstaged && !after_commit[0].staged);
        let subject = String::from_utf8(
            Command::new("git")
                .args(["log", "-1", "--format=%B"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(subject.trim(), "Commit selected\nfixture");

        let helper_after = Command::new("git")
            .args(["config", "--get", "credential.helper"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(helper_after.status.success());
        assert_eq!(String::from_utf8_lossy(&helper_after.stdout).trim(), helper);
        assert!(!credential_store.exists());

        fs::write(repo.join("selected.txt"), "selected again\n").unwrap();
        tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: path.clone(),
            paths: vec!["selected.txt".to_string()],
            operation_id: "stage-selected-again".to_string(),
        }))
        .unwrap();
        tauri::async_runtime::block_on(repo_unstage(UnstagePathsRequest {
            path: path.clone(),
            paths: vec!["selected.txt".to_string()],
            operation_id: "unstage-selected-again".to_string(),
        }))
        .unwrap();
        let after_head_unstage =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path })).unwrap();
        let selected = after_head_unstage
            .iter()
            .find(|change| change.path == "selected.txt")
            .unwrap();
        assert!(!selected.staged && selected.unstaged);
    }

    #[test]
    fn cleanup_preview_reports_merged_candidates_and_blocks_main_locked_dirty_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_real_git_dir(&repo);
        git_fixture(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        fs::write(repo.join("fixture.txt"), "fixture\n").unwrap();
        git_fixture(&repo, &["add", "fixture.txt"]);
        git_fixture(&repo, &["commit", "--quiet", "-m", "fixture"]);
        git_fixture(&repo, &["branch", "merged-candidate"]);

        let linked = tmp.path().join("linked");
        git_fixture(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked-candidate",
                linked.to_str().unwrap(),
            ],
        );
        git_fixture(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "private fixture",
                linked.to_str().unwrap(),
            ],
        );
        fs::write(linked.join("untracked-secret.txt"), "fixture\n").unwrap();

        let path = repo.to_string_lossy().into_owned();
        let preview = tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
            path: path.clone(),
            operation_id: "cleanup-preview-blocked".to_string(),
        }))
        .unwrap();
        let merged = preview
            .branches
            .iter()
            .find(|branch| branch.name == "merged-candidate")
            .unwrap();
        assert!(merged.merged);
        assert!(merged.candidate);
        assert!(merged.eligible);
        assert!(preview
            .branches
            .iter()
            .find(|branch| branch.name == "main")
            .unwrap()
            .blocked
            .contains(&"mainBranch".to_string()));
        let linked_path =
            host_path_spelling(&linked.canonicalize().unwrap(), GIT_CLEANUP_ERROR).unwrap();
        let linked_entry = preview
            .worktrees
            .iter()
            .find(|worktree| worktree.path == linked_path)
            .unwrap();
        assert!(!linked_entry.eligible);
        assert!(linked_entry.blocked.contains(&"locked".to_string()));
        assert!(linked_entry.blocked.contains(&"untracked".to_string()));
        let preview_revision = preview.revision.clone();
        let blocked_result = tauri::async_runtime::block_on(repo_cleanup(CleanupRequest {
            path: path.clone(),
            branch_names: vec!["merged-candidate".to_string()],
            worktree_paths: vec![linked_entry.path.clone()],
            preview_revision,
            operation_id: "cleanup-blocked-selection".to_string(),
        }))
        .unwrap();
        assert_eq!(blocked_result.attempted, 0);
        assert_eq!(blocked_result.removed, 0);
        assert!(blocked_result
            .items
            .iter()
            .any(|item| item.kind == "worktree" && item.outcome == "blocked"));
        assert!(!git_fixture(&repo, &["branch", "--list", "merged-candidate"]).is_empty());
        assert!(linked.join("untracked-secret.txt").exists());
        assert!(repo.join(".git").exists());
    }

    #[test]
    fn cleanup_preview_handles_an_unborn_primary_head_without_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("unborn-repo");
        init_real_git_dir(&repo);
        git_fixture(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);

        let preview = tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
            path: repo.to_string_lossy().into_owned(),
            operation_id: "cleanup-preview-unborn".to_string(),
        }))
        .unwrap();
        assert!(preview.current_head.is_none());
        assert!(preview.current_branch.is_none());
        assert!(preview.branches.is_empty());
        assert_eq!(preview.worktrees.len(), 1);
        assert!(preview.worktrees[0]
            .blocked
            .contains(&"mainWorktree".to_string()));
    }

    #[test]
    fn cleanup_preview_uses_the_selected_linked_worktree_head_for_merge_candidates() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_real_git_dir(&repo);
        fs::write(repo.join("base.txt"), "base\n").unwrap();
        git_fixture(&repo, &["add", "base.txt"]);
        git_fixture(&repo, &["commit", "--quiet", "-m", "base"]);
        let linked = tmp.path().join("linked");
        git_fixture(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        );

        // Advance only the primary worktree. A branch pointing at this commit
        // is merged into primary/main, but not into the linked worktree's
        // older HEAD. The preview must use the selected context's HEAD.
        fs::write(repo.join("primary-only.txt"), "primary\n").unwrap();
        git_fixture(&repo, &["add", "primary-only.txt"]);
        git_fixture(&repo, &["commit", "--quiet", "-m", "primary only"]);
        let primary_head = git_fixture(&repo, &["rev-parse", "HEAD"])
            .trim()
            .to_string();
        git_fixture(&repo, &["branch", "primary-only"]);
        let linked_head = git_fixture(&linked, &["rev-parse", "HEAD"])
            .trim()
            .to_string();

        let linked_preview =
            tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
                path: linked.to_string_lossy().into_owned(),
                operation_id: "cleanup-preview-linked-head".to_string(),
            }))
            .unwrap();
        assert_eq!(
            linked_preview.current_head.as_deref(),
            Some(linked_head.as_str())
        );
        let linked_branch = linked_preview
            .branches
            .iter()
            .find(|branch| branch.name == "primary-only")
            .unwrap();
        assert!(!linked_branch.merged);
        assert!(!linked_branch.candidate);

        let primary_preview =
            tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
                path: repo.to_string_lossy().into_owned(),
                operation_id: "cleanup-preview-primary-head".to_string(),
            }))
            .unwrap();
        assert_eq!(
            primary_preview.current_head.as_deref(),
            Some(primary_head.as_str())
        );
        assert!(primary_preview
            .branches
            .iter()
            .find(|branch| branch.name == "primary-only")
            .is_some_and(|branch| branch.merged));
    }

    #[test]
    fn cleanup_revision_binds_the_common_git_directory_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let alternate = tmp.path().join("alternate-common");
        fs::create_dir(&alternate).unwrap();
        let worktree_identity = filesystem_identity(tmp.path(), true).unwrap();
        let common_a = filesystem_identity(tmp.path(), true).unwrap();
        let common_b = filesystem_identity(&alternate, true).unwrap();
        let preview =
            crate::core::cleanup::classify_preview(None, &[], &[], &HashSet::new(), &[], 0);
        let context_a = RepositoryContext {
            worktree: tmp.path().to_path_buf(),
            worktree_identity,
            common_git_identity: common_a,
        };
        let context_b = RepositoryContext {
            common_git_identity: common_b,
            ..context_a.clone()
        };
        let revision_a = cleanup_identity_revision(preview.clone(), &[], &context_a).revision;
        let revision_b = cleanup_identity_revision(preview, &[], &context_b).revision;
        assert_ne!(revision_a, revision_b);
    }

    #[test]
    fn cleanup_context_revalidation_propagates_cancel_and_expired_deadline() {
        let tmp = tempfile::tempdir().unwrap();
        init_real_git_dir(tmp.path());
        let context =
            repository_context_for_worktree(tmp.path().canonicalize().unwrap(), GIT_CLEANUP_ERROR)
                .unwrap();

        let cancelled = AtomicBool::new(true);
        assert_eq!(
            revalidate_repository_context_with_timeout_and_cancel(
                &context,
                GIT_CLEANUP_STATE_CHANGED,
                Duration::from_secs(5),
                Some(&cancelled),
                Some(Instant::now() + Duration::from_secs(5)),
            )
            .unwrap_err(),
            GIT_CLEANUP_CANCELLED
        );

        let active = AtomicBool::new(false);
        assert_eq!(
            revalidate_repository_context_with_timeout_and_cancel(
                &context,
                GIT_CLEANUP_STATE_CHANGED,
                Duration::from_secs(5),
                Some(&active),
                Some(Instant::now()),
            )
            .unwrap_err(),
            GIT_CLEANUP_STATE_CHANGED
        );
    }

    #[test]
    fn cleanup_read_runner_maps_pre_cancel_to_fixed_cleanup_error() {
        let cancellation = AtomicBool::new(true);
        assert_eq!(
            run_git_cleanup_bounded_with_timeout(
                &git_cleanup_branch_args(),
                Path::new("."),
                Duration::from_secs(5),
                Some(&cancellation),
            )
            .unwrap_err(),
            GIT_CLEANUP_CANCELLED
        );
    }

    #[test]
    fn cleanup_executes_only_previewed_targets_and_rejects_stale_revision() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_real_git_dir(&repo);
        git_fixture(&repo, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        fs::write(repo.join("fixture.txt"), "fixture\n").unwrap();
        git_fixture(&repo, &["add", "fixture.txt"]);
        git_fixture(&repo, &["commit", "--quiet", "-m", "fixture"]);
        git_fixture(&repo, &["branch", "merged-candidate"]);
        let path = repo.to_string_lossy().into_owned();

        let preview = tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
            path: path.clone(),
            operation_id: "cleanup-preview-main".to_string(),
        }))
        .unwrap();
        let result = tauri::async_runtime::block_on(repo_cleanup(CleanupRequest {
            path: path.clone(),
            branch_names: vec!["merged-candidate".to_string()],
            worktree_paths: Vec::new(),
            preview_revision: preview.revision.clone(),
            operation_id: "cleanup-merged-candidate".to_string(),
        }))
        .unwrap();
        assert_eq!(result.removed, 1);
        assert_eq!(result.items[0].outcome, "removed");
        assert!(git_fixture(&repo, &["branch", "--list", "merged-candidate"]).is_empty());

        let linked = tmp.path().join("linked");
        git_fixture(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked-candidate",
                linked.to_str().unwrap(),
            ],
        );
        let linked_context_preview =
            tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
                path: linked.to_string_lossy().into_owned(),
                operation_id: "cleanup-preview-linked-context".to_string(),
            }))
            .unwrap();
        let linked_context_entry = linked_context_preview
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("linked-candidate"))
            .unwrap();
        assert!(linked_context_entry
            .blocked
            .contains(&"currentWorktree".to_string()));
        let second_preview =
            tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
                path: path.clone(),
                operation_id: "cleanup-preview-linked-main".to_string(),
            }))
            .unwrap();
        let linked_path = second_preview
            .worktrees
            .iter()
            .find(|worktree| worktree.branch.as_deref() == Some("linked-candidate"))
            .unwrap()
            .path
            .clone();
        let linked_result = tauri::async_runtime::block_on(repo_cleanup(CleanupRequest {
            path: path.clone(),
            branch_names: Vec::new(),
            worktree_paths: vec![linked_path],
            preview_revision: second_preview.revision.clone(),
            operation_id: "cleanup-linked-candidate".to_string(),
        }))
        .unwrap();
        assert_eq!(linked_result.removed, 1);
        assert!(!linked.exists());

        git_fixture(&repo, &["branch", "stale-candidate"]);
        let stale_preview =
            tauri::async_runtime::block_on(repo_cleanup_preview(CleanupPreviewRequest {
                path: path.clone(),
                operation_id: "cleanup-preview-stale".to_string(),
            }))
            .unwrap();
        git_fixture(&repo, &["branch", "new-after-preview"]);
        let error = tauri::async_runtime::block_on(repo_cleanup(CleanupRequest {
            path,
            branch_names: vec!["stale-candidate".to_string()],
            worktree_paths: Vec::new(),
            preview_revision: stale_preview.revision,
            operation_id: "cleanup-stale-preview".to_string(),
        }))
        .unwrap_err();
        assert_eq!(error, GIT_CLEANUP_STATE_CHANGED);
        assert!(!git_fixture(&repo, &["branch", "--list", "stale-candidate"]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn local_commit_can_be_cancelled_by_its_opaque_operation_id() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_real_git_dir(repo);
        fs::write(repo.join("base.txt"), "base\n").unwrap();
        git_fixture(repo, &["add", "base.txt"]);
        git_fixture(repo, &["commit", "--quiet", "-m", "base"]);
        fs::write(repo.join("pending.txt"), "pending\n").unwrap();
        git_fixture(repo, &["add", "pending.txt"]);
        let head_before = git_fixture(repo, &["rev-parse", "HEAD"]);
        let hook_started = repo.join("hook-started");
        let hook = repo.join(".git/hooks/pre-commit");
        fs::write(&hook, "#!/bin/sh\necho started > hook-started\nsleep 5\n").unwrap();
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();

        let path = repo.to_string_lossy().into_owned();
        let started = Instant::now();
        let worker = std::thread::spawn(move || {
            tauri::async_runtime::block_on(repo_commit(CommitRequest {
                path,
                message: "cancelled commit".to_string(),
                operation_id: "cancel-local-commit".to_string(),
            }))
        });
        assert!((0..200).any(|_| {
            if hook_started.exists() {
                true
            } else {
                std::thread::sleep(Duration::from_millis(5));
                false
            }
        }));
        assert!(repo_local_cancel(RemoteCancelRequest {
            operation_id: "cancel-local-commit".to_string(),
        })
        .unwrap());
        assert_eq!(worker.join().unwrap().unwrap_err(), GIT_LOCAL_CANCELLED);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(git_fixture(repo, &["rev-parse", "HEAD"]), head_before);
    }

    #[test]
    fn selecting_both_worktree_sides_then_one_staged_rename_unstages_both() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_real_git_dir(repo);
        fs::write(repo.join("old-name.txt"), "rename fixture\n").unwrap();
        git_fixture(repo, &["add", "old-name.txt"]);
        git_fixture(repo, &["commit", "--quiet", "-m", "rename fixture"]);
        fs::rename(repo.join("old-name.txt"), repo.join("new-name.txt")).unwrap();

        let path = repo.to_string_lossy().into_owned();
        let changes =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path: path.clone() }))
                .unwrap();
        assert!(changes
            .iter()
            .any(|change| change.path == "old-name.txt" && change.kind == "deleted"));
        assert!(changes
            .iter()
            .any(|change| change.path == "new-name.txt" && change.kind == "untracked"));

        tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: path.clone(),
            paths: vec!["old-name.txt".to_string(), "new-name.txt".to_string()],
            operation_id: "stage-rename".to_string(),
        }))
        .unwrap();
        let cached = git_fixture(repo, &["diff", "--cached", "--name-status"]);
        assert_eq!(cached, "R100\told-name.txt\tnew-name.txt\n");

        tauri::async_runtime::block_on(repo_unstage(UnstagePathsRequest {
            path: path.clone(),
            paths: vec!["new-name.txt".to_string()],
            operation_id: "unstage-rename".to_string(),
        }))
        .unwrap();
        assert!(git_fixture(repo, &["diff", "--cached", "--name-status"]).is_empty());

        let after_unstage =
            tauri::async_runtime::block_on(repo_changes(RepoChangesRequest { path })).unwrap();
        assert!(after_unstage
            .iter()
            .any(|change| change.path == "old-name.txt" && change.kind == "deleted"));
        assert!(after_unstage
            .iter()
            .any(|change| change.path == "new-name.txt" && change.kind == "untracked"));
        assert!(after_unstage
            .iter()
            .all(|change| !change.staged && change.unstaged));
    }

    #[test]
    fn mutation_validation_failure_is_fixed_and_does_not_touch_the_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        for (key, value) in [("user.email", "safe@example.test"), ("user.name", "Safe")] {
            assert!(Command::new("git")
                .args(["config", key, value])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("keep.txt"), "keep\n").unwrap();
        let path = repo.to_string_lossy().into_owned();
        let secret = "credential-path-secret";
        let error = tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: path.clone(),
            paths: vec![format!("../{secret}")],
            operation_id: "stage-invalid-parent".to_string(),
        }))
        .unwrap_err();
        assert_eq!(error, GIT_MUTATION_ERROR);
        assert!(!error.contains(secret));

        let error = tauri::async_runtime::block_on(repo_stage(StagePathsRequest {
            path: repo.to_string_lossy().into_owned(),
            paths: vec!["not-in-status.txt".to_string()],
            operation_id: "stage-invalid-selection".to_string(),
        }))
        .unwrap_err();
        assert_eq!(error, GIT_MUTATION_ERROR);
        assert!(!error.contains("not-in-status.txt"));

        let error = tauri::async_runtime::block_on(repo_commit(CommitRequest {
            path,
            message: format!("invalid\0{secret}"),
            operation_id: "commit-invalid-message".to_string(),
        }))
        .unwrap_err();
        assert_eq!(error, GIT_MUTATION_ERROR);
        assert!(!error.contains(secret));
        assert!(!repo.join("keep.txt").to_string_lossy().contains(secret));
    }

    #[test]
    fn selected_path_arguments_are_literal_and_have_a_separator() {
        let wildcard = "folder/*.txt".to_string();
        let stage = git_stage_args(std::slice::from_ref(&wildcard));
        let unstage = git_unstage_args_for_head(std::slice::from_ref(&wildcard), true);
        for args in [stage, unstage] {
            assert!(args.iter().any(|arg| arg == "--literal-pathspecs"));
            let separator = args.iter().position(|arg| arg == "--").unwrap();
            assert_eq!(args.get(separator + 1), Some(&wildcard));
            assert!(!args.iter().any(|arg| arg == "reset" || arg == "clean"));
        }
    }

    #[test]
    fn cleanup_argv_is_bounded_and_never_force_or_repair() {
        let branch = git_cleanup_branch_args();
        let merged = git_cleanup_merged_args("0123456789abcdef0123456789abcdef01234567");
        let worktree = git_cleanup_worktree_args();
        let status = git_cleanup_status_args();
        let delete = git_cleanup_delete_branch_args("feature/cleanup");
        let remove = git_cleanup_remove_worktree_args(Path::new("/tmp/linked worktree"));
        let args = [
            branch.clone(),
            merged.clone(),
            worktree.clone(),
            status.clone(),
            delete.clone(),
            remove.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        assert!(cleanup_read_argv_allowed(&branch));
        assert!(cleanup_read_argv_allowed(&merged));
        assert!(cleanup_read_argv_allowed(&worktree));
        assert!(cleanup_read_argv_allowed(&status));
        assert!(cleanup_mutation_argv_allowed(&delete));
        assert!(cleanup_mutation_argv_allowed(&remove));
        assert!(cleanup_mutation_argv_allowed(
            &git_cleanup_remove_worktree_args(Path::new(r"\\?\C:\long\linked"),)
        ));
        assert!(args.iter().any(|arg| arg == "--delete"));
        assert!(args.iter().any(|arg| arg == "remove"));
        assert!(!args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "-D" | "--force" | "-f" | "reset" | "clean" | "prune"
            )
        }));
        assert!(delete
            .windows(2)
            .any(|pair| pair[0] == "--" && pair[1] == "feature/cleanup"));
        assert!(remove
            .windows(2)
            .any(|pair| pair[0] == "--" && pair[1] == "/tmp/linked worktree"));

        let mut force_delete = delete.clone();
        force_delete[3] = "-D".to_string();
        assert!(!cleanup_mutation_argv_allowed(&force_delete));
        let mut injected_merged =
            git_cleanup_merged_args("0123456789abcdef0123456789abcdef01234567");
        injected_merged[3] = "--merged=HEAD;touch".to_string();
        assert!(!cleanup_read_argv_allowed(&injected_merged));
        assert!(!cleanup_mutation_argv_allowed(
            &git_cleanup_remove_worktree_args(Path::new(r"\\?\PhysicalDrive0"),)
        ));
        for root in ["/", r"C:\", r"\\server\share", r"\\?\C:\"] {
            assert!(!cleanup_mutation_argv_allowed(
                &git_cleanup_remove_worktree_args(Path::new(root)),
            ));
        }
    }

    #[test]
    fn safety_argv_state_matrix_is_fixed_and_read_only() {
        assert_eq!(
            git_safety_status_args(),
            vec![
                "--no-pager",
                "--no-optional-locks",
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
                "-z",
                "--",
            ]
        );
        assert_eq!(
            git_safety_marker_args(),
            vec![
                "--no-pager",
                "--no-optional-locks",
                "rev-parse",
                "--git-path",
                "rebase-merge",
                "--git-path",
                "rebase-apply",
                "--git-path",
                "MERGE_HEAD",
            ]
        );
        let args = [git_safety_status_args(), git_safety_marker_args()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert!(!args
            .iter()
            .any(|arg| { matches!(arg.as_str(), "push" | "reset" | "clean" | "--force" | "-f") }));
    }

    #[test]
    fn marker_path_parser_is_bounded_and_redacts_untrusted_values() {
        let cwd = Path::new("/safe/repository");
        let output = "/safe/repository/.git/rebase-merge\n/safe/repository/.git/rebase-apply\n/safe/repository/.git/MERGE_HEAD\n";
        let paths = parse_safety_marker_paths(output, cwd).unwrap();
        assert_eq!(paths[0], Path::new("/safe/repository/.git/rebase-merge"));
        assert_eq!(paths[1], Path::new("/safe/repository/.git/rebase-apply"));
        assert_eq!(paths[2], Path::new("/safe/repository/.git/MERGE_HEAD"));

        let secret = "credential-marker-secret";
        for output in [
            format!("/safe/.git/{secret}\n/safe/.git/rebase-apply\n/safe/.git/MERGE_HEAD\n"),
            "/safe/.git/rebase-merge\n/safe/.git/rebase-apply\n".to_string(),
            "/safe/.git/../outside\n/safe/.git/rebase-apply\n/safe/.git/MERGE_HEAD\n".to_string(),
            "/safe/.git/../rebase-merge\n/safe/.git/rebase-apply\n/safe/.git/MERGE_HEAD\n"
                .to_string(),
            "/safe/.git/rebase-merge\n/safe/.git/rebase-apply\n/safe/.git/MERGE_HEAD\n\n"
                .to_string(),
        ] {
            let error = parse_safety_marker_paths(&output, cwd).unwrap_err();
            assert_eq!(error, GIT_SAFETY_ERROR);
            assert!(!error.contains(secret));
        }
    }

    #[test]
    fn real_preflight_detects_dirty_no_upstream_detached_and_operation_markers_without_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        for (key, value) in [
            ("user.email", "safety@example.test"),
            ("user.name", "Safety"),
        ] {
            assert!(Command::new("git")
                .args(["config", key, value])
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("fixture.txt"), "fixture\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "fixture.txt"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "--quiet", "-m", "safety fixture"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());

        let path = repo.to_string_lossy().into_owned();
        let clean = tauri::async_runtime::block_on(repo_preflight(RepoPreflightRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(!clean.dirty);
        assert!(clean.no_upstream);
        assert!(!clean.detached);
        assert!(!clean.diverged);
        assert!(!clean.rebase_in_progress);
        assert!(!clean.merge_in_progress);
        assert_eq!(clean.issues, vec!["noUpstream"]);

        fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();
        let dirty = tauri::async_runtime::block_on(repo_preflight(RepoPreflightRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(dirty.dirty);
        assert!(dirty.issues.contains(&"dirty".to_string()));

        assert!(Command::new("git")
            .args(["checkout", "--quiet", "--detach", "HEAD"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        let detached = tauri::async_runtime::block_on(repo_preflight(RepoPreflightRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(detached.detached);
        assert!(!detached.no_upstream);

        fs::create_dir(repo.join(".git/rebase-merge")).unwrap();
        fs::write(repo.join(".git/MERGE_HEAD"), "marker\n").unwrap();
        let in_progress = tauri::async_runtime::block_on(repo_preflight(RepoPreflightRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(in_progress.rebase_in_progress);
        assert!(in_progress.merge_in_progress);

        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(head.status.success());
        assert_eq!(
            String::from_utf8_lossy(&head.stdout).trim().len(),
            40,
            "preflight must not change refs"
        );
    }

    #[test]
    fn preflight_failures_are_fixed_and_do_not_reflect_unmounted_or_secret_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let secret = "unmounted-credential-path";
        let error = tauri::async_runtime::block_on(repo_preflight(RepoPreflightRequest {
            path: tmp.path().join(secret).to_string_lossy().into_owned(),
        }))
        .unwrap_err();
        assert_eq!(error, GIT_SAFETY_ERROR);
        assert!(!error.contains(secret));
    }

    fn init_git_dir(dir: &Path) {
        fs::create_dir_all(dir.join(".git")).unwrap();
    }

    #[test]
    fn finds_repo_at_root() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(result.repos.len(), 1);
        assert!(!result.truncated);
    }

    #[test]
    fn inbound_repository_metadata_matches_scan_identity_without_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let path = tmp.path().to_string_lossy().into_owned();

        let inbound = prepare_inbound_repository(path.clone()).unwrap();
        let scanned = scan_root(path).unwrap();

        assert_eq!(scanned.repos, vec![inbound]);
    }

    #[test]
    fn explicit_copy_revalidates_repository_and_hides_rejected_path() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(tmp.path());
        let path = tmp.path().to_string_lossy().into_owned();
        assert_eq!(repository_copy_path(path.clone()).unwrap(), path);

        let secret = "copy-path-secret-must-not-appear";
        let error = repository_copy_path(format!("relative/{secret}"))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "repository 경로가 올바르지 않습니다");
        assert!(!error.contains(secret));
    }

    #[test]
    fn prunes_ignored_dirs_before_recursing() {
        let tmp = tempfile::tempdir().unwrap();
        // node_modules 아래 숨은 .git은 "발견"이 아니라 "가지치기"돼야 한다 — 이 트리를 실제로
        // 걸어 들어가면(진짜 프로젝트의 node_modules는 수만 개 파일이라) 느려지거나 멎는다.
        init_git_dir(&tmp.path().join("node_modules/some-pkg"));
        init_git_dir(&tmp.path().join("real-repo"));
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        let paths: Vec<&str> = result.repos.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(result.repos.len(), 1);
        assert!(paths.iter().any(|p| p.ends_with("real-repo")));
        assert!(!result.truncated);
    }

    #[test]
    fn depth_cap_stops_and_marks_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        // MAX_SCAN_DEPTH(12)를 넘는 중첩 아래에 repo를 둔다.
        let mut deep = tmp.path().to_path_buf();
        for i in 0..(MAX_SCAN_DEPTH + 3) {
            deep = deep.join(format!("d{i}"));
        }
        init_git_dir(&deep);
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert!(result.repos.is_empty(), "상한 밖의 repo는 발견되면 안 된다");
        assert!(
            result.truncated,
            "상한에 걸렸으면 truncated=true여야 한다 (조용한 누락 금지)"
        );
    }

    #[test]
    fn shallow_tree_within_limits_is_not_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        init_git_dir(&tmp.path().join("a/b/c"));
        let result = scan_root(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(result.repos.len(), 1);
        assert!(!result.truncated);
    }

    #[test]
    fn remote_argv_is_exact_and_has_no_destructive_or_force_operation() {
        let push = git_push_args("origin", "origin/main").unwrap();
        assert_eq!(
            git_remote_status_args(),
            vec![
                "--no-pager",
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--branch",
                "--untracked-files=all",
                "--",
            ]
        );
        assert_eq!(
            git_fetch_args(),
            vec!["--no-pager", "--no-optional-locks", "fetch", "--no-tags"]
        );
        assert_eq!(
            git_pull_args(),
            vec![
                "--no-pager",
                "--no-optional-locks",
                "pull",
                "--ff-only",
                "--no-rebase",
            ]
        );
        assert_eq!(
            push,
            vec![
                "--no-pager",
                "--no-optional-locks",
                "push",
                "--",
                "origin",
                "HEAD:refs/heads/main",
            ]
        );

        for args in [
            git_fetch_args(),
            git_pull_args(),
            git_push_args("origin", "origin/main").unwrap(),
        ] {
            assert!(!args.iter().any(|arg| {
                matches!(
                    arg.as_str(),
                    "--force"
                        | "--force-with-lease"
                        | "-f"
                        | "reset"
                        | "clean"
                        | "merge"
                        | "rebase"
                )
            }));
        }

        assert_eq!(parse_remote_name("team/remote\n").unwrap(), "team/remote");
        assert_eq!(
            git_push_args("team/remote", "team/remote/release/v1").unwrap(),
            vec![
                "--no-pager",
                "--no-optional-locks",
                "push",
                "--",
                "team/remote",
                "HEAD:refs/heads/release/v1",
            ]
        );
        for (remote, upstream) in [
            ("origin", "origin/../main"),
            ("origin", "origin/.hidden"),
            ("origin", "origin/main.lock"),
            ("origin", "origin/feature@{1}"),
            ("origin", "origin/feature~1"),
            ("bad..remote", "bad..remote/main"),
        ] {
            assert_eq!(
                git_push_args(remote, upstream),
                Err(GIT_REMOTE_ERROR.to_string())
            );
        }
        for value in ["origin", ".\n", "https://secret@example.test\n"] {
            assert_eq!(parse_remote_name(value).unwrap_err(), GIT_REMOTE_ERROR);
        }
        assert_eq!(
            git_push_args("origin", "upstream/main").unwrap_err(),
            GIT_REMOTE_ERROR
        );
    }

    #[test]
    fn remote_state_and_preflight_use_fixed_errors_without_raw_repository_details() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_real_git_dir(repo);
        fs::write(repo.join("fixture.txt"), "fixture\n").unwrap();
        git_fixture(repo, &["add", "fixture.txt"]);
        git_fixture(repo, &["commit", "--quiet", "-m", "fixture"]);
        let path = repo.to_string_lossy().into_owned();

        let status = tauri::async_runtime::block_on(repo_remote_status(RemoteSyncRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(status.current_branch.is_some());
        assert!(status.upstream.is_none());

        let error = tauri::async_runtime::block_on(repo_pull(remote_operation_request(
            path,
            "no-upstream-pull",
        )))
        .unwrap_err();
        assert_eq!(
            error,
            "현재 branch에 upstream이 없어 pull/push를 실행할 수 없습니다."
        );
        assert!(!error.contains(repo.to_string_lossy().as_ref()));
    }

    #[test]
    fn remote_real_git_fixture_covers_ff_pull_push_and_diverged_block() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let local = tmp.path().join("local");
        let updater = tmp.path().join("updater");
        git_fixture(tmp.path(), &["init", "--bare", "--quiet", "remote.git"]);
        git_fixture(tmp.path(), &["clone", "--quiet", "remote.git", "local"]);
        git_fixture(&local, &["config", "user.email", "remote@example.test"]);
        git_fixture(&local, &["config", "user.name", "Remote Fixture"]);

        fs::write(local.join("fixture.txt"), "base\n").unwrap();
        git_fixture(&local, &["add", "fixture.txt"]);
        git_fixture(&local, &["commit", "--quiet", "-m", "base"]);
        git_fixture(
            &local,
            &["push", "--quiet", "--set-upstream", "origin", "HEAD"],
        );
        git_fixture(tmp.path(), &["clone", "--quiet", "remote.git", "updater"]);
        git_fixture(&updater, &["config", "user.email", "remote@example.test"]);
        git_fixture(&updater, &["config", "user.name", "Remote Fixture"]);
        let local_path = local.to_string_lossy().into_owned();

        let initial = tauri::async_runtime::block_on(repo_remote_status(RemoteSyncRequest {
            path: local_path.clone(),
        }))
        .unwrap();
        assert!(initial.upstream.is_some());
        assert!(!initial.dirty);

        // A competing local writer can change the working tree after the
        // admission read. The deterministic hook stands in for that race and
        // proves the final native recheck blocks the pull before Git mutates
        // refs or invokes a merge.
        let head_before_race = git_fixture(&local, &["rev-parse", "HEAD"]);
        let race_error =
            run_remote_operation_with_hook(&local, RemoteAction::Pull, "race-writer", || {
                fs::write(local.join("race-writer.txt"), "changed\n").unwrap()
            })
            .unwrap_err();
        assert_eq!(race_error, GIT_REMOTE_STATE_CHANGED);
        assert_eq!(
            git_fixture(&local, &["rev-parse", "HEAD"]),
            head_before_race
        );
        fs::remove_file(local.join("race-writer.txt")).unwrap();

        // Changing branch.<name>.remote after the final status snapshot must
        // invalidate the exact push target. Neither the original nor the new
        // configured remote may receive a ref update from the rejected run.
        let config_branch = git_fixture(&local, &["branch", "--show-current"])
            .trim()
            .to_owned();
        let remote_head_before = git_fixture(
            &remote,
            &["rev-parse", &format!("refs/heads/{config_branch}")],
        );
        git_fixture(
            &local,
            &["remote", "add", "backup", remote.to_str().unwrap()],
        );
        let config_race_error = run_remote_operation_with_hooks(
            &local,
            RemoteAction::Push,
            "push-config-race",
            || {},
            || {
                git_fixture(
                    &local,
                    &[
                        "config",
                        &format!("branch.{config_branch}.remote"),
                        "backup",
                    ],
                );
            },
        )
        .unwrap_err();
        assert_eq!(config_race_error, GIT_REMOTE_STATE_CHANGED);
        assert_eq!(
            git_fixture(
                &remote,
                &["rev-parse", &format!("refs/heads/{config_branch}")]
            ),
            remote_head_before
        );
        git_fixture(
            &local,
            &[
                "config",
                &format!("branch.{config_branch}.remote"),
                "origin",
            ],
        );

        // Pull/push never start with uncommitted work in the working tree.
        fs::write(local.join("uncommitted.txt"), "dirty\n").unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(repo_pull(remote_operation_request(
                local_path.clone(),
                "dirty-pull",
            )))
            .unwrap_err(),
            "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다."
        );
        assert_eq!(
            tauri::async_runtime::block_on(repo_push(remote_operation_request(
                local_path.clone(),
                "dirty-push",
            )))
            .unwrap_err(),
            "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다."
        );
        fs::remove_file(local.join("uncommitted.txt")).unwrap();

        // A detached HEAD has no current branch target and must not be used
        // for either mutation, even while its configured remote still exists.
        let branch = git_fixture(&local, &["branch", "--show-current"])
            .trim()
            .to_owned();
        git_fixture(&local, &["checkout", "--quiet", "--detach", "HEAD"]);
        assert_eq!(
            tauri::async_runtime::block_on(repo_pull(remote_operation_request(
                local_path.clone(),
                "detached-pull",
            )))
            .unwrap_err(),
            "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다."
        );
        assert_eq!(
            tauri::async_runtime::block_on(repo_push(remote_operation_request(
                local_path.clone(),
                "detached-push",
            )))
            .unwrap_err(),
            "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다."
        );
        git_fixture(&local, &["checkout", "--quiet", &branch]);

        // A fast-forward update is published by a separate fixture clone and
        // then pulled through the command under test.
        fs::write(updater.join("fixture.txt"), "base\nremote\n").unwrap();
        git_fixture(&updater, &["add", "fixture.txt"]);
        git_fixture(&updater, &["commit", "--quiet", "-m", "remote update"]);
        git_fixture(&updater, &["push", "--quiet"]);
        tauri::async_runtime::block_on(repo_fetch(remote_operation_request(
            local_path.clone(),
            "ff-fetch",
        )))
        .unwrap();
        let before_pull = tauri::async_runtime::block_on(repo_remote_status(RemoteSyncRequest {
            path: local_path.clone(),
        }))
        .unwrap();
        assert_eq!(before_pull.behind, 1);
        tauri::async_runtime::block_on(repo_pull(remote_operation_request(
            local_path.clone(),
            "ff-pull",
        )))
        .unwrap();
        assert_eq!(
            fs::read_to_string(local.join("fixture.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "base\nremote\n"
        );

        // A normal current-branch push is allowed after a clean local commit.
        fs::write(local.join("fixture.txt"), "base\nremote\nlocal\n").unwrap();
        git_fixture(&local, &["add", "fixture.txt"]);
        git_fixture(&local, &["commit", "--quiet", "-m", "local update"]);
        tauri::async_runtime::block_on(repo_push(remote_operation_request(
            local_path.clone(),
            "normal-push",
        )))
        .unwrap();

        // Publish another remote-only commit, then create a local commit
        // before fetching it.  Once fetched, both sides are ahead and behind;
        // pull/push must stop before Git can attempt merge or force behavior.
        git_fixture(&updater, &["pull", "--quiet", "--ff-only"]);
        fs::write(
            updater.join("fixture.txt"),
            "base\nremote\nlocal\nremote-2\n",
        )
        .unwrap();
        git_fixture(&updater, &["add", "fixture.txt"]);
        git_fixture(&updater, &["commit", "--quiet", "-m", "remote update 2"]);
        git_fixture(&updater, &["push", "--quiet"]);
        fs::write(local.join("fixture.txt"), "base\nremote\nlocal\nlocal-2\n").unwrap();
        git_fixture(&local, &["add", "fixture.txt"]);
        git_fixture(&local, &["commit", "--quiet", "-m", "local update 2"]);
        tauri::async_runtime::block_on(repo_fetch(remote_operation_request(
            local_path.clone(),
            "diverged-fetch",
        )))
        .unwrap();
        let diverged = tauri::async_runtime::block_on(repo_remote_status(RemoteSyncRequest {
            path: local_path.clone(),
        }))
        .unwrap();
        assert!(diverged.diverged);
        assert_eq!(
            tauri::async_runtime::block_on(repo_pull(remote_operation_request(
                local_path.clone(),
                "diverged-pull",
            )))
            .unwrap_err(),
            "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다."
        );
        assert_eq!(
            tauri::async_runtime::block_on(repo_push(remote_operation_request(
                local_path,
                "diverged-push",
            )))
            .unwrap_err(),
            "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다."
        );

        // The fixture never invokes reset, clean, merge, rebase, or force
        // commands; all external state changes above are ordinary commits and
        // pushes in disposable temporary repositories.
        assert!(remote.is_dir());
    }

    #[test]
    fn remote_operation_in_progress_blocks_even_fetch_and_dirty_is_redacted() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_real_git_dir(repo);
        fs::write(repo.join("fixture.txt"), "fixture\n").unwrap();
        git_fixture(repo, &["add", "fixture.txt"]);
        git_fixture(repo, &["commit", "--quiet", "-m", "fixture"]);
        let marker = git_fixture(repo, &["rev-parse", "--git-path", "MERGE_HEAD"]);
        let marker = Path::new(marker.trim());
        let git_dir = if marker.is_absolute() {
            marker.to_path_buf()
        } else {
            repo.join(marker)
        };
        fs::write(git_dir, "0000000000000000000000000000000000000000\n").unwrap();
        let path = repo.to_string_lossy().into_owned();
        let status = tauri::async_runtime::block_on(repo_remote_status(RemoteSyncRequest {
            path: path.clone(),
        }))
        .unwrap();
        assert!(status.operation_in_progress);
        let error = tauri::async_runtime::block_on(repo_fetch(remote_operation_request(
            path,
            "in-progress-fetch",
        )))
        .unwrap_err();
        assert_eq!(
            error,
            "다른 Git 작업 또는 merge/rebase가 진행 중이라 원격 작업을 실행할 수 없습니다."
        );
    }

    #[test]
    fn opaque_cancel_id_survives_repository_removal_and_enforces_single_flight() {
        let tmp = tempfile::tempdir().unwrap();
        let operation_path = tmp.path().join("vanishing-repository");
        init_real_git_dir(&operation_path);
        let context = repository_context_for_worktree(
            operation_path.canonicalize().unwrap(),
            GIT_REMOTE_ERROR,
        )
        .unwrap();

        let pending = begin_git_operation(
            "cancel-before-path-validation",
            GIT_REMOTE_ERROR,
            GIT_REMOTE_BUSY,
        )
        .unwrap();
        assert!(cancel_git_operation("cancel-before-path-validation"));
        assert!(pending.cancellation.load(Ordering::Acquire));
        drop(pending);
        assert!(!cancel_git_operation("cancel-before-path-validation"));

        let operation_id = "opaque-cancel-id";
        let mut operation =
            begin_git_operation(operation_id, GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap();
        operation
            .bind_repository(
                context.common_git_identity,
                GIT_REMOTE_ERROR,
                GIT_REMOTE_BUSY,
            )
            .unwrap();
        assert!(!operation.cancellation.load(Ordering::Acquire));
        assert_eq!(
            begin_git_operation(operation_id, GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap_err(),
            GIT_REMOTE_BUSY
        );
        let mut second =
            begin_git_operation("second-operation", GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap();
        assert_eq!(
            second
                .bind_repository(
                    context.common_git_identity,
                    GIT_REMOTE_ERROR,
                    GIT_REMOTE_BUSY,
                )
                .unwrap_err(),
            GIT_REMOTE_BUSY
        );
        drop(second);

        fs::remove_dir_all(&operation_path).unwrap();
        assert!(cancel_git_operation(operation_id));
        assert!(operation.cancellation.load(Ordering::Acquire));
        drop(operation);
        assert!(!cancel_git_operation(operation_id));
    }

    #[test]
    fn local_and_remote_mutations_share_one_repository_lock() {
        let tmp = tempfile::tempdir().unwrap();
        init_real_git_dir(tmp.path());
        let context =
            repository_context_for_worktree(tmp.path().canonicalize().unwrap(), GIT_REMOTE_ERROR)
                .unwrap();
        let mut local =
            begin_git_operation("active-local", GIT_MUTATION_ERROR, GIT_MUTATION_ERROR).unwrap();
        local
            .bind_repository(
                context.common_git_identity,
                GIT_MUTATION_ERROR,
                GIT_MUTATION_ERROR,
            )
            .unwrap();
        let mut blocked_remote =
            begin_git_operation("blocked-remote", GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap();
        assert_eq!(
            blocked_remote
                .bind_repository(
                    context.common_git_identity,
                    GIT_REMOTE_ERROR,
                    GIT_REMOTE_BUSY,
                )
                .unwrap_err(),
            GIT_REMOTE_BUSY
        );
        drop(blocked_remote);
        assert!(active_git_operation_in_progress(
            context.common_git_identity,
            None,
            GIT_REMOTE_ERROR,
        )
        .unwrap());
        drop(local);

        let mut remote =
            begin_git_operation("active-remote", GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap();
        remote
            .bind_repository(
                context.common_git_identity,
                GIT_REMOTE_ERROR,
                GIT_REMOTE_BUSY,
            )
            .unwrap();
        let mut blocked_local =
            begin_git_operation("blocked-local", GIT_MUTATION_ERROR, GIT_MUTATION_ERROR).unwrap();
        assert_eq!(
            blocked_local
                .bind_repository(
                    context.common_git_identity,
                    GIT_MUTATION_ERROR,
                    GIT_MUTATION_ERROR,
                )
                .unwrap_err(),
            GIT_MUTATION_ERROR
        );
        drop(blocked_local);
        assert!(!remote.cancellation.load(Ordering::Acquire));
        drop(remote);
        assert!(!active_git_operation_in_progress(
            context.common_git_identity,
            None,
            GIT_REMOTE_ERROR,
        )
        .unwrap());
    }

    #[test]
    fn linked_worktrees_share_the_common_git_directory_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main");
        let linked = tmp.path().join("linked");
        let independent = tmp.path().join("independent");
        init_real_git_dir(&main);
        fs::write(main.join("fixture.txt"), "fixture\n").unwrap();
        git_fixture(&main, &["add", "fixture.txt"]);
        git_fixture(&main, &["commit", "--quiet", "-m", "fixture"]);
        git_fixture(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "linked-fixture",
                linked.to_str().unwrap(),
            ],
        );
        init_real_git_dir(&independent);

        let main_context =
            repository_context_for_worktree(main.canonicalize().unwrap(), GIT_REMOTE_ERROR)
                .unwrap();
        let linked_context =
            repository_context_for_worktree(linked.canonicalize().unwrap(), GIT_REMOTE_ERROR)
                .unwrap();
        let independent_context =
            repository_context_for_worktree(independent.canonicalize().unwrap(), GIT_REMOTE_ERROR)
                .unwrap();
        assert_eq!(
            main_context.common_git_identity,
            linked_context.common_git_identity
        );
        assert_ne!(
            main_context.worktree_identity,
            linked_context.worktree_identity
        );
        assert_ne!(
            main_context.common_git_identity,
            independent_context.common_git_identity
        );

        let mut main_operation =
            begin_git_operation("main-worktree-lock", GIT_MUTATION_ERROR, GIT_MUTATION_ERROR)
                .unwrap();
        main_operation
            .bind_repository(
                main_context.common_git_identity,
                GIT_MUTATION_ERROR,
                GIT_MUTATION_ERROR,
            )
            .unwrap();
        let mut linked_operation =
            begin_git_operation("linked-worktree-lock", GIT_REMOTE_ERROR, GIT_REMOTE_BUSY).unwrap();
        assert_eq!(
            linked_operation
                .bind_repository(
                    linked_context.common_git_identity,
                    GIT_REMOTE_ERROR,
                    GIT_REMOTE_BUSY,
                )
                .unwrap_err(),
            GIT_REMOTE_BUSY
        );
        let linked_state = read_remote_state(&linked_context, None, None).unwrap();
        assert!(linked_state.operation_in_progress);
        let blocked_target = tmp.path().join("blocked-worktree");
        assert_eq!(
            tauri::async_runtime::block_on(create_worktree(
                main.to_string_lossy().into_owned(),
                "blocked-worktree-fixture".to_string(),
                blocked_target.to_string_lossy().into_owned(),
            ))
            .unwrap_err(),
            GIT_WORKTREE_ERROR
        );
        assert!(!blocked_target.exists());

        let mut independent_operation = begin_git_operation(
            "independent-repository-lock",
            GIT_REMOTE_ERROR,
            GIT_REMOTE_BUSY,
        )
        .unwrap();
        independent_operation
            .bind_repository(
                independent_context.common_git_identity,
                GIT_REMOTE_ERROR,
                GIT_REMOTE_BUSY,
            )
            .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn repository_symlink_alias_resolves_to_the_same_operation_identity() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let repository = tmp.path().join("repository");
        let alias = tmp.path().join("repository-alias");
        init_real_git_dir(&repository);
        symlink(&repository, &alias).unwrap();

        let direct =
            validated_repository_context(repository.to_string_lossy().as_ref(), GIT_REMOTE_ERROR)
                .unwrap();
        let through_alias =
            validated_repository_context(alias.to_string_lossy().as_ref(), GIT_REMOTE_ERROR)
                .unwrap();
        assert_eq!(direct.worktree_identity, through_alias.worktree_identity);
        assert_eq!(
            direct.common_git_identity,
            through_alias.common_git_identity
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_marker_symlink_fails_closed() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        init_real_git_dir(repo);
        let marker = git_fixture(repo, &["rev-parse", "--git-path", "MERGE_HEAD"]);
        let marker = Path::new(marker.trim());
        let marker_path = if marker.is_absolute() {
            marker.to_path_buf()
        } else {
            repo.join(marker)
        };
        let target = repo.join("unrelated-marker-target");
        fs::write(&target, "not a merge marker\n").unwrap();
        symlink(&target, &marker_path).unwrap();

        assert_eq!(
            remote_marker_exists(repo, "MERGE_HEAD", None).unwrap_err(),
            GIT_REMOTE_ERROR
        );
    }

    #[test]
    fn remote_marker_lookup_accepts_only_fixed_marker_names() {
        let tmp = tempfile::tempdir().unwrap();
        init_real_git_dir(tmp.path());
        for marker in ["../../outside", "hooks/post-checkout", "MERGE_HEAD\0secret"] {
            assert_eq!(
                remote_marker_exists(tmp.path(), marker, None).unwrap_err(),
                GIT_REMOTE_ERROR
            );
        }
    }

    fn init_real_git_dir(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git_fixture(dir, &["init", "--quiet"]);
        git_fixture(dir, &["config", "user.email", "test@example.test"]);
        git_fixture(dir, &["config", "user.name", "Remote Test"]);
    }

    fn remote_operation_request(path: String, operation_id: &str) -> RemoteOperationRequest {
        RemoteOperationRequest {
            path,
            operation_id: operation_id.to_owned(),
        }
    }

    fn git_fixture(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "git fixture failed: {:?}", args);
        String::from_utf8(output.stdout).unwrap()
    }
}
