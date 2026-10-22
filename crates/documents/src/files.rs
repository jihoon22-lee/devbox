//! Lease-less operations other domains need besides text: byte reads and
//! exclusive byte creates (image attachments, rendering) and moves into or out of
//! `.trash/` (note delete and undo). All of them use the same scope resolution and
//! `O_NOFOLLOW` directory chain as text documents.
use crate::model::*;
use crate::scope::{in_trash, is_internal};
use crate::service::{DocumentsService, Located, blocking, sys_fail, write_checked};
use crate::{CODE_LIMIT, R, sys};
use rustix::fs::FileType;

impl DocumentsService {
    /// Reads a regular file (a final symlink is followed only inside the roots).
    /// `Ok(None)` = absent; `too_large` when it exceeds `limit` bytes. The returned
    /// key is the resolved one, so callers can check which root it landed in.
    pub async fn read_bytes(&self, key: &DocKey, limit: u64) -> R<Option<(DocKey, Vec<u8>)>> {
        let Located::At(r) = self.locate(key.clone(), false, false).await? else {
            return Ok(None);
        };
        blocking(move || {
            let Some(file) = sys::read_at(&r.dir, &r.name, limit).map_err(sys_fail)? else {
                return Ok(None);
            };
            match file.bytes {
                Some(bytes) => Ok(Some((r.key.clone(), bytes))),
                None => Err(DocumentsError::TooLarge { limit }.into()),
            }
        })
        .await
    }

    /// Creates a new file with exactly `bytes` (`RENAME_NOREPLACE`); never replaces.
    pub async fn create_bytes(&self, key: &DocKey, bytes: &[u8]) -> R<DiskRev> {
        if bytes.len() as u64 > CODE_LIMIT {
            return Err(DocumentsError::TooLarge { limit: CODE_LIMIT }.into());
        }
        let Located::At(r) = self.locate(key.clone(), true, false).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        let _guard = self.s.leases.lock(&r.key).await;
        let (s, bytes) = (self.s.clone(), bytes.to_vec());
        blocking(move || write_checked(&s, &r, &bytes, Expect::Absent, None, false)).await
    }

    /// Moves a vault entry to `.trash/…`. Leases and journals stay on the old key:
    /// an open editor sees it like an external delete (conflict → save recreates),
    /// so unsaved edits are never moved under an internal key.
    pub async fn trash_entry(&self, from: &DocKey, to: &DocKey) -> R<DocKey> {
        if is_internal(&from.path) || !in_trash(&to.path) {
            return Err(DocumentsError::InvalidInput.into());
        }
        self.trash_move(from, to, false).await
    }
    /// Moves a trashed regular file back to a free, non-internal key.
    pub async fn restore_entry(&self, from: &DocKey, to: &DocKey) -> R<DocKey> {
        if !in_trash(&from.path) || is_internal(&to.path) {
            return Err(DocumentsError::InvalidInput.into());
        }
        self.trash_move(from, to, true).await
    }
    async fn trash_move(&self, from: &DocKey, to: &DocKey, file_only: bool) -> R<DocKey> {
        if from.root != to.root {
            return Err(DocumentsError::InvalidInput.into());
        }
        let Located::At(src) = self.locate_with(from.clone(), false, true, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        match src.kind {
            None => return Err(DocumentsError::NotFound.into()),
            Some(kind) if file_only && kind != FileType::RegularFile => {
                return Err(DocumentsError::InvalidInput.into());
            }
            Some(_) => {}
        }
        // Probe without creating folders, so a refused move leaves nothing behind.
        let probe = self.locate_with(to.clone(), false, true, true).await?;
        let probed = match &probe {
            Located::At(r) => &r.key,
            Located::Missing(k) => k,
        };
        if probed != to {
            return Err(DocumentsError::InvalidInput.into());
        }
        let Located::At(dst) = self.locate_with(to.clone(), true, true, true).await? else {
            return Err(DocumentsError::NotFound.into());
        };
        // Both sides must stay what was asked: a symlinked `.trash` (or target folder)
        // must not redirect the move somewhere else in the root.
        if src.key != *from
            || dst.key != *to
            || dst.key.path.starts_with(&format!("{}/", src.key.path))
        {
            return Err(DocumentsError::InvalidInput.into());
        }
        if dst.kind.is_some() {
            return Err(DocumentsError::Exists.into());
        }
        let (a, b) = (src.key.clone(), dst.key.clone());
        let (first, second) = if a < b { (&a, &b) } else { (&b, &a) };
        let _first = self.s.leases.lock(first).await;
        let _second = self.s.leases.lock(second).await;
        // A restore target may still be open in an editor (deleted, then undone): its
        // lease and journal stay valid because the restored bytes match its base rev.
        // NOREPLACE alone keeps anything that exists at the target.
        blocking(move || {
            sys::move_no_replace(&src.dir, &src.name, &dst.dir, &dst.name).map_err(sys_fail)?;
            Ok(dst.key.clone())
        })
        .await
    }
}
