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
fn before(text: &str, at: usize) -> bool {
    at == 0
        || text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_whitespace() || "\"'`=([{,:;<>".contains(c))
}
fn after(text: &str, at: usize) -> bool {
    at == text.len()
        || text[at..]
            .chars()
            .next()
            .is_some_and(|c| c.is_whitespace() || "/\"'`)]},;:<>".contains(c))
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
}
