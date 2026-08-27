//! Pure validation and parsing for the Repo Manager stage/unstage/commit flow.
//!
//! Git is asked for porcelain-v1 status with NUL separators.  This keeps a
//! filename containing whitespace from changing the status record while the
//! command layer can still reject control, absolute, traversal, and pathspec
//! magic values before they reach a mutating Git command.

use serde::Serialize;

/// A single fixed error for all stage/unstage/commit command failures.  Git
/// diagnostics, repository paths, commit messages, hooks, and credential
/// helper output never cross the command boundary.
pub const GIT_MUTATION_ERROR: &str = "Git 변경 사항을 적용하지 못했습니다.";

/// Status output is bounded independently from the history/diff readers.
pub const MAX_STATUS_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_CHANGE_ENTRIES: usize = 512;
pub const MAX_CHANGE_PATH_BYTES: usize = 16 * 1024;
pub const MAX_SELECTED_PATHS: usize = 256;
pub const MAX_COMMIT_MESSAGE_BYTES: usize = 16 * 1024;

fn fixed_error() -> String {
    GIT_MUTATION_ERROR.to_string()
}

/// A Git porcelain-v1 status record exposed to the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    /// New-side repository-relative path.  For a rename this is the path Git
    /// expects in a stage/unstage pathspec.
    pub path: String,
    pub old_path: Option<String>,
    /// Porcelain XY status characters.  A space means no status in that side;
    /// `?` is the untracked worktree marker.
    pub index_status: String,
    pub worktree_status: String,
    pub kind: String,
    pub staged: bool,
    pub unstaged: bool,
}

/// Validate a repository-relative path before passing it after Git's `--`
/// pathspec separator.  Git pathspec magic (`:(...)`) is not needed by the UI
/// and is rejected so a displayed filename cannot turn into an exclusion or
/// attribute pathspec.
pub fn validate_change_path(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_CHANGE_PATH_BYTES
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
    Ok(value.to_string())
}

/// Validate the explicit commit message.  Newlines, carriage returns, and
/// tabs are valid commit-message data; all other control characters and an
/// empty/whitespace-only message are rejected without echoing the value.
pub fn validate_commit_message(value: &str) -> Result<String, String> {
    if value.trim().is_empty()
        || value.len() > MAX_COMMIT_MESSAGE_BYTES
        || value
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(fixed_error());
    }
    Ok(value.to_string())
}

/// Parse `git status --porcelain=v1 -z --untracked-files=all`.
///
/// Ordinary records are `XY path\0`; rename/copy records have a second NUL
/// record containing the old path.  The parser rejects malformed records and
/// unknown status bytes rather than exposing an untrusted partial result.
pub fn parse_status_changes(input: &str) -> Result<Vec<ChangeEntry>, String> {
    if input.len() > MAX_STATUS_OUTPUT_BYTES {
        return Err(fixed_error());
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if !input.ends_with('\0') {
        return Err(fixed_error());
    }

    let records: Vec<&str> = input.split_terminator('\0').collect();
    let mut entries = Vec::with_capacity(records.len().min(MAX_CHANGE_ENTRIES));
    let mut index = 0usize;
    while index < records.len() {
        if entries.len() >= MAX_CHANGE_ENTRIES {
            return Err(fixed_error());
        }
        let record = records[index];
        if record.len() < 4 || record.as_bytes().get(2) != Some(&b' ') {
            return Err(fixed_error());
        }
        let status = record.as_bytes();
        let index_status = status[0] as char;
        let worktree_status = status[1] as char;
        if !valid_status_code(index_status) || !valid_status_code(worktree_status) {
            return Err(fixed_error());
        }
        let path = validate_change_path(&record[3..])?;
        let rename_or_copy =
            matches!(index_status, 'R' | 'C') || matches!(worktree_status, 'R' | 'C');
        let old_path = if rename_or_copy {
            index += 1;
            let old = records.get(index).ok_or_else(fixed_error)?;
            Some(validate_change_path(old)?)
        } else {
            None
        };
        let staged = index_status != ' ' && index_status != '?';
        let unstaged = worktree_status != ' ' && worktree_status != '!';
        entries.push(ChangeEntry {
            path,
            old_path,
            index_status: index_status.to_string(),
            worktree_status: worktree_status.to_string(),
            kind: classify_kind(index_status, worktree_status),
            staged,
            unstaged,
        });
        index += 1;
    }
    Ok(entries)
}

fn valid_status_code(value: char) -> bool {
    matches!(
        value,
        ' ' | 'M' | 'A' | 'D' | 'R' | 'C' | 'T' | 'U' | '?' | '!'
    )
}

fn classify_kind(index_status: char, worktree_status: char) -> String {
    if index_status == 'U' || worktree_status == 'U' {
        "conflict".to_string()
    } else if index_status == '?' || worktree_status == '?' {
        "untracked".to_string()
    } else if index_status == 'R' || worktree_status == 'R' {
        "renamed".to_string()
    } else if index_status == 'C' || worktree_status == 'C' {
        "copied".to_string()
    } else if index_status == 'A' || worktree_status == 'A' {
        "added".to_string()
    } else if index_status == 'D' || worktree_status == 'D' {
        "deleted".to_string()
    } else {
        "modified".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_staged_unstaged_untracked_deleted_and_rename_records() {
        let input = concat!(
            "M  staged.rs\0",
            " M both.rs\0",
            "?? new file.txt\0",
            " D removed.rs\0",
            "R  new-name.rs\0old-name.rs\0",
        );
        let entries = parse_status_changes(input).unwrap();
        assert_eq!(entries.len(), 5);
        assert!(entries[0].staged);
        assert!(!entries[0].unstaged);
        assert!(!entries[1].staged);
        assert!(entries[1].unstaged);
        assert_eq!(entries[2].kind, "untracked");
        assert!(entries[2].unstaged);
        assert_eq!(entries[3].kind, "deleted");
        assert_eq!(entries[4].old_path.as_deref(), Some("old-name.rs"));
        assert_eq!(entries[4].kind, "renamed");
    }

    #[test]
    fn rejects_malformed_status_and_unsafe_paths_without_echoing_values() {
        let secret = "credential-status-secret";
        for input in [
            "M  path",  // missing NUL terminator
            "M path\0", // missing porcelain separator
            &format!("M  ../{secret}\0"),
            &format!("M  :({secret})\0"),
            &format!("M  /{secret}\0"),
        ] {
            let error = parse_status_changes(input).unwrap_err();
            assert_eq!(error, GIT_MUTATION_ERROR);
            assert!(!error.contains(secret));
        }
    }

    #[test]
    fn validates_message_bounds_and_allowed_newlines_without_reflection() {
        assert!(validate_commit_message("summary\n\nbody\t").is_ok());
        for value in [
            "",
            " \n\t",
            "bad\0message",
            &"x".repeat(MAX_COMMIT_MESSAGE_BYTES + 1),
        ] {
            let error = validate_commit_message(value).unwrap_err();
            assert_eq!(error, GIT_MUTATION_ERROR);
            if !value.is_empty() {
                assert!(!error.contains(value));
            }
        }
    }

    #[test]
    fn rejects_unknown_status_codes_and_excess_entries() {
        assert_eq!(
            parse_status_changes("X  suspicious\0").unwrap_err(),
            GIT_MUTATION_ERROR
        );
        let input = (0..=MAX_CHANGE_ENTRIES)
            .map(|index| format!("M  file-{index}.txt\0"))
            .collect::<String>();
        assert_eq!(
            parse_status_changes(&input).unwrap_err(),
            GIT_MUTATION_ERROR
        );
    }
}
