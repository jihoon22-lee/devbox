//! Optimistic editor writes, independent of watcher delivery and derived indexes.
use crate::platform::document_publish;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);

pub const CONFLICT: &str = "note_conflict";
const UNAVAILABLE: &str = "note_unavailable";
const MAX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
#[ts(rename = "NoteSnapshot")]
pub struct Snapshot {
    pub content: Option<String>,
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_outcome: Option<SaveOutcome>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct SaveOutcome {
    pub state: &'static str,
    pub recovery_directory: Option<String>,
    pub warning: &'static str,
}

pub fn read(path: &Path) -> Result<Snapshot, String> {
    read_at(path, path)
}

// A displaced file keeps the original logical pathname in its revision hash.
fn read_at(path: &Path, logical_path: &Path) -> Result<Snapshot, String> {
    let parent = logical_path.parent().ok_or(UNAVAILABLE)?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| UNAVAILABLE)?;
    let parent_id = devbox_filesystem::filesystem_identity(parent, true)
        .map_err(|_| UNAVAILABLE)?
        .content_components();
    let mut hash = Sha256::new();
    hash.update(b"knowledge-document/v1\0");
    hash.update(parent_id.0.to_be_bytes());
    hash.update(parent_id.1.to_be_bytes());
    hash.update(logical_path.as_os_str().as_encoded_bytes());
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
            let (volume, entry) = identity.content_components();
            hash.update(volume.to_be_bytes());
            hash.update(entry.to_be_bytes());
            hash.update(format!("{modified:?}"));
            hash.update(&bytes);
            Some(String::from_utf8(bytes).map_err(|_| UNAVAILABLE)?)
        }
    };
    Ok(Snapshot {
        save_outcome: None,
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
    save_with(path, content, expected_revision, |_| {})
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Validated,
    Prepared,
    Publishing,
    Published,
}

fn save_with(
    path: &Path,
    content: &str,
    expected_revision: &str,
    mut hook: impl FnMut(Phase),
) -> Result<Snapshot, String> {
    if content.len() as u64 > MAX_BYTES || expected_revision.len() != 64 {
        return Err("note_invalid".into());
    }
    let original = read(path)?;
    if original.revision != expected_revision {
        return Err(CONFLICT.into());
    }
    hook(Phase::Validated);
    let directory = prepare(path, content)?;
    let staged = directory.join("submitted.md");
    let previous = directory.join("previous.md");
    hook(Phase::Prepared);
    // This check avoids unnecessary publication, but preservation below closes
    // the race left between the check and the actual filesystem operation.
    if read(path).map_or(true, |snapshot| snapshot.revision != expected_revision) {
        let _ = fs::remove_dir_all(&directory);
        return Err(CONFLICT.into());
    }
    hook(Phase::Publishing);
    let published = if original.content.is_none() {
        document_publish::create(&staged, path)
    } else {
        document_publish::replace(&staged, path, &previous)
    };
    if published.is_err() {
        return Ok(recovery(&directory, "unknown", "note_commit_unknown"));
    }
    hook(Phase::Published);
    if original.content.is_some()
        && read_at(&previous, path).map_or(true, |snapshot| snapshot.revision != expected_revision)
    {
        return Ok(recovery(
            &directory,
            "appliedWithConflict",
            "note_displaced_conflict",
        ));
    }
    let mut saved = match read(path) {
        Ok(snapshot) if snapshot.content.as_deref() == Some(content) => snapshot,
        _ => return Ok(recovery(&directory, "unknown", "note_commit_unknown")),
    };
    let durability = devbox_filesystem::finish_replacement(path);
    let native_sync = document_publish::sync_parent(path);
    if fs::remove_dir_all(&directory).is_err() {
        saved.save_outcome = Some(SaveOutcome {
            state: "applied",
            recovery_directory: recovery_name(&directory),
            warning: "note_cleanup_pending",
        });
    } else if durability.durability_warning.is_some() || native_sync.is_err() {
        saved.save_outcome = Some(SaveOutcome {
            state: "applied",
            recovery_directory: None,
            warning: "note_durability_warning",
        });
    }
    Ok(saved)
}

fn recovery_name(directory: &Path) -> Option<String> {
    directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}
fn recovery(directory: &Path, state: &'static str, warning: &'static str) -> Snapshot {
    Snapshot {
        content: None,
        revision: String::new(),
        save_outcome: Some(SaveOutcome {
            state,
            recovery_directory: recovery_name(directory),
            warning,
        }),
    }
}
fn prepare(path: &Path, content: &str) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    for _ in 0..32 {
        let directory = parent.join(format!(
            ".devbox-save-{}-{}",
            std::process::id(),
            NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
        ));
        match document_publish::private_directory(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(UNAVAILABLE.into()),
        }
        let result = (|| -> std::io::Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut staged = options.open(directory.join("submitted.md"))?;
            staged.write_all(content.as_bytes())?;
            #[cfg(unix)]
            match fs::metadata(path) {
                Ok(metadata) => staged.set_permissions(metadata.permissions())?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            document_publish::staging_permissions(&directory.join("submitted.md"), path)?;
            staged.sync_all()
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&directory);
            return Err(UNAVAILABLE.into());
        }
        return Ok(directory);
    }
    Err(UNAVAILABLE.into())
}

