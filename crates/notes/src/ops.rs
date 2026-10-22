//! Note writes (create, delete to `.trash/`, restore, attach) and rendering.
//! Every write is a `devbox-documents` CAS or no-replace operation.
use crate::R;
use crate::attachments::{self, AssetError, MAX_ASSET_BYTES};
use crate::links::{join_relative, percent_decode};
use crate::model::*;
use crate::render::{Budget, ImageResult, RESPONSE_BUDGET, render_bounded};
use crate::service::{NotesService, VAULT, docs_fail, invalid};
use crate::templates::{self, TemplateInput};
use crate::vault::{self, MAX_TITLE_BYTES, hidden};
use data_encoding::BASE64;
use devbox_documents::{DocKind, DocumentsError, Expect, NOTE_LIMIT};
use devbox_protocol::{Done, Fail};
use std::collections::HashMap;

const MAX_TRIES: usize = 100;
/// Distinct images one render response loads; later ones become `budget` notices.
pub(crate) const MAX_IMAGES: usize = 256;

fn image_problem(e: AssetError) -> Fail<NotesError> {
    let problem = match e {
        AssetError::EmptyInput => ImageProblem::Empty,
        AssetError::TooLarge => ImageProblem::TooLarge,
        AssetError::UnsupportedFormat => ImageProblem::Unsupported,
        AssetError::InvalidDimensions => ImageProblem::Dimensions,
        AssetError::InvalidNotePath => return NotesError::InvalidInput.into(),
        _ => return Fail::Internal("attachment name".into()),
    };
    NotesError::InvalidImage { problem }.into()
}
fn template_problem(problem: TemplateProblem) -> Fail<NotesError> {
    NotesError::InvalidTemplate { problem }.into()
}

/// Image `src` values in document order (remote ones are blocked by the renderer).
fn image_sources(body: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Options, Parser, Tag};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    Parser::new_ext(body, options)
        .filter_map(|e| match e {
            Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        })
        .collect()
}

impl NotesService {
    pub async fn create(&self, p: CreateNote) -> R<CreatedNote> {
        let title = p.title.trim();
        if !vault::valid_folder(&p.folder)
            || title.is_empty()
            || title.len() > MAX_TITLE_BYTES
            || title.chars().any(char::is_control)
        {
            return Err(NotesError::InvalidName.into());
        }
        let name = vault::note_file_name(title).ok_or(NotesError::InvalidName)?;
        let template = match p.template.as_deref() {
            None => None,
            Some(name) => Some(self.template_text(name).await?),
        };
        let (date, time) = (self.s.today)();
        let pc = (self.s.pc)().await;
        for n in 1..=MAX_TRIES {
            let path = vault::with_suffix(&vault::join(&p.folder, &name), n);
            let text = match &template {
                None => format!("# {title}\n"),
                Some(content) => {
                    let input = TemplateInput {
                        target: path.clone(),
                        title: title.into(),
                        date: date.clone(),
                        time: time.clone(),
                        pc: pc.clone(),
                    };
                    templates::render(content, &input).map_err(template_problem)?
                }
            };
            match self
                .s
                .docs
                .write_disk(&self.key(&path), None, Expect::Absent, &text)
                .await
            {
                Ok(rev) => {
                    self.refresh(vec![path.clone()]).await?;
                    return Ok(CreatedNote { path, rev });
                }
                Err(Fail::Domain(DocumentsError::Exists)) => continue,
                Err(e) => return Err(docs_fail(e)),
            }
        }
        Err(NotesError::Exists.into())
    }
    async fn template_text(&self, name: &str) -> R<String> {
        let path = format!("{}/{name}.md", self.s.settings.templates_folder);
        let not_found = || template_problem(TemplateProblem::NotFound);
        if name.contains(['/', '\\']) || !vault::valid_note_path(&path) {
            return Err(not_found());
        }
        let doc = self
            .s
            .docs
            .read_disk(&self.key(&path), DocKind::Note)
            .await
            .map_err(|_| not_found())?;
        let text = doc.text.ok_or_else(not_found)?;
        if text.len() > templates::MAX_TEMPLATE_CONTENT_BYTES {
            return Err(template_problem(TemplateProblem::InvalidContent));
        }
        Ok(text)
    }

    pub async fn delete(&self, p: NotePath) -> R<Trashed> {
        if !vault::valid_note_path(&p.path) {
            return invalid();
        }
        let (date, _) = (self.s.today)();
        for n in 1..=MAX_TRIES {
            let trash = vault::trash_path(&date, &p.path, n);
            match self
                .s
                .docs
                .trash_entry(&self.key(&p.path), &self.key(&trash))
                .await
            {
                Ok(_) => {
                    self.refresh(vec![p.path.clone()]).await?;
                    return Ok(Trashed { trash_path: trash });
                }
                Err(Fail::Domain(DocumentsError::Exists)) => continue,
                Err(e) => return Err(docs_fail(e)),
            }
        }
        Err(NotesError::Exists.into())
    }
    pub async fn restore(&self, p: RestoreNote) -> R<Done> {
        // Only a trashed note file comes back (never a date folder or other file);
        // `documents` also refuses anything but a regular file.
        if !devbox_documents::in_trash(&p.trash_path)
            || !vault::is_note(&p.trash_path)
            || !vault::valid_note_path(&p.path)
        {
            return invalid();
        }
        self.s
            .docs
            .restore_entry(&self.key(&p.trash_path), &self.key(&p.path))
            .await
            .map_err(docs_fail)?;
        self.refresh(vec![p.path]).await?;
        Ok(Done {
            rev: self.rev().to_string(),
        })
    }

