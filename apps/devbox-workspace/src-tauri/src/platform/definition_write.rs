//! Reviewed JSON writes. Existing files reuse the editor's native conflict
//! checks; first creation publishes a complete sibling without overwriting.
use crate::private_metadata::MetadataRoot;
use code_pad_lib::commands::file::{self, ExpectedFileSnapshot, OpenedFile};
use devbox_filesystem::{ensure_no_links, filesystem_identity};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const LIMIT: u64 = 256 * 1024;

pub struct DefinitionTarget {
    path: PathBuf,
    parent: Option<MetadataRoot>,
    existing: Option<OpenedFile>,
}
impl DefinitionTarget {
    pub fn capture(path: &Path, expected: Option<&[u8]>) -> Result<Self> {
        let parent_path = path.parent().ok_or("unsafe_project_definition")?;
        let parent = match fs::symlink_metadata(parent_path) {
            Ok(_) => Some(MetadataRoot::open(parent_path)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("project_definition_unavailable"),
        };
        let existing = if let Some(bytes) = expected {
            ensure_no_links(path).map_err(|_| "unsafe_project_definition")?;
            let opened = file::open_path_limited(path, LIMIT)
                .map_err(|_| "project_definition_unavailable")?;
            if opened.size > LIMIT
                || opened.content_hash != super::super::definitions::digest(bytes)
            {
                return Err("project_definition_changed");
            }
            Some(opened)
        } else {
            if !matches!(fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("project_definition_changed");
            }
            None
        };
        Ok(Self {
            path: path.into(),
            parent,
            existing,
        })
    }
    pub fn write(
        self,
        bytes: &[u8],
        validate: impl FnOnce() -> Result<()>,
    ) -> Result<Option<String>> {
        if bytes.len() as u64 > LIMIT {
            return Err("project_definition_limit");
        }
        validate()?;
        if let Some(parent) = &self.parent {
            parent.revalidate()?;
        }
        if let Some(opened) = &self.existing {
            ensure_no_links(&self.path).map_err(|_| "unsafe_project_definition")?;
            let saved = file::save_path_limited(
                &self.path,
                std::str::from_utf8(bytes).map_err(|_| "invalid_manifest")?,
                opened.encoding,
                opened.line_ending,
                ExpectedFileSnapshot {
                    mtime: opened.mtime,
                    size: opened.size,
                    content_hash: &opened.content_hash,
                    identity: Some(opened.native_identity()),
                },
                opened.lossy,
                Some(LIMIT),
            )
            .map_err(|_| "project_definition_changed")?;
            return Ok(saved
                .durability_warning
                .map(|_| "definition_durability_warning".into()));
        }
        let parent_path = self.path.parent().ok_or("unsafe_project_definition")?;
        let parent = if let Some(parent) = self.parent {
            parent
        } else {
            // Creation is itself exclusive. Never adopt a directory that
            // appeared after review. A failure may leave this empty directory.
            fs::create_dir(parent_path).map_err(|_| "project_definition_changed")?;
            MetadataRoot::open(parent_path)?
        };
        let temporary =
            parent_path.join(format!(".devbox-definition-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "project_definition_unavailable")?;
        let result = (|| {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| "project_definition_unavailable")?;
            drop(file);
            parent.revalidate()?;
            ensure_no_links(&temporary).map_err(|_| "unsafe_project_definition")?;
            let identity =
                filesystem_identity(&temporary, false).map_err(|_| "project_definition_changed")?;
            // hard_link is an atomic no-replace publication on Windows and
            // Unix. Unsupported volumes fail with the destination untouched.
            fs::hard_link(&temporary, &self.path).map_err(|_| "project_definition_changed")?;
            let warning = parent.revalidate().is_err()
                || filesystem_identity(&self.path, false).ok() != Some(identity);
            Ok(warning.then(|| "definition_durability_warning".into()))
        })();
        let _ = fs::remove_file(temporary);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_definition_is_complete_and_never_replaces_a_concurrent_creator() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project.json");
        let first = DefinitionTarget::capture(&path, None).unwrap();
        let stale = DefinitionTarget::capture(&path, None).unwrap();
        assert_eq!(
            first.write(b"{\"schemaVersion\":1}", || Ok(())).unwrap(),
            None
        );
        assert!(stale.write(b"concurrent overwrite", || Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{\"schemaVersion\":1}");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn changed_bytes_or_objects_preserve_the_external_definition() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("project.json");
        fs::write(&path, b"first").unwrap();
        let stale = DefinitionTarget::capture(&path, Some(b"first")).unwrap();
        fs::write(&path, b"second").unwrap();
        assert!(stale.write(b"mine", || Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let stale = DefinitionTarget::capture(&path, Some(b"second")).unwrap();
        fs::rename(&path, root.path().join("original.json")).unwrap();
        fs::write(&path, b"second").unwrap();
        assert!(stale.write(b"mine", || Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"second");
    }
    #[test]
    fn reviewed_parent_creation_cannot_adopt_an_unreviewed_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".devbox/project.json");
        let stale = DefinitionTarget::capture(&path, None).unwrap();
        fs::create_dir(path.parent().unwrap()).unwrap();
        assert!(stale.write(b"mine", || Ok(())).is_err());
        assert!(!path.exists());
        let next = DefinitionTarget::capture(&path, None).unwrap();
        assert!(next.write(b"complete", || Ok(())).is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"complete");
    }
    #[test]
    fn validation_failure_never_creates_a_directory_and_existing_crlf_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".devbox/project.json");
        let pending = DefinitionTarget::capture(&path, None).unwrap();
        assert!(pending.write(b"mine", || Err("request_expired")).is_err());
        assert!(!path.parent().unwrap().exists());
        let pending = DefinitionTarget::capture(&path, None).unwrap();
        pending.write(b"{\r\n}\r\n", || Ok(())).unwrap();
        let pending = DefinitionTarget::capture(&path, Some(b"{\r\n}\r\n")).unwrap();
        pending
            .write(b"{\n  \"schemaVersion\": 1\n}\n", || Ok(()))
            .unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"{\r\n  \"schemaVersion\": 1\r\n}\r\n"
        );
    }
    #[cfg(unix)]
    #[test]
    fn replacing_a_reviewed_parent_with_a_link_never_writes_outside() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let parent = root.path().join(".devbox");
        fs::create_dir(&parent).unwrap();
        let target = DefinitionTarget::capture(&parent.join("project.json"), None).unwrap();
        fs::rename(&parent, root.path().join("old")).unwrap();
        std::os::unix::fs::symlink(outside.path(), &parent).unwrap();
        assert!(target.write(b"mine", || Ok(())).is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
