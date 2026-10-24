//! Single-line excerpts around matches with UTF-16 highlight ranges.
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use std::ops::Range;

/// Highlight ranges per excerpt at most.
pub const MAX_RANGES: usize = 32;

/// Non-empty byte ranges of `matcher` in `text`, at most `max`.
pub(crate) fn find_all(matcher: &RegexMatcher, text: &str, max: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let _ = matcher.find_iter(text.as_bytes(), |m| {
        if m.start() < m.end() {
            out.push(m.start()..m.end());
        }
        out.len() < max
    });
    out
}

/// An excerpt of at most `max_chars` characters starting at the first match's
/// line, at most `lead` characters before the match (the text start when there
/// is none). Line breaks and tabs become spaces (one UTF-16 unit each, so the
/// ranges stay valid); ranges are clipped to the excerpt.
pub(crate) fn excerpt(
    text: &str,
    matches: &[Range<usize>],
    max_chars: usize,
    lead: usize,
) -> (String, Vec<(u32, u32)>) {
    let start = match matches.first() {
        None => 0,
        Some(m) => {
            let before = &text[..m.start];
            let line = before.rfind('\n').map_or(0, |i| i + 1);
            let back = before
                .char_indices()
                .rev()
                .take(lead)
                .last()
                .map_or(m.start, |(i, _)| i);
            line.max(back)
        }
    };
    let rest = &text[start..];
    let end = start
        + rest
            .char_indices()
            .nth(max_chars)
            .map_or(rest.len(), |(i, _)| i);
    let window = &text[start..end];
    let snippet = window
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let units = |byte: usize| window[..byte - start].encode_utf16().count() as u32;
    let ranges = matches
        .iter()
        .filter(|m| m.start >= start && m.start < end)
        .take(MAX_RANGES)
        .map(|m| (units(m.start), units(m.end.min(end))))
        .collect();
    (snippet, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_start_near_the_match_and_clip_ranges() {
        let text = "first line\nsecond 가나 needle and more";
        let at = text.find("needle").expect("fixture");
        let found = at..at + 6;
        let (s, r) = excerpt(text, std::slice::from_ref(&found), 11, 3);
        assert_eq!(s, "가나 needle a");
        assert_eq!(r, [(3, 9)]);
        let (s, r) = excerpt(text, &[], 5, 4);
        assert_eq!((s.as_str(), r.len()), ("first", 0));
        let (s, _) = excerpt("a\tb\r\nc", &[], 10, 0);
        assert_eq!(s, "a b  c");
    }
}
