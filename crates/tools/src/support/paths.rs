//! Literal root representations with path boundaries; matches refer to the original text.
use super::VaultSnapshot;
use crate::R;
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: u8,
}
impl Span {
    pub fn replacement(&self) -> &'static str {
        match self.kind {
            2 => "<notes>",
            1 => "~",
            _ => "[REDACTED]",
        }
    }
}
pub(super) struct Paths {
    forms: Vec<(String, u8)>,
}
/// `/` admits `file:///<root>`: an absolute root can only follow `/` as a doubled separator.
fn before(text: &str, at: usize) -> bool {
    at == 0
        || text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || "\"'`=([{,:;<>/".contains(c))
}
/// Characters that end a path in logs, JSON, Debug output and prose.
fn terminator(c: char) -> bool {
    c.is_whitespace() || "\"'`)]},;:<>\\".contains(c)
}
/// The root ends at a separator, an escape (`\n`, `\"`) or a terminator. A `.` ends it only
/// as sentence punctuation, so a sibling such as `<root>.old` is never treated as the root.
fn after(text: &str, at: usize) -> bool {
    let mut rest = text[at..].chars();
    match rest.next() {
        None => true,
        Some('/') => true,
        Some('.') => rest.next().is_none_or(terminator),
        Some(c) => terminator(c),
    }
}
impl Paths {
    pub fn new(home: &Path, vault: &VaultSnapshot) -> R<Self> {
        let mut forms = Vec::new();
        for (path, kind) in [
            (vault.configured.as_path(), 2),
            (vault.canonical.as_path(), 2),
            (home, 1),
        ] {
            let raw = path
                .to_str()
                .ok_or_else(|| super::failed("non UTF8 path"))?;
            if raw.is_empty() {
                return Err(super::failed("empty path"));
            }
            let raw = if raw == "/" {
                raw
            } else {
                raw.trim_end_matches('/')
            };
            let json = serde_json::to_string(raw).map_err(super::failed)?;
            let debug = format!("{raw:?}");
            for form in [
                raw.to_owned(),
                json[1..json.len() - 1].to_owned(),
                debug[1..debug.len() - 1].to_owned(),
            ] {
                if !forms.contains(&(form.clone(), kind)) {
                    forms.push((form, kind));
                }
            }
        }
        Ok(Self { forms })
    }
    pub fn spans(&self, text: &str) -> Vec<Span> {
        let mut matches = Vec::new();
        for (needle, kind) in &self.forms {
            for (start, _) in text.match_indices(needle) {
                let end = start + needle.len();
                if before(text, start) && (needle == "/" || after(text, end)) {
                    matches.push(Span {
                        start,
                        end,
                        kind: *kind,
                    });
                }
            }
        }
        matches.sort_by_key(|s| (s.start, s.end));
        matches
    }
}
/// The final archive pass must not re-redact placeholders emitted before JSON serialization.
pub(super) fn masks(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    for marker in ["<notes>", "~", "[REDACTED]"] {
        out.extend(text.match_indices(marker).map(|(start, _)| Span {
            start,
            end: start + marker.len(),
            kind: 0,
        }));
    }
    out.sort_by_key(|s| s.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filesystem_root_and_trailing_separator_cover_descendants() {
        let root = VaultSnapshot {
            configured: "/".into(),
            canonical: "/".into(),
        };
        let paths = Paths::new(Path::new("/home/person"), &root).expect("paths");
        assert!(
            paths
                .spans("note=/private.md")
                .iter()
                .any(|s| s.start == 5 && s.end == 6 && s.kind == 2)
        );
        let alias = VaultSnapshot {
            configured: "/alias/".into(),
            canonical: "/actual".into(),
        };
        let paths = Paths::new(Path::new("/home/person"), &alias).expect("paths");
        assert!(
            paths
                .spans("note=/alias/private.md")
                .iter()
                .any(|s| s.start == 5 && s.end == 11 && s.kind == 2)
        );
        assert!(paths.spans("note=/alias-sibling/private.md").is_empty());
    }
    #[test]
    fn uri_escaped_and_sentence_forms_are_masked_but_siblings_are_not() {
        let vault = VaultSnapshot {
            configured: "/mnt/d/vault".into(),
            canonical: "/mnt/d/vault".into(),
        };
        let paths = Paths::new(Path::new("/home/person"), &vault).expect("paths");
        let kinds = |text: &str| -> Vec<(usize, usize, u8)> {
            paths
                .spans(text)
                .iter()
                .map(|s| (s.start, s.end, s.kind))
                .collect()
        };
        assert_eq!(kinds("file:///mnt/d/vault/a.md"), [(7, 19, 2)]);
        assert_eq!(kinds("file:///home/person/x"), [(7, 19, 1)]);
        assert_eq!(kinds(r#""/mnt/d/vault\n""#), [(1, 13, 2)]);
        assert_eq!(kinds(r#"{\"vault\":\"/mnt/d/vault\"}"#), [(13, 25, 2)]);
        assert_eq!(kinds("saved in /mnt/d/vault."), [(9, 21, 2)]);
        assert_eq!(kinds("in /mnt/d/vault. Next"), [(3, 15, 2)]);
        for sibling in [
            "/mnt/d/vault.old/a.md",
            "/mnt/d/vault-old/a.md",
            "/mnt/d/vaults",
            "/home/person2/x",
            "/home/person.bak",
        ] {
            assert!(kinds(sibling).is_empty(), "{sibling}");
        }
    }
}
