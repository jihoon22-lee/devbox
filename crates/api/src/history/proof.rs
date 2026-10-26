//! Private session receipts pin the bytes this daemon wrote; history files cannot mint export authority.
use super::files;
use crate::HistoryEntry;
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

#[derive(Clone)]
struct FileProof {
    length: usize,
    digest: [u8; 32],
}
impl FileProof {
    fn new(bytes: &[u8]) -> Self {
        Self {
            length: bytes.len(),
            digest: Sha256::digest(bytes).into(),
        }
    }
    fn read(&self, path: &Path, retain: usize) -> io::Result<Vec<u8>> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)?;
        let meta = file.metadata()?;
        if !meta.is_file() || meta.len() != self.length as u64 {
            return Err(invalid());
        }
        let mut hash = Sha256::new();
        let mut kept = Vec::with_capacity(retain.min(self.length));
        let mut total = 0;
        let mut buffer = [0; 64 * 1024];
        loop {
            // One excess byte detects growth without unbounded reads or allocation.
            let room = (self.length - total).saturating_add(1).min(buffer.len());
            let n = file.read(&mut buffer[..room])?;
            if n == 0 {
                break;
            }
            total += n;
            if total > self.length {
                return Err(invalid());
            }
            hash.update(&buffer[..n]);
            let take = n.min(retain.saturating_sub(kept.len()));
            kept.extend_from_slice(&buffer[..take]);
        }
        if total != self.length || <[u8; 32]>::from(hash.finalize()) != self.digest {
            return Err(invalid());
        }
        Ok(kept)
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "response history changed")
}
#[derive(Clone)]
pub(crate) struct Receipt {
    id: String,
    day: String,
    json: FileProof,
    body: Option<FileProof>,
}
impl Receipt {
    pub(crate) fn new(id: String, day: String, json: &[u8], body: &[u8]) -> Self {
        Self {
            id,
            day,
            json: FileProof::new(json),
            body: (body.len() > files::INLINE_BODY).then(|| FileProof::new(body)),
        }
    }
}
pub(crate) fn read(root: &Path, receipt: &Receipt) -> io::Result<HistoryEntry> {
    let bytes = receipt.json.read(
        &files::entry_path(root, &receipt.day, &receipt.id),
        receipt.json.length,
    )?;
    let mut entry: HistoryEntry = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if entry.id != receipt.id {
        return Err(invalid());
    }
    if let Some(body) = &receipt.body {
        let bytes = body.read(
            &files::body_path(root, &receipt.day, &receipt.id),
            files::INLINE_BODY,
        )?;
        entry.body_base64 = data_encoding::BASE64.encode(&bytes);
        entry.body_truncated = true;
    }
    Ok(entry)
}
