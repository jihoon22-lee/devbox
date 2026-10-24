//! Source file operations share the document namespace and lease locks. Destructive
//! operations pin the parent fd and compare the inspected entry before deleting it.
use crate::service::{Located, blocking, sys_fail, write_checked};
use crate::{
    DiskRev, DocKey, DocumentsError, DocumentsService, EntryInfo, EntryKind, Expect, R, ReadOnly,
    sys,
};
use devbox_protocol::{Done, Fail};
use rustix::fs::{
    AtFlags, FileType, RenameFlags, fsync, readlinkat, renameat_with, statat, unlinkat,
};
use rustix::io::Errno;
use sha2::{Digest, Sha256};
use std::os::fd::OwnedFd;

fn io(e: Errno) -> Fail<DocumentsError> {
    Fail::Internal(format!("document entry io: {e}"))
}
fn stamp(hash: String, stat: &rustix::fs::Stat) -> DiskRev {
    DiskRev {
        hash,
        size: stat.st_size.max(0) as u64,
        mtime_ns: (i128::from(stat.st_mtime) * 1_000_000_000 + stat.st_mtime_nsec as i128)
            .to_string(),
        ino: stat.st_ino.to_string(),
    }
}
fn inspect(dir: &OwnedFd, name: &str, key: DocKey) -> R<EntryInfo> {
    let stat = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|e| {
        if e == Errno::NOENT {
            DocumentsError::NotFound.into()
        } else {
            io(e)
        }
    })?;
    let (kind, rev, read_only) = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => {
            let file = sys::read_at(dir, name, crate::CODE_LIMIT)
                .map_err(sys_fail)?
                .ok_or(DocumentsError::NotFound)?;
            let content = sys::classify(&file, crate::MAX_WIRE_TEXT);
            (
                EntryKind::File,
                file.rev,
                content.read_only.map(|reason| ReadOnly { reason }),
            )
        }
        FileType::Directory => (EntryKind::Directory, stamp("directory".into(), &stat), None),
        FileType::Symlink => {
            let target = readlinkat(dir, name, Vec::new()).map_err(io)?;
            (
                EntryKind::Symlink,
                stamp(sys::hex(&Sha256::digest(target.as_bytes())), &stat),
                None,
            )
        }
        _ => return Err(DocumentsError::InvalidInput.into()),
    };
    Ok(EntryInfo {
        key,
        kind,
        rev,
        read_only,
    })
}

impl DocumentsService {
    pub async fn stat_entry(&self, key: &DocKey) -> R<EntryInfo> {
        let _namespace = self.s.namespace.read().await;
        let Located::At(r) = self.locate(key.clone(), false, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        blocking(move || inspect(&r.dir, &r.name, r.key.clone())).await
    }
    /// Empty-file creation tests the entry itself, never a dangling symlink's target.
    pub async fn create_empty(&self, key: &DocKey) -> R<EntryInfo> {
        let _namespace = self.s.namespace.read().await;
        let Located::At(r) = self.locate(key.clone(), true, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        let _lease = self.s.leases.lock(&r.key).await;
        if r.kind.is_some() {
            return Err(DocumentsError::Exists.into());
        }
        let s = self.s.clone();
        blocking(move || {
            write_checked(&s, &r, b"", Expect::Absent, None, false)?;
            s.commits.record(r.key.clone());
            inspect(&r.dir, &r.name, r.key.clone())
        })
        .await
    }
    /// Removes one confirmed entry. No recursive delete; an interrupted/raced delete
    /// keeps the entry at its original name or at a visible recovery sibling.
    pub async fn remove_entry(&self, key: &DocKey, expect: DiskRev) -> R<Done> {
        let _namespace = self.s.namespace.write().await;
        let Located::At(r) = self.locate(key.clone(), false, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        let _lease = self.s.leases.lock(&r.key).await;
        let key = r.key.clone();
        #[cfg(test)]
        if let Some(hook) = &self.s.hooks.before_entry_remove {
            hook().await;
        }
        blocking(move || {
            let current = inspect(&r.dir, &r.name, r.key.clone())?;
            if current.rev != expect {
                return Err(DocumentsError::Conflict {
                    rev: Some(current.rev),
                }
                .into());
            }
            // Rename first so a replaced entry is never unlinked by its old name.
            // This visible name is deliberately not swept as an internal temp on restart.
            let held = sys::recovery_name(&r.name);
            renameat_with(
                &r.dir,
                r.name.as_str(),
                &r.dir,
                held.as_str(),
                RenameFlags::NOREPLACE,
            )
            .map_err(io)?;
            let outcome = (|| {
                let moved = inspect(&r.dir, &held, r.key.clone())?;
                if moved.kind != current.kind || moved.rev != expect {
                    return Err(DocumentsError::Conflict {
                        rev: Some(moved.rev),
                    }
                    .into());
                }
                let flags = if moved.kind == EntryKind::Directory {
                    AtFlags::REMOVEDIR
                } else {
                    AtFlags::empty()
                };
                unlinkat(&r.dir, held.as_str(), flags).map_err(|e| {
                    if e == Errno::NOTEMPTY || e == Errno::EXIST {
                        DocumentsError::NotEmpty.into()
                    } else {
                        io(e)
                    }
                })?;
                fsync(&r.dir).map_err(io)
            })();
            if outcome.is_err() {
                // NOREPLACE keeps a concurrently recreated destination untouched.
                let _ = renameat_with(
                    &r.dir,
                    held.as_str(),
                    &r.dir,
                    r.name.as_str(),
                    RenameFlags::NOREPLACE,
                );
                let _ = fsync(&r.dir);
            }
            outcome
        })
        .await?;
        self.s.commits.record(key.clone());
        let rev = self.next_rev();
        self.s
            .hub
            .publish::<crate::DocumentsChanged>(rev, &crate::DocumentChanged { key, rev: None });
        Ok(Done {
            rev: rev.to_string(),
        })
    }
}
