//! Native-owned local journal. Source access is needed only to seed the root cache.
use crate::core::journal::{JournalEntry, JournalError, JournalFile, JournalView, MAX_FILE_BYTES};
use crate::core::vault::VaultIdentity;
use rusqlite::Connection;
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

const UNAVAILABLE: &str = "journal_unavailable";
static NEXT_BACKUP: AtomicU64 = AtomicU64::new(0);
struct CachedRoot {
    configured: PathBuf,
    canonical: String,
}
pub struct NoteJournalStore {
    path: PathBuf,
    lock: Mutex<()>,
    root: Mutex<Option<CachedRoot>>,
}
enum ReadFailure {
    Corrupt(Vec<u8>),
    Unavailable,
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}
fn issue(error: JournalError) -> String {
    if error == JournalError::Limit {
        "journal_limit"
    } else {
        UNAVAILABLE
    }
    .into()
}
fn configured_root(db: &Mutex<Connection>) -> Result<PathBuf, String> {
    let conn = db.lock().map_err(|_| UNAVAILABLE)?;
    super::docs::resolve_configured_root(&conn).map_err(|_| UNAVAILABLE.into())
}
impl NoteJournalStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
            root: Mutex::new(None),
        }
    }

    /// Called only after native validation, while the caller still owns the selected binding.
    pub(crate) fn remember_root(&self, configured: &Path, canonical: &Path) -> Result<(), String> {
        let canonical = canonical.to_str().ok_or(UNAVAILABLE)?.to_owned();
        *self.root.lock().map_err(|_| UNAVAILABLE)? = Some(CachedRoot {
            configured: configured.into(),
            canonical,
        });
        Ok(())
    }
    fn resolve_with(
        &self,
        db: &Mutex<Connection>,
        inspect: impl FnOnce(&Path) -> Result<String, String>,
    ) -> Result<String, String> {
        let configured = configured_root(db)?;
        if let Some(cache) = self
            .root
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .as_ref()
            .filter(|c| c.configured == configured)
        {
            return Ok(cache.canonical.clone());
        }
        // Neither DB nor cache mutex spans remote source I/O.
        let canonical = inspect(&configured)?;
        let conn = db.lock().map_err(|_| UNAVAILABLE)?;
        if super::docs::resolve_configured_root(&conn).map_err(|_| UNAVAILABLE)? != configured {
            return Err(UNAVAILABLE.into());
        }
        self.remember_root(&configured, Path::new(&canonical))?;
        Ok(canonical)
    }
    pub(crate) fn active_root(&self, db: &Mutex<Connection>) -> Result<String, String> {
        self.resolve_with(db, |root| {
            let vault = VaultIdentity::inspect(root).map_err(|_| UNAVAILABLE)?;
            Ok(vault
                .canonical_path()
                .to_str()
                .ok_or(UNAVAILABLE)?
                .to_owned())
        })
    }
    fn read(&self) -> Result<JournalFile, ReadFailure> {
        let unavailable = |_| ReadFailure::Unavailable;
        match fs::symlink_metadata(&self.path) {
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(JournalFile::default()),
            Err(_) => return Err(ReadFailure::Unavailable),
            Ok(m) if !m.is_file() || m.len() > MAX_FILE_BYTES as u64 => {
                return Err(ReadFailure::Unavailable)
            }
            _ => {}
        }
        devbox_filesystem::ensure_no_links(&self.path).map_err(unavailable)?;
        let (file, identity) =
            devbox_filesystem::open_filesystem_object(&self.path, false).map_err(unavailable)?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(unavailable)?;
        if bytes.len() > MAX_FILE_BYTES
            || devbox_filesystem::filesystem_identity(&self.path, false).map_err(unavailable)?
                != identity
        {
            return Err(ReadFailure::Unavailable);
        }
        JournalFile::decode(&bytes).map_err(|error| match error {
            JournalError::FutureSchema | JournalError::Limit => ReadFailure::Unavailable,
            JournalError::Invalid => ReadFailure::Corrupt(bytes),
        })
    }
    fn preserve_corrupt(&self, bytes: &[u8]) -> Result<(), String> {
        // Exclusive backup creation cannot overwrite an earlier recovery file.
        // Keep the original until atomic publication of the replacement succeeds.
        for _ in 0..32 {
            let suffix = NEXT_BACKUP.fetch_add(1, Ordering::Relaxed);
            let aside = self.path.with_extension(format!(
                "corrupt-{}-{}-{suffix}.json",
                now_ms(),
                std::process::id()
            ));
            match OpenOptions::new().write(true).create_new(true).open(&aside) {
                Ok(mut backup) => {
                    return backup
                        .write_all(bytes)
                        .and_then(|_| backup.sync_all())
                        .map_err(|_| UNAVAILABLE.into())
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(UNAVAILABLE.into()),
            }
        }
        Err(UNAVAILABLE.into())
    }
    fn write(&self, file: &JournalFile) -> Result<(), String> {
        if file.entries.is_empty() {
            return match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
                Err(_) => Err(UNAVAILABLE.into()),
            };
        }
        let bytes = file.encode().map_err(issue)?;
        devbox_filesystem::atomic_write(&self.path, &bytes).map_err(|_| UNAVAILABLE.into())
    }
    pub(crate) fn save(
        &self,
        root: &str,
        path: String,
        content: String,
        revision: String,
    ) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| UNAVAILABLE)?;
        let (mut file, corrupt) = match self.read() {
            Ok(file) => (file, None),
            Err(ReadFailure::Corrupt(bytes)) => (JournalFile::default(), Some(bytes)),
            Err(ReadFailure::Unavailable) => return Err(UNAVAILABLE.into()),
        };
        file.upsert(JournalEntry {
            vault_root: root.into(),
            path,
            content,
            base_revision: revision,
            saved_at_ms: now_ms(),
        })
        .map_err(issue)?;
        if let Some(bytes) = corrupt {
            self.preserve_corrupt(&bytes)?;
        }
        self.write(&file)
    }
    pub(crate) fn clear(&self, root: &str, path: &str) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| UNAVAILABLE)?;
        let mut file = self.read().map_err(|_| UNAVAILABLE)?;
        if file.remove(root, path) {
            self.write(&file)?;
        }
        Ok(())
    }
    pub(crate) fn load(&self, root: &str) -> Result<JournalView, String> {
        let _guard = self.lock.lock().map_err(|_| UNAVAILABLE)?;
        Ok(self.read().map_err(|_| UNAVAILABLE)?.view(root))
    }
    pub(crate) fn discard_other(&self, root: &str) -> Result<(), String> {
        let _guard = self.lock.lock().map_err(|_| UNAVAILABLE)?;
        let mut file = self.read().map_err(|_| UNAVAILABLE)?;
        if file.discard_other(root) {
            self.write(&file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
