//! Daily notes and the per-PC "오늘 한 일" block (pure text rules). Two PCs share
//! one vault through Git, so each PC only ever rewrites the lines between its own
//! `<!-- devbox:summary:<pc> -->` and the next `<!-- /devbox:summary -->`.
use crate::model::TextEdit;

pub const SUMMARY_END: &str = "<!-- /devbox:summary -->";
const START_PREFIX: &str = "<!-- devbox:summary:";
/// Daily note body when `<templates>/daily.md` does not exist. The empty title
/// makes the index fall back to the file name (AR-F10).
pub(crate) const DEFAULT_DAILY: &str = "---\ntitle: \"\"\n---\n# {{date}}\n";
pub(crate) const MAX_PC_BYTES: usize = 128;

pub fn summary_start(pc: &str) -> String {
    format!("{START_PREFIX}{pc} -->")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryError {
    /// Empty, too long, multi-line or comment-like PC name.
    InvalidPc,
    /// The block text itself contains comment markers.
    InvalidContent,
    /// This PC's start marker has no end marker before the next block or the end.
    Broken,
}

fn valid_pc(pc: &str) -> bool {
    !pc.trim().is_empty()
        && pc.trim() == pc
        && pc.len() <= MAX_PC_BYTES
        && !pc.chars().any(char::is_control)
        && !pc.contains("<!--")
        && !pc.contains("-->")
}
fn is_start(line: &str) -> bool {
    let line = line.trim_end();
    line.starts_with(START_PREFIX) && line.ends_with("-->")
}

/// Replaces (or adds) this PC's summary block. `doc` and `content` are LF text.
/// - Only the lines between this PC's start marker (a whole line) and the next
///   end marker change. A start marker whose block runs into another block or the
///   end of the note is [`SummaryError::Broken`]: nothing is guessed or removed.
/// - Without a block, it goes right below this PC's `## <pc>` heading, or at the
///   end under a new heading.
pub fn replace_summary(doc: &str, pc: &str, content: &str) -> Result<String, SummaryError> {
    if !valid_pc(pc) {
        return Err(SummaryError::InvalidPc);
    }
    if content.contains("<!--") || content.contains("-->") {
        return Err(SummaryError::InvalidContent);
    }
    let mut content = content.to_owned();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    let start = summary_start(pc);
    let heading = format!("## {pc}");
    let mut lines = Vec::new();
    let mut at = 0;
    for line in doc.split_inclusive('\n') {
        lines.push((at, line));
        at += line.len();
    }
    if let Some(i) = lines.iter().position(|(_, l)| l.trim_end() == start) {
        let after = &lines[i + 1..];
        let end = after
            .iter()
            .find(|(_, l)| l.trim_end() == SUMMARY_END || is_start(l))
            .filter(|(_, l)| l.trim_end() == SUMMARY_END)
            .ok_or(SummaryError::Broken)?;
        let body_from = lines[i].0 + lines[i].1.len();
        return Ok(format!("{}{content}{}", &doc[..body_from], &doc[end.0..]));
    }
    let block = format!("{start}\n{content}{SUMMARY_END}\n");
    if let Some((h_at, h)) = lines.iter().find(|(_, l)| l.trim_end() == heading) {
        let below = h_at + h.len();
        let newline = if h.ends_with('\n') { "" } else { "\n" };
        return Ok(format!(
            "{}{newline}{block}{}",
            &doc[..below],
            &doc[below..]
        ));
    }
    let gap = match doc {
        "" => "",
        d if d.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    Ok(format!("{doc}{gap}{heading}\n{block}"))
}

fn lines_of(text: &str) -> Vec<&str> {
    let text = text.trim_end_matches(['\r', '\n']);
    text.split('\n').map(|l| l.trim_end_matches('\r')).collect()
}
/// `- hh:mm <first line>`, later lines indented by two spaces.
pub(crate) fn capture_entry(time: &str, text: &str) -> String {
    let mut out = String::new();
    for (i, line) in lines_of(text).into_iter().enumerate() {
        match (i, line.is_empty()) {
            (0, _) => out.push_str(&format!("- {time} {line}")),
            (_, true) => {}
            (_, false) => out.push_str(&format!("  {line}")),
        }
        out.push('\n');
    }
    out
}
/// The text on its own line(s).
pub(crate) fn plain_entry(text: &str) -> String {
    let mut out = lines_of(text).join("\n");
    out.push('\n');
    out
}
/// `existing` followed by `entry` on a new line.
pub(crate) fn appended(existing: &str, entry: &str) -> String {
    match existing {
        "" => entry.to_owned(),
        e if e.ends_with('\n') => format!("{e}{entry}"),
        e => format!("{e}\n{entry}"),
    }
}
pub(crate) fn daily_path(folder: &str, date: &str) -> String {
    format!("{folder}/{date}.md")
}

/// One edit turning `old` into `new` (common prefix and suffix kept), in UTF-16.
pub(crate) fn single_edit(old: &str, new: &str) -> TextEdit {
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((i, _), _)| i);
    let prefix = (0..=prefix)
        .rev()
        .find(|&i| old.is_char_boundary(i) && new.is_char_boundary(i))
        .unwrap_or(0);
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum::<usize>();
    let utf16 = |s: &str| s.encode_utf16().count() as u32;
    TextEdit {
        from_utf16: utf16(&old[..prefix]),
        to_utf16: utf16(&old[..old.len() - suffix]),
        text: new_rest[..new_rest.len() - suffix].to_owned(),
    }
}
