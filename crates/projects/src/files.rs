use crate::file_model::*;
use devbox_documents::{
    DocKey, DocumentsError, DocumentsService, EntryInfo, EntryKind, is_internal,
};
use devbox_protocol::{Done, Exporter, Fail, Internal, method};
use rustix::fs::{AtFlags, Dir, FileType, statat};
use sha2::{Digest, Sha256};
use std::sync::Arc;

type R<T> = Result<T, Fail<FilesError>>;
const PAGE: usize = 256;
const MAX_ENTRIES: usize = 20_000;
method!(pub FilesTree = "files.tree", query, FileTreeQuery => FileTree, FilesError);
method!(pub FilesStat = "files.stat", query, FileKey => EntryInfo, FilesError);
method!(pub FilesCreate = "files.create", mutation, FileKey => EntryInfo, FilesError);
method!(pub FilesDelete = "files.delete", mutation(always), DeleteFile => Done, FilesError);
method!(pub FilesExternalEditor = "files.external_editor", mutation, OpenSourceEditor => SourceEditorOpened, FilesError);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<FilesExternalEditor>();
    x.method::<FilesTree>();
    x.method::<FilesStat>();
    x.method::<FilesCreate>();
    x.method::<FilesDelete>();
}

pub struct FilesService {
    documents: Arc<DocumentsService>,
}
fn failure(error: Fail<DocumentsError>) -> Fail<FilesError> {
    match error {
        Fail::Domain(DocumentsError::NotFound) => FilesError::NotFound.into(),
        Fail::Domain(DocumentsError::Exists) => FilesError::Exists.into(),
        Fail::Domain(DocumentsError::NotEmpty) => FilesError::NotEmpty.into(),
        Fail::Domain(DocumentsError::Conflict { .. }) => FilesError::StateChanged.into(),
        Fail::Domain(DocumentsError::OutOfScope) => FilesError::OutOfScope.into(),
        Fail::Internal(message) => Fail::Internal(message),
        _ => FilesError::InvalidInput.into(),
    }
}
impl FilesService {
    pub fn new(documents: Arc<DocumentsService>) -> Self {
        Self { documents }
    }
    async fn scoped(&self, key: &DocKey) -> R<DocumentsService> {
        if !key.root.starts_with("project:") || key.root.len() <= 8 || is_internal(&key.path) {
            return Err(FilesError::OutOfScope.into());
        }
        let scope = self.documents.scope().await.map_err(failure)?;
        let agents = scope
            .roots()
            .filter(|(id, _)| id.starts_with("agents:"))
            .map(|(_, path)| path.to_path_buf())
            .collect();
        Ok(self.documents.scoped_to(&key.root).excluding(agents))
    }
    pub async fn stat(&self, p: FileKey) -> R<EntryInfo> {
        self.scoped(&p.key)
            .await?
            .stat_entry(&p.key)
            .await
            .map_err(failure)
    }
    pub async fn create(&self, p: FileKey) -> R<EntryInfo> {
        self.scoped(&p.key)
            .await?
            .create_empty(&p.key)
            .await
            .map_err(failure)
    }
    pub async fn delete(&self, p: DeleteFile) -> R<Done> {
        self.scoped(&p.key)
            .await?
            .remove_entry(&p.key, p.expect)
            .await
            .map_err(failure)
    }
    pub async fn external_path(&self, key: &DocKey) -> R<String> {
        let documents = self.scoped(key).await?;
        documents.stat_entry(key).await.map_err(failure)?;
        let scope = documents.scope().await.map_err(failure)?;
        let at = scope
            .resolve(key, false)
            .map_err(|_| FilesError::OutOfScope)?;
        if at.kind != Some(FileType::RegularFile) || is_internal(&at.key.path) {
            return Err(FilesError::InvalidInput.into());
        }
        at.abs()
            .to_str()
            .map(str::to_owned)
            .ok_or(FilesError::InvalidInput.into())
    }
    pub async fn tree(&self, p: FileTreeQuery) -> R<FileTree> {
        let root = format!("project:{}", p.project_id);
        let probe = DocKey {
            root: root.clone(),
            path: if p.dir.is_empty() {
                "__devbox_listing__".into()
            } else {
                format!("{}/__devbox_listing__", p.dir)
            },
        };
        let scope = self.scoped(&probe).await?.scope().await.map_err(failure)?;
        tokio::task::spawn_blocking(move || {
            let resolved = scope
                .resolve_entry(&probe, false)
                .map_err(|_| FilesError::OutOfScope)?;
            if resolved.key.root != root || is_internal(&resolved.key.path) {
                return Err(FilesError::OutOfScope.into());
            }
            let folder = resolved
                .key
                .path
                .strip_suffix("__devbox_listing__")
                .ok_or(FilesError::InvalidInput)?;
            let mut entries = Vec::new();
            let mut scanned = 0;
            let dir = Dir::read_from(&resolved.dir).internal("read source directory")?;
            let mut versions = Vec::new();
            for row in dir {
                scanned += 1;
                if scanned > MAX_ENTRIES {
                    return Err(FilesError::LimitExceeded.into());
                }
                let row = row.internal("read source entry")?;
                let Ok(name) = row.file_name().to_str() else {
                    continue;
                };
                if matches!(name, "." | "..")
                    || is_internal(name)
                    || (!p.show_hidden && name.starts_with('.'))
                {
                    continue;
                }
                let key = DocKey {
                    root: root.clone(),
                    path: format!("{folder}{name}"),
                };
                let Ok(at) = scope.resolve_entry(&key, false) else {
                    continue;
                };
                if is_internal(&at.key.path) {
                    continue;
                }
                let Ok(stamp) = statat(&resolved.dir, name, AtFlags::SYMLINK_NOFOLLOW) else {
                    continue;
                };
                let kind = match FileType::from_raw_mode(stamp.st_mode) {
                    FileType::RegularFile => EntryKind::File,
                    FileType::Directory => EntryKind::Directory,
                    FileType::Symlink => EntryKind::Symlink,
                    _ => continue,
                };
                versions.push(format!(
                    "{name}\0{}:{}:{}:{}",
                    stamp.st_ino, stamp.st_mode, stamp.st_mtime, stamp.st_mtime_nsec
                ));
                entries.push(FileEntry {
                    key: at.key,
                    name: name.into(),
                    kind,
                });
            }
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            versions.sort();
            let rev = format!(
                "{:x}",
                Sha256::digest(format!("{}\0{folder}\0{}", root, versions.join("\n")))
            );
            let start = if let Some(cursor) = p.cursor {
                if cursor.rev != rev {
                    return Err(FilesError::StateChanged.into());
                }
                entries.partition_point(|e| e.name <= cursor.after)
            } else {
                0
            };
            let page: Vec<_> = entries.iter().skip(start).take(PAGE).cloned().collect();
            let next_cursor = if start + page.len() < entries.len() {
                page.last().map(|e| FileTreeCursor {
                    after: e.name.clone(),
                    rev,
                })
            } else {
                None
            };
            Ok(FileTree {
                entries: page,
                next_cursor,
            })
        })
        .await
        .internal("join source tree")?
    }
}
