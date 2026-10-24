//! Markdown body → sanitized HTML. Ported from v0.9.0 `crates/markdown` with the
//! same pulldown-cmark options and ammonia settings. One change: remote `http(s)`
//! images are never fetched — Markdown images become a `remote-blocked` notice and
//! raw HTML `src="http…"` is dropped (01 §6.4, CSP `img-src 'self' data:`).
//! Image loading is injected, so this module stays pure.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};
use std::borrow::Cow;
use std::collections::HashSet;

/// Result of loading one image `src`. Each failure has its own notice so the
/// user can tell why an image is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageResult {
    /// A local image read and inlined as a `data:` URI.
    Inlined(String),
    NotFound,
    TooLarge,
    OutsideRoot,
    /// `http(s)`/network-path images are never requested.
    RemoteBlocked,
    /// The response's inline image budget is used up.
    Budget,
}

/// Renders a body (frontmatter already removed) to sanitized HTML.
///
/// Returns `(html, mermaid sources)`: each mermaid block becomes
/// `<div class="mermaid-block" data-idx="N"></div>` and its source is pushed in order.
/// `load_image` gets the image `src` as written; a failure only replaces that image.
pub fn render(body: &str, load_image: &dyn Fn(&str) -> ImageResult) -> (String, Vec<String>) {
    let mut mermaid_blocks: Vec<String> = Vec::new();

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let parser = Parser::new_ext(body, options);

    let events = rewrite_events(parser, &mut mermaid_blocks, load_image);

    let mut raw_html = String::new();
    html::push_html(&mut raw_html, events.into_iter());

    let clean_html = sanitize(&raw_html);
    (clean_html, mermaid_blocks)
}

