//! Bounded, native `.vscode/tasks.json` preview and trust projection.
//!
//! This module never starts VS Code, an extension, a shell, or a task. It reads
//! one link-free source below a user-selected project root and projects only
//! the execution fields that Run Manager can own. The same function is used by
//! preview, apply, trust, enable, and the final pre-spawn revalidation path.

use crate::core::imports::{canonical_project_root, ensure_root_identity, safe_display_root};
use crate::core::models::TargetKind;
use devbox_filesystem::{filesystem_identity, open_filesystem_object, FilesystemIdentity};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::DefaultHasher, BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const WORKSPACE_TASK_SCHEMA_VERSION: u32 = 2;
pub const TASKS_JSON_RELATIVE_PATH: &str = ".vscode/tasks.json";
pub const MAX_TASK_SOURCE_BYTES: u64 = 512 * 1024;
pub const MAX_TASKS: usize = 128;
pub const MAX_TASK_LABEL_BYTES: usize = 256;
pub const MAX_TASK_STRING_BYTES: usize = 16 * 1024;
pub const MAX_TASK_ARGUMENTS: usize = 128;
pub const MAX_TASK_ARGV_BYTES: usize = 64 * 1024;
pub const MAX_ENVIRONMENT_KEYS: usize = 64;
pub const MAX_DEPENDENCY_EDGES: usize = 512;
pub const MAX_MATCHER_REGEXP_BYTES: usize = 16 * 1024;
pub const MAX_MATCHER_CAPTURE_GROUP: u32 = 32;
const MAX_REVISION_BYTES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTaskError {
    InvalidRoot,
    UnsafeSource,
    SourceUnavailable,
    SourceTooLarge,
    SourceChanged,
    InvalidJsonc,
    InvalidVersion,
    InvalidTasks,
    TooManyTasks,
    InvalidTarget,
}

impl std::fmt::Display for WorkspaceTaskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidRoot => "workspace-task-invalid-root",
            Self::UnsafeSource => "workspace-task-unsafe-source",
            Self::SourceUnavailable => "workspace-task-source-unavailable",
            Self::SourceTooLarge => "workspace-task-source-too-large",
            Self::SourceChanged => "workspace-task-source-changed",
            Self::InvalidJsonc => "workspace-task-invalid-jsonc",
            Self::InvalidVersion => "workspace-task-invalid-version",
            Self::InvalidTasks => "workspace-task-invalid-tasks",
            Self::TooManyTasks => "workspace-task-too-many-tasks",
            Self::InvalidTarget => "workspace-task-invalid-target",
        })
    }
}

impl std::error::Error for WorkspaceTaskError {}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTaskKind {
    Process,
    Shell,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTaskDependsOrder {
    #[default]
    Parallel,
    Sequence,
}

/// A deliberately small subset of VS Code's problem matcher. Named matchers,
/// multi-line patterns and background state machines require an extension host
/// and are therefore never projected into executable state.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProblemMatcher {
    pub regexp: String,
    pub file: u32,
    pub line: u32,
    pub column: Option<u32>,
    pub message: u32,
    pub severity: Option<u32>,
}

impl WorkspaceTaskKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Shell => "shell",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskItem {
    pub id: String,
    pub source_index: u32,
    pub label: String,
    pub status: String,
    pub task_kind: Option<WorkspaceTaskKind>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub environment_keys: Vec<String>,
    pub applied_override: Option<String>,
    pub depends_on: Vec<String>,
    pub depends_order: WorkspaceTaskDependsOrder,
    pub has_problem_matcher: bool,
    pub problem_matcher: Option<WorkspaceProblemMatcher>,
    pub blocked_reason: Option<String>,
}

impl WorkspaceTaskItem {
    pub fn is_ready_process(&self) -> bool {
        self.status == "ready"
            && self.task_kind == Some(WorkspaceTaskKind::Process)
            && self.blocked_reason.is_none()
            && self.command.is_some()
            && self.cwd.is_some()
    }

