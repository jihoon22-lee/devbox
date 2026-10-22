//! Link edits a rename needs: every link that resolved to one note before the move
//! must resolve to the same (moved) note after it. A link that would stop doing so
//! gets the smallest replacement that does: the new file name, else the vault path
//! (`[[z/a2]]`), and for Markdown links the path relative to the source note.
//! Only the target part changes; alias, `#heading` and spacing stay.
//!
//! Resolution uses the same `pick` rule as `notes.resolve`, over index candidates
//! with the moved note taken out and put back with its old or new keys. So the
//! result does not depend on whether the index already saw the move, and running
//! it again on an already rewritten note finds nothing to do (resume is idempotent).
use crate::frontmatter;
use crate::index::title_of;
use crate::links::{
    markdown_note_target, normalize_link_key, note_link_keys, note_link_target,
    parse_markdown_links, parse_wikilinks, path_key, pick,
};
use crate::vault::file_stem;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;

/// The moved note and the link keys it answers to before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub from: String,
    pub to: String,
    pub old_keys: Vec<String>,
    pub new_keys: Vec<String>,
}
impl Move {
    /// `text` is the moved note (its frontmatter title, or the file name when blank).
    pub fn new(from: &str, to: &str, text: &str) -> Self {
        let (meta, _) = frontmatter::parse(text);
        Self {
            from: from.into(),
            to: to.into(),
            old_keys: note_link_keys(from, &title_of(&meta, from)),
            new_keys: note_link_keys(to, &title_of(&meta, to)),
        }
    }
    fn remap(&self, path: &str) -> String {
        if path == self.from {
            self.to.clone()
        } else {
            path.into()
        }
    }
    /// The logical (pre-move) path of a note found at `path`.
    pub fn before(&self, path: &str) -> String {
        if path == self.to {
            self.from.clone()
        } else {
            path.into()
        }
    }
}

/// Index candidates per link key, as `NoteIndex::candidates` returns them.
pub type Candidates = HashMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Planned {
    pub edits: Vec<Edit>,
    /// A link needs a rewrite this module cannot do safely (reference-style link).
    pub unsupported: bool,
}
impl Planned {
    pub fn clean(&self) -> bool {
        self.edits.is_empty() && !self.unsupported
    }
}

#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}
struct World<'a> {
    mv: &'a Move,
    cands: &'a Candidates,
    missing: Vec<String>,
}
impl World<'_> {
    /// The single note `key` resolves to from `src`, or `None` (none or ambiguous).
    fn resolve(&mut self, key: &str, src: &str, exact: bool, side: Side) -> Option<String> {
        let Some(known) = self.cands.get(key) else {
            self.missing.push(key.into());
            return None;
        };
        let mv = self.mv;
        let mut c: Vec<String> = known
            .iter()
            .filter(|p| **p != mv.from && **p != mv.to)
            .cloned()
            .collect();
        let (path, keys) = match side {
            Side::Old => (&mv.from, &mv.old_keys),
            Side::New => (&mv.to, &mv.new_keys),
        };
        if keys.iter().any(|k| k == key) {
            c.push(path.clone());
        }
        if exact {
            c.retain(|p| path_key(p).as_deref() == Some(key));
        }
        let r = pick(key, Some(src), c);
        match (r.ambiguous, r.paths.as_slice()) {
            (false, [only]) => Some(only.clone()),
            _ => None,
        }
    }
}

/// Edits for the note logically at `src` (its pre-move path; the moved note is
/// `mv.from`). `Err(keys)`: look these keys up and call again.
pub fn plan(text: &str, mv: &Move, src: &str, cands: &Candidates) -> Result<Planned, Vec<String>> {
    let mut w = World {
        mv,
        cands,
        missing: Vec::new(),
    };
    let after = mv.remap(src);
    let mut out = Planned::default();
    for link in parse_wikilinks(text) {
        let Some(key) = link.target_key.as_deref() else {
            continue;
        };
        let Some(old) = w.resolve(key, src, false, Side::Old) else {
            continue;
        };
        let target = mv.remap(&old);
        if w.resolve(key, &after, false, Side::New).as_deref() == Some(target.as_str()) {
            continue;
        }
        let range = wiki_target(text, link.from_byte, link.to_byte);
        let written = &text[range.clone()];
        let mut forms = vec![note_link_target(&target)];
        if !written.contains('/') {
            forms.insert(0, file_stem(&target));
        }
        let form = forms.into_iter().find(|form| {
            normalize_link_key(form).is_some_and(|k| {
                w.resolve(&k, &after, false, Side::New).as_deref() == Some(target.as_str())
            })
        });
        match form {
            Some(form) => out.edits.push(Edit { range, text: form }),
            None => out.unsupported = true,
        }
    }
    for link in parse_markdown_links(text, Some(src)) {
        let Some(key) = link.target_key.as_deref() else {
            continue;
        };
        let Some(old) = w.resolve(key, src, true, Side::Old) else {
            continue;
        };
        let target = mv.remap(&old);
        let located = md_destination(&text[link.span.clone()]).filter(|(r, _)| {
            let dest = unescape(&text[link.span.start + r.start..link.span.start + r.end]);
            markdown_note_target(Some(src), &dest)
                .and_then(|(p, _)| path_key(&p))
                .as_deref()
                == Some(key)
        });
        let Some((range, angle)) = located else {
            // Unlocatable (reference-style): only a problem when the link would break.
            if old == mv.from || after != src {
                out.unsupported = true;
            }
            continue;
        };
        let range = link.span.start + range.start..link.span.start + range.end;
        let dest = unescape(&text[range.clone()]);
        let still = markdown_note_target(Some(&after), &dest)
            .and_then(|(p, _)| path_key(&p))
            .and_then(|k| w.resolve(&k, &after, true, Side::New));
        if still.as_deref() == Some(target.as_str()) {
            continue;
        }
        let fragment = text[range.clone()]
            .find('#')
            .map_or("", |i| &text[range.start + i..range.end]);
        let rel = relative(folder(&after), &target);
        let written = if angle { rel } else { encode(&rel) } + fragment;
        let ok = markdown_note_target(Some(&after), &unescape(&written))
            .is_some_and(|(p, _)| p == target);
        if ok {
            out.edits.push(Edit {
                range,
                text: written,
            });
        } else {
            out.unsupported = true;
        }
    }
    if w.missing.is_empty() {
        out.edits.sort_by_key(|e| e.range.start);
        Ok(out)
    } else {
        w.missing.sort();
        w.missing.dedup();
        Err(w.missing)
    }
}

