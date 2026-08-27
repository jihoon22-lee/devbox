//! Remote Git state parsing and preflight decisions.
//!
//! This module deliberately does not execute Git.  The command layer owns
//! repository validation and subprocess lifetime; keeping the state machine
//! here makes the safety boundary testable with fixed, non-destructive
//! fixtures.

use serde::Serialize;

pub const MAX_REMOTE_BRANCH_BYTES: usize = 16 * 1024;
pub const GIT_REMOTE_ERROR: &str = "Git 원격 작업을 실행하지 못했습니다.";
pub const GIT_REMOTE_CANCELLED: &str = "Git 원격 작업을 취소했습니다.";
pub const GIT_REMOTE_BUSY: &str = "이미 다른 Git 작업이 진행 중입니다.";
pub const GIT_REMOTE_STATE_CHANGED: &str =
    "저장소 상태가 변경되어 Git 원격 작업을 실행하지 않았습니다.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAction {
    Fetch,
    Pull,
    Push,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteStateParseError {
    MissingBranchHeader,
    MultipleBranchHeaders,
    InvalidBranch,
    InvalidUpstream,
    InvalidAheadBehind,
    MalformedAheadBehind,
    MalformedStatus,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteState {
    /// `None` means detached HEAD.  A branch name is returned only after the
    /// fixed status parser has checked its size and control characters.
    pub current_branch: Option<String>,
    /// The short upstream name shown by Git, such as `origin/main`.  URL/path
    /// shaped upstream metadata is rejected so no remote URL or credential
    /// data is ever exposed.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub detached: bool,
    pub diverged: bool,
    pub operation_in_progress: bool,
}

struct ParsedHeader {
    current_branch: Option<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    detached: bool,
}

/// Parse the bounded, line-oriented output of
/// `git status --porcelain=v1 --branch --untracked-files=all`.
///
/// Git emits one `##` header followed by zero or more change records.  The
/// parser only retains branch/upstream metadata and a dirty bit, so filenames
/// never cross the command/UI boundary.  Malformed branch metadata is a hard
/// failure rather than a permissive fallback: a remote mutation must not run
/// when its preflight state is ambiguous.
pub fn parse_remote_status(
    input: &str,
    operation_in_progress: bool,
) -> Result<RemoteState, RemoteStateParseError> {
    let mut header: Option<&str> = None;
    let mut dirty = false;

    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            if header.replace(rest).is_some() {
                return Err(RemoteStateParseError::MultipleBranchHeaders);
            }
        } else if !line.trim().is_empty() {
            if !is_porcelain_record(line) {
                return Err(RemoteStateParseError::MalformedStatus);
            }
            dirty = true;
        }
    }

    let Some(header) = header else {
        return Err(RemoteStateParseError::MissingBranchHeader);
    };

    let parsed = parse_header(header)?;
    Ok(RemoteState {
        current_branch: parsed.current_branch,
        upstream: parsed.upstream,
        ahead: parsed.ahead,
        behind: parsed.behind,
        dirty,
        detached: parsed.detached,
        diverged: parsed.ahead > 0 && parsed.behind > 0,
        operation_in_progress,
    })
}

fn is_porcelain_record(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() > 3
        && bytes[2] == b' '
        && [bytes[0], bytes[1]]
            .into_iter()
            .all(|status| b" MADRCU?!".contains(&status))
}

