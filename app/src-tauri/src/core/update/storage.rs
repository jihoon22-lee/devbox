use super::{R, UpdateError, release::Candidate};
use crate::core::channel::AppChannel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tempfile::{NamedTempFile, TempDir};

#[derive(Serialize, Deserialize)]
struct Marker {
    schema: u8,
    channel: String,
}
pub struct Root {
    path: PathBuf,
    channel: AppChannel,
}
fn safe(path: &Path) -> R<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(UpdateError::Storage);
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(UpdateError::Storage);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(UpdateError::Storage),
        }
    }
    Ok(())
}
impl Root {
    pub fn open(path: &Path, channel: AppChannel) -> R<Self> {
        if !path.is_absolute() {
            return Err(UpdateError::Storage);
        }
        safe(path)?;
        let marker = path.join("owner.json");
        if !path.exists() {
            fs::create_dir_all(path.parent().ok_or(UpdateError::Storage)?)
                .map_err(|_| UpdateError::Storage)?;
            fs::create_dir(path).map_err(|_| UpdateError::Storage)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(|_| UpdateError::Storage)?;
            serde_json::to_writer(
                &mut file,
                &Marker {
                    schema: 1,
                    channel: channel.config().instance.into(),
                },
            )
            .map_err(|_| UpdateError::Storage)?;
            file.sync_all().map_err(|_| UpdateError::Storage)?;
        }
        safe(&marker)?;
        let bytes = bounded(&marker, 1024)?;
        let found: Marker = serde_json::from_slice(&bytes).map_err(|_| UpdateError::Storage)?;
        if found.schema != 1 || found.channel != channel.config().instance {
            return Err(UpdateError::Storage);
        }
        Ok(Self {
            path: path.into(),
            channel,
        })
    }
    fn validate(&self) -> R<()> {
        let found: Marker = serde_json::from_slice(&bounded(&self.path.join("owner.json"), 1024)?)
            .map_err(|_| UpdateError::Storage)?;
        if found.schema != 1 || found.channel != self.channel.config().instance {
            return Err(UpdateError::Storage);
        }
        Ok(())
    }
    pub fn automatic_due(&self, now: u64) -> R<bool> {
        self.validate()?;
        let path = self.path.join("last-check.json");
        safe(&path)?;
        let last = match bounded(&path, 64) {
            Ok(bytes) => serde_json::from_slice::<u64>(&bytes).map_err(|_| UpdateError::Storage)?,
            Err(_) if !path.exists() => 0,
            Err(error) => return Err(error),
        };
        // A future stamp suppresses automatic traffic; an explicit user check remains available.
        if last != 0 && now.saturating_sub(last) < 86400 {
            return Ok(false);
        }
        let mut temp = NamedTempFile::new_in(&self.path).map_err(|_| UpdateError::Storage)?;
        serde_json::to_writer(&mut temp, &now).map_err(|_| UpdateError::Storage)?;
        temp.as_file()
            .sync_all()
            .map_err(|_| UpdateError::Storage)?;
        temp.persist(path).map_err(|_| UpdateError::Storage)?;
        Ok(true)
    }
    pub fn pending(&self) -> R<Pending> {
        self.validate()?;
        let dir = tempfile::Builder::new()
            .prefix("download-")
            .tempdir_in(&self.path)
            .map_err(|_| UpdateError::Storage)?;
        let file = NamedTempFile::new_in(dir.path()).map_err(|_| UpdateError::Storage)?;
        Ok(Pending {
            file,
            dir,
            hash: Sha256::new(),
            bytes: 0,
        })
    }
}
pub fn bounded(path: &Path, cap: u64) -> R<Vec<u8>> {
    safe(path)?;
    let file = File::open(path).map_err(|_| UpdateError::Storage)?;
    if !file.metadata().map_err(|_| UpdateError::Storage)?.is_file() {
        return Err(UpdateError::Storage);
    }
    let mut bytes = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| UpdateError::Storage)?;
    if bytes.len() as u64 > cap {
        return Err(UpdateError::Size);
    }
    Ok(bytes)
}
pub struct Pending {
    file: NamedTempFile,
    dir: TempDir,
    hash: Sha256,
    bytes: u64,
}
impl Pending {
    pub fn push(&mut self, bytes: &[u8], expected: u64) -> R<u64> {
        let total = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or(UpdateError::Size)?;
        if total > expected || total > super::release::MAX_INSTALLER {
            return Err(UpdateError::Size);
        }
        self.file
            .write_all(bytes)
            .map_err(|_| UpdateError::Storage)?;
        self.hash.update(bytes);
        self.bytes = total;
        Ok(total)
    }
    pub fn finish(self, candidate: &Candidate) -> R<ReadyProof> {
        if self.bytes != candidate.size {
            return Err(UpdateError::Size);
        }
        if <[u8; 32]>::from(self.hash.finalize()) != candidate.digest {
            return Err(UpdateError::Digest);
        }
        self.file
            .as_file()
            .sync_all()
            .map_err(|_| UpdateError::Storage)?;
        let path = self.dir.path().join("installer.exe");
        // No replacement, even if an unexpected entry appeared inside our owned directory.
        let sealed = match self.file.persist_noclobber(&path) {
            Ok(file) => file,
            Err(error) => {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    let _ = self.dir.keep();
                }
                return Err(UpdateError::Storage);
            }
        };
        drop(sealed);
        let file = read_locked(&path)?;
        let identity = identity(&file)?;
        let proof = ReadyProof {
            file,
            dir: self.dir,
            identity,
            digest: candidate.digest,
            size: candidate.size,
        };
        proof.revalidate()?;
        Ok(proof)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    volume: u64,
    index: u64,
}
pub(super) fn read_locked(path: &Path) -> R<File> {
    safe(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    options.open(path).map_err(|_| UpdateError::Storage)
}
pub(super) fn identity(file: &File) -> R<Identity> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the borrowed file handle remains alive throughout this query.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| UpdateError::Storage)?;
        Ok(Identity {
            volume: info.dwVolumeSerialNumber as u64,
            index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
        })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().map_err(|_| UpdateError::Storage)?;
        Ok(Identity {
            volume: meta.dev(),
            index: meta.ino(),
        })
    }
}
/// Internal proof, never accepted as an IPC argument. File drops before its owned directory.
pub struct ReadyProof {
    file: File,
    dir: TempDir,
    identity: Identity,
    digest: [u8; 32],
    size: u64,
}
impl ReadyProof {
    /// Installation will call this seam again immediately before committing its permit (S6-3b).
    pub fn revalidate(&self) -> R<()> {
        let mut file = read_locked(&self.dir.path().join("installer.exe"))?;
        if identity(&file)? != self.identity || identity(&self.file)? != self.identity {
            return Err(UpdateError::Storage);
        }
        let mut hash = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = [0; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|_| UpdateError::Storage)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > self.size {
                return Err(UpdateError::Size);
            }
            hash.update(&buffer[..count]);
        }
        if total != self.size {
            return Err(UpdateError::Size);
        }
        if <[u8; 32]>::from(hash.finalize()) != self.digest {
            return Err(UpdateError::Digest);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