    pub fn is_ready_importable(&self) -> bool {
        self.status == "ready"
            && matches!(
                self.task_kind,
                Some(WorkspaceTaskKind::Process | WorkspaceTaskKind::Shell)
            )
            && self.blocked_reason.is_none()
            && self.command.is_some()
            && self.cwd.is_some()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskPlan {
    pub schema_version: u32,
    pub source_root: String,
    pub source_path: String,
    pub project_identity: String,
    pub revision: String,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    pub selected_platform: String,
    pub items: Vec<WorkspaceTaskItem>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskState {
    pub job_id: String,
    pub source_id: String,
    pub label: String,
    pub task_kind: WorkspaceTaskKind,
    pub source_root: String,
    pub revision: String,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    pub environment_keys: Vec<String>,
    pub applied_override: Option<String>,
    pub depends_on: Vec<String>,
    pub depends_order: WorkspaceTaskDependsOrder,
    pub has_problem_matcher: bool,
    pub trusted: bool,
    pub shell_trusted: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskApplyResult {
    pub source_id: String,
    pub created: u32,
    pub updated: u32,
    pub made_unavailable: u32,
    pub skipped_conflicts: u32,
}

/// Internal execution projection. It is deliberately not serializable: raw
/// executable/argv/cwd remain inside storage, verifier, and adapter layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTaskExecution {
    pub job_id: String,
    pub source_id: String,
    pub source_index: u32,
    pub label: String,
    pub task_kind: WorkspaceTaskKind,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub environment_keys: Vec<String>,
    pub depends_on: Vec<String>,
    pub depends_order: WorkspaceTaskDependsOrder,
    pub problem_matcher: Option<WorkspaceProblemMatcher>,
    pub source_root: String,
    pub project_identity: String,
    pub revision: String,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    pub trusted: bool,
    pub shell_trusted: bool,
    pub available: bool,
}

impl WorkspaceTaskPlan {
    pub fn validate_claim(
        &self,
        source_root: &str,
        project_identity: &str,
        revision: &str,
    ) -> Result<(), WorkspaceTaskError> {
        if source_root != self.source_root
            || project_identity != self.project_identity
            || revision != self.revision
            || !valid_digest(project_identity)
            || !valid_digest(revision)
        {
            return Err(WorkspaceTaskError::SourceChanged);
        }
        Ok(())
    }
}

struct SourceSnapshot {
    root: PathBuf,
    display_root: String,
    root_identity: FilesystemIdentity,
    file_identity: FilesystemIdentity,
    bytes: Vec<u8>,
}

pub fn preview_workspace_tasks(
    root: &Path,
    target_kind: TargetKind,
    target_distro: Option<&str>,
) -> Result<WorkspaceTaskPlan, WorkspaceTaskError> {
    validate_target(target_kind, target_distro)?;
    let source = read_source(root)?;
    let selected_platform = match target_kind {
        TargetKind::Windows => "windows",
        TargetKind::Wsl => "linux",
    };
    let target_root = target_workspace_root(&source.display_root, target_kind, target_distro)?;
    let project_identity = identity_digest(source.root_identity, b"project");
    let revision = source_revision(
        source.root_identity,
        source.file_identity,
        &source.bytes,
        target_kind,
        target_distro,
    );
    let document = parse_jsonc(&source.bytes)?;
    let object = document
        .as_object()
        .ok_or(WorkspaceTaskError::InvalidTasks)?;
    if object.get("version").and_then(Value::as_str) != Some("2.0.0") {
        return Err(WorkspaceTaskError::InvalidVersion);
    }
    let tasks = object
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or(WorkspaceTaskError::InvalidTasks)?;
    if tasks.len() > MAX_TASKS {
        return Err(WorkspaceTaskError::TooManyTasks);
    }

    let mut seen_labels = BTreeSet::new();
    let mut duplicate_labels = BTreeSet::new();
    let labels = tasks
        .iter()
        .map(|task| task.get("label").and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    for label in labels.iter().flatten() {
        if !seen_labels.insert(label.clone()) {
            duplicate_labels.insert(label.clone());
        }
    }

    let mut items = Vec::with_capacity(tasks.len());
    for (index, task) in tasks.iter().enumerate() {
        let duplicate = labels[index]
            .as_ref()
            .is_some_and(|label| duplicate_labels.contains(label));
        items.push(project_task(
            task,
            index,
            &revision,
            selected_platform,
            &target_root,
            target_kind,
            duplicate,
        ));
    }
    validate_dependency_graph(&mut items);
    ensure_root_identity(&source.root, source.root_identity)
        .map_err(|_| WorkspaceTaskError::SourceChanged)?;
    if filesystem_identity(source.root.join(TASKS_JSON_RELATIVE_PATH), false)
        .map_err(|_| WorkspaceTaskError::SourceChanged)?
        != source.file_identity
    {
        return Err(WorkspaceTaskError::SourceChanged);
    }

    Ok(WorkspaceTaskPlan {
        schema_version: WORKSPACE_TASK_SCHEMA_VERSION,
        source_root: source.display_root,
        source_path: TASKS_JSON_RELATIVE_PATH.to_owned(),
        project_identity,
        revision,
        target_kind,
        target_distro: target_distro.map(str::to_owned),
        selected_platform: selected_platform.to_owned(),
        items,
    })
}

pub fn verify_workspace_task_plan(
    root: &Path,
    target_kind: TargetKind,
    target_distro: Option<&str>,
    source_root: &str,
    project_identity: &str,
    revision: &str,
) -> Result<WorkspaceTaskPlan, WorkspaceTaskError> {
    if !valid_digest(project_identity) || !valid_digest(revision) {
        return Err(WorkspaceTaskError::SourceChanged);
    }
    let plan = preview_workspace_tasks(root, target_kind, target_distro)?;
    plan.validate_claim(source_root, project_identity, revision)?;
    Ok(plan)
}

/// Re-read a stored task's source and prove that the approved argv is still
/// the exact projection of the claimed root/revision. Database corruption or
/// an outdated parser row never degrades to a shell command.
pub fn revalidate_workspace_task_execution(
    execution: &WorkspaceTaskExecution,
) -> Result<WorkspaceTaskPlan, WorkspaceTaskError> {
    if !execution.available
        || !execution.trusted
        || (execution.task_kind == WorkspaceTaskKind::Shell && !execution.shell_trusted)
    {
        return Err(WorkspaceTaskError::SourceChanged);
    }
    verify_workspace_task_execution(execution)
}

/// Verify the persisted projection against the current source without making
/// a trust decision. The explicit trust command uses this path before it flips
/// the durable source bit; execution uses the stricter wrapper above.
pub fn verify_workspace_task_execution(
    execution: &WorkspaceTaskExecution,
) -> Result<WorkspaceTaskPlan, WorkspaceTaskError> {
    verify_workspace_task_executions(std::slice::from_ref(execution))
}

/// Verify a whole source projection with one bounded source read. This is used
/// before dependency planning so a 128-node graph does not reopen and reparse
/// the same file 128 times.
pub fn verify_workspace_task_executions(
    executions: &[WorkspaceTaskExecution],
) -> Result<WorkspaceTaskPlan, WorkspaceTaskError> {
    let first = executions
        .first()
        .ok_or(WorkspaceTaskError::SourceChanged)?;
    if executions.len() > MAX_TASKS
        || executions.iter().any(|execution| {
            execution.source_id != first.source_id
                || execution.source_root != first.source_root
                || execution.project_identity != first.project_identity
                || execution.revision != first.revision
                || execution.target_kind != first.target_kind
                || execution.target_distro != first.target_distro
        })
    {
        return Err(WorkspaceTaskError::SourceChanged);
    }
    let plan = verify_workspace_task_plan(
        Path::new(&first.source_root),
        first.target_kind,
        first.target_distro.as_deref(),
        &first.source_root,
        &first.project_identity,
        &first.revision,
    )?;
    for execution in executions {
        let item = plan
            .items
            .iter()
            .find(|item| {
                item.source_index == execution.source_index && item.label == execution.label
            })
            .ok_or(WorkspaceTaskError::SourceChanged)?;
        if !item.is_ready_importable()
            || item.task_kind != Some(execution.task_kind)
            || item.command.as_deref() != Some(execution.command.as_str())
            || item.args != execution.args
            || item.cwd.as_deref() != Some(execution.cwd.as_str())
            || item.environment_keys != execution.environment_keys
            || item.depends_on != execution.depends_on
            || item.depends_order != execution.depends_order
            || item.problem_matcher != execution.problem_matcher
        {
            return Err(WorkspaceTaskError::SourceChanged);
        }
    }
    Ok(plan)
}

fn validate_target(
    target_kind: TargetKind,
    target_distro: Option<&str>,
) -> Result<(), WorkspaceTaskError> {
    match (target_kind, target_distro) {
        (TargetKind::Windows, None) => Ok(()),
        (TargetKind::Wsl, Some(distro)) => devbox_wsl::distro::validate_distro_name(distro)
            .map_err(|_| WorkspaceTaskError::InvalidTarget),
        _ => Err(WorkspaceTaskError::InvalidTarget),
    }
}

fn read_source(root: &Path) -> Result<SourceSnapshot, WorkspaceTaskError> {
    let canonical_root =
        canonical_project_root(root).map_err(|_| WorkspaceTaskError::InvalidRoot)?;
    let display_root =
        safe_display_root(&canonical_root, root).map_err(|_| WorkspaceTaskError::UnsafeSource)?;
    let root_identity =
        filesystem_identity(&canonical_root, true).map_err(|_| WorkspaceTaskError::UnsafeSource)?;
    let vscode = canonical_root.join(".vscode");
    let path = vscode.join("tasks.json");
    devbox_filesystem::ensure_no_links(&path).map_err(|_| WorkspaceTaskError::UnsafeSource)?;
    let vscode_metadata =
        fs::symlink_metadata(&vscode).map_err(|_| WorkspaceTaskError::SourceUnavailable)?;
    let file_metadata =
        fs::symlink_metadata(&path).map_err(|_| WorkspaceTaskError::SourceUnavailable)?;
    if vscode_metadata.file_type().is_symlink()
        || !vscode_metadata.is_dir()
        || file_metadata.file_type().is_symlink()
        || !file_metadata.is_file()
    {
        return Err(WorkspaceTaskError::UnsafeSource);
    }
    if file_metadata.len() > MAX_TASK_SOURCE_BYTES {
        return Err(WorkspaceTaskError::SourceTooLarge);
    }
    let canonical_file = path
        .canonicalize()
        .map_err(|_| WorkspaceTaskError::UnsafeSource)?;
    if canonical_file.parent().and_then(Path::parent) != Some(canonical_root.as_path())
        || canonical_file
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(".vscode")
        || canonical_file.file_name().and_then(|name| name.to_str()) != Some("tasks.json")
    {
        return Err(WorkspaceTaskError::UnsafeSource);
    }
    let vscode_identity =
        filesystem_identity(&vscode, true).map_err(|_| WorkspaceTaskError::UnsafeSource)?;
    let (file, file_identity) = open_filesystem_object(&canonical_file, false)
        .map_err(|_| WorkspaceTaskError::SourceUnavailable)?;
    let opened = file
        .metadata()
        .map_err(|_| WorkspaceTaskError::SourceUnavailable)?;
    if !opened.is_file() || opened.len() != file_metadata.len() {
        return Err(WorkspaceTaskError::SourceChanged);
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    let mut bounded = file.take(MAX_TASK_SOURCE_BYTES + 1);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|_| WorkspaceTaskError::SourceUnavailable)?;
    if bytes.len() as u64 > MAX_TASK_SOURCE_BYTES {
        return Err(WorkspaceTaskError::SourceTooLarge);
    }
    if filesystem_identity(&canonical_root, true).map_err(|_| WorkspaceTaskError::SourceChanged)?
        != root_identity
        || filesystem_identity(&vscode, true).map_err(|_| WorkspaceTaskError::SourceChanged)?
            != vscode_identity
        || filesystem_identity(&canonical_file, false)
            .map_err(|_| WorkspaceTaskError::SourceChanged)?
            != file_identity
    {
        return Err(WorkspaceTaskError::SourceChanged);
    }
    Ok(SourceSnapshot {
        root: canonical_root,
        display_root,
        root_identity,
        file_identity,
        bytes,
    })
}

fn parse_jsonc(bytes: &[u8]) -> Result<Value, WorkspaceTaskError> {
    let text = std::str::from_utf8(bytes).map_err(|_| WorkspaceTaskError::InvalidJsonc)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let without_comments = strip_jsonc_comments(text)?;
    let strict = strip_trailing_commas(&without_comments);
    serde_json::from_str(&strict).map_err(|_| WorkspaceTaskError::InvalidJsonc)
}

fn strip_jsonc_comments(input: &str) -> Result<String, WorkspaceTaskError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        String,
        Escape,
        LineComment,
        BlockComment,
    }

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut state = State::Normal;
    let mut index = 0usize;
    while index < bytes.len() {
        let current = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Normal if current == b'"' => {
                output.push(current);
                state = State::String;
                index += 1;
            }
            State::Normal if current == b'/' && next == Some(b'/') => {
                output.extend_from_slice(b"  ");
                state = State::LineComment;
                index += 2;
            }
            State::Normal if current == b'/' && next == Some(b'*') => {
                output.extend_from_slice(b"  ");
                state = State::BlockComment;
                index += 2;
            }
            State::Normal => {
                output.push(current);
                index += 1;
            }
            State::String if current == b'\\' => {
                output.push(current);
                state = State::Escape;
                index += 1;
            }
            State::String if current == b'"' => {
                output.push(current);
                state = State::Normal;
                index += 1;
            }
            State::String => {
                output.push(current);
                index += 1;
            }
            State::Escape => {
                output.push(current);
                state = State::String;
                index += 1;
            }
            State::LineComment if matches!(current, b'\n' | b'\r') => {
                output.push(current);
                state = State::Normal;
                index += 1;
            }
            State::LineComment => {
                output.push(b' ');
                index += 1;
            }
            State::BlockComment if current == b'*' && next == Some(b'/') => {
                output.extend_from_slice(b"  ");
                state = State::Normal;
                index += 2;
            }
            State::BlockComment => {
                output.push(if matches!(current, b'\n' | b'\r') {
                    current
                } else {
                    b' '
                });
                index += 1;
            }
        }
    }
    if matches!(state, State::String | State::Escape | State::BlockComment) {
        return Err(WorkspaceTaskError::InvalidJsonc);
    }
    String::from_utf8(output).map_err(|_| WorkspaceTaskError::InvalidJsonc)
}

fn strip_trailing_commas(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = bytes.to_vec();
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
            continue;
        }
        if byte != b',' {
            continue;
        }
        let next = bytes[index + 1..]
            .iter()
            .copied()
            .find(|candidate| !candidate.is_ascii_whitespace());
        if matches!(next, Some(b'}' | b']')) {
            output[index] = b' ';
        }
    }
    String::from_utf8(output).expect("input was valid UTF-8")
}