fn parse_header(header: &str) -> Result<ParsedHeader, RemoteStateParseError> {
    if header.starts_with("HEAD (") {
        let detached_at = header
            .strip_prefix("HEAD (detached at ")
            .and_then(|value| value.strip_suffix(')'));
        let detached_from = header
            .strip_prefix("HEAD (detached from ")
            .and_then(|value| value.strip_suffix(')'));
        let valid_detached = detached_at
            .or(detached_from)
            .is_some_and(|value| validate_metadata(value).is_ok());
        if header != "HEAD (no branch)" && !valid_detached {
            return Err(RemoteStateParseError::InvalidBranch);
        }
        return Ok(ParsedHeader {
            current_branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            detached: true,
        });
    }

    // An unborn branch is reported as `No commits yet on <branch>`.
    let branch_part = header
        .strip_prefix("No commits yet on ")
        .unwrap_or_else(|| {
            header
                .split_once("...")
                .map_or(header, |(branch, _)| branch)
        });
    let attached_branch = validate_metadata(branch_part)?;
    let Some((_, upstream_and_counts)) = header.split_once("...") else {
        return Ok(ParsedHeader {
            current_branch: Some(attached_branch),
            upstream: None,
            ahead: 0,
            behind: 0,
            detached: false,
        });
    };

    let upstream = upstream_and_counts
        .split_once('[')
        .map_or(upstream_and_counts, |(value, _)| value)
        .trim();
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut saw_ahead = false;
    let mut saw_behind = false;
    let upstream_gone = upstream_and_counts
        .split_once('[')
        .is_some_and(|(_, counts)| counts.trim_matches(']').trim() == "gone");
    let validated_upstream = if upstream.is_empty() {
        return Err(RemoteStateParseError::InvalidUpstream);
    } else {
        Some(validate_upstream(upstream)?)
    };
    let upstream = if upstream_gone {
        // `[gone]` means the configured remote-tracking ref disappeared.  It
        // is not a usable upstream for pull/push, so expose it as no upstream
        // and let the normal preflight stop before Git mutation. Validate the
        // hidden name first so malformed/path-like metadata cannot bypass the
        // parser merely by claiming that its remote is gone.
        None
    } else {
        validated_upstream
    };

    if let Some((_, counts)) = upstream_and_counts.split_once('[') {
        let counts = counts
            .strip_suffix(']')
            .ok_or(RemoteStateParseError::MalformedAheadBehind)?
            .trim();
        if counts == "gone" {
            // Counts remain zero for a missing remote-tracking ref.
        } else if !counts.is_empty() {
            for part in counts.split(',') {
                let part = part.trim();
                if let Some(value) = part.strip_prefix("ahead ") {
                    if saw_ahead {
                        return Err(RemoteStateParseError::MalformedAheadBehind);
                    }
                    saw_ahead = true;
                    ahead = parse_count(value)?;
                } else if let Some(value) = part.strip_prefix("behind ") {
                    if saw_behind {
                        return Err(RemoteStateParseError::MalformedAheadBehind);
                    }
                    saw_behind = true;
                    behind = parse_count(value)?;
                } else {
                    return Err(RemoteStateParseError::MalformedAheadBehind);
                }
            }
        }
    }

    Ok(ParsedHeader {
        current_branch: Some(attached_branch),
        upstream,
        ahead,
        behind,
        detached: false,
    })
}

fn validate_metadata(value: &str) -> Result<String, RemoteStateParseError> {
    if !valid_ref_metadata(value) {
        return Err(RemoteStateParseError::InvalidBranch);
    }
    Ok(value.to_owned())
}

/// Validate branch-like metadata without treating it as a filesystem path.
/// Git names legitimately contain `/` (for example `feature/ui` and
/// `origin/release/v1`), but path traversal, absolute/path-shaped input, and
/// ref syntax that Git itself rejects must never be accepted from status
/// output. Keeping this check local also means detached/upstream metadata
/// shares the same fail-closed boundary.
fn valid_ref_metadata(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REMOTE_BRANCH_BYTES
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

fn validate_upstream(value: &str) -> Result<String, RemoteStateParseError> {
    if value.starts_with('/')
        || value.starts_with('\\')
        || value.contains("://")
        || value.contains('@')
        || value.contains(':')
    {
        // A branch can be configured to track a URL or local path.  Refuse to
        // expose that metadata because it may contain a credential or secret
        // repository path; a normal `origin/main` upstream still passes.
        return Err(RemoteStateParseError::InvalidUpstream);
    }
    validate_metadata(value).map_err(|_| RemoteStateParseError::InvalidUpstream)
}

fn parse_count(value: &str) -> Result<u32, RemoteStateParseError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| RemoteStateParseError::InvalidAheadBehind)
}