    pub async fn attach(&self, p: AttachImage) -> R<Attached> {
        if !vault::valid_note_path(&p.note_path) || p.name.len() > 255 || p.name.contains('\0') {
            return invalid();
        }
        // base64 of the 2 MiB limit, with padding.
        if p.data.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
            let problem = ImageProblem::TooLarge;
            return Err(NotesError::InvalidImage { problem }.into());
        }
        let bytes = BASE64.decode(p.data.as_bytes()).map_err(|_| {
            let problem = ImageProblem::Encoding;
            NotesError::InvalidImage { problem }
        })?;
        let format = attachments::inspect(&bytes).map_err(image_problem)?;
        let hash = attachments::content_hash_hex(&bytes);
        let path =
            attachments::asset_relative_path(&hash, &p.name, format).map_err(image_problem)?;
        let markdown = attachments::markdown_link(&p.note_path, &path).map_err(image_problem)?;
        let key = self.key(&path);
        match self.s.docs.create_bytes(&key, &bytes).await {
            Ok(_) => {}
            // Same name means same hash prefix and name: reuse it when the bytes match.
            Err(Fail::Domain(DocumentsError::Exists)) => {
                let existing = self.s.docs.read_bytes(&key, MAX_ASSET_BYTES as u64).await;
                match existing.map_err(docs_fail)? {
                    Some((_, same)) if same == bytes => {}
                    _ => return Err(NotesError::Exists.into()),
                }
            }
            Err(e) => return Err(docs_fail(e)),
        }
        Ok(Attached { path, markdown })
    }

    pub async fn render(&self, p: RenderNote) -> R<RenderedNote> {
        if let Some(path) = &p.path
            && !vault::valid_note_path(path)
        {
            return invalid();
        }
        let text = match (p.text, p.path.as_deref()) {
            (Some(text), _) => {
                if text.contains('\0') {
                    return invalid();
                }
                if text.len() as u64 > NOTE_LIMIT {
                    return Err(NotesError::TooLarge { limit: NOTE_LIMIT }.into());
                }
                text
            }
            (None, Some(path)) => {
                let doc = self.s.docs.read_disk(&self.key(path), DocKind::Note).await;
                let doc = doc.map_err(docs_fail)?;
                match (doc.rev, doc.text) {
                    (None, _) => return Err(NotesError::NotFound.into()),
                    (Some(_), None) => {
                        return Err(NotesError::TooLarge { limit: NOTE_LIMIT }.into());
                    }
                    (Some(_), Some(text)) => text,
                }
            }
            (None, None) => return invalid(),
        };
        let (_, body) = crate::frontmatter::parse(&text);
        let images = self.load_images(p.path.as_deref(), body).await;
        let loader = |src: &str| images.get(src).cloned().unwrap_or(ImageResult::NotFound);
        let (html, mermaid) =
            render_bounded(body, &loader, Budget::default()).map_err(|_| NotesError::TooLarge {
                limit: RESPONSE_BUDGET as u64,
            })?;
        Ok(RenderedNote { html, mermaid })
    }
    /// Loads local images in document order until the inline budget is spent or
    /// [`MAX_IMAGES`] distinct images were seen; every later image gets a `budget`
    /// notice without being read.
    async fn load_images(&self, from: Option<&str>, body: &str) -> HashMap<String, ImageResult> {
        let mut out = HashMap::new();
        let (mut spent, mut full) = (0, false);
        for src in image_sources(body) {
            if out.contains_key(&src) {
                continue;
            }
            if full || out.len() >= MAX_IMAGES {
                out.insert(src, ImageResult::Budget);
                continue;
            }
            let result = match self.load_image(from, &src).await {
                ImageResult::Inlined(uri) => {
                    let cost = uri
                        .split_once(',')
                        .map_or(uri.len(), |(_, data)| data.len());
                    if spent + cost > Budget::default().images {
                        full = true;
                        ImageResult::Budget
                    } else {
                        spent += cost;
                        ImageResult::Inlined(uri)
                    }
                }
                other => other,
            };
            out.insert(src, result);
        }
        out
    }
    async fn load_image(&self, from: Option<&str>, src: &str) -> ImageResult {
        let lower = src.trim().to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
        {
            return ImageResult::RemoteBlocked;
        }
        let local = !src.contains(':') && !src.starts_with('/') && !src.contains('\\');
        let path = percent_decode(src.split(['#', '?']).next().unwrap_or(""))
            .filter(|_| local)
            .and_then(|src| join_relative(from, &src));
        let Some(path) = path.filter(|p| !hidden(p)) else {
            return ImageResult::OutsideRoot;
        };
        match self
            .s
            .docs
            .read_bytes(&self.key(&path), MAX_ASSET_BYTES as u64)
            .await
        {
            Ok(Some((key, bytes))) if key.root == VAULT => {
                match attachments::detect_format(&bytes) {
                    Some(format) => ImageResult::Inlined(format!(
                        "data:{};base64,{}",
                        format.mime(),
                        BASE64.encode(&bytes)
                    )),
                    None => ImageResult::NotFound,
                }
            }
            Ok(Some(_)) => ImageResult::OutsideRoot,
            Ok(None) => ImageResult::NotFound,
            Err(Fail::Domain(DocumentsError::TooLarge { .. })) => ImageResult::TooLarge,
            Err(Fail::Domain(DocumentsError::OutOfScope | DocumentsError::InvalidInput)) => {
                ImageResult::OutsideRoot
            }
            Err(_) => ImageResult::NotFound,
        }
    }
}
