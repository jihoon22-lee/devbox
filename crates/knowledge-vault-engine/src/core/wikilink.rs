use std::collections::BTreeSet;

pub const MAX_WIKILINKS_PER_DOC: usize = 2_000;
pub const MAX_WIKILINK_TARGET_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedWikilink {
    pub target: String,
    pub label: String,
    pub target_key: Option<String>,
    pub line: usize,
    /// 1-based UTF-16 column, matching CodeMirror's document offsets.
    pub column: usize,
    pub from_utf16: usize,
    pub to_utf16: usize,
    pub from_byte: usize,
    pub to_byte: usize,
}

/// Markdown code spans/fences and escaped openers are deliberately ignored.
/// A link never crosses a line boundary and `target|label` aliases are supported.
pub fn parse_wikilinks(content: &str) -> Vec<ParsedWikilink> {
    let mut links = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    let mut byte_offset = 0;
    let mut utf16_offset = 0;
    let mut in_frontmatter = content
        .split(['\r', '\n'])
        .next()
        .is_some_and(|line| line == "---");

    for (line_index, segment) in content.split_inclusive('\n').enumerate() {
        let line = segment.strip_suffix('\n').unwrap_or(segment);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if in_frontmatter {
            if line_index > 0 && line == "---" {
                in_frontmatter = false;
            }
        } else if let Some(marker) = fence_marker(line) {
            match fence {
                Some((character, minimum))
                    if marker.0 == character
                        && marker.1 >= minimum
                        && is_closing_fence(line, character, minimum) =>
                {
                    fence = None;
                }
                None => fence = Some(marker),
                _ => {}
            }
        } else if fence.is_none() {
            parse_line(line, line_index + 1, byte_offset, utf16_offset, &mut links);
        }

        if links.len() >= MAX_WIKILINKS_PER_DOC {
            break;
        }
        byte_offset += segment.len();
        utf16_offset += line.encode_utf16().count() + usize::from(segment.ends_with('\n'));
    }

    // `split_inclusive` produces no segment for an empty string.
    if content.is_empty() {
        return Vec::new();
    }
    links
}

fn parse_line(
    line: &str,
    line_number: usize,
    line_byte_offset: usize,
    line_utf16_offset: usize,
    links: &mut Vec<ParsedWikilink>,
) {
    let bytes = line.as_bytes();
    let mut cursor = 0;
    let mut cursor_utf16 = 0;
    let mut inline_code_ticks: Option<usize> = None;

    while cursor < bytes.len() && links.len() < MAX_WIKILINKS_PER_DOC {
        if bytes[cursor] == b'`' {
            let run = ascii_run(bytes, cursor, b'`');
            match inline_code_ticks {
                Some(opening) if opening == run => inline_code_ticks = None,
                None => inline_code_ticks = Some(run),
                _ => {}
            }
            cursor += run;
            cursor_utf16 += run;
            continue;
        }

        if inline_code_ticks.is_none()
            && bytes[cursor..].starts_with(b"[[")
            && !is_escaped(bytes, cursor)
        {
            let search_from = cursor + 2;
            if let Some(relative_end) = line[search_from..].find("]]") {
                let end = search_from + relative_end;
                let to = end + 2;
                let raw = &line[search_from..end];
                let (target, label) = raw
                    .split_once('|')
                    .map(|(target, label)| (target.trim(), label.trim()))
                    .unwrap_or_else(|| {
                        let target = raw.trim();
                        (target, target)
                    });
                let display_target = bounded_text(target, MAX_WIKILINK_TARGET_BYTES);
                let display_label = if label.is_empty() {
                    display_target.clone()
                } else {
                    bounded_text(label, MAX_WIKILINK_TARGET_BYTES)
                };
                let link_utf16 = line[cursor..to].encode_utf16().count();
                links.push(ParsedWikilink {
                    target: display_target,
                    label: display_label,
                    target_key: normalize_link_key(target),
                    line: line_number,
                    column: cursor_utf16 + 1,
                    from_utf16: line_utf16_offset + cursor_utf16,
                    to_utf16: line_utf16_offset + cursor_utf16 + link_utf16,
                    from_byte: line_byte_offset + cursor,
                    to_byte: line_byte_offset + to,
                });
                cursor = to;
                cursor_utf16 += link_utf16;
                continue;
            }
        }

        let character = line[cursor..].chars().next();
        cursor += character.map(char::len_utf8).unwrap_or(1);
        cursor_utf16 += character.map(char::len_utf16).unwrap_or(1);
    }
}

fn fence_marker(line: &str) -> Option<(u8, usize)> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 {
        return None;
    }
    let bytes = line.as_bytes();
    let marker = *bytes.get(indent)?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let count = ascii_run(bytes, indent, marker);
    (count >= 3).then_some((marker, count))
}

