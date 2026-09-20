//! Pure Git state preflight parsing and classification.
//!
//! The command layer asks Git for porcelain-v2 status with branch metadata.
//! This module deliberately only classifies the bounded output; it never
//! performs a recovery operation or exposes a raw Git diagnostic.

use serde::Serialize;

/// One fixed error for all safety-preflight failures. Git diagnostics,
/// repository paths, remote URLs, and credential-helper output never cross
/// the command boundary.
pub const GIT_SAFETY_ERROR: &str = "Git 상태를 확인하지 못했습니다.";

/// Status output is bounded before it reaches the parser.
pub const MAX_SAFETY_OUTPUT_BYTES: usize = 512 * 1024;
/// A repository with an unexpectedly large number of records fails closed
/// instead of returning a partial safety result.
pub const MAX_SAFETY_RECORDS: usize = 2_048;
pub const MAX_SAFETY_LABEL_BYTES: usize = 4 * 1024;
pub const MAX_SAFETY_PATH_BYTES: usize = 16 * 1024;

fn fixed_error() -> String {
    GIT_SAFETY_ERROR.to_string()
}

/// State parsed from `git status --porcelain=v2 --branch -z` before metadata
/// markers (rebase/merge) are checked by the command layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSafetyStatus {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub detached: bool,
}

/// Read-only state returned to the frontend. `safe` means that this bounded
/// snapshot has none of the known Git safety blockers. It does not authorize
/// or perform any Git operation.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitSafetySnapshot {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub detached: bool,
    pub no_upstream: bool,
    pub diverged: bool,
    pub rebase_in_progress: bool,
    pub merge_in_progress: bool,
    pub safe: bool,
    /// Stable machine-readable issue IDs in deterministic order. The UI owns
    /// the localized explanation and never receives raw Git output.
    pub issues: Vec<String>,
}

/// Parse Git's NUL-delimited porcelain-v2 status and branch headers.
///
/// This parser intentionally requires the exact format emitted by the fixed
/// command argv. Unknown records, malformed metadata, invalid statuses, and
/// missing NUL termination discard the complete result with one fixed error.
pub fn parse_porcelain_v2(input: &str) -> Result<ParsedSafetyStatus, String> {
    if input.is_empty() || input.len() > MAX_SAFETY_OUTPUT_BYTES || !input.ends_with('\0') {
        return Err(fixed_error());
    }

    let records = input.split_terminator('\0').collect::<Vec<_>>();
    if records.is_empty() || records.len() > MAX_SAFETY_RECORDS {
        return Err(fixed_error());
    }

    let mut branch_oid_seen = false;
    let mut branch_head_seen = false;
    let mut upstream_seen = false;
    let mut ahead_behind_seen = false;
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut dirty = false;
    let mut expect_rename_source = false;

    for record in records {
        if expect_rename_source {
            validate_status_path(record)?;
            expect_rename_source = false;
            continue;
        }

        if let Some(value) = record.strip_prefix("# branch.oid ") {
            if branch_oid_seen || !valid_object_id(value) {
                return Err(fixed_error());
            }
            branch_oid_seen = true;
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.head ") {
            if branch_head_seen {
                return Err(fixed_error());
            }
            let value = validate_label(value)?;
            if value == "(unknown)" {
                return Err(fixed_error());
            }
            let detached = value == "(detached)";
            branch_head_seen = true;
            branch = Some((value, detached));
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.upstream ") {
            if upstream_seen {
                return Err(fixed_error());
            }
            upstream_seen = true;
            upstream = Some(validate_label(value)?);
            continue;
        }
        if let Some(value) = record.strip_prefix("# branch.ab ") {
            if ahead_behind_seen {
                return Err(fixed_error());
            }
            let (parsed_ahead, parsed_behind) = parse_ahead_behind(value)?;
            ahead = parsed_ahead;
            behind = parsed_behind;
            ahead_behind_seen = true;
            continue;
        }

        match record.as_bytes().first().copied() {
            Some(b'1') => {
                parse_fixed_status_record(record, b'1', 9)?;
                dirty = true;
            }
            Some(b'2') => {
                parse_fixed_status_record(record, b'2', 10)?;
                dirty = true;
                expect_rename_source = true;
            }
            Some(b'u') => {
                parse_fixed_status_record(record, b'u', 11)?;
                dirty = true;
            }
            Some(b'?') => {
                if !record.starts_with("? ") {
                    return Err(fixed_error());
                }
                validate_status_path(&record[2..])?;
                dirty = true;
            }
            // Ignored records are not emitted by this command (there is no
            // --ignored flag), so accepting one would make the argv/result
            // contract ambiguous.
            _ => return Err(fixed_error()),
        }
    }

    if expect_rename_source || !branch_oid_seen {
        return Err(fixed_error());
    }
    let Some((branch, detached)) = branch else {
        return Err(fixed_error());
    };
    if ahead_behind_seen && !upstream_seen {
        return Err(fixed_error());
    }
    if detached && upstream.is_some() {
        return Err(fixed_error());
    }

    Ok(ParsedSafetyStatus {
        branch,
        upstream,
        ahead,
        behind,
        dirty,
        detached,
    })
}

