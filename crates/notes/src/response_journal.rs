//! Small durable intent/attempt/commit receipts; no response body is stored here.
use crate::response_append::Selection;
use crate::{AppendResponseNote, ResponseNoteAppended};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

const MAX_RECEIPT: u64 = 16 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum Phase {
    Pending,
    Attempted,
    Committed { result: ResponseNoteAppended },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub version: u32,
    pub request: AppendResponseNote,
    pub target: Selection,
    pub digest: String,
    pub phase: Phase,
}
fn file(dir: &Path, op: &str) -> PathBuf {
    dir.join(format!("{op}.json"))
}
pub(crate) fn read(dir: &Path, op: &str) -> io::Result<Option<Receipt>> {
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(file(dir, op))
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !f.metadata()?.is_file() {
        return Err(io::Error::other("append receipt is not a regular file"));
    }
    let mut bytes = Vec::new();
    f.take(MAX_RECEIPT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT {
        return Err(io::Error::other("append receipt too large"));
    }
    let record: Receipt = serde_json::from_slice(&bytes)?;
    if record.version != 1 || record.request.operation_id != op {
        return Err(io::Error::other("append receipt invalid"));
    }
    Ok(Some(record))
}
pub(crate) fn write(dir: &Path, receipt: &Receipt) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
        return Err(io::Error::other("append receipt directory invalid"));
    }
    let bytes = serde_json::to_vec(receipt)?;
    if bytes.len() as u64 > MAX_RECEIPT {
        return Err(io::Error::other("append receipt too large"));
    }
    let tmp = dir.join(format!(".{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, file(dir, &receipt.request.operation_id))?;
        std::fs::File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