#[allow(clippy::too_many_arguments)]
fn project_task(
    raw: &Value,
    index: usize,
    revision: &str,
    platform: &str,
    target_root: &str,
    target_kind: TargetKind,
    duplicate_label: bool,
) -> WorkspaceTaskItem {
    let source_index = u32::try_from(index).unwrap_or(u32::MAX);
    let fallback_label = format!("Task {}", index + 1);
    let raw_label = raw
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or(&fallback_label);
    let label = bounded_display_label(raw_label, &fallback_label);
    let id = task_id(revision, source_index, &label);
    let blocked = |reason: &str, kind: Option<WorkspaceTaskKind>| WorkspaceTaskItem {
        id: id.clone(),
        source_index,
        label: label.clone(),
        status: "blocked".to_owned(),
        task_kind: kind,
        command: None,
        args: Vec::new(),
        cwd: None,
        environment_keys: Vec::new(),
        applied_override: None,
        has_problem_matcher: false,
        depends_on: Vec::new(),
        depends_order: WorkspaceTaskDependsOrder::Parallel,
        problem_matcher: None,
        blocked_reason: Some(reason.to_owned()),
    };
    let Some(base) = raw.as_object() else {
        return blocked("invalid-task", None);
    };
    if raw.get("label").and_then(Value::as_str).is_none()
        || raw_label.is_empty()
        || raw_label.len() > MAX_TASK_LABEL_BYTES
        || raw_label.chars().any(char::is_control)
    {
        return blocked("invalid-label", None);
    }
    if duplicate_label {
        return blocked("duplicate-label", None);
    }
    let (task, applied_override) = match merge_platform_override(base, platform) {
        Ok(value) => value,
        Err(reason) => return blocked(reason, None),
    };
    let kind = match task.get("type").and_then(Value::as_str) {
        Some("process") => WorkspaceTaskKind::Process,
        Some("shell") => WorkspaceTaskKind::Shell,
        Some(_) => return blocked("unsupported-task-type", None),
        None => return blocked("missing-task-type", None),
    };
    let (depends_on, depends_order) = match project_dependencies(&task) {
        Ok(value) => value,
        Err(reason) => return blocked(reason, Some(kind)),
    };
    if task.get("isBackground").is_some() {
        return blocked("background-task-unsupported", Some(kind));
    }
    if task.get("runOptions").is_some() {
        return blocked("run-options-unsupported", Some(kind));
    }
    let Some(command) = task.get("command").and_then(Value::as_str) else {
        return blocked("invalid-command", Some(kind));
    };
    let command = match resolve_variables(command, target_root, target_kind) {
        Ok(value) => value,
        Err(reason) => return blocked(reason, Some(kind)),
    };
    if command.is_empty() || command.len() > MAX_TASK_STRING_BYTES {
        return blocked("invalid-command", Some(kind));
    }
    let mut args = Vec::new();
    if let Some(raw_args) = task.get("args") {
        let Some(raw_args) = raw_args.as_array() else {
            return blocked("invalid-arguments", Some(kind));
        };
        if raw_args.len() > MAX_TASK_ARGUMENTS {
            return blocked("arguments-too-large", Some(kind));
        }
        for raw_arg in raw_args {
            let Some(raw_arg) = raw_arg.as_str() else {
                return blocked("quoted-argument-unsupported", Some(kind));
            };
            let value = match resolve_variables(raw_arg, target_root, target_kind) {
                Ok(value) => value,
                Err(reason) => return blocked(reason, Some(kind)),
            };
            if value.len() > MAX_TASK_STRING_BYTES {
                return blocked("arguments-too-large", Some(kind));
            }
            args.push(value);
        }
    }
    let argv_bytes = command
        .len()
        .saturating_add(args.iter().map(String::len).sum::<usize>());
    if argv_bytes > MAX_TASK_ARGV_BYTES {
        return blocked("arguments-too-large", Some(kind));
    }

    let (cwd, environment_keys) = match project_options(&task, target_root, target_kind) {
        Ok(value) => value,
        Err(reason) => return blocked(reason, Some(kind)),
    };
    let problem_matcher = match project_problem_matcher(task.get("problemMatcher")) {
        Ok(value) => value,
        Err(reason) => return blocked(reason, Some(kind)),
    };
    WorkspaceTaskItem {
        id,
        source_index,
        label,
        status: "ready".to_owned(),
        task_kind: Some(kind),
        command: Some(command),
        args,
        cwd: Some(cwd),
        environment_keys,
        applied_override,
        depends_on,
        depends_order,
        has_problem_matcher: problem_matcher.is_some(),
        problem_matcher,
        blocked_reason: None,
    }
}

