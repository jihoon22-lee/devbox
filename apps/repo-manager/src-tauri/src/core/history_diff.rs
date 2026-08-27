//! Bounded, read-only Git history and diff parsing.
//!
//! The command layer owns repository validation and subprocess execution. This
//! module only parses the deliberately small, NUL-delimited history/detail
//! format and the text-only unified diff emitted by Git. It never writes to a
//! repository and never formats subprocess errors or request paths.

use serde::Serialize;

/// One stable error for history/detail/diff failures. In particular, Git
/// stderr, repository paths, credential helpers, and commit contents are not
/// copied into this message.
pub const GIT_VIEW_ERROR: &str = "Git history 또는 diff를 불러올 수 없습니다.";

pub const MAX_HISTORY_LIMIT: usize = 100;
pub const MAX_HISTORY_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_DETAIL_OUTPUT_BYTES: usize = 128 * 1024;
pub const MAX_DIFF_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIFF_FILES: usize = 256;
pub const MAX_DIFF_FILE_BYTES: usize = 512 * 1024;
pub const MAX_COMMIT_ID_LENGTH: usize = 64;
pub const MAX_COMMIT_SUBJECT_BYTES: usize = 4 * 1024;
pub const MAX_COMMIT_BODY_BYTES: usize = 64 * 1024;
pub const MAX_AUTHOR_FIELD_BYTES: usize = 512;
pub const MAX_TIMESTAMP_BYTES: usize = 64;
pub const MAX_DIFF_PATH_BYTES: usize = 16 * 1024;

fn fixed_error() -> String {
    GIT_VIEW_ERROR.to_string()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitSummary {
    pub id: String,
    pub short_id: String,
    pub parents: Vec<String>,
    pub authored_at: String,
    pub author: String,
    pub author_email: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub entries: Vec<CommitSummary>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    pub id: String,
    pub parents: Vec<String>,
    pub authored_at: String,
    pub author: String,
    pub author_email: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    /// `workingTree` compares `HEAD` with tracked current index/worktree changes. `commit`
    /// compares one selected commit with its parent (including root commits).
    pub scope: String,
    pub commit_id: Option<String>,
    pub files: Vec<DiffFile>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    /// The new-side repository-relative path. Deleted files retain their
    /// repository path here; `/dev/null` is never exposed as a user path.
    pub path: String,
    pub old_path: Option<String>,
    pub status: String,
    pub binary: bool,
    pub patch: String,
    pub truncated: bool,
}

/// Parse the NUL-delimited `%H %P %aI %an %ae %s` history format.
pub fn parse_history(input: &str, limit: usize) -> Result<HistoryResult, String> {
    if !(1..=MAX_HISTORY_LIMIT).contains(&limit) {
        return Err(fixed_error());
    }

    let normalized = normalize_nul_output(input, MAX_HISTORY_OUTPUT_BYTES, true)?;
    let fields: Vec<&str> = normalized.split_terminator('\0').collect();
    if !fields.len().is_multiple_of(6) || fields.len() / 6 > limit.saturating_add(1) {
        return Err(fixed_error());
    }

    let mut entries = Vec::with_capacity(fields.len() / 6);
    for record in fields.chunks_exact(6) {
        entries.push(CommitSummary {
            id: parse_commit_id(record[0])?,
            short_id: short_id(record[0])?,
            parents: parse_parents(record[1])?,
            authored_at: parse_git_timestamp(record[2])?,
            author: bounded_text(record[3], MAX_AUTHOR_FIELD_BYTES, false)?,
            author_email: bounded_text(record[4], MAX_AUTHOR_FIELD_BYTES, false)?,
            subject: bounded_text(record[5], MAX_COMMIT_SUBJECT_BYTES, false)?,
        });
    }

    let has_more = entries.len() > limit;
    entries.truncate(limit);
    Ok(HistoryResult { entries, has_more })
}

/// Parse the NUL-delimited `%H %P %aI %an %ae %s %b` detail format.
pub fn parse_detail(input: &str) -> Result<CommitDetail, String> {
    let normalized = normalize_nul_output(input, MAX_DETAIL_OUTPUT_BYTES, false)?;
    let fields: Vec<&str> = normalized.split_terminator('\0').collect();
    if fields.len() != 7 {
        return Err(fixed_error());
    }
    Ok(CommitDetail {
        id: parse_commit_id(fields[0])?,
        parents: parse_parents(fields[1])?,
        authored_at: parse_git_timestamp(fields[2])?,
        author: bounded_text(fields[3], MAX_AUTHOR_FIELD_BYTES, false)?,
        author_email: bounded_text(fields[4], MAX_AUTHOR_FIELD_BYTES, false)?,
        subject: bounded_text(fields[5], MAX_COMMIT_SUBJECT_BYTES, false)?,
        body: bounded_text(fields[6], MAX_COMMIT_BODY_BYTES, true)?,
    })
}

/// Validate a user-selected commit revision before it reaches Git argv.
/// Revisions are intentionally restricted to hexadecimal object IDs, not
/// arbitrary rev expressions or pathspecs.
pub fn validate_commit_id(value: &str) -> Result<String, String> {
    if !(7..=MAX_COMMIT_ID_LENGTH).contains(&value.len())
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(fixed_error());
    }
    Ok(value.to_ascii_lowercase())
}

