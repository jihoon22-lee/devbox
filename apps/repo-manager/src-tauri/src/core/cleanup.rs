//! Pure parsing and classification for Repo Manager's safe cleanup flow.
//!
//! The command layer owns repository validation and Git process lifetime.  This
//! module only accepts bounded, fixed-format Git output and turns it into a
//! preview.  A preview is deliberately conservative: a target is removable
//! only when it is an explicit candidate and no safety blocker was observed.

use serde::Serialize;
use std::collections::HashSet;

pub const GIT_CLEANUP_ERROR: &str = "Git 정리 작업을 실행하지 못했습니다.";
pub const GIT_CLEANUP_CANCELLED: &str = "Git 정리 작업을 취소했습니다.";
pub const GIT_CLEANUP_BUSY: &str = "이미 다른 Git 작업이 진행 중입니다.";
pub const GIT_CLEANUP_STATE_CHANGED: &str =
    "저장소 상태가 변경되어 Git 정리를 실행하지 않았습니다.";

pub const MAX_CLEANUP_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_CLEANUP_RECORDS: usize = 2_048;
pub const MAX_CLEANUP_BRANCHES: usize = 1_024;
pub const MAX_CLEANUP_WORKTREES: usize = 256;
pub const MAX_CLEANUP_PATH_BYTES: usize = 32_767;
pub const MAX_CLEANUP_REF_BYTES: usize = 16 * 1024;
pub const MAX_CLEANUP_SELECTIONS: usize = 256;

