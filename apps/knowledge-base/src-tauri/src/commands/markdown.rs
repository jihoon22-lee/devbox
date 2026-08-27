use crate::commands::docs::{resolve_root, AppState};
use crate::core::db::{self, AnalyzedWikilink, LinkResolution};
use crate::core::{assets::MAX_ASSET_BYTES, frontmatter, vault::VaultIdentity};
use ::markdown::{self, ImageResult};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use std::io::Read;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::SystemTime;

/// 이미지 하나 크기 상한. 초과하면 [`ImageResult::TooLarge`]로 대체 표시한다.
const MAX_IMAGE_BYTES: u64 = MAX_ASSET_BYTES as u64;

/// 이미지 인라인 캐시 상한 항목 수. 초과하면 캐시를 통째로 비운다.
const IMAGE_CACHE_CAP: usize = 32;

#[derive(Debug, Clone, Serialize)]
pub struct RenderedDoc {
    pub title: Option<String>,
    pub tags: Vec<String>,
    /// 살균 완료된 HTML. mermaid 블록은 placeholder로 치환됨.
    pub html: String,
    /// placeholder `data-idx` 순서의 mermaid 원문.
    pub mermaid: Vec<String>,
}

/// 마크다운 원문(저장 전 편집 중 내용)을 살균된 HTML + mermaid 목록으로 렌더링한다.
///
/// `.md` 확장자 여부는 서버에서 검증하지 않는다 — 프론트가 분할/프리뷰 버튼을
/// 비활성화하는 것으로 충분하고, 임의 텍스트를 마크다운으로 렌더하는 것 자체는
/// 무해하다.
#[tauri::command]
pub fn render_markdown(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: String,
) -> Result<RenderedDoc, String> {
    let (root, wikilinks) = {
        let conn = state.db.lock().unwrap();
        let root = resolve_root(&conn)?;
        let wikilinks = db::analyze_wikilinks(&conn, &content)
            .map_err(|_| "위키링크를 렌더링할 수 없습니다".to_string())?;
        (root, wikilinks)
    };
    let vault =
        VaultIdentity::inspect(&root).map_err(|_| "마크다운을 렌더링할 수 없습니다".to_string())?;

    let rewritten = rewrite_wikilinks_for_preview(&content, &wikilinks);
    let (meta, body) = frontmatter::parse(&rewritten);
    // 이미지·상대 링크는 문서가 위치한 디렉터리를 기준으로 해석한다.
    let doc_dir = Path::new(&rel).parent().unwrap_or_else(|| Path::new(""));
    let app_state: &AppState = state.inner().as_ref();

    let load_image = |src: &str| -> ImageResult { load_image(app_state, &vault, doc_dir, src) };
    let (html, mermaid) = markdown::render(body, &load_image);

    Ok(RenderedDoc {
        title: meta.title,
        tags: meta.tags,
        html,
        mermaid,
    })
}

fn rewrite_wikilinks_for_preview(content: &str, links: &[AnalyzedWikilink]) -> String {
    let mut rewritten = content.to_string();
    for link in links.iter().rev() {
        let display = if link.occurrence.label.is_empty() {
            "(invalid wikilink)"
        } else {
            &link.occurrence.label
        };
        let display = html_escape(display);
        let replacement = match &link.resolution {
            LinkResolution::Resolved(path) => format!(
                r#"<a class="wikilink resolved" href="/{}">{display}</a>"#,
                html_escape(path)
            ),
            LinkResolution::Missing => format!(
                r#"<span class="wikilink unresolved" title="대상 노트 없음">{display}</span>"#
            ),
            LinkResolution::Ambiguous => format!(
                r#"<span class="wikilink unresolved" title="같은 이름의 노트가 여러 개임">{display}</span>"#
            ),
            LinkResolution::Invalid => format!(
                r#"<span class="wikilink unresolved" title="올바르지 않은 위키링크 대상">{display}</span>"#
            ),
        };
        if link.occurrence.to_byte <= rewritten.len()
            && rewritten.is_char_boundary(link.occurrence.from_byte)
            && rewritten.is_char_boundary(link.occurrence.to_byte)
        {
            rewritten.replace_range(
                link.occurrence.from_byte..link.occurrence.to_byte,
                &replacement,
            );
        }
    }
    rewritten
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 실제 이미지 로더: `safe_join` 기반 경로 검증 → 크기 검사 → `fs::read` →
/// 확장자→MIME 추론 → base64 인코딩. 전부 IO/OS 의존이므로 core가 아니라
/// command 레이어에 둔다(`markdown` crate의 `render`는 이 함수를 클로저로
/// 주입받을 뿐 직접 알지 못한다).
fn load_image(state: &AppState, vault: &VaultIdentity, doc_dir: &Path, src: &str) -> ImageResult {
    let Some(rel_path) = normalize_image_path(doc_dir, src) else {
        return ImageResult::OutsideRoot;
    };
    let Ok(path) = vault.existing_entry(&rel_path) else {
        return ImageResult::NotFound;
    };
    let Ok(path_metadata) = std::fs::symlink_metadata(&path) else {
        return ImageResult::NotFound;
    };
    if !path_metadata.is_file() {
        return ImageResult::NotFound;
    }

    let Ok(file) = std::fs::File::open(&path) else {
        return ImageResult::NotFound;
    };
    let Ok(metadata) = file.metadata() else {
        return ImageResult::NotFound;
    };
    if !metadata.is_file() {
        return ImageResult::NotFound;
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return ImageResult::TooLarge;
    }
    let open_identity = VaultIdentity::entry_identity_from_metadata(&path, &metadata);
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    {
        let cache = state.image_cache.lock().unwrap();
        if let Some((cached_mtime, cached_len, cached_identity, data_uri)) = cache.get(&path) {
            if *cached_mtime == mtime
                && *cached_len == metadata.len()
                && cached_identity.matches(&open_identity)
            {
                return ImageResult::Inlined(data_uri.clone());
            }
        }
    }

    let mut bytes = Vec::new();
    if file
        .take(MAX_IMAGE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return ImageResult::NotFound;
    }
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return ImageResult::TooLarge;
    }
    if vault.revalidate().is_err() {
        return ImageResult::NotFound;
    }
    // The canonical path is safe only for the metadata snapshot that produced
    // it. Re-check the root-relative entry after the read so a concurrent
    // unlink/reparse replacement cannot turn a previously approved image
    // path into a different object before it reaches the data URI cache.
    let Ok(current_path) = vault.existing_entry(&rel_path) else {
        return ImageResult::NotFound;
    };
    if current_path != path {
        return ImageResult::NotFound;
    }
    let Ok(current_identity) = vault.existing_file_identity(&current_path) else {
        return ImageResult::NotFound;
    };
    if !open_identity.matches(&current_identity) {
        return ImageResult::NotFound;
    }
    let data_uri = format!(
        "data:{};base64,{}",
        mime_from_ext(&path),
        BASE64.encode(bytes)
    );

    let mut cache = state.image_cache.lock().unwrap();
    if cache.len() >= IMAGE_CACHE_CAP {
        cache.clear();
    }
    cache.insert(
        path,
        (mtime, metadata.len(), open_identity, data_uri.clone()),
    );

    ImageResult::Inlined(data_uri)
}