/// Parse a bounded unified diff. Git is invoked without external diff/textconv
/// and without `--binary`, so binary files produce a marker rather than raw
/// bytes. Merge commits are requested as parent-by-parent standard patches.
/// Each file patch and the total command output have independent caps.
pub fn parse_diff(
    input: &str,
    scope: &str,
    commit_id: Option<String>,
    subprocess_truncated: bool,
) -> Result<DiffResult, String> {
    if input.len() > MAX_DIFF_OUTPUT_BYTES || !matches!(scope, "workingTree" | "commit") {
        return Err(fixed_error());
    }
    let commit_id = match (scope, commit_id) {
        ("workingTree", None) => None,
        ("commit", Some(value)) => Some(validate_commit_id(&value)?),
        _ => return Err(fixed_error()),
    };

    let mut files = Vec::new();
    let mut current: Option<DiffFileBuilder> = None;
    let mut truncated = subprocess_truncated;
    let mut skip_remaining_files = false;

    for line in input.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            if let Some(file) = current.take() {
                let file = file.finish()?;
                truncated |= file.truncated;
                files.push(file);
            }
            if files.len() >= MAX_DIFF_FILES {
                truncated = true;
                skip_remaining_files = true;
                continue;
            }
            if skip_remaining_files {
                continue;
            }
            let (old_path, new_path) = parse_git_header(header)?;
            let mut builder = DiffFileBuilder::new(old_path, new_path);
            builder.append_fixed_header();
            current = Some(builder);
            continue;
        }

        if skip_remaining_files {
            continue;
        }

        let Some(file) = current.as_mut() else {
            if line.trim().is_empty() || subprocess_truncated {
                truncated = true;
                continue;
            }
            // A non-empty prefix without a canonical diff header is not a
            // response we can safely classify, so fail without echoing it.
            return Err(fixed_error());
        };

        if line.starts_with("Binary files ")
            || (line.starts_with("Files ") && line.ends_with(" differ"))
        {
            file.binary = true;
            file.append_line("Binary files differ");
            continue;
        }
        if file.binary {
            // Do not retain any further bytes after a binary marker even if a
            // Git implementation emits an unexpected continuation.
            continue;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            file.append_line(&format!("--- {}", display_side_path(path, true)?));
            continue;
        }
        if let Some(path) = line.strip_prefix("+++ ") {
            file.append_line(&format!("+++ {}", display_side_path(path, false)?));
            continue;
        }
        if line.starts_with("new file mode ") {
            file.status = "added".to_string();
        } else if line.starts_with("deleted file mode ") {
            file.status = "deleted".to_string();
        } else if line.starts_with("rename from ") {
            file.status = "renamed".to_string();
            // The canonical diff header already supplied the old/new path;
            // do not copy an unvalidated rename payload into the response.
            continue;
        } else if line.starts_with("rename to ") {
            continue;
        }
        file.append_line(line);
    }

    if let Some(file) = current {
        let file = file.finish()?;
        truncated |= file.truncated;
        files.push(file);
    }
    if files.len() > MAX_DIFF_FILES {
        files.truncate(MAX_DIFF_FILES);
        truncated = true;
    }
    Ok(DiffResult {
        scope: scope.to_string(),
        commit_id,
        files,
        truncated,
    })
}