/// Creation publishes complete bytes without overwriting a competing creator.
pub fn create(path: &Path, content: &str) -> Result<(), String> {
    create_with(path, content, || {})
}
fn create_with(path: &Path, content: &str, before_publish: impl FnOnce()) -> Result<(), String> {
    if content.len() as u64 > MAX_BYTES {
        return Err("note_invalid".into());
    }
    let parent = path.parent().ok_or(UNAVAILABLE)?;
    let mut existing = parent;
    while !existing.exists() {
        existing = existing.parent().ok_or(UNAVAILABLE)?;
    }
    devbox_filesystem::ensure_no_links(existing).map_err(|_| UNAVAILABLE)?;
    fs::create_dir_all(parent).map_err(|_| UNAVAILABLE)?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| UNAVAILABLE)?;
    let directory = prepare(path, content)?;
    before_publish();
    let result = document_publish::create(&directory.join("submitted.md"), path);
    let cleanup = fs::remove_dir_all(directory);
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(CONFLICT.into()),
        Err(_) => Err(UNAVAILABLE.into()),
        Ok(())
            if cleanup.is_err()
                || devbox_filesystem::finish_replacement(path)
                    .durability_warning
                    .is_some()
                || document_publish::sync_parent(path).is_err() =>
        {
            Err("note_applied_postprocessing_failed".into())
        }
        Ok(()) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn competing_creators_never_clobber_complete_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new.md");
        let result = create_with(&path, "ours", || fs::write(&path, "external").unwrap());
        assert_eq!(result.unwrap_err(), CONFLICT);
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn missing_revision_cannot_overwrite_a_competing_creation() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new.md");
        let opened = read(&path).unwrap();
        let saved = save_with(&path, "ours", &opened.revision, |at| {
            if at == Phase::Publishing {
                fs::write(&path, "external").unwrap();
            }
        })
        .unwrap();
        assert_eq!(saved.save_outcome.unwrap().state, "unknown");
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        let nested = root.path().join("one/two/three/new.md");
        create(&nested, "nested").unwrap();
        assert_eq!(fs::read_to_string(nested).unwrap(), "nested");
    }

    #[test]
    fn writers_at_every_publication_boundary_keep_their_data() {
        for phase in [
            Phase::Validated,
            Phase::Prepared,
            Phase::Publishing,
            Phase::Published,
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("note.md");
            fs::write(&path, "original").unwrap();
            let opened = read(&path).unwrap();
            let result = save_with(&path, "our draft", &opened.revision, |at| {
                if at == phase {
                    fs::write(&path, "external update").unwrap();
                }
            });
            if phase == Phase::Validated || phase == Phase::Prepared {
                assert_eq!(result.unwrap_err(), CONFLICT);
                assert_eq!(fs::read_to_string(&path).unwrap(), "external update");
            } else {
                let saved = result.unwrap();
                let outcome = saved.save_outcome.unwrap();
                let recovery = root.path().join(outcome.recovery_directory.unwrap());
                if phase == Phase::Publishing {
                    assert_eq!(outcome.state, "appliedWithConflict");
                    assert_eq!(
                        fs::read_to_string(recovery.join("previous.md")).unwrap(),
                        "external update"
                    );
                    assert_eq!(fs::read_to_string(&path).unwrap(), "our draft");
                } else {
                    assert_eq!(outcome.state, "unknown");
                    assert_eq!(fs::read_to_string(&path).unwrap(), "external update");
                }
            }
        }
    }

    #[test]
    fn replacement_writer_and_post_commit_read_failure_preserve_evidence() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("note.md");
        fs::write(&path, "original").unwrap();
        let opened = read(&path).unwrap();
        let saved = save_with(&path, "ours", &opened.revision, |at| {
            if at == Phase::Publishing {
                devbox_filesystem::atomic_write(&path, b"external replacement").unwrap();
            }
        })
        .unwrap();
        let directory = root
            .path()
            .join(saved.save_outcome.unwrap().recovery_directory.unwrap());
        assert_eq!(
            fs::read_to_string(directory.join("previous.md")).unwrap(),
            "external replacement"
        );
        let opened = read(&path).unwrap();
        let saved = save_with(&path, "next", &opened.revision, |at| {
            if at == Phase::Published {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
        })
        .unwrap();
        assert_eq!(saved.save_outcome.unwrap().state, "unknown");
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn note_permissions_and_private_recovery_directory() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [0o600, 0o640, 0o644] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("note.md");
            fs::write(&path, "old").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            let opened = read(&path).unwrap();
            save(&path, "new", &opened.revision).unwrap();
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                mode
            );
            let opened = read(&path).unwrap();
            let saved = save_with(&path, "ours", &opened.revision, |at| {
                if at == Phase::Publishing {
                    fs::write(&path, "external").unwrap();
                }
            })
            .unwrap();
            let directory = root
                .path()
                .join(saved.save_outcome.unwrap().recovery_directory.unwrap());
            assert_eq!(
                fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

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