/// Combine the parsed status with filesystem marker observations from the
/// command layer. Marker state is supplied explicitly so this function stays
/// pure and straightforward to fixture.
pub fn classify(
    status: ParsedSafetyStatus,
    rebase_in_progress: bool,
    merge_in_progress: bool,
) -> GitSafetySnapshot {
    let no_upstream = !status.detached && status.upstream.is_none();
    let diverged = status.ahead > 0 && status.behind > 0;
    let mut issues = Vec::with_capacity(6);
    if status.dirty {
        issues.push("dirty".to_string());
    }
    if status.detached {
        issues.push("detached".to_string());
    }
    if no_upstream {
        issues.push("noUpstream".to_string());
    }
    if diverged {
        issues.push("diverged".to_string());
    }
    if rebase_in_progress {
        issues.push("rebaseInProgress".to_string());
    }
    if merge_in_progress {
        issues.push("mergeInProgress".to_string());
    }
    GitSafetySnapshot {
        branch: status.branch,
        upstream: status.upstream,
        ahead: status.ahead,
        behind: status.behind,
        dirty: status.dirty,
        detached: status.detached,
        no_upstream,
        diverged,
        rebase_in_progress,
        merge_in_progress,
        safe: issues.is_empty(),
        issues,
    }
}

fn valid_object_id(value: &str) -> bool {
    if value == "(initial)" {
        return true;
    }
    (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Branch/upstream labels are metadata rather than user-selected pathspecs.
/// Keep them bounded and reject whitespace, controls, URL-like values, and
/// credential-shaped data before returning them to the frontend.
fn validate_label(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_SAFETY_LABEL_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || value.contains("://")
        || value.contains('@')
    {
        return Err(fixed_error());
    }
    Ok(value.to_string())
}

fn parse_ahead_behind(value: &str) -> Result<(u32, u32), String> {
    let mut fields = value.split(' ');
    let ahead = parse_signed_count(fields.next(), b'+')?;
    let behind = parse_signed_count(fields.next(), b'-')?;
    if fields.next().is_some() {
        return Err(fixed_error());
    }
    Ok((ahead, behind))
}

fn parse_signed_count(value: Option<&str>, sign: u8) -> Result<u32, String> {
    let Some(value) = value else {
        return Err(fixed_error());
    };
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != sign || !bytes[1..].iter().all(u8::is_ascii_digit) {
        return Err(fixed_error());
    }
    std::str::from_utf8(&bytes[1..])
        .ok()
        .and_then(|number| number.parse::<u32>().ok())
        .ok_or_else(fixed_error)
}

fn parse_fixed_status_record(record: &str, kind: u8, field_count: usize) -> Result<(), String> {
    let fields = record.splitn(field_count, ' ').collect::<Vec<_>>();
    if fields.len() != field_count
        || fields[0].as_bytes() != [kind]
        || fields[1].len() != 2
        || !fields[1].as_bytes().iter().copied().all(valid_status_byte)
    {
        return Err(fixed_error());
    }
    if fields[2..field_count - 1]
        .iter()
        .any(|field| field.is_empty() || field.chars().any(char::is_control))
    {
        return Err(fixed_error());
    }
    validate_status_path(fields[field_count - 1])
}

fn valid_status_byte(value: u8) -> bool {
    matches!(
        value,
        b' ' | b'.' | b'M' | b'A' | b'D' | b'R' | b'C' | b'T' | b'U' | b'?' | b'!'
    )
}

fn validate_status_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_SAFETY_PATH_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(fixed_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "0123456789abcdef0123456789abcdef01234567";

    fn status(headers: &str, records: &str) -> String {
        let mut output = format!("# branch.oid {OID}\0{headers}{records}");
        if !output.ends_with('\0') {
            output.push('\0');
        }
        output
    }

    #[test]
    fn parses_exact_clean_upstream_matrix() {
        let parsed = parse_porcelain_v2(&status(
            "# branch.head main\0# branch.upstream origin/main\0# branch.ab +0 -0\0",
            "",
        ))
        .unwrap();
        assert_eq!(
            classify(parsed, false, false),
            GitSafetySnapshot {
                branch: "main".into(),
                upstream: Some("origin/main".into()),
                ahead: 0,
                behind: 0,
                dirty: false,
                detached: false,
                no_upstream: false,
                diverged: false,
                rebase_in_progress: false,
                merge_in_progress: false,
                safe: true,
                issues: vec![],
            }
        );
    }

    #[test]
    fn classifies_dirty_detached_no_upstream_and_diverged_states() {
        let dirty = parse_porcelain_v2(&status(
            "# branch.head main\0# branch.upstream origin/main\0# branch.ab +1 -0\0",
            "1 .M N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 src/app.rs",
        ))
        .unwrap();
        assert_eq!(classify(dirty, false, false).issues, vec!["dirty"]);

        let detached = parse_porcelain_v2(&status("# branch.head (detached)\0", "")).unwrap();
        let detached = classify(detached, false, false);
        assert!(detached.detached);
        assert!(!detached.no_upstream);
        assert_eq!(detached.issues, vec!["detached"]);

        let no_upstream = parse_porcelain_v2(&status("# branch.head feature\0", "")).unwrap();
        let no_upstream = classify(no_upstream, false, false);
        assert!(no_upstream.no_upstream);
        assert_eq!(no_upstream.issues, vec!["noUpstream"]);

        let diverged = parse_porcelain_v2(&status(
            "# branch.head feature\0# branch.upstream origin/feature\0# branch.ab +2 -3\0",
            "",
        ))
        .unwrap();
        let diverged = classify(diverged, false, false);
        assert!(diverged.diverged);
        assert_eq!(diverged.issues, vec!["diverged"]);

        for ahead_behind in ["+1 -0", "+0 -2"] {
            let status = parse_porcelain_v2(&status(
                &format!(
                    "# branch.head feature\0# branch.upstream origin/feature\0# branch.ab {ahead_behind}\0"
                ),
                "",
            ))
            .unwrap();
            let state = classify(status, false, false);
            assert!(!state.diverged);
            assert!(state.issues.is_empty());
        }
    }

    #[test]
    fn classifies_rebase_and_merge_markers_in_stable_order() {
        let parsed = parse_porcelain_v2(&status(
            "# branch.head feature\0# branch.upstream origin/feature\0# branch.ab +1 -2\0",
            "? untracked.txt",
        ))
        .unwrap();
        let state = classify(parsed, true, true);
        assert_eq!(
            state.issues,
            vec!["dirty", "diverged", "rebaseInProgress", "mergeInProgress"]
        );
        assert!(!state.safe);
    }

    #[test]
    fn parses_rename_source_and_rejects_malformed_or_unknown_records() {
        let renamed = status(
            "# branch.head main\0",
            "2 R. N... 100644 100644 100644 0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 R100 new.txt\0old.txt",
        );
        assert!(parse_porcelain_v2(&renamed).is_ok());

        for input in [
            "# branch.oid (initial)\0# branch.head main", // missing final NUL record separator
            &status("# branch.head main\0", "X suspicious"),
            &status(
                "# branch.head main\0# branch.upstream https://token@host/main\0",
                "",
            ),
            &status("# branch.head main\0# branch.ab +1 -0\0", ""),
            "# branch.oid (unknown)\0# branch.head main\0",
            &status("# branch.head (unknown)\0", ""),
        ] {
            let error = parse_porcelain_v2(input).unwrap_err();
            assert_eq!(error, GIT_SAFETY_ERROR);
            assert!(!error.contains("token"));
        }
    }

    #[test]
    fn rejects_status_output_overflow_and_path_controls_without_reflection() {
        let secret = "credential-status-secret";
        let oversized = "x".repeat(MAX_SAFETY_OUTPUT_BYTES + 1);
        for input in [
            oversized,
            status("# branch.head main\0", &format!("? {secret}\n")),
        ] {
            let error = parse_porcelain_v2(&input).unwrap_err();
            assert_eq!(error, GIT_SAFETY_ERROR);
            assert!(!error.contains(secret));
        }
    }

    #[test]
    fn negative_operation_flags_are_not_part_of_the_read_only_state_contract() {
        let source = include_str!("../commands.rs");
        assert!(!source.contains("git reset"));
        assert!(!source.contains("git clean"));
        assert!(!source.contains("push --force"));
    }
}
