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
    io::{Read, Write},
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
    pub fn archived(&self, operation_id: &str) -> Result<Journal> {
        if uuid::Uuid::parse_str(operation_id).is_err() {
            return Err("suite_archive_invalid");
        }
        self.revalidate()?;
        let path = self.root.join(format!("completed-{operation_id}.json"));
        ensure_no_links(&path).map_err(|_| "suite_archive_unsafe")?;
        let (file, identity) =
            open_filesystem_object(&path, false).map_err(|_| "suite_archive_unavailable")?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "suite_archive_unavailable")?;
        if bytes.len() as u64 > MAX_BYTES
            || filesystem_identity(&path, false).map_err(|_| "suite_archive_changed")? != identity
        {
            return Err("suite_archive_changed");
        }
        let journal: Journal =
            serde_json::from_slice(&bytes).map_err(|_| "suite_archive_invalid")?;
        journal.validate()?;
        if journal.operation_id != operation_id
            || !matches!(
                journal.phase,
                super::delivery::Phase::Complete | super::delivery::Phase::Recovered
            )
        {
            return Err("suite_archive_invalid");
        }
        self.revalidate()?;
        Ok(journal)
    }
    /// Start a later operation only after the previous operation has settled.
    /// Archive first, then replace the current journal: an interrupted archive
    /// is harmless and a retry must observe identical bytes, never overwrite it.
    pub fn begin(&self, expected_digest: Option<&str>, journal: &Journal) -> Result<String> {
        use super::delivery::Phase;
        journal.validate()?;
        if journal.revision != 0 || journal.phase != Phase::Inventory {
            return Err("suite_journal_conflict");
        }
        let previous = self.read()?;
        if previous.as_ref().map(|(_, hash)| hash.as_str()) != expected_digest {
            return Err("suite_journal_stale");
        }
        let Some((old, _)) = previous else {
            if journal.previous.is_some() {
                return Err("suite_previous_journal_missing");
            }
            return self.write(None, journal);
        };
        if !matches!(old.phase, Phase::Complete | Phase::Recovered)
            || old.operation_id == journal.operation_id
            || old.installation_key != journal.installation_key
            || old.candidate.installation_id != journal.candidate.installation_id
            || old.candidate.generation == journal.candidate.generation
        {
            return Err("suite_previous_operation_unsettled");
        }
        let active = if old.committed {
            Some(&old.candidate)
        } else {
            old.previous.as_ref()
        };
        if journal.previous.as_ref() != active {
            return Err("suite_previous_generation_changed");
        }
        let bytes = self.bytes()?.ok_or("suite_journal_changed")?;
        let archive = self
            .root
            .join(format!("completed-{}.json", old.operation_id));
        match fs::symlink_metadata(&archive) {
            Ok(_) => {
                ensure_no_links(&archive).map_err(|_| "suite_archive_unsafe")?;
                let (file, identity) = open_filesystem_object(&archive, false)
                    .map_err(|_| "suite_archive_unavailable")?;
                let mut saved = Vec::new();
                file.take(MAX_BYTES + 1)
                    .read_to_end(&mut saved)
                    .map_err(|_| "suite_archive_unavailable")?;
                if saved != bytes
                    || filesystem_identity(&archive, false).map_err(|_| "suite_archive_changed")?
                        != identity
                {
                    return Err("suite_archive_changed");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Publish without replacing any existing record. A crash leaves
                // either a private pending file or the complete linked archive.
                let pending = self
                    .root
                    .join(format!("archive-{}.pending", uuid::Uuid::new_v4()));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&pending)
                    .map_err(|_| "suite_archive_write_failed")?;
                file.write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|_| "suite_archive_write_failed")?;
                drop(file);
                fs::hard_link(&pending, &archive).map_err(|_| "suite_archive_write_failed")?;
                // This is only our private publication name, not backup data.
                fs::remove_file(&pending).map_err(|_| "suite_archive_write_failed")?;
            }
            Err(_) => return Err("suite_archive_unavailable"),
        }
        self.revalidate()?;
        let bytes = serde_json::to_vec(journal).map_err(|_| "suite_journal_invalid")?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("suite_journal_limit");
        }
        devbox_filesystem::atomic_write(self.root.join("journal.json"), &bytes)
            .map_err(|_| "suite_journal_write_failed")?;
        self.revalidate()?;
        Ok(digest(&bytes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::delivery::{Phase, Proof};
    use product_contract::installation::{Manifest, Member, PRODUCTS};
    fn journal(generation: &str, previous: Option<Manifest>) -> Journal {
        Journal::begin(
            generation.into(),
            "a".repeat(64),
            previous,
            Manifest {
                schema_version: 1,
                installation_id: "fixture".into(),
                generation: generation.into(),
                suite_version: "0.8.0".into(),
                protocol_version: 1,
                members: PRODUCTS
                    .iter()
                    .map(|product| Member {
                        product: (*product).into(),
                        sha256: "b".repeat(64),
                        executable: format!(
                            "generations/{generation}/products/{product}/devbox-{product}.exe"
                        ),
                    })
                    .collect(),
            },
        )
        .unwrap()
    }
    #[test]
    fn new_operation_preserves_completed_journal_and_rejects_unsettled_or_wrong_previous() {
        let root = std::env::temp_dir().join(format!("devbox-journal-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let store = Store::open(&root).unwrap();
        let mut old = journal("first", None);
        let mut hash = store.begin(None, &old).unwrap();
        let next = journal("second", Some(old.candidate.clone()));
        assert_eq!(
            store.begin(Some(&hash), &next),
            Err("suite_previous_operation_unsettled")
        );
        while old.phase != Phase::Complete {
            old.advance(
                old.revision,
                Proof {
                    phase: old.phase,
                    generation: "first".into(),
                    revision: "c".repeat(64),
                },
            )
            .unwrap();
            hash = store.write(Some(&hash), &old).unwrap();
        }
        assert_eq!(
            store.begin(Some(&hash), &journal("second", None)),
            Err("suite_previous_generation_changed")
        );
        // Simulate a crash after the archive was published but before rotation.
        let archived = store.bytes().unwrap().unwrap();
        fs::write(store.root.join("completed-first.json"), &archived).unwrap();
        let current_hash = store.begin(Some(&hash), &next).unwrap();
        assert_eq!(store.read().unwrap().unwrap().0, next);
        assert_eq!(
            fs::read(store.root.join("completed-first.json")).unwrap(),
            archived
        );
        assert_eq!(store.begin(Some(&hash), &next), Err("suite_journal_stale"));
        assert_eq!(store.read().unwrap().unwrap().1, current_hash);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recovered_first_install_can_restart_without_replacing_data_or_reusing_its_slot() {
        let root = std::env::temp_dir().join(format!("devbox-restart-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("imported-user-data.json"),
            b"retained fixture data",
        )
        .unwrap();
        let store = Store::open(&root).unwrap();
        let first = uuid::Uuid::new_v4().to_string();
        let mut old = journal(&first, None);
        let mut hash = store.begin(None, &old).unwrap();
        old.fail(old.revision, "installation_cancelled").unwrap();
        hash = store.write(Some(&hash), &old).unwrap();
        old.advance(
            old.revision,
            Proof {
                phase: Phase::Recover,
                generation: first.clone(),
                revision: "d".repeat(64),
            },
        )
        .unwrap();
        hash = store.write(Some(&hash), &old).unwrap();
        assert!(store.begin(Some(&hash), &journal(&first, None)).is_err());
        let next = journal(&uuid::Uuid::new_v4().to_string(), None);
        store.begin(Some(&hash), &next).unwrap();
        assert_eq!(store.archived(&first).unwrap(), old);
        assert!(store.archived("../journal").is_err());
        assert_eq!(
            fs::read(root.join("imported-user-data.json")).unwrap(),
            b"retained fixture data"
        );
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
}
