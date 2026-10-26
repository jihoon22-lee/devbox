//! Read-only source authority for an already sent response. No collection bytes are trusted as output.
use super::files::{self, Root};
use crate::{CollectionRoot, RequestOrigin};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq)]
struct Directory {
    canonical: PathBuf,
    device: u64,
    inode: u64,
}
fn directory(path: &Path) -> Result<Directory, ()> {
    let canonical = std::fs::canonicalize(path).map_err(|_| ())?;
    let meta = std::fs::metadata(&canonical).map_err(|_| ())?;
    if !meta.is_dir() {
        return Err(());
    }
    Ok(Directory {
        canonical,
        device: meta.dev(),
        inode: meta.ino(),
    })
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Authority {
    source_root: CollectionRoot,
    origin: Option<RequestOrigin>,
    anchor: Directory,
    collection: Option<Directory>,
}
impl Authority {
    pub(crate) async fn capture(
        config: &Path,
        projects: &devbox_projects::ProjectsService,
        source_root: &Option<CollectionRoot>,
        origin: &Option<RequestOrigin>,
    ) -> Result<Self, ()> {
        let source_root = source_root.as_ref().ok_or(())?;
        if origin.as_ref().is_some_and(|o| &o.root != source_root) {
            return Err(());
        }
        let (anchor, below) = match source_root {
            CollectionRoot::Project { project_id } => {
                let project = projects.get(project_id).await.map_err(|_| ())?.ok_or(())?;
                (PathBuf::from(project.path), &[".devbox", "api"][..])
            }
            _ => (config.to_owned(), &["api"][..]),
        };
        let root = Root {
            anchor: anchor.clone(),
            below,
            private: false,
        };
        let source_root = source_root.clone();
        let origin = origin.clone();
        tokio::task::spawn_blocking(move || {
            let anchor = directory(&anchor)?;
            let collection = if let Some(origin) = &origin {
                let dir = files::export_source_dir(&root, &origin.path).map_err(|_| ())?;
                Some(directory(&dir)?)
            } else {
                None
            };
            Ok(Self {
                source_root,
                origin,
                anchor,
                collection,
            })
        })
        .await
        .map_err(|_| ())?
    }
    pub(crate) async fn valid(
        &self,
        config: &Path,
        projects: &devbox_projects::ProjectsService,
    ) -> bool {
        Self::capture(
            config,
            projects,
            &Some(self.source_root.clone()),
            &self.origin,
        )
        .await
        .as_ref()
            == Ok(self)
    }
}