fn project_dependencies(
    task: &Map<String, Value>,
) -> Result<(Vec<String>, WorkspaceTaskDependsOrder), &'static str> {
    let mut dependencies = Vec::new();
    if let Some(value) = task.get("dependsOn") {
        match value {
            Value::String(label) => dependencies.push(label.clone()),
            Value::Array(labels) => {
                if labels.len() > MAX_TASKS {
                    return Err("dependency-graph-too-large");
                }
                for label in labels {
                    let Some(label) = label.as_str() else {
                        return Err("invalid-dependency");
                    };
                    dependencies.push(label.to_owned());
                }
            }
            _ => return Err("invalid-dependency"),
        }
    }
    let mut unique = BTreeSet::new();
    for dependency in &dependencies {
        if dependency.is_empty()
            || dependency.len() > MAX_TASK_LABEL_BYTES
            || dependency.chars().any(char::is_control)
            || !unique.insert(dependency.clone())
        {
            return Err("invalid-dependency");
        }
    }
    let order = match task.get("dependsOrder") {
        None => WorkspaceTaskDependsOrder::Parallel,
        Some(Value::String(value)) if value == "parallel" => WorkspaceTaskDependsOrder::Parallel,
        Some(Value::String(value)) if value == "sequence" => WorkspaceTaskDependsOrder::Sequence,
        Some(_) => return Err("invalid-dependency-order"),
    };
    if dependencies.is_empty() && task.get("dependsOrder").is_some() {
        return Err("dependency-order-without-dependency");
    }
    Ok((dependencies, order))
}

