//! Durable one-operation suite journal. Lock ownership and filesystem identities
//! are retained across compare/write; a failed write never advances memory state.
use super::delivery::Journal;
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, try_lock_exclusive,
    FilesystemIdentity,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 16 * 1024 * 1024;
type Result<T> = std::result::Result<T, &'static str>;
pub struct Store {
    root: PathBuf,
    root_identity: FilesystemIdentity,
    _root: File,
    lock: File,
    lock_identity: FilesystemIdentity,
}
impl Store {
    /// `parent` comes from the native product's own data namespace only.
    pub fn open(parent: &Path) -> Result<Self> {
        ensure_no_links(parent).map_err(|_| "suite_store_unsafe")?;
        let root = parent.join("suite-delivery-v1");
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("suite_store_unavailable"),
        }
        ensure_no_links(&root).map_err(|_| "suite_store_unsafe")?;
        let (root_handle, root_identity) =
            open_filesystem_object(&root, true).map_err(|_| "suite_store_unavailable")?;
        let path = root.join("owner.lock");
        let lock = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure_no_links(&path).map_err(|_| "suite_lock_unsafe")?;
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|_| "suite_lock_unavailable")?
            }
            Err(_) => return Err("suite_lock_unavailable"),
        };
        let lock_identity = devbox_filesystem::opened_filesystem_identity(&lock, false)
            .map_err(|_| "suite_lock_unsafe")?;
        if !try_lock_exclusive(&lock).map_err(|_| "suite_lock_unavailable")? {
            return Err("suite_update_busy");
        }
        let store = Self {
            root,
            root_identity,
            _root: root_handle,
            lock,
            lock_identity,
        };
        store.revalidate()?;
        Ok(store)
    }
    /// Diagnostics never creates a store or repairs a malformed journal.
    pub fn inspect(parent: &Path) -> Result<Option<(Journal, String)>> {
        let root = parent.join("suite-delivery-v1");
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("suite_store_unavailable"),
            Ok(_) => {}
        }
        Self::open(parent)?.read()
    }
    fn revalidate(&self) -> Result<()> {
        ensure_no_links(&self.root).map_err(|_| "suite_store_changed")?;
        if filesystem_identity(&self.root, true).map_err(|_| "suite_store_changed")?
            != self.root_identity
            || filesystem_identity(self.root.join("owner.lock"), false)
                .map_err(|_| "suite_store_changed")?
                != self.lock_identity
        {
            return Err("suite_store_changed");
        }
        Ok(())
    }
    fn bytes(&self) -> Result<Option<Vec<u8>>> {
        self.revalidate()?;
        let path = self.root.join("journal.json");
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("suite_journal_unavailable"),
            Ok(_) => {}
        }
        ensure_no_links(&path).map_err(|_| "suite_journal_unsafe")?;
        let (file, identity) =
            open_filesystem_object(&path, false).map_err(|_| "suite_journal_unavailable")?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "suite_journal_unavailable")?;
        if bytes.len() as u64 > MAX_BYTES
            || filesystem_identity(&path, false).map_err(|_| "suite_journal_changed")? != identity
        {
            return Err("suite_journal_changed");
        }
        self.revalidate()?;
        Ok(Some(bytes))
    }
    pub fn read(&self) -> Result<Option<(Journal, String)>> {
        self.bytes()?
            .map(|bytes| {
                let journal: Journal =
                    serde_json::from_slice(&bytes).map_err(|_| "suite_journal_invalid")?;
                journal.validate()?;
                Ok((journal, digest(&bytes)))
            })
            .transpose()
    }
    pub fn write(&self, expected_digest: Option<&str>, journal: &Journal) -> Result<String> {
        journal.validate()?;
        let previous = self.read()?;
        if previous.as_ref().map(|(_, hash)| hash.as_str()) != expected_digest {
            return Err("suite_journal_stale");
        }
        if let Some((old, _)) = &previous {
            if old.installation_key != journal.installation_key
                || old.operation_id != journal.operation_id
                || old.candidate != journal.candidate
                || Some(journal.revision) != old.revision.checked_add(1)
            {
                return Err("suite_journal_conflict");
            }
        } else if journal.revision != 0 {
            return Err("suite_journal_conflict");
        }
        let bytes = serde_json::to_vec(journal).map_err(|_| "suite_journal_invalid")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("suite_journal_limit");
        }
        self.revalidate()?;
        devbox_filesystem::atomic_write(self.root.join("journal.json"), &bytes)
            .map_err(|_| "suite_journal_write_failed")?;
        self.revalidate()?;
        Ok(digest(&bytes))
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = devbox_filesystem::unlock_exclusive(&self.lock);
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
