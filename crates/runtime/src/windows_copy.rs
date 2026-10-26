//! Owned Windows copy identity and content revisions. Extra target files are never removed.
use crate::{
    ExecutionConfig,
    windows::{
        SyncReport, WindowsError, reject_links, target_directory, valid_component, windows_to_wsl,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

const MARKER: &str = ".devbox-owner.json";
const MAX_LIST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FILES: usize = 65_536;
fn hash(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CopyIdentity {
    pub key: String,
    pub owner: String,
    pub project_id: String,
    pub worktree_id: String,
    pub source: PathBuf,
    pub base: String,
    pub mapped_base: PathBuf,
}
impl CopyIdentity {
    pub fn new(
        execution: &ExecutionConfig,
        project: &str,
        worktree: &str,
        source: &Path,
    ) -> Result<Self, WindowsError> {
        let root = execution
            .windows_root
            .as_deref()
            .ok_or(WindowsError::Path)?;
        if !valid_component(project) || worktree.is_empty() {
            return Err(WindowsError::Path);
        }
        let owner = execution.windows_owner.clone();
        let namespace = target_directory(
            &target_directory(root, ".devbox-v1")?,
            &hash(&[&owner])[..32],
        )?;
        let project_dir = target_directory(&namespace, project)?;
        let component = if worktree == "main" {
            "main".to_owned()
        } else {
            format!("worktree-{}", &hash(&[worktree])[..32])
        };
        let base = target_directory(&project_dir, &component)?;
        let key = hash(&[&base.to_ascii_lowercase(), &owner, project, worktree]);
        let mapped_base = windows_to_wsl(&base)?;
        Ok(Self {
            key,
            owner,
            project_id: project.into(),
            worktree_id: worktree.into(),
            source: source.into(),
            base,
            mapped_base,
        })
    }
    pub fn directory(&self) -> String {
        format!("{}\\files", self.base)
    }
    pub fn control_directory(&self) -> String {
        format!("{}\\control", self.base)
    }
    pub fn metadata_paths(&self) -> Vec<String> {
        let mut paths = vec![
            self.directory(),
            self.control_directory(),
            format!("{}\\{MARKER}", self.control_directory()),
        ];
        let mut part = self.base.as_str();
        for _ in 0..4 {
            paths.push(part.to_owned());
            paths.push(format!("{part}\\{MARKER}"));
            if let Some((parent, _)) = part.rsplit_once('\\') {
                part = parent;
            }
        }
        paths
    }
    pub fn target(&self) -> Result<PathBuf, WindowsError> {
        Ok(self.mapped_base.join("files"))
    }
    pub fn check_source(&self) -> Result<(), WindowsError> {
        reject_links(&self.source)?;
        if fs::canonicalize(&self.source)? != self.source {
            return Err(WindowsError::Path);
        }
        Ok(())
    }
    pub fn ensure_owned(&self) -> Result<(), WindowsError> {
        self.check_source()?;
        let base = self.mapped_base.clone();
        let project = base.parent().ok_or(WindowsError::Path)?;
        let owner = project.parent().ok_or(WindowsError::Path)?;
        let namespace = owner.parent().ok_or(WindowsError::Path)?;
        ensure_marker(namespace, &("devbox-windows-copies", 1))?;
        ensure_marker(owner, &(1, &self.owner))?;
        ensure_marker(project, &(1, &self.owner, &self.project_id))?;
        ensure_marker(&base, &(1, self))?;
        safe_dir(&base.join("files"))?;
        ensure_marker(&base.join("control"), &(1, &self.key))?;
        Ok(())
    }
}
pub(crate) fn safe_dir(path: &Path) -> Result<(), WindowsError> {
    reject_links(path)?;
    if let Ok(meta) = fs::symlink_metadata(path)
        && !meta.is_dir()
    {
        return Err(WindowsError::Path);
    }
    fs::create_dir_all(path)?;
    reject_links(path)?;
    Ok(())
}
fn marker_matches(marker: &Path, expected: &[u8]) -> Result<bool, WindowsError> {
    let meta = match fs::symlink_metadata(marker) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() > 16 * 1024
        || fs::read(marker)? != expected
    {
        return Err(WindowsError::Path);
    }
    Ok(true)
}
fn ensure_marker(path: &Path, value: &impl Serialize) -> Result<(), WindowsError> {
    reject_links(path)?;
    let marker = path.join(MARKER);
    let expected = serde_json::to_vec(value).map_err(|_| WindowsError::Path)?;
    if expected.len() > 16 * 1024 {
        return Err(WindowsError::Limit);
    }
    if marker_matches(&marker, &expected)? {
        return Ok(());
    }
    if path.exists() && fs::read_dir(path)?.next().is_some() {
        // A peer may have finished the same marker after our first missing read.
        return if marker_matches(&marker, &expected)? {
            Ok(())
        } else {
            Err(WindowsError::Path)
        };
    }
    safe_dir(path)?;
    if marker_matches(&marker, &expected)? {
        return Ok(());
    }
    if fs::read_dir(path)?.next().is_some() {
        return if marker_matches(&marker, &expected)? {
            Ok(())
        } else {
            Err(WindowsError::Path)
        };
    }
    // Keep the temporary outside this not-yet-owned directory. Peer initializers
    // see either no marker or the complete immutable marker, never partial JSON.
    let parent = path.parent().ok_or(WindowsError::Path)?;
    let temp = Temporary(parent.join(format!(
        ".devbox-marker-{}.tmp",
        uuid::Uuid::new_v4().simple()
    )));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp.0)?;
    file.write_all(&expected)?;
    file.sync_all()?;
    drop(file);
    match fs::hard_link(&temp.0, &marker) {
        Ok(()) => {
            File::open(path)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if !marker_matches(&marker, &expected)? {
                return Err(WindowsError::Path);
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct InputFile {
    pub path: PathBuf,
    hash: String,
    size: u64,
    modified: SystemTime,
}
#[derive(Debug, Clone)]
pub(crate) struct CopyManifest {
    pub revision: String,
    pub files: Vec<InputFile>,
    pub skipped: u64,
}
fn cancelled(cancel: &CancellationToken) -> Result<(), WindowsError> {
    if cancel.is_cancelled() {
        Err(WindowsError::Changed)
    } else {
        Ok(())
    }
}
async fn admitted(source: &Path) -> Result<Vec<PathBuf>, WindowsError> {
    let mut child = tokio::process::Command::new("git")
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(source)
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or(WindowsError::Git)?
        .take(MAX_LIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 > MAX_LIST_BYTES {
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(WindowsError::Limit);
    }
    if !child.wait().await?.success() {
        return Err(WindowsError::Git);
    }
    let mut paths = std::collections::BTreeSet::new();
    let mut casing = std::collections::HashMap::new();
    for item in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let name = std::str::from_utf8(item).map_err(|_| WindowsError::Path)?;
        let path = Path::new(name);
        if name.len() > 4096
            || name.contains('\\')
            || !path
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Err(WindowsError::Path);
        }
        let mut prefix = String::new();
        for part in name.split('/') {
            if !valid_component(part) {
                return Err(WindowsError::Path);
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if casing
                .insert(prefix.to_lowercase(), prefix.clone())
                .is_some_and(|old| old != prefix)
            {
                return Err(WindowsError::Path);
            }
        }
        paths.insert(path.to_path_buf());
        if paths.len() > MAX_FILES {
            return Err(WindowsError::Limit);
        }
    }
    Ok(paths.into_iter().collect())
}
fn file_hash(
    path: &Path,
    cancel: &CancellationToken,
) -> Result<(String, u64, SystemTime), WindowsError> {
    reject_links(path)?;
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(WindowsError::Path);
    }
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
        total += n as u64;
    }
    let after = file.metadata()?;
    if before.len() != total
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(WindowsError::Changed);
    }
    Ok((
        format!("{:x}", digest.finalize()),
        total,
        before.modified()?,
    ))
}
pub(crate) async fn capture(
    source: &Path,
    cancel: CancellationToken,
) -> Result<CopyManifest, WindowsError> {
    let paths = admitted(source).await?;
    let source = source.to_owned();
    tokio::task::spawn_blocking(move || {
        reject_links(&source)?;
        let mut files = Vec::new();
        let mut skipped = 0;
        let mut digest = Sha256::new();
        for path in paths {
            cancelled(&cancel)?;
            let from = source.join(&path);
            reject_links(&from)?;
            let meta = match fs::symlink_metadata(&from) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    skipped += 1;
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            if !meta.is_file() {
                skipped += 1;
                continue;
            }
            let (hash, size, modified) = file_hash(&from, &cancel)?;
            let name = path.to_str().ok_or(WindowsError::Path)?;
            digest.update((name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(size.to_le_bytes());
            digest.update(hash.as_bytes());
            files.push(InputFile {
                path,
                hash,
                size,
                modified,
            });
        }
        Ok(CopyManifest {
            revision: format!("{:x}", digest.finalize()),
            files,
            skipped,
        })
    })
    .await
    .map_err(|_| WindowsError::Changed)?
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn copy_one(
    source: &Path,
    target: &Path,
    input: &InputFile,
    cancel: &CancellationToken,
) -> Result<bool, WindowsError> {
    let from = source.join(&input.path);
    let to = target.join(&input.path);
    reject_links(&from)?;
    reject_links(&to)?;
    if let Ok(meta) = fs::symlink_metadata(&to) {
        if !meta.is_file() {
            return Err(WindowsError::Path);
        }
        if meta.len() == input.size && file_hash(&to, cancel)?.0 == input.hash {
            return Ok(false);
        }
    }
    let parent = to.parent().ok_or(WindowsError::Path)?;
    safe_dir(parent)?;
    let temp = Temporary(parent.join(format!(
        ".devbox-copy-{}.tmp",
        uuid::Uuid::new_v4().simple()
    )));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp.0)?;
    let mut file = File::open(&from)?;
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        digest.update(&buf[..n]);
        total += n as u64;
    }
    if total != input.size || format!("{:x}", digest.finalize()) != input.hash {
        return Err(WindowsError::Changed);
    }
    out.set_times(fs::FileTimes::new().set_modified(input.modified))?;
    out.sync_all()?;
    drop(out);
    reject_links(&to)?;
    fs::rename(&temp.0, &to)?;
    File::open(parent)?.sync_all()?;
    Ok(true)
}
pub(crate) async fn publish(
    source: &Path,
    target: &Path,
    manifest: &CopyManifest,
    cancel: CancellationToken,
) -> Result<SyncReport, WindowsError> {
    publish_with_hook(source, target, manifest, cancel, None).await
}
pub(crate) async fn publish_with_hook(
    source: &Path,
    target: &Path,
    manifest: &CopyManifest,
    cancel: CancellationToken,
    hook: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) -> Result<SyncReport, WindowsError> {
    let source = source.to_owned();
    let target = target.to_owned();
    let manifest = manifest.clone();
    tokio::task::spawn_blocking(move || {
        reject_links(&source)?;
        safe_dir(&target)?;
        // All destination paths are checked before any admitted file is replaced.
        for input in &manifest.files {
            reject_links(&target.join(&input.path))?;
        }
        if let Some(hook) = hook {
            hook();
        }
        let mut report = SyncReport {
            skipped: manifest.skipped,
            ..Default::default()
        };
        for input in &manifest.files {
            if copy_one(&source, &target, input, &cancel)? {
                report.copied += 1;
            } else {
                report.skipped += 1;
            }
        }
        Ok(report)
    })
    .await
    .map_err(|_| WindowsError::Changed)?
}
pub(crate) async fn copy_files(source: &Path, target: &Path) -> Result<SyncReport, WindowsError> {
    let cancel = CancellationToken::new();
    let before = capture(source, cancel.clone()).await?;
    let result = publish(source, target, &before, cancel.clone()).await?;
    if capture(source, cancel).await?.revision != before.revision {
        return Err(WindowsError::Changed);
    }
    Ok(result)
}