fn project_problem_matcher(
    value: Option<&Value>,
) -> Result<Option<WorkspaceProblemMatcher>, &'static str> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_string() || value.as_array().is_some() {
        return Err("named-problem-matcher-unsupported");
    }
    let matcher = value.as_object().ok_or("invalid-problem-matcher")?;
    if matcher.get("background").is_some() || matcher.get("watching").is_some() {
        return Err("background-problem-matcher-unsupported");
    }
    if matcher.keys().any(|key| {
        !matches!(
            key.as_str(),
            "owner" | "source" | "applyTo" | "fileLocation" | "pattern" | "severity"
        )
    }) {
        return Err("unsupported-problem-matcher-field");
    }
    if matcher
        .get("fileLocation")
        .is_some_and(|location| !matches!(location, Value::String(value) if value == "relative"))
    {
        return Err("unsupported-problem-matcher-location");
    }
    let pattern = matcher
        .get("pattern")
        .and_then(Value::as_object)
        .ok_or("invalid-problem-matcher")?;
    if pattern.keys().any(|key| {
        !matches!(
            key.as_str(),
            "regexp" | "file" | "line" | "column" | "message" | "severity"
        )
    }) {
        return Err("unsupported-problem-matcher-field");
    }
    let regexp = pattern
        .get("regexp")
        .and_then(Value::as_str)
        .ok_or("invalid-problem-matcher")?;
    if regexp.is_empty()
        || regexp.len() > MAX_MATCHER_REGEXP_BYTES
        || regexp.chars().any(char::is_control)
    {
        return Err("invalid-problem-matcher");
    }
    let compiled = regex::Regex::new(regexp).map_err(|_| "invalid-problem-matcher")?;
    let capture = |name: &str, required: bool| -> Result<Option<u32>, &'static str> {
        let value = pattern.get(name).and_then(Value::as_u64);
        if required && value.is_none() {
            return Err("invalid-problem-matcher");
        }
        let value = value
            .map(|value| u32::try_from(value).map_err(|_| "invalid-problem-matcher"))
            .transpose()?;
        if value.is_some_and(|value| {
            value == 0
                || value > MAX_MATCHER_CAPTURE_GROUP
                || usize::try_from(value)
                    .ok()
                    .is_none_or(|index| index >= compiled.captures_len())
        }) {
            return Err("invalid-problem-matcher");
        }
        Ok(value)
    };
    Ok(Some(WorkspaceProblemMatcher {
        regexp: regexp.to_owned(),
        file: capture("file", true)?.expect("required capture"),
        line: capture("line", true)?.expect("required capture"),
        column: capture("column", false)?,
        message: capture("message", true)?.expect("required capture"),
        severity: capture("severity", false)?,
    }))
}