fn parse_commit_id(value: &str) -> Result<String, String> {
    if !(40..=MAX_COMMIT_ID_LENGTH).contains(&value.len())
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(fixed_error());
    }
    Ok(value.to_ascii_lowercase())
}

fn short_id(value: &str) -> Result<String, String> {
    let id = parse_commit_id(value)?;
    Ok(id[..12].to_string())
}

fn parse_parents(value: &str) -> Result<Vec<String>, String> {
    value.split_whitespace().map(parse_commit_id).collect()
}

/// Validate the exact timestamp shape emitted by Git's `%aI` pretty-format.
///
/// `%aI` is not an arbitrary ISO-8601 input field: Git emits an extended
/// calendar date, a `T` separator, a seconds-resolution wall-clock time, and
/// either `Z` or a signed offset.  Keep the original value for display only
/// after checking every component.  In particular, parsing only the shape
/// would allow impossible dates such as February 31 or offsets outside the
/// range accepted by Git's raw identity format.
fn parse_git_timestamp(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() > MAX_TIMESTAMP_BYTES || bytes.len() < 20 || !value.is_ascii() {
        return Err(fixed_error());
    }

    // The year is four or more decimal digits. Git's formatter uses a
    // variable-width year for dates beyond 9999, so do not silently reject a
    // valid Git object solely because its year is expanded.
    let separator = bytes
        .iter()
        .position(|byte| *byte == b'T')
        .ok_or_else(fixed_error)?;
    if separator < 10
        || bytes[separator - 6] != b'-'
        || bytes[separator - 3] != b'-'
        || !bytes[..separator - 6]
            .iter()
            .all(|byte| byte.is_ascii_digit())
        || bytes[..separator - 6].iter().all(|byte| *byte == b'0')
    {
        return Err(fixed_error());
    }

    let year = &bytes[..separator - 6];
    let month = parse_two_digits(bytes, separator - 5).ok_or_else(fixed_error)?;
    let day = parse_two_digits(bytes, separator - 2).ok_or_else(fixed_error)?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return Err(fixed_error());
    }

    // `HH:MM:SS` is exactly eight bytes and the timezone begins immediately
    // after it. This also rejects fractional seconds and trailing whitespace.
    if separator + 9 > bytes.len() || bytes[separator + 3] != b':' || bytes[separator + 6] != b':' {
        return Err(fixed_error());
    }
    let hour = parse_two_digits(bytes, separator + 1).ok_or_else(fixed_error)?;
    let minute = parse_two_digits(bytes, separator + 4).ok_or_else(fixed_error)?;
    let second = parse_two_digits(bytes, separator + 7).ok_or_else(fixed_error)?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(fixed_error());
    }

    let offset = &bytes[separator + 9..];
    if offset == b"Z" {
        return Ok(value.to_owned());
    }
    if offset.len() != 6 || !matches!(offset[0], b'+' | b'-') || offset[3] != b':' {
        return Err(fixed_error());
    }
    let offset_hour = parse_two_digits(offset, 1).ok_or_else(fixed_error)?;
    let offset_minute = parse_two_digits(offset, 4).ok_or_else(fixed_error)?;
    // Git accepts offsets through ±14:00; values beyond that are not emitted
    // by `%aI`, and 14:01..14:59 are not valid timezone offsets either.
    if offset_hour > 14 || offset_minute > 59 || (offset_hour == 14 && offset_minute != 0) {
        return Err(fixed_error());
    }

    Ok(value.to_owned())
}