/// A branch older than this threshold is shown as an `inactive` stale
/// candidate when it is not protected by another safety rule.  Stale is a
/// preview rationale, never an authority to delete a branch by itself.
pub const STALE_BRANCH_AGE_SECONDS: i64 = 90 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBranch {
    pub name: String,
    pub head: String,
    pub upstream: Option<String>,
    pub upstream_gone: bool,
    pub last_commit_unix: i64,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWorktree {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub bare: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorktreeStatus {
    pub dirty: bool,
    pub untracked: bool,
    pub ignored: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPreview {
    /// Opaque revision for the exact bounded preview.  It is used only to
    /// reject a stale confirmation; it is not a persistent identifier.
    pub revision: String,
    pub current_branch: Option<String>,
    pub current_head: Option<String>,
    pub branches: Vec<BranchCleanupEntry>,
    pub worktrees: Vec<WorktreeCleanupEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BranchCleanupEntry {
    pub name: String,
    pub head: String,
    pub upstream: Option<String>,
    pub last_commit_unix: i64,
    pub current: bool,
    pub checked_out: bool,
    pub protected: bool,
    pub merged: bool,
    pub stale: bool,
    pub candidate: bool,
    pub eligible: bool,
    /// Stable rationale IDs; the frontend owns localized text.
    pub reasons: Vec<String>,
    /// Stable blocker IDs; no Git diagnostics or raw path is included.
    pub blocked: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCleanupEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_main: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
    pub dirty: bool,
    pub untracked: bool,
    pub ignored: bool,
    pub candidate: bool,
    pub eligible: bool,
    pub reasons: Vec<String>,
    pub blocked: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemResult {
    pub kind: String,
    pub target: String,
    pub outcome: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub preview_revision: String,
    pub attempted: u32,
    pub removed: u32,
    pub items: Vec<CleanupItemResult>,
}

fn fixed_error() -> String {
    GIT_CLEANUP_ERROR.to_string()
}

/// Validate a local branch/ref name emitted by Git or accepted from an
/// explicit preview selection.  Keeping this validator public lets the
/// command layer enforce the same ref grammar at its final argv boundary.
pub fn valid_ref_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CLEANUP_REF_BYTES
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.contains("@{")
        && value != "@"
        && !value.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
        && value.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component.starts_with('.')
                && !component.ends_with(".lock")
        })
}

fn valid_object_id(value: &str) -> bool {
    (40..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Git reports an unborn worktree HEAD as an all-zero object ID. The width is
/// selected by the repository object format: SHA-1 uses 40 bytes and
/// SHA-256 uses 64 bytes. Keep this check independent from the normal object
/// ID validator so both formats remain explicit and no fixed-width constant
/// can accidentally make the other format look like a real HEAD.
fn is_unborn_head(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte == b'0')
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CLEANUP_PATH_BYTES
        && !value.chars().any(char::is_control)
        && !is_unsafe_device_path(value)
        && !is_filesystem_root(value)
        && !value
            .split(['/', '\\'])
            .any(|component| matches!(component, "." | ".."))
}

/// A worktree path may be outside the repository, but a filesystem root is
/// never a safe removal target. Keep the same guard available at the final
/// argv boundary as well as in the porcelain parser.
pub fn valid_cleanup_worktree_path(value: &str) -> bool {
    valid_path(value)
}

fn is_filesystem_root(value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty() || normalized.chars().all(|character| character == '/') {
        return true;
    }
    let lower = normalized.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2..].iter().all(|byte| *byte == b'/')
    {
        return true;
    }
    if let Some(rest) = lower.strip_prefix("//?/") {
        let rest_bytes = rest.as_bytes();
        if rest_bytes.len() >= 3
            && rest_bytes[0].is_ascii_alphabetic()
            && rest_bytes[1] == b':'
            && rest_bytes[2..].iter().all(|byte| *byte == b'/')
        {
            return true;
        }
        if let Some(unc) = rest.strip_prefix("unc/") {
            let components = unc.split('/').filter(|component| !component.is_empty());
            if components.count() == 2 {
                return true;
            }
        }
    }
    if lower.starts_with("//") {
        let components = lower
            .trim_start_matches('/')
            .split('/')
            .filter(|component| !component.is_empty());
        if components.count() == 2 {
            return true;
        }
    }
    std::path::Path::new(value)
        .parent()
        .is_some_and(|parent| parent == std::path::Path::new(value))
}

/// Git for Windows may report a canonical long path with the `\\?\` prefix.
/// Keep drive/UNC long paths usable, but reject physical-device and NT object
/// manager namespaces before they can reach status/remove argv.
fn is_unsafe_device_path(value: &str) -> bool {
    let normalized = value.replace('\\', "/").to_ascii_lowercase();
    if normalized.starts_with("//./") || normalized.starts_with("/??/") {
        return true;
    }
    let Some(rest) = normalized.strip_prefix("//?/") else {
        return false;
    };
    let bytes = rest.as_bytes();
    let drive_path =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    !(drive_path || rest.starts_with("unc/"))
}

/// Parse the fixed NUL-delimited branch format emitted by
/// `for-each-ref --format=... refs/heads`.
pub fn parse_branch_records(input: &str) -> Result<Vec<ParsedBranch>, String> {
    if input.len() > MAX_CLEANUP_OUTPUT_BYTES {
        return Err(fixed_error());
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if !input.ends_with('\n') {
        return Err(fixed_error());
    }

    let mut branches = Vec::new();
    let mut seen = HashSet::new();
    for line in input.split_terminator('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let fields = line.split('\0').collect::<Vec<_>>();
        // name, object ID, upstream, tracking summary, timestamp, HEAD mark,
        // and the trailing NUL field before Git's record newline.
        if fields.len() != 7 || !fields[6].is_empty() {
            return Err(fixed_error());
        }
        let name = fields[0];
        let head = fields[1];
        let upstream = fields[2];
        let tracking = fields[3];
        let timestamp = fields[4];
        let head_mark = fields[5];
        if !valid_ref_name(name)
            || !valid_object_id(head)
            || !valid_tracking_summary(tracking)
            || !matches!(head_mark, "*" | " ")
            || !seen.insert(name.to_string())
        {
            return Err(fixed_error());
        }
        let upstream = if upstream.is_empty() {
            None
        } else if valid_ref_name(upstream) {
            Some(upstream.to_string())
        } else {
            return Err(fixed_error());
        };
        let upstream_gone = tracking == "[gone]";
        if upstream_gone && upstream.is_none() {
            return Err(fixed_error());
        }
        let last_commit_unix = timestamp.parse::<i64>().map_err(|_| fixed_error())?;
        branches.push(ParsedBranch {
            name: name.to_string(),
            head: head.to_string(),
            upstream,
            upstream_gone,
            last_commit_unix,
            current: head_mark == "*",
        });
        if branches.len() > MAX_CLEANUP_BRANCHES {
            return Err(fixed_error());
        }
    }
    Ok(branches)
}

fn valid_tracking_summary(value: &str) -> bool {
    if value.is_empty() || value == "[gone]" {
        return true;
    }
    if !value.starts_with('[') || !value.ends_with(']') {
        return false;
    }
    let body = &value[1..value.len() - 1];
    if body.is_empty() {
        return false;
    }
    let mut ahead_seen = false;
    let mut behind_seen = false;
    for field in body.split(',') {
        let field = field.trim();
        let (kind, count) = if let Some(count) = field.strip_prefix("ahead ") {
            ("ahead", count)
        } else if let Some(count) = field.strip_prefix("behind ") {
            ("behind", count)
        } else {
            return false;
        };
        if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        if kind == "ahead" {
            if ahead_seen {
                return false;
            }
            ahead_seen = true;
        } else {
            if behind_seen {
                return false;
            }
            behind_seen = true;
        }
    }
    ahead_seen || behind_seen
}

/// Parse `git for-each-ref --merged=<head> --format=%(refname:strip=2)`.
pub fn parse_merged_branch_names(input: &str) -> Result<HashSet<String>, String> {
    if input.len() > MAX_CLEANUP_OUTPUT_BYTES {
        return Err(fixed_error());
    }
    if input.is_empty() {
        return Ok(HashSet::new());
    }
    if !input.ends_with('\n') {
        return Err(fixed_error());
    }
    let mut merged = HashSet::new();
    for line in input.split_terminator('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !valid_ref_name(line)
            || merged.len() >= MAX_CLEANUP_BRANCHES
            || !merged.insert(line.to_string())
        {
            return Err(fixed_error());
        }
    }
    Ok(merged)
}

/// Parse `git worktree list --porcelain -z`.  Reasons attached to `locked` or
/// `prunable` are intentionally discarded; they are diagnostics and may
/// contain a path or user-provided text.
pub fn parse_worktree_records(input: &str) -> Result<Vec<ParsedWorktree>, String> {
    if input.is_empty() || input.len() > MAX_CLEANUP_OUTPUT_BYTES || !input.ends_with('\0') {
        return Err(fixed_error());
    }

    let records = input.split_terminator('\0').collect::<Vec<_>>();
    if records.is_empty() || records.len() > MAX_CLEANUP_RECORDS {
        return Err(fixed_error());
    }
    let mut worktrees = Vec::new();
    let mut seen_paths = HashSet::new();
    let mut index = 0usize;
    while index < records.len() {
        // Empty records separate worktrees and are consumed by the inner
        // loop. Seeing one here means the output had a duplicate separator.
        if records[index].is_empty() {
            return Err(fixed_error());
        }
        let path = records[index]
            .strip_prefix("worktree ")
            .ok_or_else(fixed_error)?;
        if !valid_path(path) || !seen_paths.insert(path.to_string()) {
            return Err(fixed_error());
        }
        index += 1;
        let mut worktree = ParsedWorktree {
            path: path.to_string(),
            head: None,
            branch: None,
            locked: false,
            prunable: false,
            bare: false,
        };
        let mut saw_head = false;
        let mut saw_separator = false;
        while index < records.len() {
            let record = records[index];
            index += 1;
            if record.is_empty() {
                saw_separator = true;
                break;
            }
            if let Some(value) = record.strip_prefix("HEAD ") {
                if saw_head || !valid_object_id(value) {
                    return Err(fixed_error());
                }
                saw_head = true;
                if !is_unborn_head(value) {
                    worktree.head = Some(value.to_string());
                }
            } else if let Some(value) = record.strip_prefix("branch ") {
                let branch = value.strip_prefix("refs/heads/").ok_or_else(fixed_error)?;
                if worktree.branch.is_some() || !valid_ref_name(branch) {
                    return Err(fixed_error());
                }
                worktree.branch = Some(branch.to_string());
            } else if record == "bare" {
                if worktree.bare {
                    return Err(fixed_error());
                }
                worktree.bare = true;
            } else if record == "locked" || record.starts_with("locked ") {
                if worktree.locked {
                    return Err(fixed_error());
                }
                if record.chars().any(char::is_control) {
                    return Err(fixed_error());
                }
                worktree.locked = true;
            } else if record == "prunable" || record.starts_with("prunable ") {
                if worktree.prunable {
                    return Err(fixed_error());
                }
                if record.chars().any(char::is_control) {
                    return Err(fixed_error());
                }
                worktree.prunable = true;
            } else {
                return Err(fixed_error());
            }
        }
        if !saw_separator || (!saw_head && !worktree.bare) {
            return Err(fixed_error());
        }
        worktrees.push(worktree);
        if worktrees.len() > MAX_CLEANUP_WORKTREES {
            return Err(fixed_error());
        }
    }
    if worktrees.is_empty() {
        return Err(fixed_error());
    }
    Ok(worktrees)
}

/// Parse a worktree's fixed NUL-delimited porcelain status.  The parser only
/// returns safety bits and validates every path, so malformed output can never
/// be mistaken for a clean target.
pub fn parse_worktree_status(input: &str) -> Result<WorktreeStatus, String> {
    if input.len() > MAX_CLEANUP_OUTPUT_BYTES {
        return Err(fixed_error());
    }
    if input.is_empty() {
        return Ok(WorktreeStatus::default());
    }
    if !input.ends_with('\0') {
        return Err(fixed_error());
    }
    let records = input.split_terminator('\0').collect::<Vec<_>>();
    if records.len() > MAX_CLEANUP_RECORDS {
        return Err(fixed_error());
    }
    let mut status = WorktreeStatus::default();
    let mut index = 0usize;
    while index < records.len() {
        let record = records[index];
        if record.len() < 4 || record.as_bytes()[2] != b' ' {
            return Err(fixed_error());
        }
        let index_status = record.as_bytes()[0] as char;
        let worktree_status = record.as_bytes()[1] as char;
        if !valid_status_code(index_status) || !valid_status_code(worktree_status) {
            return Err(fixed_error());
        }
        validate_status_path(&record[3..])?;
        if index_status == '?' || worktree_status == '?' {
            status.untracked = true;
        }
        if (index_status, worktree_status) != (' ', ' ') {
            if (index_status, worktree_status) == ('!', '!') {
                status.ignored = true;
            } else {
                status.dirty = true;
            }
        }
        if matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C') {
            index += 1;
            let old_path = records.get(index).ok_or_else(fixed_error)?;
            validate_status_path(old_path)?;
        }
        index += 1;
    }
    Ok(status)
}

fn valid_status_code(value: char) -> bool {
    matches!(
        value,
        ' ' | 'M' | 'A' | 'D' | 'R' | 'C' | 'T' | 'U' | '?' | '!'
    )
}

fn validate_status_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_CLEANUP_PATH_BYTES
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.starts_with(":(")
        || value.contains(':')
        || value.chars().any(char::is_control)
        || value
            .split(['/', '\\'])
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(fixed_error());
    }
    Ok(())
}