fn validate_dependency_graph(items: &mut [WorkspaceTaskItem]) {
    let edge_count = items
        .iter()
        .map(|item| item.depends_on.len())
        .sum::<usize>();
    if edge_count > MAX_DEPENDENCY_EDGES {
        for item in items.iter_mut().filter(|item| !item.depends_on.is_empty()) {
            block_projected_item(item, "dependency-graph-too-large");
        }
        return;
    }

    let labels = items
        .iter()
        .enumerate()
        .map(|(index, item)| (item.label.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for (index, item) in items.iter_mut().enumerate() {
        if item.status != "ready" {
            continue;
        }
        let invalid = item.depends_on.iter().any(|dependency| {
            labels
                .get(dependency)
                .is_none_or(|dependency_index| *dependency_index == index)
        });
        if invalid {
            block_projected_item(item, "invalid-dependency");
        }
    }

    // Kahn's algorithm leaves cycle members and nodes which depend on a cycle
    // unvisited. Blocking the whole remainder gives a deterministic, fail-
    // closed preview without guessing which edge the user intended.
    let mut indegree = vec![0usize; items.len()];
    let mut dependents = vec![Vec::<usize>::new(); items.len()];
    for (index, item) in items.iter().enumerate() {
        if item.status != "ready" {
            continue;
        }
        for dependency in &item.depends_on {
            if let Some(dependency_index) = labels.get(dependency).copied() {
                if items[dependency_index].status == "ready" {
                    indegree[index] = indegree[index].saturating_add(1);
                    dependents[dependency_index].push(index);
                }
            }
        }
    }
    let mut ready = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| {
            (items[index].status == "ready" && *degree == 0).then_some(index)
        })
        .collect::<Vec<_>>();
    let mut visited = vec![false; items.len()];
    while let Some(index) = ready.pop() {
        visited[index] = true;
        for dependent in &dependents[index] {
            indegree[*dependent] = indegree[*dependent].saturating_sub(1);
            if indegree[*dependent] == 0 {
                ready.push(*dependent);
            }
        }
    }
    for index in 0..items.len() {
        if items[index].status == "ready" && !visited[index] {
            block_projected_item(&mut items[index], "dependency-cycle");
        }
    }

    // A task cannot be imported when any required predecessor is blocked.
    // Iterate to a fixed point so an unavailable leaf propagates through a
    // longer dependency chain.
    loop {
        let blocked_labels = items
            .iter()
            .filter(|item| item.status != "ready")
            .map(|item| item.label.clone())
            .collect::<BTreeSet<_>>();
        let affected = items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                (item.status == "ready"
                    && item
                        .depends_on
                        .iter()
                        .any(|dependency| blocked_labels.contains(dependency)))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if affected.is_empty() {
            break;
        }
        for index in affected {
            block_projected_item(&mut items[index], "dependency-unavailable");
        }
    }
}

fn block_projected_item(item: &mut WorkspaceTaskItem, reason: &str) {
    item.status = "blocked".to_owned();
    item.command = None;
    item.args.clear();
    item.cwd = None;
    item.environment_keys.clear();
    item.problem_matcher = None;
    item.has_problem_matcher = false;
    item.blocked_reason = Some(reason.to_owned());
}

fn merge_platform_override(
    base: &Map<String, Value>,
    platform: &str,
) -> Result<(Map<String, Value>, Option<String>), &'static str> {
    let mut merged = base.clone();
    for key in ["windows", "linux", "osx"] {
        merged.remove(key);
    }
    let Some(override_value) = base.get(platform) else {
        return Ok((merged, None));
    };
    let Some(override_object) = override_value.as_object() else {
        return Err("invalid-os-override");
    };
    for (key, value) in override_object {
        if key == "options" {
            let Some(incoming) = value.as_object() else {
                return Err("invalid-options");
            };
            let mut options = merged
                .get("options")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            options.extend(incoming.clone());
            merged.insert(key.clone(), Value::Object(options));
        } else if matches!(
            key.as_str(),
            "type" | "command" | "args" | "dependsOn" | "dependsOrder" | "problemMatcher"
        ) {
            merged.insert(key.clone(), value.clone());
        } else if !matches!(key.as_str(), "presentation" | "group" | "detail") {
            return Err("unsupported-os-override-field");
        }
    }
    Ok((merged, Some(platform.to_owned())))
}

fn project_options(
    task: &Map<String, Value>,
    target_root: &str,
    target_kind: TargetKind,
) -> Result<(String, Vec<String>), &'static str> {
    let Some(options) = task.get("options") else {
        return Ok((target_root.to_owned(), Vec::new()));
    };
    let Some(options) = options.as_object() else {
        return Err("invalid-options");
    };
    if options.get("shell").is_some() {
        return Err("custom-shell-unsupported");
    }
    for key in options.keys() {
        if !matches!(key.as_str(), "cwd" | "env") {
            return Err("unsupported-options-field");
        }
    }
    let cwd = match options.get("cwd") {
        Some(value) => {
            let Some(value) = value.as_str() else {
                return Err("invalid-cwd");
            };
            let value = resolve_variables(value, target_root, target_kind)?;
            normalize_task_cwd(target_root, &value, target_kind)?
        }
        None => target_root.to_owned(),
    };
    let mut environment_keys = Vec::new();
    if let Some(environment) = options.get("env") {
        let Some(environment) = environment.as_object() else {
            return Err("invalid-environment");
        };
        if environment.len() > MAX_ENVIRONMENT_KEYS {
            return Err("environment-too-large");
        }
        let mut folded = BTreeSet::new();
        for (key, value) in environment {
            if !value.is_string()
                || crate::core::shell::validate_environment_key(key).is_err()
                || !folded.insert(key.to_ascii_uppercase())
            {
                return Err("invalid-environment");
            }
            environment_keys.push(key.clone());
        }
        environment_keys.sort();
    }
    Ok((cwd, environment_keys))
}

fn resolve_variables(
    input: &str,
    target_root: &str,
    target_kind: TargetKind,
) -> Result<String, &'static str> {
    if input.len() > MAX_TASK_STRING_BYTES || input.chars().any(char::is_control) {
        return Err("invalid-variable-value");
    }
    let safe_root = devbox_filesystem::parse_safe_project_path(target_root)
        .ok_or("invalid-workspace-folder")?;
    let separator = match target_kind {
        TargetKind::Windows => "\\",
        TargetKind::Wsl => "/",
    };
    let replacements = BTreeMap::from([
        ("workspaceFolder", safe_root.as_str()),
        ("workspaceFolderBasename", safe_root.name()),
        ("pathSeparator", separator),
        ("/", separator),
    ]);
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0usize;
    while let Some(relative) = input[cursor..].find("${") {
        let start = cursor + relative;
        output.push_str(&input[cursor..start]);
        let value_start = start + 2;
        let Some(close_relative) = input[value_start..].find('}') else {
            return Err("unsupported-variable");
        };
        let close = value_start + close_relative;
        let name = &input[value_start..close];
        let replacement = replacements.get(name).ok_or("unsupported-variable")?;
        output.push_str(replacement);
        if output.len() > MAX_TASK_STRING_BYTES {
            return Err("variable-result-too-large");
        }
        cursor = close + 1;
    }
    output.push_str(&input[cursor..]);
    if output.len() > MAX_TASK_STRING_BYTES || output.chars().any(char::is_control) {
        return Err("variable-result-too-large");
    }
    Ok(output)
}