fn parse_two_digits(bytes: &[u8], start: usize) -> Option<u8> {
    let first = *bytes.get(start)?;
    let second = *bytes.get(start + 1)?;
    if !first.is_ascii_digit() || !second.is_ascii_digit() {
        return None;
    }
    Some((first - b'0') * 10 + second - b'0')
}

fn days_in_month(year: &[u8], month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: &[u8]) -> bool {
    // Avoid parsing an expanded Git year into a fixed-width integer. The
    // Gregorian rule depends only on the year modulo 4, 100, and 400.
    let modulo = |divisor: u16| {
        year.iter().fold(0u16, |remainder, digit| {
            (remainder * 10 + u16::from(digit - b'0')) % divisor
        })
    };
    modulo(4) == 0 && (modulo(100) != 0 || modulo(400) == 0)
}

fn bounded_text(value: &str, max_bytes: usize, allow_newlines: bool) -> Result<String, String> {
    if value.len() > max_bytes
        || value.chars().any(|character| {
            character.is_control() && !(allow_newlines && matches!(character, '\n' | '\r' | '\t'))
        })
    {
        return Err(fixed_error());
    }
    Ok(value.to_string())
}

/// Git's pretty-format command appends a line terminator after each formatted
/// record. Remove only that terminator when it follows our NUL record marker;
/// newlines inside a commit body remain data and are still bounded below.
fn normalize_nul_output(
    input: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<String, String> {
    if input.len() > max_bytes {
        return Err(fixed_error());
    }
    if input.is_empty() && allow_empty {
        return Ok(String::new());
    }
    let normalized = input.replace("\0\r\n", "\0").replace("\0\n", "\0");
    if !normalized.ends_with('\0') {
        return Err(fixed_error());
    }
    Ok(normalized)
}

fn parse_git_header(value: &str) -> Result<(String, String), String> {
    let value = value.strip_prefix("a/").ok_or_else(fixed_error)?;
    let mut candidate = None;
    // Every diff command fixes `--no-renames`, so the canonical old/new path
    // in this header must be identical. Searching for that equality avoids
    // treating an ordinary ` b/` inside a directory name as the separator.
    for (split, _) in value.match_indices(" b/") {
        let old = &value[..split];
        let new = &value[split + 3..];
        if old != new {
            continue;
        }
        let path = validate_relative_path(old)?;
        if candidate.replace(path).is_some() {
            return Err(fixed_error());
        }
    }
    let path = candidate.ok_or_else(fixed_error)?;
    Ok((path.clone(), path))
}

fn display_side_path(value: &str, old_side: bool) -> Result<String, String> {
    if value == "/dev/null" {
        return Ok(value.to_string());
    }
    // Git appends one tab to disambiguate an unquoted path containing spaces.
    // A real control character in a path remains quoted even with
    // `core.quotePath=false` and is rejected by the relative-path validator.
    let value = value.strip_suffix('\t').unwrap_or(value);
    let prefix = if old_side { "a/" } else { "b/" };
    let path = value.strip_prefix(prefix).ok_or_else(fixed_error)?;
    Ok(format!("{prefix}{}", validate_relative_path(path)?))
}

fn validate_relative_path(value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.len() > MAX_DIFF_PATH_BYTES
        || value.starts_with('/')
        || value.contains(':')
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(fixed_error());
    }
    Ok(value.to_string())
}

struct DiffFileBuilder {
    path: String,
    old_path: Option<String>,
    status: String,
    binary: bool,
    patch: String,
    truncated: bool,
}

impl DiffFileBuilder {
    fn new(old_path: String, path: String) -> Self {
        let old_path = (old_path != path).then_some(old_path);
        Self {
            path,
            old_path,
            status: "modified".to_string(),
            binary: false,
            patch: String::new(),
            truncated: false,
        }
    }

    fn append_fixed_header(&mut self) {
        let old = self.old_path.as_deref().unwrap_or(&self.path);
        self.append_line(&format!("diff --git a/{old} b/{}", self.path));
    }

    fn append_line(&mut self, line: &str) {
        if self
            .patch
            .len()
            .saturating_add(line.len())
            .saturating_add(1)
            > MAX_DIFF_FILE_BYTES
        {
            self.truncated = true;
            return;
        }
        self.patch.push_str(line);
        self.patch.push('\n');
    }

