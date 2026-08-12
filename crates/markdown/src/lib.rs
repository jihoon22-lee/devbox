//! 마크다운 본문을 살균된 HTML로 렌더링한다.
//!
//! OS 의존 없음(CONVENTIONS §4) — 이미지 로딩은 클로저로 주입받아 순수 함수로 유지한다.
//! 실제 파일시스템 접근 로더는 각 소비자의 command 레이어가 만든다.

use pulldown_cmark::{html, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// 이미지 로더가 하나의 `src`를 처리한 결과.
///
/// variant를 실패 사유별로 나누는 이유: 이미지가 안 보일 때 사용자가 이유를 알아야
/// 고칠 수 있다. 대체 표시 문구가 사유마다 달라야 한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageResult {
    /// 로컬 이미지를 성공적으로 읽어 `data:` URI로 인라인했다.
    Inlined(String),
    /// http/https 등 외부 URL — 원본 `src`를 그대로 둔다.
    Passthrough,
    /// 경로에 파일이 없다.
    NotFound,
    /// 소비자가 정한 크기 상한을 초과한다.
    TooLarge,
    /// 루트 밖을 벗어나는 경로다.
    OutsideRoot,
}

/// 마크다운 본문(frontmatter가 이미 제거된 상태)을 살균된 HTML로 렌더링한다.
///
/// 반환값: `(html, mermaid 원문 목록)`. mermaid 블록은 HTML 안에서
/// `<div class="mermaid-block" data-idx="N"></div>` placeholder로 치환되고,
/// 원문은 인덱스 순서대로 두 번째 값에 쌓인다.
///
/// `load_image`는 이미지 `src`(마크다운에 쓰인 원문 그대로)를 받아 [`ImageResult`]를
/// 반환하는 순수 콜백이다. 실패해도 해당 이미지 자리만 대체 표시로 바뀔 뿐 나머지
/// 문서 렌더링에는 영향을 주지 않는다.
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

/// pulldown-cmark 이벤트 스트림을 순회하며 mermaid 코드블록과 이미지를 치환한다.
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
                // 이미지 alt 텍스트(자식 이벤트)를 End(Image)까지 모아둔다 — 실패 시
                // 대체 표시 문구에 쓰고, 성공 시 그대로 되돌려 놓는다.
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

                let is_remote = dest_url.starts_with("http://") || dest_url.starts_with("https://");
                let result = if is_remote {
                    ImageResult::Passthrough
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
                    ImageResult::Passthrough => {
                        out.push(Event::Start(Tag::Image {
                            link_type,
                            dest_url,
                            title,
                            id,
                        }));
                        out.extend(alt_events);
                        out.push(Event::End(TagEnd::Image));
                    }
                    ImageResult::NotFound | ImageResult::TooLarge | ImageResult::OutsideRoot => {
                        let reason_label = match result {
                            ImageResult::NotFound => "파일 없음",
                            ImageResult::TooLarge => "크기 초과(2MB)",
                            ImageResult::OutsideRoot => "루트 밖 경로",
                            _ => unreachable!(),
                        };
                        let reason_attr = match result {
                            ImageResult::NotFound => "not-found",
                            ImageResult::TooLarge => "too-large",
                            ImageResult::OutsideRoot => "outside-root",
                            _ => unreachable!(),
                        };
                        let alt = if alt_text.is_empty() {
                            dest_url.to_string()
                        } else {
                            alt_text
                        };
                        out.push(Event::Html(
                            format!(
                                r#"<span class="md-image-missing" data-reason="{reason_attr}">이미지를 표시할 수 없습니다 ({reason_label}): {alt}</span>"#,
                                reason_attr = reason_attr,
                                reason_label = reason_label,
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

/// ammonia로 살균한다.
///
/// 기본 허용 URL 스킴에 `data:`가 없다 — 명시적으로 허용하지 않으면 인라인한
/// 이미지가 여기서 통째로 잘려 나간다(img[src]에 한정해서 허용).
fn sanitize(raw_html: &str) -> String {
    let mut builder = ammonia::Builder::default();
    builder
        .add_generic_attributes(["class", "data-idx", "data-reason"])
        .add_tag_attributes("img", ["data-idx"])
        .add_url_schemes(["data"])
        .url_relative(ammonia::UrlRelative::PassThrough);
    builder.clean(raw_html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_images(_src: &str) -> ImageResult {
        ImageResult::NotFound
    }

    /// 1. mermaid 블록이 placeholder로 치환되고 원문이 순서대로 Vec<String>에 반환된다
    #[test]
    fn mermaid_block_becomes_placeholder() {
        let body = "# T\n\n```mermaid\ngraph TD; A-->B;\n```\n";
        let (html, mermaid) = render(body, &no_images);
        assert!(html.contains(r#"<div class="mermaid-block" data-idx="0"></div>"#));
        assert_eq!(mermaid, vec!["graph TD; A-->B;\n".to_string()]);
    }

    /// 2. mermaid 블록이 여러 개일 때 data-idx가 0, 1, 2…로 매겨진다
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

    /// 3. <script>가 살균된다
    #[test]
    fn script_tags_are_sanitized() {
        let body = "Hello\n\n<script>alert('xss')</script>\n\nWorld";
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("<script>"));
        assert!(!html.contains("alert("));
    }

    /// 4. 상대 링크 href가 유지된다
    #[test]
    fn relative_link_href_is_preserved() {
        let body = "[다른 문서](../Notes/other.md)";
        let (html, _) = render(body, &no_images);
        assert!(html.contains(r#"href="../Notes/other.md""#));
    }

    /// 5. javascript: href가 제거된다
    #[test]
    fn javascript_href_is_removed() {
        let body = "[클릭](javascript:alert(1))";
        let (html, _) = render(body, &no_images);
        assert!(!html.contains("javascript:"));
    }

    /// 6. 이미지 로더가 실패를 반환하면 대체 표시가 들어간다
    #[test]
    fn failed_image_load_shows_fallback() {
        let body = "![대체텍스트](missing.png)";
        let (html, _) = render(body, &|_| ImageResult::NotFound);
        assert!(!html.contains("<img"));
        assert!(html.contains("md-image-missing"));
        assert!(html.contains("파일 없음"));
        assert!(html.contains("대체텍스트"));
    }

    /// 7. 이미지 로더가 성공하면 src에 data: URI가 들어간다
    #[test]
    fn successful_image_load_inlines_data_uri() {
        let body = "![로고](logo.png)";
        let (html, _) = render(body, &|_| {
            ImageResult::Inlined("data:image/png;base64,AAAA".to_string())
        });
        assert!(html.contains(r#"src="data:image/png;base64,AAAA""#));
    }

    /// 8. 일반 코드블록(예: ```rust)은 mermaid로 취급되지 않는다
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
