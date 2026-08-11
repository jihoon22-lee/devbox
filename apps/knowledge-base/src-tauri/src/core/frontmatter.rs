/// 문서 메타데이터 (frontmatter에서 추출)
#[derive(Debug, Clone, Default)]
pub struct DocMeta {
    pub title: Option<String>,
    pub tags: Vec<String>,
}

/// Markdown frontmatter 파싱.
/// ```text
/// ---
/// title: Tauri study
/// tags: [rust, tauri]
/// ---
/// body...
/// ```
pub fn parse(content: &str) -> (DocMeta, &str) {
    let Some(rest) = content.strip_prefix("---\n") else {
        return (DocMeta::default(), content);
    };
    let Some(end) = rest.find("\n---") else {
        return (DocMeta::default(), content);
    };
    let front = &rest[..end];
    let body = &rest[end + 4..];
    let mut meta = DocMeta::default();

    for line in front.lines() {
        if let Some(v) = line.strip_prefix("title:") {
            meta.title = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("tags:") {
            let v = v.trim();
            // `[a, b]` 또는 다음 줄에 `- a` 형태
            if let Some(inner) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                meta.tags = inner
                    .split(',')
                    .map(|t| t.trim().trim_matches('"').to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            } else if !v.is_empty() && !v.starts_with('-') {
                meta.tags.push(v.trim_matches('"').to_string());
            } else if v.is_empty() {
                // `tags:` 아래의 `- item` 줄들을 수집
                let items: Vec<String> = front
                    .lines()
                    .skip_while(|l| !l.trim_start().starts_with("tags:"))
                    .skip(1)
                    .take_while(|l| l.trim_start().starts_with('-'))
                    .map(|l| {
                        l.trim()
                            .trim_start_matches('-')
                            .trim()
                            .trim_matches('"')
                            .to_string()
                    })
                    .filter(|t| !t.is_empty())
                    .collect();
                meta.tags = items;
            }
        }
    }
    (meta, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_title_and_tags() {
        let content = "---\ntitle: Hello\n---\n\n# Body\n";
        let (meta, body) = parse(content);
        assert_eq!(meta.title.as_deref(), Some("Hello"));
        assert!(body.contains("# Body"));
    }

    #[test]
    fn parses_inline_tag_list() {
        let content = "---\ntags: [rust, tauri]\n---\nbody";
        let (meta, _) = parse(content);
        assert_eq!(meta.tags, vec!["rust", "tauri"]);
    }

    #[test]
    fn parses_dash_tags() {
        let content = "---\ntags:\n  - rust\n  - wsl\n---\nbody";
        let (meta, _) = parse(content);
        assert_eq!(meta.tags, vec!["rust", "wsl"]);
    }

    #[test]
    fn no_frontmatter_returns_default() {
        let (meta, body) = parse("# just body");
        assert!(meta.title.is_none());
        assert!(meta.tags.is_empty());
        assert_eq!(body, "# just body");
    }
}
