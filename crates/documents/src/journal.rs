//! Unsaved-edit journals in `<data>/journals/`, one (latest) per document key.
//! Writes are atomic (temp + fsync + rename + directory fsync).
use crate::model::{DiskRev, DocKey};
use crate::sys::hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalFile {
    pub key: DocKey,
    pub seq: u64,
    pub base_rev: Option<DiskRev>,
    pub text: String,
    pub saved_ms: i64,
}

pub struct JournalStore {
    dir: PathBuf,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl JournalStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    fn name(key: &DocKey) -> String {
        let mut h = Sha256::new();
        h.update(key.root.as_bytes());
        h.update([0]);
        h.update(key.path.as_bytes());
        format!("{}.json", hex(&h.finalize()))
    }
    pub fn load(&self, key: &DocKey) -> io::Result<Option<JournalFile>> {
        match std::fs::read(self.dir.join(Self::name(key))) {
            Ok(bytes) => Ok(serde_json::from_slice::<JournalFile>(&bytes)
                .ok()
                .filter(|j| &j.key == key)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    pub fn create(key: &DocKey, seq: u64, base_rev: Option<DiskRev>, text: String) -> JournalFile {
        JournalFile {
            key: key.clone(),
            seq,
            base_rev,
            text,
            saved_ms: now_ms(),
        }
    }
    pub fn write(&self, journal: &JournalFile) -> io::Result<()> {
        crate::ledger::private_dir(&self.dir)?;
        let name = Self::name(&journal.key);
        let tmp = self
            .dir
            .join(format!(".{name}.{}", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(&serde_json::to_vec(journal)?)?;
            file.sync_all()?;
            std::fs::rename(&tmp, self.dir.join(&name))?;
            std::fs::File::open(&self.dir)?.sync_all()
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
    pub fn remove(&self, key: &DocKey) -> io::Result<()> {
        match std::fs::remove_file(self.dir.join(Self::name(key))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => std::fs::File::open(&self.dir)
                .and_then(|d| d.sync_all())
                .or(Ok(())),
        }
    }
    /// Compare-and-delete: removes the journal only when its seq is `<= upto`.
    pub fn drop_upto(&self, key: &DocKey, upto: u64) -> io::Result<bool> {
        match self.load(key)? {
            Some(j) if j.seq <= upto => self.remove(key).map(|()| true),
            _ => Ok(false),
        }
    }
    pub fn list(&self) -> io::Result<Vec<JournalFile>> {
        let read = match std::fs::read_dir(&self.dir) {
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut out = vec![];
        for entry in read.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }
            if let Ok(bytes) = std::fs::read(entry.path())
                && let Ok(journal) = serde_json::from_slice::<JournalFile>(&bytes)
                && Self::name(&journal.key) == name
            {
                out.push(journal);
            }
        }
        Ok(out)
    }
}