/// Markdown destinations are relative to the current note. Normalize `..`
/// segments before passing the path to the canonical root/symlink boundary;
/// `store::safe_join` intentionally rejects all parent segments and therefore
/// cannot represent a nested note linking to the root-level assets directory.
fn normalize_image_path(doc_dir: &Path, src: &str) -> Option<String> {
    if src.is_empty()
        || src.contains(['\\', '\0'])
        || src.chars().any(char::is_control)
        || src.starts_with('/')
        || src.starts_with('~')
    {
        return None;
    }
    let mut segments = Vec::new();
    for component in doc_dir.components() {
        let Component::Normal(value) = component else {
            return None;
        };
        let value = value.to_str()?;
        if value.is_empty() || value == "." || value == ".." || value.contains(':') {
            return None;
        }
        segments.push(value);
    }
    for component in src.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            value if value.contains(':') => return None,
            value => segments.push(value),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

fn mime_from_ext(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// frontmatter는 앱의 메타데이터 계층에서 분리한 뒤 공용 렌더러에 넘긴다.
    #[test]
    fn frontmatter_is_stripped_before_render() {
        let content = "---\ntitle: Hello\ntags: [rust]\n---\n\n# Body\n";
        let (_, body) = frontmatter::parse(content);
        let (html, _) = markdown::render(body, &|_| ImageResult::NotFound);
        assert!(!html.contains("title: Hello"));
        assert!(!html.contains("tags:"));
        assert!(html.contains("<h1>Body</h1>"));
    }

    #[test]
    fn preview_rewrite_uses_only_resolved_index_paths_and_escapes_labels() {
        let content = "[[Notes/Rust|Rust <safe>]] [[Missing]]";
        let parsed = crate::core::wikilink::parse_wikilinks(content);
        let links = vec![
            AnalyzedWikilink {
                occurrence: parsed[0].clone(),
                resolution: LinkResolution::Resolved("Notes/Rust.md".into()),
            },
            AnalyzedWikilink {
                occurrence: parsed[1].clone(),
                resolution: LinkResolution::Missing,
            },
        ];

        let rewritten = rewrite_wikilinks_for_preview(content, &links);
        assert!(rewritten.contains(r#"href="/Notes/Rust.md">Rust &lt;safe&gt;</a>"#));
        assert!(rewritten.contains("class=\"wikilink unresolved\""));
        assert!(!rewritten.contains("[[Missing]]"));

        let (html, _) = markdown::render(&rewritten, &|_| ImageResult::NotFound);
        assert!(html.contains(r#"class="wikilink resolved" href="/Notes/Rust.md""#));
        assert!(html.contains(r#"class="wikilink unresolved""#));
        assert!(html.contains("Rust &lt;safe&gt;"));
        assert!(!html.contains("<safe>"));
    }

    #[test]
    fn nested_note_asset_destination_normalizes_inside_root() {
        assert_eq!(
            normalize_image_path(Path::new("Notes/deep"), "../../assets/hash.png"),
            Some("assets/hash.png".to_string())
        );
        assert_eq!(
            normalize_image_path(Path::new("Notes/deep"), "../../../outside.png"),
            None
        );
    }

    #[test]
    fn image_destination_rejects_absolute_windows_and_control_paths() {
        for source in [
            "/etc/passwd",
            "\\\\server\\share\\secret.png",
            "C:/secret.png",
            "assets/secret\0.png",
        ] {
            assert_eq!(normalize_image_path(Path::new("Notes"), source), None);
        }
    }
}