/// Replaces mermaid code blocks and images in the event stream.
fn rewrite_events<'a>(
    parser: Parser<'a>,
    mermaid_blocks: &mut Vec<String>,
    load_image: &dyn Fn(&str) -> ImageResult,
) -> Vec<Event<'a>> {
    let mut out: Vec<Event<'a>> = Vec::new();
    let mut in_mermaid = false;
    let mut mermaid_buf = String::new();

    let mut iter = parser.into_iter();
    while let Some(event) = iter.next() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if lang.as_ref() == "mermaid" =>
            {
                in_mermaid = true;
                mermaid_buf.clear();
            }
            Event::Text(text) if in_mermaid => {
                mermaid_buf.push_str(&text);
            }
            Event::End(TagEnd::CodeBlock) if in_mermaid => {
                in_mermaid = false;
                let idx = mermaid_blocks.len();
                mermaid_blocks.push(std::mem::take(&mut mermaid_buf));
                out.push(Event::Html(
                    format!(r#"<div class="mermaid-block" data-idx="{idx}"></div>"#).into(),
                ));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                // Collect the alt text until End(Image): the notice uses it on failure,
                // success puts the events back.
                let mut alt_events: Vec<Event<'a>> = Vec::new();
                let mut alt_text = String::new();
                for child in iter.by_ref() {
                    if matches!(child, Event::End(TagEnd::Image)) {
                        break;
                    }
                    if let Event::Text(ref t) = child {
                        alt_text.push_str(t);
                    }
                    alt_events.push(child);
                }

                let lower = dest_url.trim().to_ascii_lowercase();
                let is_remote = lower.starts_with("http://")
                    || lower.starts_with("https://")
                    || lower.starts_with("//");
                let result = if is_remote {
                    ImageResult::RemoteBlocked
                } else {
                    load_image(&dest_url)
                };

                match result {
                    ImageResult::Inlined(data_uri) => {
                        out.push(Event::Start(Tag::Image {
                            link_type,
                            dest_url: data_uri.into(),
                            title,
                            id,
                        }));
                        out.extend(alt_events);
                        out.push(Event::End(TagEnd::Image));
                    }
                    failed => {
                        let (reason_attr, reason_label) = match failed {
                            ImageResult::NotFound => ("not-found", "파일 없음"),
                            ImageResult::TooLarge => ("too-large", "크기 초과(2MB)"),
                            ImageResult::OutsideRoot => ("outside-root", "루트 밖 경로"),
                            ImageResult::RemoteBlocked => ("remote-blocked", "외부 이미지 차단"),
                            _ => ("budget", "응답 크기 초과"),
                        };
                        let alt = if alt_text.is_empty() {
                            dest_url.to_string()
                        } else {
                            alt_text
                        };
                        out.push(Event::Html(
                            format!(
                                r#"<span class="md-image-missing" data-reason="{reason_attr}">이미지를 표시할 수 없습니다 ({reason_label}): {alt}</span>"#,
                                alt = html_escape(&alt),
                            )
                            .into(),
                        ));
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn is_safe_inline_raster(value: &str) -> bool {
    const PREFIXES: [&str; 6] = [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/gif;base64,",
        "data:image/webp;base64,",
        "data:image/bmp;base64,",
        "data:image/x-icon;base64,",
    ];
    let Some(payload) = PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
    else {
        return false;
    };
    let padding = payload
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    !payload.is_empty()
        && payload.len().is_multiple_of(4)
        && padding <= 2
        && payload[..payload.len() - padding]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

fn filter_url_attribute<'a>(
    element: &str,
    attribute: &str,
    value: &'a str,
) -> Option<Cow<'a, str>> {
    let is_url_attribute = matches!(attribute, "href" | "xlink:href" | "src")
        || (element == "form" && attribute == "action")
        || (element == "object" && attribute == "data")
        || (matches!(element, "button" | "input") && attribute == "formaction")
        || (element == "a" && attribute == "ping")
        || (element == "video" && attribute == "poster");
    if !is_url_attribute {
        return Some(Cow::Borrowed(value));
    }
    let normalized = value.trim_matches(|character: char| {
        character.is_ascii_whitespace() || character.is_ascii_control()
    });
    match ammonia::Url::parse(normalized) {
        Ok(url) if url.scheme() == "data" => {
            (element == "img" && attribute == "src" && is_safe_inline_raster(normalized))
                .then_some(Cow::Borrowed(value))
        }
        // Links keep http(s); a remote `src` would make the screen fetch it.
        Ok(url) if matches!(url.scheme(), "http" | "https") => {
            (attribute != "src").then_some(Cow::Borrowed(value))
        }
        Ok(_) => None,
        Err(ammonia::url::ParseError::RelativeUrlWithoutBase)
            if !normalized.starts_with("//") && !normalized.starts_with('\u{5c}') =>
        {
            Some(Cow::Borrowed(value))
        }
        Err(_) => None,
    }
}

/// ammonia sanitizing: http(s) links and relative URLs pass; `data:` survives only
/// as a verified raster `img[src]`; remote `src` is removed.
fn sanitize(raw_html: &str) -> String {
    let mut builder = ammonia::Builder::default();
    builder
        .add_generic_attributes(["class", "data-idx", "data-reason"])
        .add_tag_attributes("img", ["data-idx"])
        .url_schemes(HashSet::from(["http", "https", "data"]))
        .attribute_filter(filter_url_attribute)
        .url_relative(ammonia::UrlRelative::PassThrough);
    builder.clean(raw_html).to_string()
}

/// Inline image base64 allowed in one render response.
pub const IMAGE_BUDGET: usize = 8 * 1024 * 1024;
/// JSON-escaped html + mermaid sources allowed in one response (frame 16 MiB).
pub const RESPONSE_BUDGET: usize = 12 * 1024 * 1024;
// One wire-text budget for every document-carrying response.
const _: () = assert!(RESPONSE_BUDGET == devbox_documents::MAX_WIRE_TEXT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub images: usize,
    pub total: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            images: IMAGE_BUDGET,
            total: RESPONSE_BUDGET,
        }
    }
}

/// The rendered html alone exceeds the response budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverBudget;

/// Length of `s` inside a JSON string literal (quotes excluded).
pub fn escaped_len(s: &str) -> usize {
    s.chars()
        .map(|c| match c {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        })
        .sum()
}

/// [`render`] within `budget`: once the inline images reach `budget.images`, this and
/// every later image becomes a `budget` notice; when html + mermaid sources exceed
/// `budget.total`, mermaid sources are emptied from the last block backwards and
/// their placeholders get `data-reason="budget"`. [`OverBudget`] when the html alone is too big.
pub fn render_bounded(
    body: &str,
    load_image: &dyn Fn(&str) -> ImageResult,
    budget: Budget,
) -> Result<(String, Vec<String>), OverBudget> {
    let used = std::cell::Cell::new(0_usize);
    let full = std::cell::Cell::new(false);
    let bounded = |src: &str| match load_image(src) {
        ImageResult::Inlined(uri) => {
            let cost = uri
                .split_once(',')
                .map_or(uri.len(), |(_, data)| data.len());
            if full.get() || used.get() + cost > budget.images {
                full.set(true);
                ImageResult::Budget
            } else {
                used.set(used.get() + cost);
                ImageResult::Inlined(uri)
            }
        }
        other => other,
    };
    let (mut html, mut mermaid) = render(body, &bounded);
    let item = |s: &String| escaped_len(s) + 3;
    let mut size = escaped_len(&html) + mermaid.iter().map(item).sum::<usize>();
    for idx in (0..mermaid.len()).rev() {
        if size <= budget.total {
            break;
        }
        size -= escaped_len(&mermaid[idx]);
        mermaid[idx].clear();
        let from = format!(r#"<div class="mermaid-block" data-idx="{idx}"></div>"#);
        let to =
            format!(r#"<div class="mermaid-block" data-idx="{idx}" data-reason="budget"></div>"#);
        html = html.replacen(&from, &to, 1);
        size += to.len() - from.len();
    }
    if size > budget.total {
        return Err(OverBudget);
    }
    Ok((html, mermaid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_images(_src: &str) -> ImageResult {
        ImageResult::NotFound
    }

    #[test]
    fn mermaid_block_becomes_placeholder() {
        let body = "# T\n\n```mermaid\ngraph TD; A-->B;\n```\n";
        let (html, mermaid) = render(body, &no_images);
        assert!(html.contains(r#"<div class="mermaid-block" data-idx="0"></div>"#));
        assert_eq!(mermaid, vec!["graph TD; A-->B;\n".to_string()]);
    }

    #[test]
    fn multiple_mermaid_blocks_are_indexed_in_order() {
        let body = "```mermaid\nA\n```\n\ntext\n\n```mermaid\nB\n```\n\n```mermaid\nC\n```\n";
        let (html, mermaid) = render(body, &no_images);
        assert!(html.contains(r#"data-idx="0""#));
        assert!(html.contains(r#"data-idx="1""#));
        assert!(html.contains(r#"data-idx="2""#));
        assert_eq!(
            mermaid,
            vec!["A\n".to_string(), "B\n".to_string(), "C\n".to_string()]
        );
    }

    #[test]
    fn script_tags_are_sanitized() {
        let body = "Hello\n\n<script>alert('xss')</script>\n\nWorld";
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("<script>"));
        assert!(!html.contains("alert("));
    }

    #[test]
    fn relative_link_href_is_preserved() {
        let body = "[다른 문서](../Notes/other.md)";
        let (html, _) = render(body, &no_images);
        assert!(html.contains(r#"href="../Notes/other.md""#));
    }

    #[test]
    fn javascript_href_is_removed() {
        let body = "[클릭](javascript:alert(1))";
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("javascript:"));
    }

    #[test]
    fn failed_image_load_shows_fallback() {
        let body = "![대체텍스트](missing.png)";
        let (html, _) = render(body, &|_| ImageResult::NotFound);
        assert!(!html.contains("<img"));
        assert!(html.contains("md-image-missing"));
        assert!(html.contains("파일 없음"));
        assert!(html.contains("대체텍스트"));
    }

    #[test]
    fn successful_image_load_inlines_data_uri() {
        let body = "![로고](logo.png)";
        let (html, _) = render(body, &|_| {
            ImageResult::Inlined("data:image/png;base64,AAAA".to_string())
        });
        assert!(html.contains(r#"src="data:image/png;base64,AAAA""#));
    }

    #[test]
    fn unsafe_link_schemes_are_removed() {
        let body =
            "[ftp](ftp://example.com/file) [ssh](ssh://example.com) [data](data:text/html,boom)";
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("ftp://"));
        assert!(!html.contains("ssh://"));
        assert!(!html.contains("data:text/html"));
        assert_eq!(html.matches("href=").count(), 0);
    }

    #[test]
    fn network_path_and_obfuscated_data_links_are_removed() {
        let body = r#"<a href="//example.com/path">network</a>
<a href=" data:text/html,boom">data</a>"#;
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("//example.com"));
        assert!(!html.contains("data:text/html"));
        assert_eq!(html.matches("href=").count(), 0);
    }

    #[test]
    fn http_and_relative_links_are_preserved() {
        let body = "[secure](https://example.com) [relative](notes/today.md)";
        let (html, _) = render(body, &no_images);
        assert!(html.contains(r#"href="https://example.com""#));
        assert!(html.contains(r#"href="notes/today.md""#));
    }

    #[test]
    fn data_uri_is_limited_to_raster_image_sources() {
        let body = r#"<a href="data:image/png;base64,AAAA">link</a>
<img src="data:image/svg+xml;base64,PHN2Zz4=" alt="svg">
<img src="data:image/png;base64,AAAA" alt="png">"#;
        let (html, _) = render(body, &no_images);
        assert!(!html.contains(r#"href="data:"#));
        assert!(!html.contains("image/svg+xml"));
        assert!(html.contains(r#"src="data:image/png;base64,AAAA""#));
    }

    #[test]
    fn non_mermaid_code_block_is_untouched() {
        let body = "```rust\nfn main() {}\n```\n";
        let (html, mermaid) = render(body, &no_images);
        assert!(mermaid.is_empty());
        assert!(!html.contains("mermaid-block"));
        assert!(html.contains("<pre>"));
        assert!(html.contains("fn main"));
    }
}