fn is_closing_fence(line: &str, marker: u8, minimum: usize) -> bool {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent > 3 || line.as_bytes().get(indent) != Some(&marker) {
        return false;
    }
    let count = ascii_run(line.as_bytes(), indent, marker);
    count >= minimum
        && line.as_bytes()[indent + count..]
            .iter()
            .all(|byte| matches!(byte, b' ' | b'\t'))
}

fn ascii_run(bytes: &[u8], start: usize, expected: u8) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == expected)
        .count()
}

fn is_escaped(bytes: &[u8], start: usize) -> bool {
    let mut slashes = 0;
    let mut cursor = start;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        slashes += 1;
        cursor -= 1;
    }
    slashes % 2 == 1
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const ELLIPSIS: &str = "…";
    let mut end = max_bytes.saturating_sub(ELLIPSIS.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &value[..end], ELLIPSIS)
}

/// A target is a root-relative note identifier, never a filesystem path.
/// Resolution only happens against indexed note keys.
pub fn normalize_link_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_WIKILINK_TARGET_BYTES
        || value.contains(['\\', '\0', '\r', '\n', ':', '#', '[', ']', '|'])
        || value.starts_with('/')
    {
        return None;
    }
    let without_extension = strip_markdown_extension(value);
    if without_extension.is_empty()
        || without_extension
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
        || without_extension.chars().any(char::is_control)
    {
        return None;
    }
    Some(without_extension.to_lowercase())
}

pub fn note_link_target(path: &str) -> String {
    strip_markdown_extension(path).to_string()
}

pub fn note_link_keys(path: &str, title: &str) -> Vec<String> {
    let mut keys = BTreeSet::new();
    if let Some(key) = normalize_link_key(path) {
        keys.insert(key);
    }
    if let Some(file_name) = path.rsplit('/').next() {
        if let Some(key) = normalize_link_key(file_name) {
            keys.insert(key);
        }
    }
    if let Some(key) = normalize_link_key(title) {
        keys.insert(key);
    }
    keys.into_iter().collect()
}

fn strip_markdown_extension(value: &str) -> &str {
    let suffix_start = value.len().saturating_sub(3);
    let Some(suffix) = value.get(suffix_start..) else {
        return value;
    };
    if suffix.eq_ignore_ascii_case(".md") {
        value.get(..suffix_start).unwrap_or(value)
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aliases_multiple_links_and_utf16_positions() {
        let content = "😀 [[Notes/Rust|Rust note]] and [[Daily]]";
        let links = parse_wikilinks(content);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Notes/Rust");
        assert_eq!(links[0].label, "Rust note");
        assert_eq!(links[0].line, 1);
        assert_eq!(links[0].column, 4);
        assert_eq!(links[0].from_utf16, 3);
        assert_eq!(
            &content[links[0].from_byte..links[0].to_byte],
            "[[Notes/Rust|Rust note]]"
        );
        assert_eq!(links[1].target_key.as_deref(), Some("daily"));
    }

    #[test]
    fn ignores_fenced_inline_and_escaped_syntax() {
        let content = "---\ntitle: '[[frontmatter]]'\n---\n`[[inline]]` \\[[escaped]] [[real]]\n```md\n```still code\n[[fenced]]\n```\n~~~\n[[tilde]]\n~~~";
        let links = parse_wikilinks(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "real");
        assert_eq!(links[0].line, 4);
    }

    #[test]
    fn invalid_targets_are_detected_without_becoming_paths() {
        let oversized = "x".repeat(MAX_WIKILINK_TARGET_BYTES + 1);
        let content =
            format!("[[../secret]] [[C:\\secret]] [[Note#heading]] [[safe.md]] [[{oversized}]]");
        let links = parse_wikilinks(&content);
        assert_eq!(links.len(), 5);
        assert!(links[..3].iter().all(|link| link.target_key.is_none()));
        assert_eq!(links[3].target_key.as_deref(), Some("safe"));
        assert!(links[4].target_key.is_none());
        assert!(links[4].target.len() <= MAX_WIKILINK_TARGET_BYTES);
    }

    #[test]
    fn note_keys_include_path_stem_filename_and_title_without_duplicates() {
        assert_eq!(
            note_link_keys("Notes/Rust.md", "Rust"),
            vec!["notes/rust".to_string(), "rust".to_string()]
        );
        assert_eq!(note_link_target("Notes/Rust.MD"), "Notes/Rust");
    }

    #[test]
    fn utf16_offsets_follow_codemirror_newline_normalization_but_bytes_keep_crlf() {
        let content = "first\r\n[[Note]]";
        let link = &parse_wikilinks(content)[0];
        assert_eq!(link.line, 2);
        assert_eq!(link.column, 1);
        assert_eq!(link.from_utf16, 6);
        assert_eq!(&content[link.from_byte..link.to_byte], "[[Note]]");
    }
}
