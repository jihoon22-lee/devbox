//! Project-scoped dirty previews use the notes renderer with a project image resolver.
use crate::{FilesError, FilesService, PreviewFile, SourcePreview, files::failure};
use data_encoding::BASE64;
use devbox_documents::{DocKey, DocKind, WriteScope, is_internal};
use devbox_notes::{
    RenderedNote,
    attachments::{MAX_ASSET_BYTES, detect_format},
    links::{join_relative, percent_decode},
    render::{Budget, ImageResult, render_bounded},
};
use devbox_protocol::{Fail, Internal};
use rustix::fs::{FileType, Mode, OFlags, fstat, openat};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    io::Read,
};

const TEXT_LIMIT: usize = 4 * 1024 * 1024;
impl FilesService {
    pub async fn preview(&self, p: PreviewFile) -> Result<SourcePreview, Fail<FilesError>> {
        let documents = self.scoped(&p.key).await?;
        let text = match p.text {
            Some(text) => text,
            None => {
                let disk = documents
                    .read_disk(&p.key, DocKind::Code)
                    .await
                    .map_err(failure)?;
                if disk.rev.is_none() {
                    return Err(FilesError::NotFound.into());
                }
                disk.text.ok_or(FilesError::InvalidInput)?
            }
        };
        if text.len() > TEXT_LIMIT {
            return Err(FilesError::LimitExceeded.into());
        }
        if text.contains('\0') {
            return Err(FilesError::InvalidInput.into());
        }
        let scope = documents.scope().await.map_err(failure)?;
        tokio::task::spawn_blocking(move || {
            let at = scope
                .resolve(&p.key, false)
                .map_err(|_| FilesError::OutOfScope)?;
            if at.key.root != p.key.root || is_internal(&at.key.path) {
                return Err(FilesError::OutOfScope.into());
            }
            let extension = at
                .key
                .path
                .rsplit('.')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if extension == "svg" {
                if devbox_notes::render::escaped_len(&text) > devbox_notes::render::RESPONSE_BUDGET
                {
                    return Err(FilesError::LimitExceeded.into());
                }
                return Ok(SourcePreview::Svg { text });
            }
            if !matches!(extension.as_str(), "md" | "markdown") {
                return Err(FilesError::InvalidInput.into());
            }
            let cache = RefCell::new(HashMap::<String, ImageResult>::new());
            let spent = Cell::new(0_usize);
            let loader = |src: &str| {
                if let Some(known) = cache.borrow().get(src) {
                    return known.clone();
                }
                if cache.borrow().len() >= 128 || spent.get() >= Budget::default().images {
                    return ImageResult::Budget;
                }
                let mut result = image(&scope, &at.key, src);
                if let ImageResult::Inlined(uri) = &result {
                    let cost = uri.len();
                    if spent.get() + cost > Budget::default().images {
                        result = ImageResult::Budget;
                        spent.set(Budget::default().images);
                    } else {
                        spent.set(spent.get() + cost);
                    }
                }
                cache.borrow_mut().insert(src.into(), result.clone());
                result
            };
            let (_, body) = devbox_notes::frontmatter::parse(&text);
            let (html, mermaid) = render_bounded(body, &loader, Budget::default())
                .map_err(|_| FilesError::LimitExceeded)?;
            Ok(SourcePreview::Markdown {
                rendered: RenderedNote { html, mermaid },
            })
        })
        .await
        .internal("join source preview")?
    }
}

fn image(scope: &WriteScope, from: &DocKey, src: &str) -> ImageResult {
    let Some(decoded) = percent_decode(src.split(['#', '?']).next().unwrap_or("")) else {
        return ImageResult::OutsideRoot;
    };
    if decoded.contains(':')
        || decoded.starts_with('/')
        || decoded.contains('\\')
        || decoded.contains('\0')
    {
        return ImageResult::OutsideRoot;
    }
    let Some(path) = join_relative(Some(&from.path), &decoded).filter(|p| !is_internal(p)) else {
        return ImageResult::OutsideRoot;
    };
    let key = DocKey {
        root: from.root.clone(),
        path,
    };
    let Ok(at) = scope.resolve(&key, false) else {
        return ImageResult::OutsideRoot;
    };
    if at.key.root != from.root || is_internal(&at.key.path) {
        return ImageResult::OutsideRoot;
    }
    if at.kind != Some(FileType::RegularFile) {
        return ImageResult::NotFound;
    }
    let Ok(fd) = openat(
        &at.dir,
        at.name.as_str(),
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    ) else {
        return ImageResult::NotFound;
    };
    let Ok(stat) = fstat(&fd) else {
        return ImageResult::NotFound;
    };
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return ImageResult::NotFound;
    }
    if stat.st_size > MAX_ASSET_BYTES as i64 {
        return ImageResult::TooLarge;
    }
    let mut bytes = Vec::new();
    if std::fs::File::from(fd)
        .take(MAX_ASSET_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return ImageResult::NotFound;
    }
    if bytes.len() > MAX_ASSET_BYTES {
        return ImageResult::TooLarge;
    }
    match detect_format(&bytes) {
        Some(format) => ImageResult::Inlined(format!(
            "data:{};base64,{}",
            format.mime(),
            BASE64.encode(&bytes)
        )),
        None => ImageResult::NotFound,
    }
}
