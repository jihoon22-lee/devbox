use crate::{R, ToolsError};
use aho_corasick::{AhoCorasick, MatchKind};
use std::{collections::BTreeMap, path::Path};
use zeroize::Zeroizing;
pub(super) type Values = BTreeMap<String, Zeroizing<Vec<u8>>>;
pub(super) struct Redactor {
    matcher: Option<AhoCorasick>,
    paths: super::paths::Paths,
    // Keep resolved values zeroizing and scoped to this single bundle.
    _values: Values,
    _patterns: Vec<Zeroizing<Vec<u8>>>,
}
impl Redactor {
    pub(super) fn new(home: &Path, vault: &super::VaultSnapshot, values: Values) -> R<Self> {
        if values.len() > 2048
            || values.values().map(|value| value.len()).sum::<usize>() > 16 * 1024 * 1024
        {
            return Err(ToolsError::SupportTooLarge.into());
        }
        let mut patterns: Vec<Zeroizing<Vec<u8>>> = Vec::new();
        let mut pattern_bytes = 0usize;
        for value in values.values().filter(|value| !value.is_empty()) {
            let text = std::str::from_utf8(value).map_err(super::failed)?;
            let json = Zeroizing::new(serde_json::to_string(text).map_err(super::failed)?);
            let debug = Zeroizing::new(format!("{text:?}"));
            // Match interiors, retaining the log's surrounding JSON/debug quotes.
            for representation in [
                value.as_slice(),
                json[1..json.len() - 1].as_bytes(),
                debug[1..debug.len() - 1].as_bytes(),
            ] {
                pattern_bytes = pattern_bytes.saturating_add(representation.len());
                if pattern_bytes > 64 * 1024 * 1024 {
                    return Err(ToolsError::SupportTooLarge.into());
                }
                patterns.push(Zeroizing::new(representation.to_vec()));
            }
        }
        let matcher = if patterns.is_empty() {
            None
        } else {
            Some(
                AhoCorasick::builder()
                    .match_kind(MatchKind::LeftmostLongest)
                    .build(patterns.iter().map(|pattern| pattern.as_slice()))
                    .map_err(super::failed)?,
            )
        };
        Ok(Self {
            matcher,
            paths: super::paths::Paths::new(home, vault)?,
            _values: values,
            _patterns: patterns,
        })
    }
    pub(super) fn text(&self, text: &str) -> R<String> {
        self.apply(text, false)
    }
    pub(super) fn finish(&self, text: &str) -> R<String> {
        self.apply(text, true)
    }
    fn apply(&self, text: &str, preserve_masks: bool) -> R<String> {
        use super::paths::{Span, masks};
        let protected = if preserve_masks {
            masks(text)
        } else {
            Vec::new()
        };
        let visible = |span: &Span| {
            let at = protected.partition_point(|mask| mask.end <= span.start);
            protected.get(at).is_none_or(|mask| mask.start >= span.end)
        };
        let mut paths = self
            .paths
            .spans(text)
            .into_iter()
            .filter(visible)
            .peekable();
        let secret_matches: Box<dyn Iterator<Item = Span> + '_> = match &self.matcher {
            Some(matcher) => Box::new(matcher.find_iter(text.as_bytes()).map(|m| Span {
                start: m.start(),
                end: m.end(),
                kind: 0,
            })),
            None => Box::new(std::iter::empty()),
        };
        let mut secrets = secret_matches.filter(visible).peekable();
        let mut out = String::new();
        let mut cursor = 0;
        let mut active: Option<Span> = None;
        loop {
            let next = match (paths.peek(), secrets.peek()) {
                (Some(path), Some(secret)) if path.start <= secret.start => paths.next(),
                (Some(_), Some(_)) | (None, Some(_)) => secrets.next(),
                (Some(_), None) => paths.next(),
                (None, None) => None,
            };
            let Some(next) = next else { break };
            if let Some(current) = &mut active
                && next.start < current.end
            {
                // A secret that overlaps a path cannot destroy its full-root match.
                current.end = current.end.max(next.end);
                current.kind = current.kind.max(next.kind);
                continue;
            }
            if let Some(current) = active.take() {
                out.push_str(&text[cursor..current.start]);
                out.push_str(current.replacement());
                cursor = current.end;
            }
            active = Some(next);
        }
        if let Some(current) = active {
            out.push_str(&text[cursor..current.start]);
            out.push_str(current.replacement());
            cursor = current.end;
        }
        out.push_str(&text[cursor..]);
        Ok(out)
    }
}
