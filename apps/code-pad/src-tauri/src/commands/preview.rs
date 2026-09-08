//! Workspace-bounded Markdown and Mermaid preview commands.

use crate::commands::folder::{canonical_workspace, is_within_workspace};
use devbox_markdown::{render, ImageResult};
use serde::Serialize;
use std::io::Read;
use std::path::Path;

const MAX_IMAGE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    /// `markdown` returns sanitized HTML and mermaid blocks; `mermaid` returns
    /// the complete source of a standalone `.mmd` file.
    pub kind: String,
    pub html: Option<String>,
    pub mermaid: Vec<String>,
    pub source: Option<String>,
}

/// Removes a leading YAML frontmatter block from a Markdown document.
///
/// Only a delimiter on the first line starts frontmatter, and only a matching
/// delimiter on a later line closes it.  If the block is unterminated the
/// input is left untouched rather than silently dropping user text.
pub fn strip_frontmatter(content: &str) -> String {
    let Some(first_newline) = content.find('\n') else {
        return content.to_string();
    };
    let first_line = content[..first_newline].trim_end_matches('\r');
    if first_line != "---" {
        return content.to_string();
    }

    let mut offset = first_newline + 1;
    while offset <= content.len() {
        let remaining = &content[offset..];
        let line_end = remaining.find('\n').unwrap_or(remaining.len());
        let line = remaining[..line_end].trim_end_matches('\r');
        if line == "---" {
            let body_start = if line_end < remaining.len() {
                offset + line_end + 1
            } else {
                offset + line_end
            };
            return content[body_start..].to_string();
        }
        if line_end == remaining.len() {
            break;
        }
        offset += line_end + 1;
    }

    content.to_string()
}

/// Renders the current in-memory document.  The workspace root is supplied by
/// the frontend so a preview cannot load an image from an unrelated folder.
#[tauri::command]
pub async fn render_preview(
    path: String,
    content: String,
    workspace_root: String,
) -> Result<PreviewResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        render_preview_blocking(&path, &content, &workspace_root)
    })
    .await
    .map_err(|error| format!("프리뷰 렌더 작업이 중단되었습니다: {error}"))?
}

fn render_preview_blocking(
    path: &str,
    content: &str,
    workspace_root: &str,
) -> Result<PreviewResponse, String> {
    let root = canonical_workspace(Path::new(workspace_root))?;
    let document = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("프리뷰 문서를 확인할 수 없습니다: {error}"))?;
    if !document.is_file() || !is_within_workspace(&root, &document) {
        return Err("프리뷰 문서가 작업 폴더 밖에 있습니다".to_string());
    }

    let extension = document
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mmd" => Ok(PreviewResponse {
            kind: "mermaid".to_string(),
            html: None,
            mermaid: Vec::new(),
            source: Some(content.to_string()),
        }),
        "md" | "markdown" => {
            let document_parent = document
                .parent()
                .ok_or_else(|| "프리뷰 문서 부모 폴더를 확인할 수 없습니다".to_string())?;
            let load_image = |src: &str| load_image(&root, document_parent, src);
            let body = strip_frontmatter(content);
            let (html, mermaid) = render(&body, &load_image);
            Ok(PreviewResponse {
                kind: "markdown".to_string(),
                html: Some(html),
                mermaid,
                source: None,
            })
        }
        _ => Err("Markdown 또는 Mermaid 파일만 프리뷰할 수 있습니다".to_string()),
    }
}

/// Loads a relative image into a data URI, retaining the markdown crate's
/// `ImageResult` contract for missing, oversized, or unsafe paths.
pub fn load_image(root: &Path, document_parent: &Path, src: &str) -> ImageResult {
    // The shared renderer normally bypasses http(s), but keep this loader
    // safe when called directly and reject every URI scheme here as well.
    if has_url_scheme(src) {
        return ImageResult::NotFound;
    }
    let source = Path::new(src);
    if source.is_absolute() || src.starts_with('/') || src.starts_with('\\') {
        return ImageResult::OutsideRoot;
    }

    // Resolve URL dot segments lexically before filesystem IO, then reject
    // links/reparse points and require the actual root among the ancestors.
    let mut candidate = std::path::PathBuf::new();
    for component in document_parent.join(source).components() {
        match component {
            std::path::Component::ParentDir => {
                if !candidate.pop() {
                    return ImageResult::OutsideRoot;
                }
            }
            std::path::Component::CurDir => {}
            other => candidate.push(other),
        }
    }
    if !is_within_workspace(root, &candidate) {
        return ImageResult::OutsideRoot;
    }
    if let Err(error) = devbox_filesystem::ensure_no_links(&candidate) {
        return if error.kind() == std::io::ErrorKind::InvalidInput {
            ImageResult::OutsideRoot
        } else {
            ImageResult::NotFound
        };
    }
    let Ok(root_identity) = devbox_filesystem::filesystem_identity(root, true) else {
        return ImageResult::NotFound;
    };
    let mut parent = candidate.parent();
    let mut ancestors = Vec::new();
    let mut owns_root = false;
    while let Some(path) = parent {
        let Ok((handle, identity)) = devbox_filesystem::open_filesystem_object(path, true) else {
            return ImageResult::NotFound;
        };
        owns_root |= identity == root_identity;
        ancestors.push((path.to_path_buf(), identity, handle));
        parent = path.parent();
    }
    if !owns_root {
        return ImageResult::OutsideRoot;
    }
    let Ok((mut file, identity)) = devbox_filesystem::open_filesystem_object(&candidate, false)
    else {
        return ImageResult::NotFound;
    };
    let Ok(metadata) = file.metadata() else {
        return ImageResult::NotFound;
    };
    if metadata.len() > MAX_IMAGE_BYTES {
        return ImageResult::TooLarge;
    }
    let Some(mime) = raster_mime_from_path(&candidate) else {
        return ImageResult::NotFound;
    };
    let mut bytes = Vec::new();
    if file
        .by_ref()
        .take(MAX_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return ImageResult::NotFound;
    }
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return ImageResult::TooLarge;
    }
    if devbox_filesystem::ensure_no_links(&candidate).is_err()
        || devbox_filesystem::filesystem_identity(&candidate, false).ok() != Some(identity)
        || ancestors.iter().any(|(path, expected, _)| {
            devbox_filesystem::filesystem_identity(path, true).ok() != Some(*expected)
        })
    {
        return ImageResult::OutsideRoot;
    }
    let data_uri = format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(bytes),
    );
    ImageResult::Inlined(data_uri)
}