    fn finish(self) -> Result<DiffFile, String> {
        Ok(DiffFile {
            path: self.path,
            old_path: self.old_path,
            status: self.status,
            binary: self.binary,
            patch: self.patch,
            truncated: self.truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OID: &str = "0123456789abcdef0123456789abcdef01234567";
    const PARENT: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn parses_bounded_history_and_has_more() {
        let first = format!(
            "{OID}\0{PARENT}\0{}\0Alice\0alice@example.test\0first\0",
            "2026-08-27T09:00:00+09:00"
        );
        let second = format!(
            "{PARENT}\0{PARENT} {OID}\0{}\0Bob\0bob@example.test\0second\0",
            "2026-08-26T09:00:00+09:00"
        );
        let input = format!("{first}\r\n{second}\r\n");
        let result = parse_history(&input, 1).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].short_id, &OID[..12]);
        assert_eq!(result.entries[0].parents, vec![PARENT]);
        assert!(result.has_more);

        let second = parse_history(&input, 2).unwrap();
        assert_eq!(second.entries[1].parents, vec![PARENT, OID]);
    }

    #[test]
    fn accepts_git_iso_strict_timestamp_forms() {
        for timestamp in [
            "1970-01-01T00:00:00Z",
            "2024-02-29T23:59:59+14:00",
            "2026-08-27T09:00:00-05:30",
            "10000-02-29T09:00:00+00:00",
        ] {
            let input = format!("{OID}\0\0{timestamp}\0Alice\0alice@example.test\0subject\0");
            assert!(parse_history(&input, 1).is_ok(), "timestamp: {timestamp}");
        }
    }

    #[test]
    fn rejects_malformed_git_iso_strict_timestamps() {
        for timestamp in [
            "2026-02-29T09:00:00+09:00",     // non-leap year
            "0000-02-29T09:00:00+09:00",     // Git dates have no year zero
            "2024-04-31T09:00:00+09:00",     // impossible calendar day
            "2026-13-01T09:00:00+09:00",     // month out of range
            "2026-08-27T24:00:00+09:00",     // hour out of range
            "2026-08-27T09:60:00+09:00",     // minute out of range
            "2026-08-27T09:00:60+09:00",     // second out of range
            "2026-08-27T09:00:00+14:01",     // Git's maximum offset is +14:00
            "2026-08-27T09:00:00-15:00",     // offset hour out of range
            "2026-08-27T09:00:00+09:60",     // offset minute out of range
            "2026-08-27 09:00:00+09:00",     // wrong separator
            "2026-08-27T09:00:00.123+09:00", // fractional seconds
            "2026-08-27T09:00:00+0900",      // non-strict offset
            "2026-08-27T09:00:00+09:00 ",    // trailing whitespace
        ] {
            let input = format!("{OID}\0\0{timestamp}\0Alice\0alice@example.test\0subject\0");
            assert_eq!(
                parse_history(&input, 1),
                Err(GIT_VIEW_ERROR.to_string()),
                "timestamp: {timestamp}"
            );
        }
    }

    #[test]
    fn parses_root_detail_with_multiline_body() {
        let input = format!(
            "{OID}\0\02026-08-27T09:00:00+09:00\0Alice\0alice@example.test\0subject\0body\nline\n\0\n"
        );
        let detail = parse_detail(&input).unwrap();
        assert!(detail.parents.is_empty());
        assert_eq!(detail.body, "body\nline\n");
    }

    #[test]
    fn rejects_malformed_or_oversized_history_without_echoing_values() {
        assert_eq!(
            validate_commit_id("--secret"),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_history("raw-secret", 1),
            Err(GIT_VIEW_ERROR.to_string())
        );
        let oversized = "x".repeat(MAX_HISTORY_OUTPUT_BYTES + 1);
        let error = parse_history(&oversized, 1).unwrap_err();
        assert_eq!(error, GIT_VIEW_ERROR);
        assert!(!error.contains('x'));
        assert_eq!(
            parse_history(
                &format!("{OID}\0\02026-08-27T09:00:00+09:00\0Alice\0alice@example.test\0subject"),
                1,
            ),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_detail(&format!(
                "{OID}\0\02026-08-27T09:00:00+09:00\0Alice\0alice@example.test\0subject\0body"
            )),
            Err(GIT_VIEW_ERROR.to_string())
        );
    }

    #[test]
    fn parses_text_and_binary_diff_without_raw_binary_bytes() {
        let input = concat!(
            "diff --git a/src/main.rs b/src/main.rs\n",
            "index 1111111..2222222 100644\n",
            "--- a/src/main.rs\n",
            "+++ b/src/main.rs\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n",
            "diff --git a/assets/icon.bin b/assets/icon.bin\n",
            "index 1111111..2222222\n",
            "Binary files a/assets/icon.bin and b/assets/icon.bin differ\n",
        );
        let result = parse_diff(input, "workingTree", None, false).unwrap();
        assert_eq!(result.files.len(), 2);
        assert!(!result.files[0].binary);
        assert!(result.files[0].patch.contains("+new"));
        assert!(result.files[1].binary);
        assert!(result.files[1].patch.ends_with("Binary files differ\n"));
        assert!(!result.files[1].patch.contains("raw-binary-bytes"));
    }

    #[test]
    fn parses_unquoted_space_paths_without_splitting_an_inner_b_prefix() {
        let input = concat!(
            "diff --git a/folder b/foo bar.txt b/folder b/foo bar.txt\n",
            "--- a/folder b/foo bar.txt\t\n",
            "+++ b/folder b/foo bar.txt\t\n",
            "@@ -1 +1 @@\n",
            "-before\n",
            "+after\n",
        );
        let parsed = parse_diff(input, "workingTree", None, false).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].path, "folder b/foo bar.txt");
        assert!(parsed.files[0].old_path.is_none());
        assert!(parsed.files[0]
            .patch
            .contains("--- a/folder b/foo bar.txt\n"));
    }

    #[test]
    fn rejects_unsafe_diff_paths_and_marks_bounded_output() {
        let unsafe_diff = "diff --git a/../secret b/../secret\n";
        assert_eq!(
            parse_diff(unsafe_diff, "commit", Some(OID.to_string()), false),
            Err(GIT_VIEW_ERROR.to_string())
        );

        let input = "diff --git a/a.txt b/a.txt\n+line\n";
        let result = parse_diff(input, "commit", Some(OID.to_string()), true).unwrap();
        assert!(result.truncated);
        assert_eq!(result.commit_id.as_deref(), Some(OID));

        let mut many_files = String::new();
        for index in 0..=MAX_DIFF_FILES {
            many_files.push_str(&format!(
                "diff --git a/file-{index}.txt b/file-{index}.txt\n"
            ));
        }
        let result = parse_diff(&many_files, "workingTree", None, false).unwrap();
        assert_eq!(result.files.len(), MAX_DIFF_FILES);
        assert!(result.truncated);

        let oversized_patch = format!(
            "diff --git a/large.txt b/large.txt\n{}\n",
            "x".repeat(MAX_DIFF_FILE_BYTES)
        );
        let result = parse_diff(&oversized_patch, "workingTree", None, false).unwrap();
        assert!(result.files[0].truncated);
        assert!(result.truncated);
    }

    #[test]
    fn rejects_unknown_scope_and_invalid_commit_ids() {
        assert_eq!(validate_commit_id("ABCDEF0"), Ok("abcdef0".to_string()));
        assert_eq!(
            validate_commit_id("abcdef"),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_diff("", "workingTree", Some("raw-secret".to_string()), false),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_diff("", "commit", None, false),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_diff("", "other", None, false),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_commit_id("not-an-object"),
            Err(GIT_VIEW_ERROR.to_string())
        );
        assert_eq!(
            parse_diff("warning: raw credential", "workingTree", None, false),
            Err(GIT_VIEW_ERROR.to_string())
        );
    }
}
