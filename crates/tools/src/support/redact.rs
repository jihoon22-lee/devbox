use crate::{R, ToolsError};
use aho_corasick::{AhoCorasick, MatchKind};
use std::{collections::BTreeMap, path::Path};
use zeroize::Zeroizing;
pub(super) type Values = BTreeMap<String, Zeroizing<Vec<u8>>>;
pub(super) struct Redactor {
    matcher: Option<AhoCorasick>,
    home: String,
    notes: String,
    // Keep resolved values zeroizing and scoped to this single bundle.
    _values: Values,
    _patterns: Vec<Zeroizing<Vec<u8>>>,
}
impl Redactor {
    pub(super) fn new(home: &Path, values: Values) -> R<Self> {
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
            home: home.to_string_lossy().into_owned(),
            notes: home.join("notes").to_string_lossy().into_owned(),
            _values: values,
            _patterns: patterns,
        })
    }
    pub(super) fn text(&self, text: &str) -> R<String> {
        let masked = if let Some(matcher) = &self.matcher {
            let mut bytes = Vec::new();
            let mut previous = 0;
            for found in matcher.find_iter(text.as_bytes()) {
                bytes.extend_from_slice(&text.as_bytes()[previous..found.start()]);
                bytes.extend_from_slice(b"[REDACTED]");
                previous = found.end();
            }
            bytes.extend_from_slice(&text.as_bytes()[previous..]);
            String::from_utf8(bytes).map_err(super::failed)?
        } else {
            text.to_owned()
        };
        Ok(masked
            .replace(&self.notes, "<notes>")
            .replace(&self.home, "~"))
    }
}