fn raster_mime_from_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

fn has_url_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    colon > 0
        && value[..colon].bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || index > 0
                    && (byte == b'+' || byte == b'-' || byte == b'.' || byte.is_ascii_digit())
        })
}

use base64::Engine;

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_render_preview(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
        content: String,
        workspace_root: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = render_preview(input.path, input.content, input.workspace_root).await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn strips_only_a_closed_leading_frontmatter_block() {
        assert_eq!(
            strip_frontmatter("---\ntitle: Example\n---\n\n# Body\n"),
            "\n# Body\n"
        );
        assert_eq!(
            strip_frontmatter("# ---\nnot metadata"),
            "# ---\nnot metadata"
        );
        let unterminated = "---\ntitle: Example\n# Body";
        assert_eq!(strip_frontmatter(unterminated), unterminated);
    }

    #[test]
    fn image_loader_enforces_root_size_and_missing_contract() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("logo.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        fs::write(docs.join("diagram.svg"), "<svg></svg>").unwrap();
        let outside = directory.path().join("outside.png");
        fs::write(&outside, [9_u8, 8, 7]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, docs.join("outside-link.png")).unwrap();
        fs::write(
            docs.join("large.png"),
            vec![0_u8; MAX_IMAGE_BYTES as usize + 1],
        )
        .unwrap();

        let root = root.canonicalize().unwrap();
        let docs = docs.canonicalize().unwrap();
        assert!(matches!(
            load_image(&root, &docs, "logo.png"),
            ImageResult::Inlined(uri) if uri.starts_with("data:image/png;base64,")
        ));
        assert_eq!(load_image(&root, &docs, "large.png"), ImageResult::TooLarge);
        assert_eq!(
            load_image(&root, &docs, "diagram.svg"),
            ImageResult::NotFound
        );
        assert_eq!(
            load_image(&root, &docs, "missing.png"),
            ImageResult::NotFound
        );
        assert_eq!(
            load_image(&root, &docs, "data:image/png;base64,AAAA"),
            ImageResult::NotFound
        );
        assert_eq!(
            load_image(&root, &docs, "ftp://example.test/image.png"),
            ImageResult::NotFound
        );
        assert_eq!(
            load_image(&root, &docs, "../../outside.png"),
            ImageResult::OutsideRoot
        );
        #[cfg(unix)]
        assert_eq!(
            load_image(&root, &docs, "outside-link.png"),
            ImageResult::OutsideRoot
        );
    }

    #[test]
    fn remote_images_follow_markdown_crate_passthrough_contract() {
        let (html, _) = render("![remote](https://example.test/image.png)", &|_| {
            panic!("remote image must not call the local loader")
        });
        assert!(html.contains("https://example.test/image.png"));
    }

    #[test]
    fn standalone_mermaid_returns_strict_source_without_markdown_rendering() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("diagram.mmd");
        std::fs::write(&path, "graph TD; A-->B;").unwrap();
        let response = render_preview_blocking(
            &path.to_string_lossy(),
            "graph TD; A-->B;",
            &root.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(response.kind, "mermaid");
        assert_eq!(response.source.as_deref(), Some("graph TD; A-->B;"));
        assert!(response.html.is_none());
        assert!(response.mermaid.is_empty());
    }

    #[test]
    fn preview_rejects_a_document_outside_the_workspace_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("workspace");
        let outside = directory.path().join("outside.md");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&outside, "# outside").unwrap();
        let error = render_preview_blocking(
            &outside.to_string_lossy(),
            "# outside",
            &root.to_string_lossy(),
        )
        .unwrap_err();
        assert!(error.contains("작업 폴더 밖"));
    }
}
