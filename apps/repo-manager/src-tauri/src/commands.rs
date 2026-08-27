//! Repo Manager command — 저장소 탐색·상태·worktree.

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
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
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

/// Execute a read-only Git query through the shared bounded runner. Its stderr,
/// timeout, argument, UTF-8, and stdout-cap failures are intentionally mapped
/// to one UI-safe error here.
fn run_git_bounded(args: &[String], cwd: &Path, max_bytes: usize) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_bounded(&args, &cwd, Duration::from_secs(5), max_bytes)
        .map_err(|_| GIT_VIEW_ERROR.to_string())
}

/// Bounded status reader for the mutable working-tree panel. Status output is
/// parsed as NUL-delimited porcelain records and all subprocess failures map
/// to the same UI-safe mutation error.
fn run_git_status_bounded(args: &[String], cwd: &Path) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_bounded(&args, &cwd, MUTATION_TIMEOUT, MAX_STATUS_OUTPUT_BYTES)
        .map_err(|_| GIT_MUTATION_ERROR.to_string())
}

fn run_git_status_bounded_with_cancel(
    args: &[String],
    cwd: &Path,
    cancellation: &AtomicBool,
) -> Result<String, String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_bounded_with_cancel(
        &args,
        &cwd,
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
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_bounded(&args, &cwd, SAFETY_TIMEOUT, max_bytes)
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
        let raw = Path::new(line);
        let expected_name = ["rebase-merge", "rebase-apply", "MERGE_HEAD"][index];
        if raw.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
            return Err(GIT_SAFETY_ERROR.to_string());
        }
        if raw
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(GIT_SAFETY_ERROR.to_string());
        }
        paths.push(if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            cwd.join(raw)
        });
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
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_mutating(&args, &cwd, MUTATION_TIMEOUT, MAX_MUTATION_OUTPUT_BYTES)
        .map(|_| ())
        .map_err(|_| GIT_MUTATION_ERROR.to_string())
}

fn run_git_mutation_with_cancel(
    args: &[String],
    cwd: &Path,
    cancellation: &AtomicBool,
) -> Result<(), String> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = cwd.to_string_lossy().into_owned();
    devbox_git::run_mutating_with_cancel(
        &args,
        &cwd,
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
    let cwd = cwd.to_string_lossy().into_owned();
    let result = match cancellation {
        Some(signal) => devbox_git::run_bounded_with_cancel(
            &args,
            &cwd,
            Duration::from_secs(5),
            max_bytes,
            signal,
        ),
        None => devbox_git::run_bounded(&args, &cwd, Duration::from_secs(5), max_bytes),
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
    let output = run_git_remote_bounded(
        &git_remote_marker_args(marker),
        cwd,
        MAX_REMOTE_MARKER_OUTPUT_BYTES,
        cancellation,
    )?;
    let marker = output.trim();
    if marker.is_empty()
        || marker.len() > MAX_REMOTE_BRANCH_BYTES
        || marker.chars().any(char::is_control)
    {
        return Err(GIT_REMOTE_ERROR.to_string());
    }
    let marker_path = Path::new(marker);
    let marker_path = if marker_path.is_absolute() {
        marker_path.to_path_buf()
    } else {
        cwd.join(marker_path)
    };
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
        let cwd_string = context.worktree.to_string_lossy().into_owned();
        devbox_git::run_mutating_with_cancel(
            &args,
            &cwd_string,
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
    let worktree_identity = filesystem_identity(&worktree, true).map_err(|_| error.to_string())?;
    let args = git_common_dir_args();
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = worktree.to_string_lossy().into_owned();
    let output = devbox_git::run_bounded(
        &args,
        &cwd,
        Duration::from_secs(5),
        MAX_REPOSITORY_PATH_BYTES + 2,
    )
    .map_err(|_| error.to_string())?;
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
    let common = Path::new(value);
    let common = if common.is_absolute() {
        common.to_path_buf()
    } else {
        worktree.join(common)
    };
    let common = common.canonicalize().map_err(|_| error.to_string())?;
    if !common.is_dir() {
        return Err(error.to_string());
    }
    let common_git_identity = filesystem_identity(&common, true).map_err(|_| error.to_string())?;
    // The worktree may have been exchanged while `rev-parse` ran. Compare the
    // exact directory object again before returning an operation authority.
    if filesystem_identity(&worktree, true).map_err(|_| error.to_string())? != worktree_identity {
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

fn revalidate_repository_context(
    expected: &RepositoryContext,
    error: &'static str,
) -> Result<(), String> {
    let current = repository_context_for_worktree(expected.worktree.clone(), error)?;
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
    let cwd = cwd.to_string_lossy().into_owned();
    match devbox_git::run_bounded_with_cancel(&args, &cwd, MUTATION_TIMEOUT, 128, cancellation) {
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
        let cwd = worktree.to_string_lossy().into_owned();
        let status =
            devbox_git::run_bounded(&args, &cwd, Duration::from_secs(5), MAX_STATUS_OUTPUT_BYTES)
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
        let cwd = worktree.to_string_lossy().into_owned();
        let out = devbox_git::run_bounded(
            &args,
            &cwd,
            Duration::from_secs(5),
            MAX_WORKTREE_OUTPUT_BYTES,
        )
        .map_err(|_| GIT_WORKTREE_ERROR.to_string())?;
        Ok(parse_worktrees(&out))
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
        let args = vec![
            "--no-pager".to_string(),
            "--no-optional-locks".to_string(),
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            branch,
            "--".to_string(),
            target.to_string_lossy().into_owned(),
        ];
        run_git_mutation(&args, &context.worktree)?;
        Ok(WorktreeCreate {
            path: target.to_string_lossy().into_owned(),
        })
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
            fs::read_to_string(local.join("fixture.txt")).unwrap(),
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
