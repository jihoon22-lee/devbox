use crate::commands::docs::{resolve_root, AppState};
use crate::core::markdown::{self, ImageResult};
use crate::core::{frontmatter, store};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

/// 이미지 하나 크기 상한. 초과하면 [`ImageResult::TooLarge`]로 대체 표시한다.
const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

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
    let root = {
        let conn = state.db.lock().unwrap();
        resolve_root(&conn)?
    };

    let (meta, body) = frontmatter::parse(&content);
    // 이미지·상대 링크는 문서가 위치한 디렉터리를 기준으로 해석한다.
    let doc_dir = Path::new(&rel).parent().unwrap_or_else(|| Path::new(""));
    let app_state: &AppState = state.inner().as_ref();

    let load_image = |src: &str| -> ImageResult { load_image(app_state, &root, doc_dir, src) };
    let (html, mermaid) = markdown::render(body, &load_image);

    Ok(RenderedDoc {
        title: meta.title,
        tags: meta.tags,
        html,
        mermaid,
    })
}

/// 실제 이미지 로더: `safe_join` 기반 경로 검증 → 크기 검사 → `fs::read` →
/// 확장자→MIME 추론 → base64 인코딩. 전부 IO/OS 의존이므로 core가 아니라
/// command 레이어에 둔다(`core/markdown.rs`의 `render`는 이 함수를 클로저로
/// 주입받을 뿐 직접 알지 못한다).
fn load_image(state: &AppState, root: &Path, doc_dir: &Path, src: &str) -> ImageResult {
    let rel_path = doc_dir.join(src);
    let Ok(path) = store::safe_join(root, &rel_path.to_string_lossy()) else {
        return ImageResult::OutsideRoot;
    };

    let Ok(metadata) = std::fs::metadata(&path) else {
        return ImageResult::NotFound;
    };
    if metadata.len() > MAX_IMAGE_BYTES {
        return ImageResult::TooLarge;
    }
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    {
        let cache = state.image_cache.lock().unwrap();
        if let Some((cached_mtime, data_uri)) = cache.get(&path) {
            if *cached_mtime == mtime {
                return ImageResult::Inlined(data_uri.clone());
            }
        }
    }

    let Ok(bytes) = std::fs::read(&path) else {
        return ImageResult::NotFound;
    };
    let data_uri = format!("data:{};base64,{}", mime_from_ext(&path), BASE64.encode(bytes));

    let mut cache = state.image_cache.lock().unwrap();
    if cache.len() >= IMAGE_CACHE_CAP {
        cache.clear();
    }
    cache.insert(path, (mtime, data_uri.clone()));

    ImageResult::Inlined(data_uri)
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