/// `text` with `edits` (non-overlapping) applied.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut out = text.to_owned();
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
    for edit in sorted {
        out.replace_range(edit.range.clone(), &edit.text);
    }
    out
}
/// CodeMirror offset of `byte` (UTF-16 units, CRLF counted as one).
pub fn utf16_at(text: &str, byte: usize) -> usize {
    let prefix = &text[..byte];
    prefix.encode_utf16().count() - prefix.matches("\r\n").count()
}

/// The trimmed note part of `[[ note#heading | alias ]]` at `from..to`.
fn wiki_target(text: &str, from: usize, to: usize) -> Range<usize> {
    let inner = &text[from + 2..to - 2];
    let end = inner.find('|').unwrap_or(inner.len());
    let end = inner[..end].find('#').unwrap_or(end);
    let part = &inner[..end];
    let lead = part.len() - part.trim_start().len();
    let start = from + 2 + lead;
    start..start + part.trim().len()
}

fn escaped(bytes: &[u8], at: usize) -> bool {
    bytes[..at]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count()
        % 2
        == 1
}
/// Destination range inside an inline link `[text](dest "title")` and whether it
/// is in `<…>`. `None` for reference-style links or anything unexpected.
fn md_destination(link: &str) -> Option<(Range<usize>, bool)> {
    let bytes = link.as_bytes();
    if bytes.last() != Some(&b')') {
        return None;
    }
    let mut depth = 0usize;
    let mut open = None;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' if !escaped(bytes, i) => depth += 1,
            b'(' if !escaped(bytes, i) => {
                depth -= 1;
                if depth == 0 {
                    open = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let open = open?;
    if open == 0 || bytes[open - 1] != b']' {
        return None;
    }
    let inner = &link[open + 1..link.len() - 1];
    let lead = inner.len() - inner.trim_start().len();
    let start = open + 1 + lead;
    let rest = &inner[lead..];
    if let Some(angled) = rest.strip_prefix('<') {
        let end = angled.find('>')?;
        return Some((start + 1..start + 1 + end, true));
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    (end > 0).then_some((start..start + end, false))
}
fn unescape(dest: &str) -> String {
    let mut out = String::with_capacity(dest.len());
    let mut chars = dest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(|n| n.is_ascii_punctuation()) {
            continue;
        }
        out.push(c);
    }
    out
}
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '%' => out.push_str("%25"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            c => out.push(c),
        }
    }
    out
}
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}
/// `target` relative to the folder `dir` (`""` = vault root).
fn relative(dir: &str, target: &str) -> String {
    let from: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let to: Vec<&str> = target.split('/').collect();
    let dirs = &to[..to.len() - 1];
    let common = from.iter().zip(dirs).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations_relative_paths_and_offsets() {
        assert_eq!(md_destination("[t](a.md)"), Some((4..8, false)));
        assert_eq!(md_destination("[t](<a (b).md> \"x\")"), Some((5..13, true)));
        assert_eq!(md_destination("[t](a.md \"t (x)\")"), Some((4..8, false)));
        assert_eq!(md_destination("[t][ref]"), None);
        assert_eq!(relative("d", "new dir/a.md"), "../new dir/a.md");
        assert_eq!(relative("z", "z/q/b.md"), "q/b.md");
        assert_eq!(relative("", "z/a2.md"), "z/a2.md");
        assert_eq!(utf16_at("😀\r\n[[a]]", 7), 4);
        let text = "[[ a#h | x ]]";
        assert_eq!(&text[wiki_target(text, 0, text.len())], "a");
    }
}