/// Classify bounded observations into a conservative preview.  A missing
/// status is represented by `None` and becomes `stateUnavailable`, which is
/// always a block for linked worktrees.
pub fn classify_preview(
    current_head: Option<String>,
    branches: &[ParsedBranch],
    worktrees: &[ParsedWorktree],
    merged: &HashSet<String>,
    statuses: &[Option<WorktreeStatus>],
    now_unix: i64,
) -> CleanupPreview {
    let current_branch = branches
        .iter()
        .find(|branch| branch.current)
        .map(|branch| branch.name.clone());
    let checked_out = worktrees
        .iter()
        .filter_map(|worktree| worktree.branch.as_deref())
        .collect::<HashSet<_>>();

    let branch_entries = branches
        .iter()
        .map(|branch| {
            let inactive =
                now_unix.saturating_sub(branch.last_commit_unix) >= STALE_BRANCH_AGE_SECONDS;
            let stale = branch.upstream_gone || inactive;
            let merged = merged.contains(&branch.name);
            let mut reasons = Vec::new();
            if merged {
                reasons.push("mergedIntoCurrent".to_string());
            }
            if branch.upstream_gone {
                reasons.push("upstreamGone".to_string());
            }
            if inactive {
                reasons.push("inactive".to_string());
            }
            let candidate = merged || stale;
            let protected = branch.name == "main";
            let checked_out = checked_out.contains(branch.name.as_str());
            let mut blocked = Vec::new();
            if branch.current {
                blocked.push("currentBranch".to_string());
            }
            if protected {
                blocked.push("mainBranch".to_string());
            }
            if checked_out {
                blocked.push("checkedOut".to_string());
            }
            let eligible = candidate && blocked.is_empty();
            BranchCleanupEntry {
                name: branch.name.clone(),
                head: branch.head.clone(),
                upstream: branch.upstream.clone(),
                last_commit_unix: branch.last_commit_unix,
                current: branch.current,
                checked_out,
                protected,
                merged,
                stale,
                candidate,
                eligible,
                reasons,
                blocked,
            }
        })
        .collect::<Vec<_>>();

    let worktree_entries = worktrees
        .iter()
        .enumerate()
        .map(|(index, worktree)| {
            let is_main = index == 0;
            let worktree_status = statuses.get(index).copied().flatten();
            let dirty = worktree_status.is_some_and(|status| status.dirty);
            let untracked = worktree_status.is_some_and(|status| status.untracked);
            let ignored = worktree_status.is_some_and(|status| status.ignored);
            let candidate = !is_main && !worktree.bare && !worktree.prunable;
            let mut reasons = Vec::new();
            if is_main {
                reasons.push("primaryWorktree".to_string());
            } else if worktree.bare {
                reasons.push("bareWorktree".to_string());
            } else if worktree.prunable {
                reasons.push("prunableWorktree".to_string());
            } else if worktree.branch.is_some() {
                reasons.push("linkedWorktree".to_string());
            } else {
                reasons.push("detachedWorktree".to_string());
            }
            let mut blocked = Vec::new();
            if is_main {
                blocked.push("mainWorktree".to_string());
            }
            if worktree.bare {
                blocked.push("bareWorktree".to_string());
            }
            if worktree.prunable {
                blocked.push("prunable".to_string());
            }
            if worktree.locked {
                blocked.push("locked".to_string());
            }
            if worktree_status.is_none() && !worktree.bare && !worktree.prunable {
                blocked.push("stateUnavailable".to_string());
            }
            if dirty {
                blocked.push("dirty".to_string());
            }
            if untracked {
                blocked.push("untracked".to_string());
            }
            if ignored {
                blocked.push("ignored".to_string());
            }
            let eligible = candidate && blocked.is_empty();
            WorktreeCleanupEntry {
                path: worktree.path.clone(),
                head: worktree.head.clone(),
                branch: worktree.branch.clone(),
                is_main,
                bare: worktree.bare,
                locked: worktree.locked,
                prunable: worktree.prunable,
                dirty,
                untracked,
                ignored,
                candidate,
                eligible,
                reasons,
                blocked,
            }
        })
        .collect::<Vec<_>>();

    CleanupPreview {
        revision: String::new(),
        current_branch,
        current_head,
        branches: branch_entries,
        worktrees: worktree_entries,
    }
}

