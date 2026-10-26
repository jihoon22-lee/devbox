//! Compiled only for unit tests or the validated accept channel; never a production URL override.
use super::{
    R, UpdateError,
    release::{self, Candidate, MAX_METADATA},
    storage::{self, Identity, Pending, ReadyProof},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Marker {
    schema: u8,
    channel: String,
    source_sha: String,
    current_version: String,
    metadata_sha256: String,
    installer_sha256: String,
}
pub struct Fixture {
    pub current_version: String,
    metadata: Vec<u8>,
    path: PathBuf,
    identity: Identity,
    _file: File,
    #[cfg(test)]
    pub delay: std::time::Duration,
}
impl Fixture {
    pub fn open(path: &Path, expected_source: &str) -> R<Self> {
        if !path.is_absolute()
            || expected_source.len() != 40
            || !expected_source.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(UpdateError::InvalidRelease);
        }
        let marker: Marker =
            serde_json::from_slice(&storage::bounded(&path.join("owner.json"), 2048)?)
                .map_err(|_| UpdateError::InvalidRelease)?;
        if marker.schema != 1
            || marker.channel != "accept"
            || marker.source_sha != expected_source
            || release::stable(&marker.current_version).is_none()
        {
            return Err(UpdateError::InvalidRelease);
        }
        let metadata = storage::bounded(&path.join("metadata.json"), MAX_METADATA as u64)?;
        if <[u8; 32]>::from(Sha256::digest(&metadata)) != release::digest(&marker.metadata_sha256)?
        {
            return Err(UpdateError::Digest);
        }
        let candidate = release::select(&metadata, &marker.current_version)?
            .ok_or(UpdateError::InvalidRelease)?;
        if candidate.digest != release::digest(&marker.installer_sha256)? {
            return Err(UpdateError::Digest);
        }
        let path = path.join("installer.exe");
        let file = storage::read_locked(&path)?;
        let identity = storage::identity(&file)?;
        let mut reader = storage::read_locked(&path)?;
        let mut bytes = 0;
        let mut hash = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = reader.read(&mut buffer).map_err(|_| UpdateError::Storage)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > candidate.size {
                return Err(UpdateError::Size);
            }
            hash.update(&buffer[..n]);
        }
        if bytes != candidate.size {
            return Err(UpdateError::Size);
        }
        if <[u8; 32]>::from(hash.finalize()) != candidate.digest {
            return Err(UpdateError::Digest);
        }
        Ok(Self {
            current_version: marker.current_version,
            metadata,
            path,
            identity,
            _file: file,
            #[cfg(test)]
            delay: std::time::Duration::ZERO,
        })
    }
    pub async fn metadata(&self, token: &CancellationToken) -> R<Vec<u8>> {
        if token.is_cancelled() {
            return Err(UpdateError::Cancelled);
        }
        Ok(self.metadata.clone())
    }
    pub async fn download(
        &self,
        candidate: &Candidate,
        mut pending: Pending,
        token: &CancellationToken,
        progress: impl Fn(u64),
    ) -> R<ReadyProof> {
        let mut file = storage::read_locked(&self.path)?;
        if storage::identity(&file)? != self.identity {
            return Err(UpdateError::Storage);
        }
        let mut buffer = [0; 65536];
        loop {
            if token.is_cancelled() {
                return Err(UpdateError::Cancelled);
            }
            let n = file.read(&mut buffer).map_err(|_| UpdateError::Storage)?;
            if n == 0 {
                break;
            }
            progress(pending.push(&buffer[..n], candidate.size)?);
            // Unit tests can delay a live writer; the single worker must wait for its actual cleanup.
            #[cfg(test)]
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            tokio::task::yield_now().await;
        }
        if token.is_cancelled() {
            return Err(UpdateError::Cancelled);
        }
        pending.finish(candidate)
    }
}
