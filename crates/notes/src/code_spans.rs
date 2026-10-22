//! Byte ranges of a note that are not prose for links: code blocks (fenced and
//! indented), inline code and HTML comments, as pulldown-cmark reads them (the
//! renderer's options). Wiki links starting inside them are neither indexed
//! (backlinks) nor rewritten by a rename: both use [`crate::links::parse_wikilinks`].
//! Comments are excluded because the rendered note does not show them.
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

pub fn excluded(content: &str) -> Vec<Range<usize>> {
    let (_, body) = crate::frontmatter::parse(content);
    let base = content.len() - body.len();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut out = Vec::new();
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        let hide = match event {
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => true,
            Event::Start(Tag::HtmlBlock) | Event::Html(_) | Event::InlineHtml(_) => {
                body[range.clone()].trim_start().starts_with("<!--")
            }
            _ => false,
        };
        if hide {
            out.push(base + range.start..base + range.end);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::links::parse_wikilinks;

    #[test]
    fn indented_code_and_comments_hide_links_but_lazy_continuations_do_not() {
        let text = "[[a]]\n\n    [[b]]\n\n<!-- [[c]] -->\n\nx <!-- [[d]] --> `[[e]]`\n\np\n    [[f]]\n\n<!--\n[[g]]\n-->\n";
        let keys: Vec<String> = parse_wikilinks(text)
            .into_iter()
            .filter_map(|l| l.target_key)
            .collect();
        assert_eq!(keys, ["a", "f"]);
    }
}