/// Fill the preview's opaque revision.  FNV-1a is sufficient here because the
/// revision is only an in-memory stale-snapshot guard, not an authenticity or
/// secrecy mechanism.  Raw paths and branch names are never exposed by the
/// revision itself.
pub fn finalize_revision(mut preview: CleanupPreview) -> CleanupPreview {
    preview.revision = revision_for_preview(&preview);
    preview
}

pub fn revision_for_preview(preview: &CleanupPreview) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    feed_option(&mut hash, preview.current_branch.as_deref());
    feed_option(&mut hash, preview.current_head.as_deref());
    for branch in &preview.branches {
        feed(&mut hash, &branch.name);
        feed(&mut hash, &branch.head);
        feed_option(&mut hash, branch.upstream.as_deref());
        feed(&mut hash, &branch.last_commit_unix.to_string());
        feed_flags(
            &mut hash,
            &[
                branch.current,
                branch.checked_out,
                branch.protected,
                branch.merged,
                branch.stale,
                branch.candidate,
                branch.eligible,
            ],
        );
        feed_list(&mut hash, &branch.reasons);
        feed_list(&mut hash, &branch.blocked);
    }
    for worktree in &preview.worktrees {
        feed(&mut hash, &worktree.path);
        feed_option(&mut hash, worktree.head.as_deref());
        feed_option(&mut hash, worktree.branch.as_deref());
        feed_flags(
            &mut hash,
            &[
                worktree.is_main,
                worktree.bare,
                worktree.locked,
                worktree.prunable,
                worktree.dirty,
                worktree.untracked,
                worktree.ignored,
                worktree.candidate,
                worktree.eligible,
            ],
        );
        feed_list(&mut hash, &worktree.reasons);
        feed_list(&mut hash, &worktree.blocked);
    }
    format!("cleanup-{hash:016x}")
}