/// Return a fixed, UI-safe block reason for a requested remote action.
///
/// Fetch never changes the working tree and does not require a current branch
/// or upstream.  It is still blocked during an in-progress merge/rebase so a
/// user cannot accidentally stack another remote transition onto an
/// unfinished operation.  Pull is explicitly fast-forward-only and push is
/// normal (never force); both require a stable attached, clean branch.
pub fn preflight_remote(
    state: &RemoteState,
    action: RemoteAction,
) -> Result<(), RemoteBlockReason> {
    if state.operation_in_progress {
        return Err(RemoteBlockReason::OperationInProgress);
    }
    if action == RemoteAction::Fetch {
        return Ok(());
    }
    if state.detached || state.current_branch.is_none() {
        return Err(RemoteBlockReason::Detached);
    }
    if state.upstream.is_none() {
        return Err(RemoteBlockReason::NoUpstream);
    }
    if state.dirty {
        return Err(RemoteBlockReason::Dirty);
    }
    if state.diverged || (action == RemoteAction::Push && state.behind > 0) {
        return Err(RemoteBlockReason::Diverged);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteBlockReason {
    Detached,
    NoUpstream,
    Dirty,
    Diverged,
    OperationInProgress,
}

impl RemoteBlockReason {
    pub const fn message(self) -> &'static str {
        match self {
            Self::Detached => "현재 HEAD가 detached 상태라 pull/push를 실행할 수 없습니다.",
            Self::NoUpstream => "현재 branch에 upstream이 없어 pull/push를 실행할 수 없습니다.",
            Self::Dirty => "working tree에 변경 사항이 있어 pull/push를 실행할 수 없습니다.",
            Self::Diverged => {
                "branch가 diverged 상태라 fast-forward pull/push를 실행할 수 없습니다."
            }
            Self::OperationInProgress => {
                "다른 Git 작업 또는 merge/rebase가 진행 중이라 원격 작업을 실행할 수 없습니다."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(input: &str) -> RemoteState {
        parse_remote_status(input, false).unwrap()
    }

    #[test]
    fn parses_upstream_and_ahead_behind_without_exposing_change_paths() {
        let parsed = state("## main...origin/main [ahead 2, behind 1]\n M secret/path.txt\n");
        assert_eq!(parsed.current_branch.as_deref(), Some("main"));
        assert_eq!(parsed.upstream.as_deref(), Some("origin/main"));
        assert_eq!(parsed.ahead, 2);
        assert_eq!(parsed.behind, 1);
        assert!(parsed.dirty);
        assert!(parsed.diverged);
        let encoded = serde_json::to_string(&parsed).unwrap();
        assert!(!encoded.contains("secret/path.txt"));
    }

    #[test]
    fn covers_no_upstream_unborn_and_detached_headers() {
        let no_upstream = state("## feature/new\n");
        assert_eq!(no_upstream.current_branch.as_deref(), Some("feature/new"));
        assert!(no_upstream.upstream.is_none());

        let unborn = state("## No commits yet on first\n?? fixture.txt\n");
        assert_eq!(unborn.current_branch.as_deref(), Some("first"));
        assert!(unborn.dirty);

        let detached = state("## HEAD (detached at 0123456)\n");
        assert!(detached.detached);
        assert!(detached.current_branch.is_none());
        assert!(detached.upstream.is_none());
    }

    #[test]
    fn accepts_nested_branch_and_upstream_ref_names() {
        let parsed = state("## feature/ui/release-v1...origin/team/release/v1 [ahead 1]\n");
        assert_eq!(
            parsed.current_branch.as_deref(),
            Some("feature/ui/release-v1")
        );
        assert_eq!(parsed.upstream.as_deref(), Some("origin/team/release/v1"));
        assert_eq!(parsed.ahead, 1);
    }

    #[test]
    fn gone_upstream_is_treated_as_no_upstream() {
        let parsed = state("## main...origin/main [gone]\n");
        assert!(parsed.upstream.is_none());
        assert_eq!(parsed.ahead, 0);
        assert_eq!(parsed.behind, 0);
    }

    #[test]
    fn rejects_ambiguous_or_unbounded_status() {
        assert_eq!(
            parse_remote_status(" M only-change.txt\n", false),
            Err(RemoteStateParseError::MissingBranchHeader)
        );
        assert_eq!(
            parse_remote_status("## main\n## other\n", false),
            Err(RemoteStateParseError::MultipleBranchHeaders)
        );
        assert_eq!(
            parse_remote_status("## main\nnot porcelain status\n", false),
            Err(RemoteStateParseError::MalformedStatus)
        );
        assert_eq!(
            parse_remote_status("## main...origin/main [ahead nope]\n", false),
            Err(RemoteStateParseError::InvalidAheadBehind)
        );
        assert_eq!(
            parse_remote_status("## main...origin/main [ahead 1, ahead 2]\n", false),
            Err(RemoteStateParseError::MalformedAheadBehind)
        );
        assert_eq!(
            parse_remote_status("## main...origin/main [ahead 1\n", false),
            Err(RemoteStateParseError::MalformedAheadBehind)
        );
        assert_eq!(
            parse_remote_status("## HEAD (no branch and extra text)\n", false),
            Err(RemoteStateParseError::InvalidBranch)
        );
        assert_eq!(
            parse_remote_status("## HEAD (detached )\n", false),
            Err(RemoteStateParseError::InvalidBranch)
        );
        assert_eq!(
            parse_remote_status(
                "## main...https://user:secret@example.test/repo/main\n",
                false
            ),
            Err(RemoteStateParseError::InvalidUpstream)
        );
        for upstream in [
            "../secret",
            "foo/../secret",
            "origin/.",
            "origin/..",
            "origin//main",
            "/origin/main",
            r"origin\main",
            "C:/secret",
        ] {
            assert_eq!(
                parse_remote_status(&format!("## main...{upstream}\n"), false),
                Err(RemoteStateParseError::InvalidUpstream),
                "upstream: {upstream}"
            );
        }
        assert_eq!(
            parse_remote_status("## main...../secret [gone]\n", false),
            Err(RemoteStateParseError::InvalidUpstream)
        );
        for branch in [
            "../secret",
            "foo/../secret",
            "origin/.",
            "origin/..",
            "origin//main",
            "/origin/main",
            r"origin\main",
        ] {
            assert_eq!(
                parse_remote_status(&format!("## {branch}\n"), false),
                Err(RemoteStateParseError::InvalidBranch),
                "branch: {branch}"
            );
        }
        let long = "x".repeat(MAX_REMOTE_BRANCH_BYTES + 1);
        assert_eq!(
            parse_remote_status(&format!("## {long}\n"), false),
            Err(RemoteStateParseError::InvalidBranch)
        );
    }

    #[test]
    fn fetch_and_pull_push_have_separate_safe_preflight_rules() {
        let dirty_detached = RemoteState {
            current_branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
            dirty: true,
            detached: true,
            diverged: false,
            operation_in_progress: false,
        };
        assert!(preflight_remote(&dirty_detached, RemoteAction::Fetch).is_ok());
        assert_eq!(
            preflight_remote(&dirty_detached, RemoteAction::Pull),
            Err(RemoteBlockReason::Detached)
        );

        let no_upstream = state("## main\n");
        assert_eq!(
            preflight_remote(&no_upstream, RemoteAction::Push),
            Err(RemoteBlockReason::NoUpstream)
        );

        let diverged = state("## main...origin/main [ahead 1, behind 1]\n");
        assert_eq!(
            preflight_remote(&diverged, RemoteAction::Pull),
            Err(RemoteBlockReason::Diverged)
        );
        assert_eq!(
            preflight_remote(&diverged, RemoteAction::Push),
            Err(RemoteBlockReason::Diverged)
        );

        let in_progress = RemoteState {
            operation_in_progress: true,
            ..state("## main...origin/main\n")
        };
        assert_eq!(
            preflight_remote(&in_progress, RemoteAction::Fetch),
            Err(RemoteBlockReason::OperationInProgress)
        );
    }
}
