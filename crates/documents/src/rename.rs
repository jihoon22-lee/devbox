//! `documents.rename`: journal copy → `RENAME_NOREPLACE` → lease/watch rekey →
//! old journal removal → `documents.moved`, all under both document locks.
use crate::R;
use crate::journal::JournalFile;
use crate::model::*;
use crate::service::{DocumentsService, Located, blocking, sys_fail};
use crate::sys;
use devbox_protocol::Internal;
use rustix::fs::FileType;

fn moved_key(old: &DocKey, from: &DocKey, to: &DocKey) -> DocKey {
    DocKey {
        root: to.root.clone(),
        path: format!("{}{}", to.path, &old.path[from.path.len()..]),
    }
}

impl DocumentsService {
    pub async fn rename(&self, p: RenameDocument) -> R<Renamed> {
        let key = self
            .move_locked(p.lease_id.as_deref(), p.key, p.to, false)
            .await?;
        Ok(Renamed { key })
    }
    /// Lease-less move for other domains (note renames, trash). Never replaces.
    pub async fn move_entry(&self, from: &DocKey, to: &DocKey) -> R<DocKey> {
        self.move_locked(None, from.clone(), to.clone(), false)
            .await
    }
    /// [`Self::move_entry`] for a regular file only (a folder is `invalid_input`).
    pub async fn move_file(&self, from: &DocKey, to: &DocKey) -> R<DocKey> {
        self.move_locked(None, from.clone(), to.clone(), true).await
    }
    async fn move_locked(
        &self,
        lease_id: Option<&str>,
        from: DocKey,
        to: DocKey,
        file_only: bool,
    ) -> R<DocKey> {
        let nested = to.path == from.path || to.path.starts_with(&format!("{}/", from.path));
        if from.root != to.root || nested {
            return Err(DocumentsError::InvalidInput.into());
        }
        let Located::At(src) = self.locate(from, false, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        match src.kind {
            None => return Err(DocumentsError::NotFound.into()),
            Some(kind) if file_only && kind != FileType::RegularFile => {
                return Err(DocumentsError::InvalidInput.into());
            }
            Some(_) => {}
        }
        let Located::At(dst) = self.locate(to, true, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        if src.key.root != dst.key.root {
            return Err(DocumentsError::InvalidInput.into());
        }
        if dst.kind.is_some() {
            return Err(DocumentsError::Exists.into());
        }
        if let Some(id) = lease_id {
            let lease = self
                .s
                .leases
                .get(id)
                .map_err(|reason| DocumentsError::LeaseLost { reason })?;
            if lease.key != src.key {
                return Err(DocumentsError::InvalidInput.into());
            }
        }
        let (from, to, to_abs) = (src.key.clone(), dst.key.clone(), dst.abs());
        let (first, second) = if from < to {
            (&from, &to)
        } else {
            (&to, &from)
        };
        let _first = self.s.leases.lock(first).await;
        let _second = self.s.leases.lock(second).await;
        #[cfg(test)]
        if let Some(hook) = &self.s.hooks.before_rename_commit {
            hook().await;
        }
        // A target that another window edits (possibly unsaved) is not free.
        if self.s.leases.occupied(&to) {
            return Err(DocumentsError::Exists.into());
        }
        let (s, f, t) = (self.s.clone(), from.clone(), to.clone());
        let old_journals = blocking(move || {
            let prefix = format!("{}/", t.path);
            let target_journal = s
                .journals
                .load(&t)
                .internal("read document journal")?
                .is_some()
                || (src.kind == Some(FileType::Directory)
                    && s.journals
                        .list()
                        .internal("list document journals")?
                        .iter()
                        .any(|j| j.key.root == t.root && j.key.path.starts_with(&prefix)));
            if target_journal {
                return Err(DocumentsError::Exists.into());
            }
            let journals: Vec<JournalFile> = if src.kind == Some(FileType::Directory) {
                let prefix = format!("{}/", f.path);
                let all = s.journals.list().internal("list document journals")?;
                all.into_iter()
                    .filter(|j| j.key.root == f.root && j.key.path.starts_with(&prefix))
                    .collect()
            } else {
                s.journals
                    .load(&f)
                    .internal("read document journal")?
                    .into_iter()
                    .collect()
            };
            // ① The new-key journal is durable before the move.
            for journal in &journals {
                let mut copy = journal.clone();
                copy.key = moved_key(&journal.key, &f, &t);
                s.journals.write(&copy).internal("copy document journal")?;
            }
            // ② Never replace; undo the copies on failure.
            if let Err(e) = sys::move_no_replace(&src.dir, &src.name, &dst.dir, &dst.name) {
                for journal in &journals {
                    let _ = s.journals.remove(&moved_key(&journal.key, &f, &t));
                }
                return Err(sys_fail(e));
            }
            Ok(journals.into_iter().map(|j| j.key).collect::<Vec<_>>())
        })
        .await?;
        // ③ Same lease ids and owners, new keys; queued journal writes follow them.
        for (old, new) in self.s.leases.rekey(&from, &to) {
            let rest = new.path[to.path.len()..].trim_start_matches('/');
            let abs = if rest.is_empty() {
                to_abs.clone()
            } else {
                to_abs.join(rest)
            };
            self.s.watch.rekey(&old, &new, abs);
        }
        // ④ Old-key journals go last, so a crash before this keeps a recoverable copy.
        let s = self.s.clone();
        blocking(move || {
            for key in old_journals {
                if s.journals.remove(&key).is_err() {
                    tracing::warn!("moved document journal was not removed");
                }
            }
            Ok(())
        })
        .await?;
        // ⑤
        let rev = self.next_rev();
        self.s.hub.publish::<crate::DocumentsMoved>(
            rev,
            &DocumentMoved {
                from,
                to: to.clone(),
            },
        );
        Ok(to)
    }
}