fn normalize_task_cwd(
    root: &str,
    requested: &str,
    target_kind: TargetKind,
) -> Result<String, &'static str> {
    let separator = match target_kind {
        TargetKind::Windows => '\\',
        TargetKind::Wsl => '/',
    };
    let absolute = devbox_filesystem::parse_safe_project_path(requested)
        .map(devbox_filesystem::SafeProjectPath::into_string)
        .or_else(|| {
            let components = requested
                .split(['/', '\\'])
                .filter(|component| !component.is_empty())
                .collect::<Vec<_>>();
            (!components.is_empty()
                && components.iter().all(|component| {
                    !matches!(*component, "." | "..") && !component.chars().any(char::is_control)
                }))
            .then(|| {
                format!(
                    "{}{}{}",
                    root.trim_end_matches(['/', '\\']),
                    separator,
                    components.join(&separator.to_string())
                )
            })
        })
        .ok_or("invalid-cwd")?;
    let root = devbox_filesystem::parse_safe_project_path(root).ok_or("invalid-cwd")?;
    let cwd = devbox_filesystem::parse_safe_project_path(&absolute).ok_or("invalid-cwd")?;
    let root_identity = root.identity();
    let cwd_identity = cwd.identity();
    let contained = cwd_identity == root_identity
        || cwd_identity
            .strip_prefix(root_identity)
            .is_some_and(|suffix| suffix.starts_with(['/', '\\']));
    contained
        .then(|| cwd.into_string())
        .ok_or("cwd-outside-project")
}

fn target_workspace_root(
    display_root: &str,
    target_kind: TargetKind,
    target_distro: Option<&str>,
) -> Result<String, WorkspaceTaskError> {
    if target_kind == TargetKind::Windows {
        return devbox_filesystem::parse_safe_project_path(display_root)
            .map(devbox_filesystem::SafeProjectPath::into_string)
            .ok_or(WorkspaceTaskError::InvalidRoot);
    }
    let distro = target_distro.ok_or(WorkspaceTaskError::InvalidTarget)?;
    if display_root.starts_with('/') {
        return devbox_filesystem::parse_safe_project_path(display_root)
            .map(devbox_filesystem::SafeProjectPath::into_string)
            .ok_or(WorkspaceTaskError::InvalidRoot);
    }
    if let Some(unc) = devbox_wsl::path::parse_wsl_unc_path(display_root)
        .map_err(|_| WorkspaceTaskError::InvalidRoot)?
    {
        if !unc.distro().eq_ignore_ascii_case(distro) {
            return Err(WorkspaceTaskError::InvalidTarget);
        }
        return Ok(unc.linux_path().to_owned());
    }
    devbox_wsl::path::windows_to_wsl(display_root).map_err(|_| WorkspaceTaskError::InvalidRoot)
}

fn bounded_display_label(value: &str, fallback: &str) -> String {
    if value.is_empty() || value.len() > MAX_TASK_LABEL_BYTES || value.chars().any(char::is_control)
    {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

fn identity_digest(identity: FilesystemIdentity, domain: &[u8]) -> String {
    let mut opaque = DefaultHasher::new();
    identity.hash(&mut opaque);
    let mut digest = Sha256::new();
    digest.update(b"run-manager-workspace-task-identity-v1");
    digest.update((domain.len() as u64).to_le_bytes());
    digest.update(domain);
    digest.update(opaque.finish().to_le_bytes());
    hex_digest(digest.finalize())
}

fn source_revision(
    root_identity: FilesystemIdentity,
    file_identity: FilesystemIdentity,
    bytes: &[u8],
    target_kind: TargetKind,
    target_distro: Option<&str>,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"run-manager-workspace-task-source-v2");
    digest.update(WORKSPACE_TASK_SCHEMA_VERSION.to_le_bytes());
    digest.update(identity_digest(root_identity, b"root"));
    digest.update(identity_digest(file_identity, b"file"));
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    digest.update(target_kind.as_str().as_bytes());
    if let Some(distro) = target_distro {
        digest.update((distro.len() as u64).to_le_bytes());
        digest.update(distro.as_bytes());
    }
    hex_digest(digest.finalize())
}

fn task_id(revision: &str, index: u32, label: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"run-manager-workspace-task-item-v1");
    digest.update(revision.as_bytes());
    digest.update(index.to_le_bytes());
    digest.update(label.as_bytes());
    hex_digest(digest.finalize())[..32].to_owned()
}

