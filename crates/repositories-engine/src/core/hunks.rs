//! Bounded Git patch parsing. Native code generates patches; callers select IDs only.
use serde::Serialize;
use sha2::{Digest, Sha256};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum HunkLineKind {
    Context,
    Add,
    Remove,
    NoNewline,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HunkLine {
    pub kind: HunkLineKind,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<HunkLine>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FilePatch {
    pub header: String,
    pub hunks: Vec<Hunk>,
}
pub fn patch_revision(patch: &str) -> String {
    Sha256::digest(patch.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn unsupported_reason(patch: &str) -> Option<&'static str> {
    for line in patch.split('\n') {
        if line.starts_with("index ")
            && matches!(line.split_whitespace().last(), Some("120000" | "160000"))
        {
            return Some("mode_change");
        }
        if line.starts_with("new file mode ") {
            return Some("new_file");
        }
        if line.starts_with("deleted file mode ") {
            return Some("deleted_file");
        }
        if line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.starts_with("copy from ")
            || line.starts_with("copy to ")
        {
            return Some("rename");
        }
        if line.starts_with("old mode ") || line.starts_with("new mode ") {
            return Some("mode_change");
        }
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            return Some("binary");
        }
    }
    None
}
fn range(text: &str, prefix: char) -> Result<(u32, u32), String> {
    let text = text.strip_prefix(prefix).ok_or("hunk_unsupported")?;
    let (start, count) = text.split_once(',').unwrap_or((text, "1"));
    if start.is_empty()
        || count.is_empty()
        || !start
            .bytes()
            .chain(count.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err("hunk_unsupported".into());
    }
    Ok((
        start.parse().map_err(|_| "hunk_unsupported")?,
        count.parse().map_err(|_| "hunk_unsupported")?,
    ))
}
fn hunk_header(line: &str) -> Result<Hunk, String> {
    let (ranges, _) = line
        .strip_prefix("@@ ")
        .and_then(|line| line.split_once(" @@"))
        .ok_or("hunk_unsupported")?;
    let (old, new) = ranges.split_once(' ').ok_or("hunk_unsupported")?;
    let (old_start, old_count) = range(old, '-')?;
    let (new_start, new_count) = range(new, '+')?;
    Ok(Hunk {
        id: String::new(),
        header: line.into(),
        old_start,
        old_count,
        new_start,
        new_count,
        lines: vec![],
    })
}
fn render(hunk: &Hunk) -> String {
    let mut text = format!("{}\n", hunk.header);
    for line in &hunk.lines {
        text.push(match line.kind {
            HunkLineKind::Context => ' ',
            HunkLineKind::Add => '+',
            HunkLineKind::Remove => '-',
            HunkLineKind::NoNewline => '\\',
        });
        text.push_str(&line.text);
        text.push('\n');
    }
    text
}
fn finish(mut hunk: Hunk) -> Result<Hunk, String> {
    let old = hunk
        .lines
        .iter()
        .filter(|line| matches!(line.kind, HunkLineKind::Context | HunkLineKind::Remove))
        .count();
    let new = hunk
        .lines
        .iter()
        .filter(|line| matches!(line.kind, HunkLineKind::Context | HunkLineKind::Add))
        .count();
    if old != hunk.old_count as usize || new != hunk.new_count as usize {
        return Err("hunk_unsupported".into());
    }
    hunk.id = patch_revision(&render(&hunk))[..16].into();
    Ok(hunk)
}
pub fn parse_file_patch(patch: &str) -> Result<FilePatch, String> {
    if !patch.starts_with("diff --git ") || !patch.ends_with('\n') {
        return Err("hunk_unsupported".into());
    }
    let mut file = FilePatch {
        header: String::new(),
        hunks: vec![],
    };
    let mut current: Option<Hunk> = None;
    for raw in patch.split_inclusive('\n') {
        // Preserve CR bytes belonging to file content; str::lines would erase them.
        let line = raw.strip_suffix('\n').ok_or("hunk_unsupported")?;
        if line.starts_with("@@ ") {
            if let Some(hunk) = current.take() {
                file.hunks.push(finish(hunk)?);
            }
            current = Some(hunk_header(line)?);
        } else if let Some(hunk) = current.as_mut() {
            let (kind, text) = match line.as_bytes().first() {
                Some(b' ') => (HunkLineKind::Context, &line[1..]),
                Some(b'+') => (HunkLineKind::Add, &line[1..]),
                Some(b'-') => (HunkLineKind::Remove, &line[1..]),
                Some(b'\\') if line == "\\ No newline at end of file" && !hunk.lines.is_empty() => {
                    (HunkLineKind::NoNewline, &line[1..])
                }
                _ => return Err("hunk_unsupported".into()),
            };
            hunk.lines.push(HunkLine {
                kind,
                text: text.into(),
            });
        } else {
            if !file.header.is_empty() && line.starts_with("diff --git ") {
                return Err("hunk_unsupported".into());
            }
            file.header.push_str(raw);
        }
    }
    if let Some(hunk) = current {
        file.hunks.push(finish(hunk)?);
    }
    if !file.header.split('\n').any(|line| line.starts_with("--- "))
        || !file.header.split('\n').any(|line| line.starts_with("+++ "))
    {
        return Err("hunk_unsupported".into());
    }
    Ok(file)
}
pub fn partial_patch(file: &FilePatch, ids: &[String]) -> Result<String, String> {
    let selected = ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    if ids.is_empty() || ids.len() > 200 || selected.len() != ids.len() {
        return Err("hunk_selection_invalid".into());
    }
    if ids
        .iter()
        .any(|id| !file.hunks.iter().any(|hunk| &hunk.id == id))
    {
        return Err("hunk_stale".into());
    }
    let mut patch = file.header.clone();
    for hunk in &file.hunks {
        if selected.contains(hunk.id.as_str()) {
            patch.push_str(&render(hunk));
        }
    }
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/app.rs b/src/app.rs\nindex 1111111..2222222 100644\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,3 +1,3 @@ fn main() {\n line one\n-line two\n+line 2\n line three\n@@ -10,2 +10,3 @@\n ten\n+ten and a half\n eleven\n\\ No newline at end of file\n";

    #[test]
    fn preserves_crlf_content_and_rejects_inconsistent_ranges_or_duplicate_selection() {
        let patch="diff --git a/a b/a\nindex a..b 100644\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\r\n+new\r\n";
        let parsed = parse_file_patch(patch).unwrap();
        assert_eq!(
            partial_patch(&parsed, &[parsed.hunks[0].id.clone()]).unwrap(),
            patch
        );
        assert!(parse_file_patch(&patch.replace("@@ -1 +1 @@", "@@ -1,2 +1 @@")).is_err());
        assert!(partial_patch(
            &parsed,
            &[parsed.hunks[0].id.clone(), parsed.hunks[0].id.clone()]
        )
        .is_err());
        assert!(partial_patch(&parsed, &vec!["missing".into(); 201]).is_err());
    }

    #[test]
    fn hunks_parse_with_ranges_and_stable_ids() {
        let file = parse_file_patch(PATCH).unwrap();
        assert!(file
            .header
            .starts_with("diff --git a/src/app.rs b/src/app.rs\n"));
        assert!(file.header.ends_with("+++ b/src/app.rs\n"));
        assert_eq!(file.hunks.len(), 2);
        assert_eq!(
            (
                file.hunks[0].old_start,
                file.hunks[0].old_count,
                file.hunks[0].new_start,
                file.hunks[0].new_count
            ),
            (1, 3, 1, 3)
        );
        assert_eq!(
            file.hunks[1].lines.last().unwrap().kind,
            HunkLineKind::NoNewline
        );
        assert_eq!(
            parse_file_patch(PATCH).unwrap().hunks[1].id,
            file.hunks[1].id
        );
        assert_ne!(file.hunks[0].id, file.hunks[1].id);
    }

    #[test]
    fn partial_patch_keeps_the_header_and_only_selected_hunks() {
        let file = parse_file_patch(PATCH).unwrap();
        let partial = partial_patch(&file, &[file.hunks[1].id.clone()]).unwrap();
        assert!(partial.starts_with(&file.header));
        assert!(partial.contains("+ten and a half\n"));
        assert!(!partial.contains("+line 2\n"));
        assert!(partial.ends_with("\\ No newline at end of file\n"));
        assert_eq!(
            partial_patch(&file, &["missing".into()]).unwrap_err(),
            "hunk_stale"
        );
        assert_eq!(
            partial_patch(&file, &[]).unwrap_err(),
            "hunk_selection_invalid"
        );
    }

    #[test]
    fn symlink_and_submodule_entries_use_file_level_actions() {
        for mode in ["120000", "160000"] {
            assert_eq!(
                unsupported_reason(&format!("diff --git a/link b/link\nindex a..b {mode}\n")),
                Some("mode_change")
            );
        }
    }

    #[test]
    fn special_files_are_reported_as_unsupported() {
        assert_eq!(
            unsupported_reason("diff --git a/n b/n\nnew file mode 100644\n"),
            Some("new_file")
        );
        assert_eq!(
            unsupported_reason("diff --git a/d b/d\ndeleted file mode 100644\n"),
            Some("deleted_file")
        );
        assert_eq!(
            unsupported_reason(
                "diff --git a/a b/b\nsimilarity index 90%\nrename from a\nrename to b\n"
            ),
            Some("rename")
        );
        assert_eq!(
            unsupported_reason("diff --git a/x b/x\nold mode 100644\nnew mode 100755\n"),
            Some("mode_change")
        );
        assert_eq!(
            unsupported_reason("diff --git a/i b/i\nBinary files a/i and b/i differ\n"),
            Some("binary")
        );
        assert_eq!(unsupported_reason(PATCH), None);
    }
}
