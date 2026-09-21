//! Optimistic editor writes, independent of watcher delivery and derived indexes.
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub const CONFLICT: &str = "note_conflict";
const UNAVAILABLE: &str = "note_unavailable";
const MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub content: Option<String>,
    pub revision: String,
}

pub fn read(path: &Path) -> Result<Snapshot, String> {
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| UNAVAILABLE)?;
    let parent_id = devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| UNAVAILABLE)?
        .components();
    let mut hash = Sha256::new();
    hash.update(b"knowledge-document/v1\0");
    hash.update(parent_id.0.to_be_bytes());
    hash.update(parent_id.1.to_be_bytes());
    hash.update(path.as_os_str().as_encoded_bytes());
    let content = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            hash.update(b"missing");
            None
        }
        Err(_) => return Err(UNAVAILABLE.into()),
        Ok(_) => {
            devbox_filesystem::ensure_no_links(path).map_err(|_| UNAVAILABLE)?;
            let (file, identity) =
                devbox_filesystem::open_filesystem_object(path, false).map_err(|_| UNAVAILABLE)?;
            let metadata = file.metadata().map_err(|_| UNAVAILABLE)?;
            let modified = metadata.modified().map_err(|_| UNAVAILABLE)?;
            let mut bytes = Vec::new();
            (&file)
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| UNAVAILABLE)?;
            if bytes.len() as u64 > MAX_BYTES
                || file
                    .metadata()
                    .map_err(|_| UNAVAILABLE)?
                    .modified()
                    .map_err(|_| UNAVAILABLE)?
                    != modified
                || devbox_filesystem::filesystem_identity(path, false).map_err(|_| UNAVAILABLE)?
                    != identity
            {
                return Err(CONFLICT.into());
            }
            let (volume, entry) = identity.components();
            hash.update(volume.to_be_bytes());
            hash.update(entry.to_be_bytes());
            hash.update(format!("{modified:?}"));
            hash.update(&bytes);
            Some(String::from_utf8(bytes).map_err(|_| UNAVAILABLE)?)
        }
    };
    Ok(Snapshot {
        content,
        revision: hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

/// An overwrite is also conditional: callers must review a fresh Snapshot.
/// A missing-file revision permits explicit recreation, never implicit repair.
pub fn save(path: &Path, content: &str, expected_revision: &str) -> Result<Snapshot, String> {
    if content.len() as u64 > MAX_BYTES || expected_revision.len() != 64 {
        return Err("note_invalid".into());
    }
    if read(path)?.revision != expected_revision {
        return Err(CONFLICT.into());
    }
    devbox_filesystem::atomic_write(path, content.as_bytes()).map_err(|_| UNAVAILABLE)?;
    let saved = read(path)?;
    if saved.content.as_deref() != Some(content) {
        return Err(CONFLICT.into());
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_edit_delete_and_same_bytes_replacement_refuse_stale_saves() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("note.md");
        fs::write(&path, "original").unwrap();
        let opened = read(&path).unwrap();
        fs::write(&path, "external edit").unwrap();
        assert_eq!(
            save(&path, "draft", &opened.revision).unwrap_err(),
            CONFLICT
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        let changed = read(&path).unwrap();
        let replacement = root.path().join("replacement.md");
        fs::write(&replacement, "external edit").unwrap();
        fs::remove_file(&path).unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(
            save(&path, "draft", &changed.revision).unwrap_err(),
            CONFLICT
        );
        let replaced = read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(
            save(&path, "draft", &replaced.revision).unwrap_err(),
            CONFLICT
        );
        assert!(!path.exists());
        let missing = read(&path).unwrap();
        assert!(missing.content.is_none());
        let saved = save(&path, "explicit recreation", &missing.revision).unwrap();
        assert_eq!(saved.content.as_deref(), Some("explicit recreation"));
        assert_eq!(
            save(&path, "replay", &missing.revision).unwrap_err(),
            CONFLICT
        );
    }

    #[test]
    fn reviewed_overwrite_is_invalidated_by_another_external_write() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("note.md");
        fs::write(&path, "external").unwrap();
        let review = read(&path).unwrap();
        fs::write(&path, "new external").unwrap();
        assert_eq!(
            save(&path, "overwrite", &review.revision).unwrap_err(),
            CONFLICT
        );
        let review = read(&path).unwrap();
        let saved = save(&path, "overwrite", &review.revision).unwrap();
        assert_ne!(saved.revision, review.revision);
    }
}