fn valid_digest(value: &str) -> bool {
    value.len() == MAX_REVISION_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    let mut output = String::with_capacity(bytes.as_ref().len() * 2);
    for byte in bytes.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_tasks(root: &Path, text: &str) {
        fs::create_dir_all(root.join(".vscode")).unwrap();
        fs::write(root.join(TASKS_JSON_RELATIVE_PATH), text).unwrap();
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_preserve_string_literals() {
        let value = parse_jsonc(
            br#"{
              // comment
              "version": "2.0.0",
              "tasks": [{ "label": "http://x/*not-comment*/", "type": "process", "command": "echo", },],
            }"#,
        )
        .unwrap();
        assert_eq!(value["tasks"][0]["label"], "http://x/*not-comment*/");
    }

    #[test]
    fn malformed_jsonc_is_rejected() {
        assert_eq!(
            parse_jsonc(br#"{"version":"2.0.0",/*"#),
            Err(WorkspaceTaskError::InvalidJsonc)
        );
        assert_eq!(
            parse_jsonc(br#"{"version":"2.0.0","tasks":["#),
            Err(WorkspaceTaskError::InvalidJsonc)
        );
    }

    #[test]
    fn process_preview_applies_linux_override_and_allowlisted_variables() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{
              "version": "2.0.0",
              "tasks": [{
                "label": "build",
                "type": "process",
                "command": "node",
                "args": ["${workspaceFolder}/tool.js", "${workspaceFolderBasename}", "${/}"],
                "options": {"env": {"TOKEN": "not-imported"}},
                "linux": {"command": "node-linux"},
                "windows": {"command": "node.exe"}
              }]
            }"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        let task = &plan.items[0];
        assert!(task.is_ready_process());
        assert_eq!(task.command.as_deref(), Some("node-linux"));
        assert_eq!(task.applied_override.as_deref(), Some("linux"));
        assert_eq!(task.environment_keys, ["TOKEN"]);
        assert!(task.args[0].ends_with("/tool.js"));
        assert_eq!(task.args[2], "/");
    }

    #[test]
    fn dangerous_variables_and_extension_tasks_are_blocked_while_shell_is_reviewable() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{
              "version": "2.0.0",
              "tasks": [
                {"label":"env", "type":"process", "command":"${env:PATH}"},
                {"label":"input", "type":"process", "command":"x", "args":["${input:name}"]},
                {"label":"extension", "type":"npm", "script":"build"},
                {"label":"shell", "type":"shell", "command":"echo ok"},
                {"label":"graph", "type":"process", "command":"x", "dependsOn":"env"}
              ]
            }"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        let reasons = plan
            .items
            .iter()
            .map(|item| item.blocked_reason.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            [
                Some("unsupported-variable"),
                Some("unsupported-variable"),
                Some("unsupported-task-type"),
                None,
                Some("dependency-unavailable")
            ]
        );
        assert!(plan.items[3].is_ready_importable());
        assert_eq!(plan.items[3].task_kind, Some(WorkspaceTaskKind::Shell));
    }

    #[test]
    fn background_run_options_and_orphan_dependency_order_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{
              "version": "2.0.0",
              "tasks": [
                {"label":"background", "type":"process", "command":"x", "isBackground":true},
                {"label":"run-options", "type":"process", "command":"x", "runOptions":{"instanceLimit":1}},
                {"label":"order", "type":"process", "command":"x", "dependsOrder":"sequence"}
              ]
            }"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        assert_eq!(
            plan.items
                .iter()
                .map(|item| item.blocked_reason.as_deref().unwrap())
                .collect::<Vec<_>>(),
            [
                "background-task-unsupported",
                "run-options-unsupported",
                "dependency-order-without-dependency"
            ]
        );
    }

    #[test]
    fn dependency_dag_and_explicit_problem_matcher_are_normalized() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{
              "version": "2.0.0",
              "tasks": [
                {"label":"compile", "type":"process", "command":"cargo", "args":["check"],
                 "problemMatcher":{"fileLocation":"relative","pattern":{
                   "regexp":"^([^:]+):(\\d+):(\\d+): (error|warning): (.+)$",
                   "file":1,"line":2,"column":3,"severity":4,"message":5}}},
                {"label":"verify", "type":"process", "command":"cargo", "args":["test"],
                 "dependsOn":["compile"], "dependsOrder":"sequence"}
              ]
            }"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        assert!(plan
            .items
            .iter()
            .all(WorkspaceTaskItem::is_ready_importable));
        assert_eq!(plan.items[1].depends_on, ["compile"]);
        assert_eq!(
            plan.items[1].depends_order,
            WorkspaceTaskDependsOrder::Sequence
        );
        let matcher = plan.items[0].problem_matcher.as_ref().unwrap();
        assert_eq!(
            (matcher.file, matcher.line, matcher.column),
            (1, 2, Some(3))
        );
        assert_eq!((matcher.severity, matcher.message), (Some(4), 5));
    }

    #[test]
    fn missing_self_and_cyclic_dependencies_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{"version":"2.0.0","tasks":[
              {"label":"missing","type":"process","command":"x","dependsOn":"absent"},
              {"label":"self","type":"process","command":"x","dependsOn":"self"},
              {"label":"a","type":"process","command":"x","dependsOn":"b"},
              {"label":"b","type":"process","command":"x","dependsOn":"a"}
            ]}"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        assert_eq!(
            plan.items[0].blocked_reason.as_deref(),
            Some("invalid-dependency")
        );
        assert_eq!(
            plan.items[1].blocked_reason.as_deref(),
            Some("invalid-dependency")
        );
        assert_eq!(
            plan.items[2].blocked_reason.as_deref(),
            Some("dependency-cycle")
        );
        assert_eq!(
            plan.items[3].blocked_reason.as_deref(),
            Some("dependency-cycle")
        );
    }

    #[test]
    fn duplicate_labels_and_outside_cwd_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{
              "version": "2.0.0",
              "tasks": [
                {"label":"same", "type":"process", "command":"x"},
                {"label":"same", "type":"process", "command":"y"},
                {"label":"outside", "type":"process", "command":"z", "options":{"cwd":"../other"}}
              ]
            }"#,
        );
        let plan = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        assert_eq!(
            plan.items[0].blocked_reason.as_deref(),
            Some("duplicate-label")
        );
        assert_eq!(
            plan.items[1].blocked_reason.as_deref(),
            Some("duplicate-label")
        );
        assert_eq!(plan.items[2].blocked_reason.as_deref(), Some("invalid-cwd"));
    }

    #[test]
    fn revision_and_claim_change_with_source_bytes() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{"version":"2.0.0","tasks":[{"label":"a","type":"process","command":"one"}]}"#,
        );
        let first = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        write_tasks(
            root.path(),
            r#"{"version":"2.0.0","tasks":[{"label":"a","type":"process","command":"two"}]}"#,
        );
        assert_eq!(
            verify_workspace_task_plan(
                root.path(),
                TargetKind::Wsl,
                Some("Ubuntu"),
                &first.source_root,
                &first.project_identity,
                &first.revision,
            ),
            Err(WorkspaceTaskError::SourceChanged)
        );
    }

    #[test]
    fn target_is_part_of_revision_and_wsl_distro_must_match_unc() {
        let root = tempfile::tempdir().unwrap();
        write_tasks(
            root.path(),
            r#"{"version":"2.0.0","tasks":[{"label":"a","type":"process","command":"x"}]}"#,
        );
        let linux = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")).unwrap();
        let other = preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Debian")).unwrap();
        assert_ne!(linux.revision, other.revision);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_tasks_file_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".vscode")).unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join(TASKS_JSON_RELATIVE_PATH)).unwrap();
        assert_eq!(
            preview_workspace_tasks(root.path(), TargetKind::Wsl, Some("Ubuntu")),
            Err(WorkspaceTaskError::UnsafeSource)
        );
    }
}
