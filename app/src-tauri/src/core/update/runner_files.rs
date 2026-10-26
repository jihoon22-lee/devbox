//! Owned staging and bounded handoff records, shared by the native runner and filesystem tests.
use super::{Binding, ProcessIdentity, Result, RunnerError};
use crate::core::{
    channel::AppChannel,
    update::storage::{self, Root, SealedSource},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tempfile::{NamedTempFile, TempDir};
use tokio_util::sync::CancellationToken;

pub const EXE_LIMIT: u64 = 512 * 1024 * 1024;
pub const RECORD_LIMIT: u64 = 32 * 1024;
pub fn locked(path: &Path) -> Result<File> {
    storage::read_locked(path).map_err(|_| RunnerError::Storage)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileProof {
    pub volume: u64,
    pub index: u64,
    pub size: u64,
    pub digest: [u8; 32],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: u8,
    pub binding: Binding,
    pub channel: String,
    pub nonce: String,
    pub parent: ProcessIdentity,
    pub parent_exe: PathBuf,
    pub runner_exe: PathBuf,
    pub installer: PathBuf,
    pub installer_proof: FileProof,
    pub runner_proof: FileProof,
    pub source_digest: [u8; 32],
}
impl Manifest {
    pub fn validate(&self, channel: AppChannel, actual_runner: &Path) -> Result<()> {
        let dir = self.runner_exe.parent().ok_or(RunnerError::Identity)?;
        let install_dir = self.parent_exe.parent().ok_or(RunnerError::Identity)?;
        if self.schema != 1
            || !self.binding.valid()
            || channel == AppChannel::Dev
            || self.channel != channel.config().instance
            || !self.parent_exe.is_absolute()
            || !self.runner_exe.is_absolute()
            || self.runner_exe != actual_runner
            || self.runner_exe.file_name().and_then(|v| v.to_str()) != Some("runner.exe")
            || self.installer != dir.join("installer.exe")
            || dir.starts_with(install_dir)
            || dir.file_name().and_then(|v| v.to_str()) != Some(self.nonce.as_str())
            || !self.nonce.starts_with("install-")
            || self.nonce.len() > 128
            || self.parent.pid == 0
            || self.parent.started == 0
            || self.installer_proof.size == 0
            || self.installer_proof.size > super::super::release::MAX_INSTALLER
            || self.runner_proof.size == 0
            || self.runner_proof.size > EXE_LIMIT
            || self.runner_proof.digest != self.source_digest
            || (channel == AppChannel::Prod
                && super::super::release::stable(&self.binding.target_version).is_none())
            || (channel == AppChannel::Prod && self.binding.fixture_install)
            || (channel == AppChannel::Accept
                && (!cfg!(any(test, devbox_update_fixture)) || !self.binding.fixture_install))
        {
            return Err(RunnerError::Identity);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Armed,
    Committed,
    Running,
    Installed,
    Failed,
    Unknown,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u8,
    pub nonce: String,
    pub binding: Binding,
    pub channel: String,
    pub runner: ProcessIdentity,
    pub outcome: Outcome,
    pub installer: Option<ProcessIdentity>,
    pub error: Option<RunnerError>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpawnRecord {
    pub nonce: String,
    pub runner: ProcessIdentity,
}

struct OwnedDir {
    dir: Option<TempDir>,
    path: PathBuf,
    files: BTreeMap<OsString, storage::Identity>,
    manifest: Option<Manifest>,
}
impl OwnedDir {
    fn new(root: &Path) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(root)
            .map_err(|_| RunnerError::Storage)?;
        let path = fs::canonicalize(dir.path()).map_err(|_| RunnerError::Storage)?;
        Ok(Self {
            dir: Some(dir),
            path,
            files: BTreeMap::new(),
            manifest: None,
        })
    }
    fn claim(&mut self, path: &Path, file: &File) -> Result<()> {
        if path.parent() != Some(self.path.as_path()) {
            return Err(RunnerError::Identity);
        }
        self.files.insert(
            path.file_name().ok_or(RunnerError::Identity)?.into(),
            storage::identity(file).map_err(|_| RunnerError::Identity)?,
        );
        Ok(())
    }
    fn matches(&self, path: &Path, file: &File) -> Result<bool> {
        Ok(self
            .files
            .get(path.file_name().ok_or(RunnerError::Identity)?)
            == Some(&storage::identity(file).map_err(|_| RunnerError::Identity)?))
    }
    fn keep(mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = dir.keep();
        }
    }
    fn can_clean(&self) -> Result<bool> {
        for (index, entry) in fs::read_dir(&self.path)
            .map_err(|_| RunnerError::Storage)?
            .enumerate()
        {
            if index >= 5 {
                return Ok(false);
            }
            let path = entry.map_err(|_| RunnerError::Storage)?.path();
            let file = locked(&path)?;
            if !file.metadata().map_err(|_| RunnerError::Storage)?.is_file() {
                return Ok(false);
            }
            if self
                .files
                .contains_key(path.file_name().ok_or(RunnerError::Identity)?)
            {
                if !self.matches(&path, &file)? {
                    return Ok(false);
                }
                continue;
            }
            let Some(manifest) = &self.manifest else {
                return Ok(false);
            };
            let bytes = storage::bounded(&path, RECORD_LIMIT).map_err(|_| RunnerError::Storage)?;
            if path.file_name().is_some_and(|name| name == "spawn.json") {
                let value: SpawnRecord =
                    serde_json::from_slice(&bytes).map_err(|_| RunnerError::Storage)?;
                if value.nonce != manifest.nonce
                    || value.runner.pid == 0
                    || value.runner.started == 0
                {
                    return Ok(false);
                }
            } else if path.file_name().is_some_and(|name| name == "outcome.json") {
                let value: Record =
                    serde_json::from_slice(&bytes).map_err(|_| RunnerError::Storage)?;
                if value.schema != 1
                    || value.nonce != manifest.nonce
                    || value.binding != manifest.binding
                    || value.channel != manifest.channel
                {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
impl Drop for OwnedDir {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            if self.can_clean().unwrap_or(false) {
                drop(dir);
            } else {
                let _ = dir.keep();
            }
        }
    }
}

// Open handles must drop before the temporary directory on Windows.
pub struct Stage {
    pub installer: File,
    pub runner: File,
    pub payload: File,
    pub manifest: Manifest,
    dir: Option<OwnedDir>,
}
impl Stage {
    pub fn create(
        root: &Path,
        parent_exe: &Path,
        mut source: SealedSource,
        binding: Binding,
        channel: AppChannel,
        parent: ProcessIdentity,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        if !binding.valid() {
            return Err(RunnerError::InvalidBinding);
        }
        if cancel.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        if source.size == 0 || source.size > super::super::release::MAX_INSTALLER {
            return Err(RunnerError::Identity);
        }
        Root::open(root, channel).map_err(|_| RunnerError::Storage)?;
        let source_identity = storage::identity(&source.file).map_err(|_| RunnerError::Identity)?;
        if source_identity != source.identity || !source.path.is_absolute() {
            return Err(RunnerError::Identity);
        }
        let mut dir = OwnedDir::new(root)?;
        let directory = dir.path.clone();
        let parent_exe = fs::canonicalize(parent_exe).map_err(|_| RunnerError::Identity)?;
        let installer_path = directory.join("installer.exe");
        let installer = copy(
            &mut source.file,
            &installer_path,
            source.size,
            Some(source.digest),
            cancel,
            &mut dir,
        )?;
        let installer_proof = proof(&installer, source.size, cancel)?;
        if installer_proof.digest != source.digest {
            return Err(RunnerError::Identity);
        }
        let runner_path = directory.join("runner.exe");
        let mut old_exe = storage::read_locked(&parent_exe).map_err(|_| RunnerError::Storage)?;
        let exe_size = old_exe.metadata().map_err(|_| RunnerError::Storage)?.len();
        if exe_size == 0 || exe_size > EXE_LIMIT {
            return Err(RunnerError::Storage);
        }
        let runner = copy(&mut old_exe, &runner_path, exe_size, None, cancel, &mut dir)?;
        let runner_proof = proof(&runner, EXE_LIMIT, cancel)?;
        let manifest = Manifest {
            schema: 1,
            binding,
            channel: channel.config().instance.into(),
            nonce: directory
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or(RunnerError::Storage)?
                .into(),
            parent,
            parent_exe,
            runner_exe: runner_path,
            installer: installer_path,
            installer_proof,
            source_digest: runner_proof.digest,
            runner_proof,
        };
        manifest.validate(channel, &manifest.runner_exe)?;
        let owner = directory.join("owner.json");
        let owner_bytes = serde_json::to_vec(&manifest).map_err(|_| RunnerError::Storage)?;
        if owner_bytes.len() as u64 > RECORD_LIMIT {
            return Err(RunnerError::Storage);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&owner)
            .map_err(|_| RunnerError::Storage)?;
        dir.claim(&owner, &file)?;
        file.write_all(&owner_bytes)
            .map_err(|_| RunnerError::Storage)?;
        file.sync_all().map_err(|_| RunnerError::Storage)?;
        drop(file);
        let payload = storage::read_locked(&owner).map_err(|_| RunnerError::Storage)?;
        if !dir.matches(&owner, &payload)? {
            return Err(RunnerError::Identity);
        }
        dir.manifest = Some(manifest.clone());
        Ok(Self {
            installer,
            runner,
            payload,
            manifest,
            dir: Some(dir),
        })
    }
    pub fn keep(&mut self) {
        if let Some(dir) = self.dir.take() {
            dir.keep();
        }
    }
    pub fn revalidate(&self, cancel: &CancellationToken) -> Result<()> {
        verify(&self.installer, &self.manifest.installer_proof, cancel)?;
        let current =
            storage::read_locked(&self.manifest.installer).map_err(|_| RunnerError::Identity)?;
        verify(&current, &self.manifest.installer_proof, cancel)?;
        verify(&self.runner, &self.manifest.runner_proof, cancel)?;
        Ok(())
    }
}
fn copy(
    source: &mut File,
    target: &Path,
    size: u64,
    expected: Option<[u8; 32]>,
    cancel: &CancellationToken,
    dir: &mut OwnedDir,
) -> Result<File> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| RunnerError::Storage)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| RunnerError::Storage)?;
    dir.claim(target, &output)?;
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0; 65536];
    loop {
        if cancel.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        let count = source.read(&mut buffer).map_err(|_| RunnerError::Storage)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > size {
            return Err(RunnerError::Identity);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| RunnerError::Storage)?;
        hash.update(&buffer[..count]);
    }
    let digest: [u8; 32] = hash.finalize().into();
    if total != size || expected.is_some_and(|v| v != digest) {
        return Err(RunnerError::Identity);
    }
    output.sync_all().map_err(|_| RunnerError::Storage)?;
    drop(output);
    let frozen = storage::read_locked(target).map_err(|_| RunnerError::Storage)?;
    if !dir.matches(target, &frozen)? {
        return Err(RunnerError::Identity);
    }
    if proof(&frozen, size, cancel)?.digest != digest {
        return Err(RunnerError::Identity);
    }
    Ok(frozen)
}
pub fn proof(file: &File, cap: u64, cancel: &CancellationToken) -> Result<FileProof> {
    let id = storage::identity(file).map_err(|_| RunnerError::Identity)?;
    let mut reader = file.try_clone().map_err(|_| RunnerError::Storage)?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| RunnerError::Storage)?;
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 65536];
    loop {
        if cancel.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        let count = reader.read(&mut buffer).map_err(|_| RunnerError::Storage)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > cap {
            return Err(RunnerError::Identity);
        }
        hash.update(&buffer[..count]);
    }
    Ok(FileProof {
        volume: id.volume,
        index: id.index,
        size,
        digest: hash.finalize().into(),
    })
}
pub fn verify(file: &File, expected: &FileProof, cancel: &CancellationToken) -> Result<()> {
    if &proof(file, expected.size, cancel)? != expected {
        return Err(RunnerError::Identity);
    }
    Ok(())
}
pub fn read_manifest(file: &mut File) -> Result<Manifest> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| RunnerError::Storage)?;
    let mut bytes = Vec::new();
    file.take(RECORD_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RunnerError::Storage)?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(RunnerError::Identity);
    }
    serde_json::from_slice(&bytes).map_err(|_| RunnerError::Identity)
}
pub fn read_record(path: &Path) -> Result<Record> {
    let bytes = storage::bounded(path, RECORD_LIMIT).map_err(|_| RunnerError::Storage)?;
    serde_json::from_slice(&bytes).map_err(|_| RunnerError::Identity)
}
pub fn write_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| RunnerError::Storage)?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(RunnerError::Storage);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| RunnerError::Storage)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| RunnerError::Storage)
}
pub fn record(
    manifest: &Manifest,
    runner: ProcessIdentity,
    outcome: Outcome,
    installer: Option<ProcessIdentity>,
    error: Option<RunnerError>,
) -> Result<()> {
    let directory = manifest.runner_exe.parent().ok_or(RunnerError::Storage)?;
    // Each writer holds the immutable owner handle; record updates touch only this operation directory.
    let mut temp = NamedTempFile::new_in(directory).map_err(|_| RunnerError::Storage)?;
    serde_json::to_writer(
        &mut temp,
        &Record {
            schema: 1,
            nonce: manifest.nonce.clone(),
            binding: manifest.binding.clone(),
            channel: manifest.channel.clone(),
            runner,
            outcome,
            installer,
            error,
        },
    )
    .map_err(|_| RunnerError::Storage)?;
    temp.as_file()
        .sync_all()
        .map_err(|_| RunnerError::Storage)?;
    temp.persist(directory.join("outcome.json"))
        .map_err(|_| RunnerError::Storage)?;
    Ok(())
}

#[cfg(test)]
#[path = "runner_files_tests.rs"]
pub(crate) mod tests;