fn feed(hash: &mut u64, value: &str) {
    for byte in value.as_bytes() {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x100000001b3);
}

fn feed_option(hash: &mut u64, value: Option<&str>) {
    feed(hash, value.unwrap_or("<none>"));
}

fn feed_flags(hash: &mut u64, flags: &[bool]) {
    for flag in flags {
        feed(hash, if *flag { "1" } else { "0" });
    }
}

fn feed_list(hash: &mut u64, values: &[String]) {
    for value in values {
        feed(hash, value);
    }
    feed(hash, &values.len().to_string());
}

pub fn valid_revision(value: &str) -> bool {
    value.len() == "cleanup-0000000000000000".len()
        && value.starts_with("cleanup-")
        && value[8..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "0123456789abcdef0123456789abcdef01234567";
    const OID_2: &str = "fedcba9876543210fedcba9876543210fedcba98";

    #[test]
    fn parses_branch_records_and_tracks_gone_upstream() {
        let input = format!(
            "feature/ui\0{OID}\0origin/ui\0[gone]\01700000000\0 \0\nmain\0{OID_2}\0origin/main\0[ahead 1, behind 2]\01700000001\0*\0\n"
        );
        let branches = parse_branch_records(&input).unwrap();
        assert_eq!(branches.len(), 2);
        assert!(branches[0].upstream_gone);
        assert!(!branches[0].current);
        assert!(branches[1].current);
        assert!(!branches[1].upstream_gone);
    }

    #[test]
    fn rejects_malformed_branch_metadata_without_echoing_input() {
        let secret = "credential-branch-secret";
        let input = format!("../{secret}\0{OID}\0\0\0not-a-time\0 \0\n");
        let error = parse_branch_records(&input).unwrap_err();
        assert_eq!(error, GIT_CLEANUP_ERROR);
        assert!(!error.contains(secret));
    }

    #[test]
    fn parses_worktree_locked_prunable_and_bare_records() {
        let input = format!(
            "worktree C:/repo\0HEAD {OID}\0branch refs/heads/main\0\0worktree C:/linked\0HEAD {OID_2}\0branch refs/heads/feature\0locked private path\0\0worktree C:/bare\0HEAD {}\0bare\0\0",
            "0".repeat(40)
        );
        let worktrees = parse_worktree_records(&input).unwrap();
        assert_eq!(worktrees.len(), 3);
        assert!(worktrees[1].locked);
        assert!(worktrees[2].bare);
        assert!(worktrees[2].head.is_none());
    }

    #[test]
    fn treats_sha1_and_sha256_zero_heads_as_unborn() {
        for zero_head in ["0".repeat(40), "0".repeat(64)] {
            let input = format!("worktree C:/unborn\0HEAD {zero_head}\0branch refs/heads/main\0\0");
            let worktrees = parse_worktree_records(&input).unwrap();
            assert_eq!(worktrees.len(), 1);
            assert!(worktrees[0].head.is_none());
        }
    }

    #[test]
    fn status_parser_distinguishes_dirty_untracked_and_ignored() {
        let status = parse_worktree_status(" M tracked.txt\0?? new.txt\0!! ignored.txt\0").unwrap();
        assert!(status.dirty);
        assert!(status.untracked);
        assert!(status.ignored);
        assert_eq!(
            parse_worktree_status("!! ignored.txt\0").unwrap(),
            WorktreeStatus {
                dirty: false,
                untracked: false,
                ignored: true,
            }
        );
    }

    #[test]
    fn classification_blocks_main_checked_out_and_dirty_targets() {
        let branches = vec![
            ParsedBranch {
                name: "main".to_string(),
                head: OID.to_string(),
                upstream: Some("origin/main".to_string()),
                upstream_gone: false,
                last_commit_unix: 1,
                current: true,
            },
            ParsedBranch {
                name: "feature".to_string(),
                head: OID_2.to_string(),
                upstream: Some("origin/feature".to_string()),
                upstream_gone: false,
                last_commit_unix: 1,
                current: false,
            },
        ];
        let worktrees = vec![
            ParsedWorktree {
                path: "C:/repo".to_string(),
                head: Some(OID.to_string()),
                branch: Some("main".to_string()),
                locked: false,
                prunable: false,
                bare: false,
            },
            ParsedWorktree {
                path: "C:/linked".to_string(),
                head: Some(OID_2.to_string()),
                branch: Some("feature".to_string()),
                locked: false,
                prunable: false,
                bare: false,
            },
        ];
        let merged = HashSet::from(["main".to_string(), "feature".to_string()]);
        let preview = classify_preview(
            Some(OID.to_string()),
            &branches,
            &worktrees,
            &merged,
            &[
                Some(WorktreeStatus::default()),
                Some(WorktreeStatus {
                    dirty: true,
                    untracked: true,
                    ignored: false,
                }),
            ],
            2,
        );
        assert!(preview.branches[0].protected);
        assert!(preview.branches[0]
            .blocked
            .contains(&"currentBranch".to_string()));
        assert!(preview.branches[1]
            .blocked
            .contains(&"checkedOut".to_string()));
        assert!(preview.worktrees[0]
            .blocked
            .contains(&"mainWorktree".to_string()));
        assert!(preview.worktrees[1].blocked.contains(&"dirty".to_string()));
        assert!(preview.worktrees[1]
            .blocked
            .contains(&"untracked".to_string()));
    }

    #[test]
    fn inactive_and_gone_upstream_are_explicit_stale_reasons() {
        let branches = vec![ParsedBranch {
            name: "old".to_string(),
            head: OID.to_string(),
            upstream: Some("origin/old".to_string()),
            upstream_gone: true,
            last_commit_unix: 1,
            current: false,
        }];
        let preview = classify_preview(
            None,
            &branches,
            &[ParsedWorktree {
                path: "C:/repo".to_string(),
                head: None,
                branch: None,
                locked: false,
                prunable: false,
                bare: false,
            }],
            &HashSet::new(),
            &[Some(WorktreeStatus::default())],
            STALE_BRANCH_AGE_SECONDS + 2,
        );
        assert!(preview.branches[0].stale);
        assert!(preview.branches[0].candidate);
        assert!(preview.branches[0]
            .reasons
            .contains(&"upstreamGone".to_string()));
        assert!(preview.branches[0]
            .reasons
            .contains(&"inactive".to_string()));
        let finalized = finalize_revision(preview);
        assert!(valid_revision(&finalized.revision));
        assert!(!finalized.revision.contains("C:/repo"));
    }

    #[test]
    fn malformed_status_and_unsafe_paths_fail_closed() {
        assert_eq!(
            parse_worktree_status(" M ../secret\0").unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        assert_eq!(
            parse_worktree_records("worktree C:/repo\0HEAD nope\0\0").unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        assert_eq!(
            parse_worktree_records(
                "worktree ../secret\0HEAD 0123456789abcdef0123456789abcdef01234567\0\0"
            )
            .unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        assert_eq!(
            parse_worktree_records(
                "worktree \\\\.\\PhysicalDrive0\0HEAD 0123456789abcdef0123456789abcdef01234567\0\0"
            )
            .unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        assert_eq!(
            parse_worktree_records("worktree /\0HEAD 0123456789abcdef0123456789abcdef01234567\0\0")
                .unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        for root in ["C://", "\\\\?\\C:\\\\"] {
            assert!(!valid_path(root));
        }
        let extended = parse_worktree_records(
            "worktree \\\\?\\C:\\secret\0HEAD 0123456789abcdef0123456789abcdef01234567\0\0",
        )
        .unwrap();
        assert_eq!(extended[0].path, r"\\?\C:\secret");
        assert_eq!(
            parse_merged_branch_names("feature\nfeature\n").unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        let bounded_merged = (0..MAX_CLEANUP_BRANCHES)
            .map(|index| format!("branch-{index}\n"))
            .collect::<String>();
        assert_eq!(
            parse_merged_branch_names(&format!("{bounded_merged}branch-over-limit\n")).unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        let oversized = "x".repeat(MAX_CLEANUP_OUTPUT_BYTES + 1);
        assert_eq!(
            parse_branch_records(&oversized).unwrap_err(),
            GIT_CLEANUP_ERROR
        );
        assert_eq!(
            parse_worktree_status(&oversized).unwrap_err(),
            GIT_CLEANUP_ERROR
        );
    }
}
