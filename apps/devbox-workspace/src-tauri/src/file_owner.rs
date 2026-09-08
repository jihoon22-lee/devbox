//! Native file approvals and open-document snapshots. Invoke only from the
//! host's bounded IO worker, with a freshly admitted ProjectLease when used.
//! Neither a renderer path nor its supplied hash can grant write authority.
use crate::platform::project_probe::ProjectLease;
use code_pad_lib::commands::file::{
    self, ExpectedFileSnapshot, FileActionRequest, OpenFileRequest, OpenedFileWire,
    RenameFileRequest, RenamedFileWire, SaveFileRequest, SavedFileWire,
};
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, parse_safe_project_path,
    FilesystemIdentity, ProjectPathKind,
};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
pub type Scope<'a> = Option<(&'a ProjectContext, &'a ProjectLease)>;
const MAX_DOCUMENTS: usize = 64;
const APPROVAL_LIFETIME: Duration = Duration::from_secs(180);

fn native_path(raw: &str) -> Result<PathBuf> {
    let path = parse_safe_project_path(raw).ok_or("invalid_file_path")?;
    #[cfg(windows)]
    if !matches!(
        path.kind(),
        ProjectPathKind::WindowsDrive | ProjectPathKind::WindowsUnc
    ) || devbox_wsl::path::parse_wsl_unc_path(raw)
        .map_err(|_| "invalid_file_path")?
        .is_some()
    {
        return Err("file_target_unavailable");
    }
    #[cfg(not(windows))]
    if path.kind() != ProjectPathKind::Posix {
        return Err("file_target_unavailable");
    }
    let path = PathBuf::from(path.as_str());
    if path.components().count() > 128 {
        return Err("file_path_limit");
    }
    Ok(path)
}
fn key(path: &Path) -> Result<String> {
    parse_safe_project_path(path.to_str().ok_or("invalid_file_path")?)
        // Canonical spelling preserves distinct files in case-sensitive NTFS
        // directories. Native identity, rather than case folding, owns writes.
        .map(|path| path.as_str().replace('\\', "/"))
        .ok_or("invalid_file_path")
}
fn display_path(path: &Path) -> Result<PathBuf> {
    let spelling = path.to_str().ok_or("invalid_file_path")?;
    let spelling = if let Some(unc) = spelling.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        spelling
            .strip_prefix(r"\\?\")
            .unwrap_or(spelling)
            .to_owned()
    };
    native_path(&spelling)
}
fn within(root: &Path, candidate: &Path) -> Result<bool> {
    let root_key = parse_safe_project_path(root.to_str().ok_or("invalid_file_path")?)
        .ok_or("invalid_file_path")?
        .identity()
        .to_owned();
    let candidate_key = parse_safe_project_path(candidate.to_str().ok_or("invalid_file_path")?)
        .ok_or("invalid_file_path")?
        .identity()
        .to_owned();
    let separator = if root_key.starts_with('/') { '/' } else { '\\' };
    Ok(candidate_key
        .strip_prefix(&root_key)
        .is_some_and(|tail| tail.starts_with(separator)))
}
fn check_scope(scope: Scope<'_>, context: &Option<ProjectContext>, path: &Path) -> Result<()> {
    if let Some(expected) = context {
        let (current, lease) = scope.ok_or("file_context_changed")?;
        if current != expected
            || current.target != lease.binding().target
            || !within(Path::new(&lease.binding().root), path)?
        {
            return Err("file_context_changed");
        }
        lease.revalidate()?;
    }
    Ok(())
}
struct Object {
    path: PathBuf,
    identity: FilesystemIdentity,
    _handle: File,
}
impl Object {
    fn open(path: &Path, directory: bool) -> Result<Self> {
        let (handle, identity) =
            open_filesystem_object(path, directory).map_err(|_| "file_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn revalidate(&self, directory: bool) -> Result<()> {
        if filesystem_identity(&self.path, directory).map_err(|_| "file_changed")? != self.identity
        {
            return Err("file_changed");
        }
        Ok(())
    }
}
struct Grant {
    file: Object,
    parents: Vec<Object>,
    // None is an actual native file-picker approval, independent of a project.
    context: Option<ProjectContext>,
}
impl Grant {
    fn open(path: &Path, context: Option<ProjectContext>) -> Result<Self> {
        ensure_no_links(path).map_err(|_| "unsafe_file_path")?;
        let original = Object::open(path, false)?;
        let canonical =
            display_path(&std::fs::canonicalize(path).map_err(|_| "file_unavailable")?)?;
        let mut parents = Vec::new();
        let mut parent = canonical.parent();
        while let Some(path) = parent {
            parents.push(Object::open(path, true)?);
            parent = path.parent();
        }
        let grant = Self {
            file: Object::open(&canonical, false)?,
            parents,
            context,
        };
        ensure_no_links(path).map_err(|_| "unsafe_file_path")?;
        original.revalidate(false)?;
        if original.identity != grant.file.identity {
            return Err("file_changed");
        }
        grant.revalidate()?;
        Ok(grant)
    }
    fn admit(&self, scope: Scope<'_>) -> Result<()> {
        check_scope(scope, &self.context, &self.file.path)?;
        if self.context.is_some() {
            let (_, lease) = scope.ok_or("file_context_changed")?;
            if !self
                .parents
                .iter()
                .any(|parent| parent.identity == lease.native_root_identity())
            {
                return Err("file_context_changed");
            }
        }
        self.revalidate()
    }
    fn revalidate(&self) -> Result<()> {
        ensure_no_links(&self.file.path).map_err(|_| "unsafe_file_path")?;
        for parent in &self.parents {
            parent.revalidate(true)?;
        }
        self.file.revalidate(false)
    }
}
struct Approval {
    grant: Grant,
    created: Instant,
}
struct Document {
    grant: Grant,
    mtime: i64,
    size: u64,
    hash: String,
    lossy: bool,
    encoding: code_pad_lib::core::encoding::Encoding,
    line_ending: code_pad_lib::core::line_ending::LineEnding,
    revision: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeChoices {
    schema_version: u32,
    paths: Vec<String>,
}
impl Document {
    fn expected(&self) -> ExpectedFileSnapshot<'_> {
        ExpectedFileSnapshot {
            mtime: self.mtime,
            size: self.size,
            content_hash: &self.hash,
            identity: Some(self.grant.file.identity),
        }
    }
    fn matches(&self, mtime: &str, size: u64, hash: &str) -> Result<()> {
        if mtime != self.mtime.to_string() || size != self.size || hash != self.hash {
            return Err("file_snapshot_changed");
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct FileOwner {
    approvals: HashMap<String, Approval>,
    documents: HashMap<String, Document>,
    // Native-owned recent picker choices, never accepted from a renderer.
    // Reopening in a new session captures a fresh object and fresh disk bytes;
    // no prior write snapshot or serialized filesystem identity is restored.
    choices: Vec<PathBuf>,
}
impl FileOwner {
    pub fn restore_native_choices(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 512 * 1024 {
            return Err("invalid_file_choices");
        }
        let choices: NativeChoices =
            serde_json::from_slice(bytes).map_err(|_| "invalid_file_choices")?;
        if choices.schema_version != 1 || choices.paths.len() > MAX_DOCUMENTS {
            return Err("invalid_file_choices");
        }
        let paths = choices
            .paths
            .iter()
            .map(|path| native_path(path))
            .collect::<Result<Vec<_>>>()?;
        let keys = paths
            .iter()
            .map(|path| key(path))
            .collect::<Result<std::collections::HashSet<_>>>()?;
        if keys.len() != paths.len() {
            return Err("invalid_file_choices");
        }
        self.choices = paths;
        Ok(())
    }
    pub fn native_choices(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(&NativeChoices {
            schema_version: 1,
            paths: self
                .choices
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        })
        .map_err(|_| "invalid_file_choices")
    }
    fn remember_choice(&mut self, path: &Path) {
        self.choices.retain(|previous| previous != path);
        self.choices.push(path.into());
        if self.choices.len() > MAX_DOCUMENTS {
            self.choices.remove(0);
        }
    }
    /// Metadata-only routing before the host probes an external project. An
    /// independently chosen file remains usable when that project is offline.
    pub fn needs_project(&self, raw: &str) -> Result<bool> {
        let path = native_path(raw)?;
        let id = key(&path)?;
        if let Some(document) = self.documents.get(&id) {
            return Ok(document.grant.context.is_some());
        }
        Ok(!self.approvals.contains_key(&id) && !self.choices.contains(&path))
    }
    /// Called only with paths returned by the native dialog, never by an IPC
    /// method accepting a renderer-supplied approval/path list.
    pub fn approve_native_selection(&mut self, path: &Path) -> Result<String> {
        self.approvals
            .retain(|_, approval| approval.created.elapsed() < APPROVAL_LIFETIME);
        if self.approvals.len() + self.documents.len() >= MAX_DOCUMENTS {
            return Err("file_limit");
        }
        let path = native_path(path.to_str().ok_or("invalid_file_path")?)?;
        let grant = Grant::open(&path, None)?;
        let display = grant
            .file
            .path
            .to_str()
            .ok_or("invalid_file_path")?
            .to_owned();
        self.remember_choice(&grant.file.path);
        self.approvals.insert(
            key(&grant.file.path)?,
            Approval {
                grant,
                created: Instant::now(),
            },
        );
        Ok(display)
    }
    pub fn open(&mut self, scope: Scope<'_>, request: OpenFileRequest) -> Result<OpenedFileWire> {
        let path = native_path(&request.path)?;
        let requested_key = key(&path)?;
        let grant = if let Some(approval) = self.approvals.remove(&requested_key) {
            if approval.created.elapsed() >= APPROVAL_LIFETIME {
                return Err("file_approval_expired");
            }
            approval.grant
        } else if let Some(document) = self
            .documents
            .get(&requested_key)
            .filter(|d| d.grant.context.is_none())
        {
            document.grant.revalidate()?;
            let next = Grant::open(&path, None)?;
            if next.file.identity != document.grant.file.identity {
                return Err("file_changed");
            }
            next
        } else if self.choices.contains(&path) {
            Grant::open(&path, None)?
        } else {
            let (context, lease) = scope.ok_or("file_selection_required")?;
            check_scope(scope, &Some(context.clone()), &path)?;
            lease.revalidate()?;
            Grant::open(&path, Some(context.clone()))?
        };
        let canonical_key = key(&grant.file.path)?;
        if !self.documents.contains_key(&canonical_key) && self.documents.len() >= MAX_DOCUMENTS {
            return Err("file_limit");
        }
        grant.admit(scope)?;
        let opened = file::open_path_with_encoding(&grant.file.path, request.encoding)
            .map_err(|_| "file_read_failed")?;
        grant.admit(scope)?;
        if opened.native_identity() != grant.file.identity {
            return Err("file_changed");
        }
        let path = grant
            .file
            .path
            .to_str()
            .ok_or("invalid_file_path")?
            .to_owned();
        let revision = self
            .documents
            .get(&canonical_key)
            .filter(|previous| {
                previous.grant.context == grant.context
                    && previous.grant.file.identity == grant.file.identity
                    && previous.grant.parents.len() == grant.parents.len()
                    && previous
                        .grant
                        .parents
                        .iter()
                        .zip(&grant.parents)
                        .all(|(a, b)| a.path == b.path && a.identity == b.identity)
                    && previous.mtime == opened.mtime
                    && previous.size == opened.size
                    && previous.hash == opened.content_hash
                    && previous.lossy == opened.lossy
                    && previous.encoding == opened.encoding
                    && previous.line_ending == opened.line_ending
            })
            .map(|previous| previous.revision.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.documents.insert(
            canonical_key,
            Document {
                grant,
                mtime: opened.mtime,
                size: opened.size,
                hash: opened.content_hash.clone(),
                lossy: opened.lossy,
                encoding: opened.encoding,
                line_ending: opened.line_ending,
                revision,
            },
        );
        let mut wire = OpenedFileWire::from(opened);
        wire.path = path;
        Ok(wire)
    }
    pub fn admitted_path(&self, scope: Scope<'_>, raw: &str) -> Result<PathBuf> {
        let path = native_path(raw)?;
        let document = self
            .documents
            .get(&key(&path)?)
            .ok_or("file_selection_required")?;
        document.grant.admit(scope)?;
        Ok(document.grant.file.path.clone())
    }
    pub fn save(&mut self, scope: Scope<'_>, request: SaveFileRequest) -> Result<SavedFileWire> {
        let path = self.admitted_path(scope, &request.path)?;
        let id = key(&path)?;
        let document = self.documents.get(&id).ok_or("file_selection_required")?;
        document.matches(
            &request.expected_mtime_nanos,
            request.expected_size,
            &request.expected_content_hash,
        )?;
        if request.source_lossy != document.lossy {
            return Err("file_snapshot_changed");
        }
        let saved = file::save_path(
            &path,
            &request.text,
            request.encoding,
            request.line_ending,
            document.expected(),
            document.lossy,
        )
        .map_err(|_| "file_save_conflict")?;
        let context = document.grant.context.clone();
        let mut wire = SavedFileWire::from(saved.clone());
        wire.path = path.to_str().ok_or("invalid_file_path")?.to_owned();
        // The engine has committed. A failed pin must revoke future writes,
        // while returning the committed save with a durability warning.
        let next = Grant::open(&path, context);
        match next {
            Ok(grant) if Some(grant.file.identity) == saved.native_identity() => {
                self.documents.insert(
                    id,
                    Document {
                        grant,
                        mtime: saved.mtime,
                        size: saved.size,
                        hash: saved.content_hash,
                        lossy: false,
                        encoding: request.encoding,
                        line_ending: request.line_ending,
                        revision: uuid::Uuid::new_v4().to_string(),
                    },
                );
            }
            _ => {
                self.documents.remove(&id);
                wire.durability_warning =
                    Some("저장은 완료됐지만 파일을 다시 확인해야 합니다.".into());
            }
        }
        Ok(wire)
    }
    pub fn rename(
        &mut self,
        scope: Scope<'_>,
        request: RenameFileRequest,
    ) -> Result<RenamedFileWire> {
        let path = self.admitted_path(scope, &request.file.path)?;
        let id = key(&path)?;
        let document = self.documents.get(&id).ok_or("file_selection_required")?;
        document.matches(
            &request.file.expected_mtime_nanos,
            request.file.expected_size,
            &request.file.expected_content_hash,
        )?;
        let mut wire = file::rename_path(&path, &request.new_name, document.expected())
            .map_err(|_| "file_rename_conflict")?;
        let next_path = path.with_file_name(&request.new_name);
        wire.path = next_path.to_str().ok_or("invalid_file_path")?.to_owned();
        let next = Grant::open(&next_path, document.grant.context.clone());
        let old = self
            .documents
            .remove(&id)
            .ok_or("file_selection_required")?;
        if old.grant.context.is_none() {
            self.choices.retain(|previous| previous != &path);
            self.remember_choice(&next_path);
        }
        if let Ok(grant) = next {
            if grant.file.identity == old.grant.file.identity {
                self.documents.insert(
                    key(&next_path)?,
                    Document {
                        grant,
                        revision: uuid::Uuid::new_v4().to_string(),
                        ..old
                    },
                );
            }
        }
        Ok(wire)
    }
    pub fn delete(&mut self, scope: Scope<'_>, request: FileActionRequest) -> Result<()> {
        let path = self.admitted_path(scope, &request.path)?;
        let id = key(&path)?;
        let document = self.documents.get(&id).ok_or("file_selection_required")?;
        document.matches(
            &request.expected_mtime_nanos,
            request.expected_size,
            &request.expected_content_hash,
        )?;
        file::delete_path(&path, document.expected()).map_err(|_| "file_delete_conflict")?;
        self.documents.remove(&id);
        self.choices.retain(|previous| previous != &path);
        Ok(())
    }
    pub fn close(&mut self, raw: &str) -> Result<()> {
        let id = key(&native_path(raw)?)?;
        self.documents.remove(&id);
        self.approvals.remove(&id);
        Ok(())
    }
    pub fn validate_open_paths(
        &self,
        context: Option<&ProjectContext>,
        paths: &[String],
    ) -> Result<()> {
        for raw in paths {
            let document = self
                .documents
                .get(&key(&native_path(raw)?)?)
                .ok_or("file_selection_required")?;
            if document
                .grant
                .context
                .as_ref()
                .is_some_and(|expected| Some(expected) != context)
            {
                return Err("file_context_changed");
            }
        }
        Ok(())
    }
    pub fn document_revision(&self, raw: &str) -> Result<String> {
        self.documents
            .get(&key(&native_path(raw)?)?)
            .map(|document| document.revision.clone())
            .ok_or("file_selection_required")
    }
    pub fn has_document(&self, raw: &str) -> bool {
        native_path(raw)
            .and_then(|path| key(&path))
            .is_ok_and(|id| self.documents.contains_key(&id))
    }
    /// Uses the opened native encoding and snapshot after the recovery owner
    /// has checked and consumed its exact one-time preview.
    pub fn apply_recovery(
        &mut self,
        scope: Scope<'_>,
        raw: &str,
        content: &str,
        revision: &str,
    ) -> Result<SavedFileWire> {
        let document = self
            .documents
            .get(&key(&native_path(raw)?)?)
            .ok_or("file_selection_required")?;
        if revision != document.revision {
            return Err("recovery_preview_stale");
        }
        let request = SaveFileRequest {
            path: raw.into(),
            text: content.into(),
            encoding: document.encoding,
            line_ending: document.line_ending,
            expected_mtime_nanos: document.mtime.to_string(),
            expected_size: document.size,
            expected_content_hash: document.hash.clone(),
            source_lossy: document.lossy,
        };
        self.save(scope, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn open_request(path: &Path) -> OpenFileRequest {
        OpenFileRequest {
            path: path.to_str().unwrap().into(),
            encoding: None,
        }
    }
    fn save_request(opened: &OpenedFileWire, text: &str) -> SaveFileRequest {
        SaveFileRequest {
            path: opened.path.clone(),
            text: text.into(),
            encoding: opened.encoding,
            line_ending: opened.line_ending,
            expected_mtime_nanos: opened.mtime_nanos.clone(),
            expected_size: opened.size,
            expected_content_hash: opened.content_hash.clone(),
            source_lossy: opened.lossy,
        }
    }
    fn action(opened: &OpenedFileWire) -> FileActionRequest {
        FileActionRequest {
            path: opened.path.clone(),
            expected_mtime_nanos: opened.mtime_nanos.clone(),
            expected_size: opened.size,
            expected_content_hash: opened.content_hash.clone(),
        }
    }
    fn context(lease: &ProjectLease) -> ProjectContext {
        ProjectContext {
            project_id: "project".into(),
            worktree_id: "tree".into(),
            revision: 1,
            target: lease.binding().target.clone(),
        }
    }
    #[test]
    fn native_choice_is_required_and_saves_retain_the_engine_encoding_contract() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("한글 space.txt");
        fs::write(&path, b"before\r\n").unwrap();
        let mut owner = FileOwner::default();
        assert!(owner.open(None, open_request(&path)).is_err());
        let chosen = owner.approve_native_selection(&path).unwrap();
        let opened = owner.open(None, open_request(Path::new(&chosen))).unwrap();
        assert_eq!(opened.text, "before\n");
        let saved = owner.save(None, save_request(&opened, "after\n")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"after\r\n");
        assert_ne!(saved.content_hash, opened.content_hash);
        assert!(owner.save(None, save_request(&opened, "stale\n")).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"after\r\n");
    }
    #[test]
    fn native_choices_restore_paths_with_fresh_objects_and_never_restore_write_snapshots() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        fs::write(&path, "first").unwrap();
        let mut owner = FileOwner::default();
        let chosen = owner.approve_native_selection(&path).unwrap();
        let opened = owner.open(None, open_request(Path::new(&chosen))).unwrap();
        let catalog = owner.native_choices().unwrap();
        drop(owner);
        fs::write(&path, "new session bytes").unwrap();
        let mut restored = FileOwner::default();
        restored.restore_native_choices(&catalog).unwrap();
        assert!(restored
            .save(None, save_request(&opened, "old snapshot"))
            .is_err());
        let fresh = restored
            .open(None, open_request(Path::new(&chosen)))
            .unwrap();
        assert_eq!(fresh.text, "new session bytes");
        assert!(restored
            .restore_native_choices(br#"{"schemaVersion":2,"paths":[]}"#)
            .is_err());
        assert!(restored.has_document(&fresh.path));
        assert_eq!(fs::read_to_string(&path).unwrap(), "new session bytes");
    }
    #[test]
    fn external_bytes_and_replaced_objects_cannot_be_approved_by_renderer_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        fs::write(&path, "before").unwrap();
        let mut owner = FileOwner::default();
        let chosen = owner.approve_native_selection(&path).unwrap();
        let opened = owner.open(None, open_request(Path::new(&chosen))).unwrap();
        fs::write(&path, "outside edit").unwrap();
        assert!(owner.save(None, save_request(&opened, "mine")).is_err());
        let external = file::open_path(&path).unwrap();
        let mut forged = save_request(&opened, "mine");
        forged.expected_content_hash = external.content_hash;
        forged.expected_mtime_nanos = external.mtime.to_string();
        forged.expected_size = external.size;
        assert_eq!(
            owner.save(None, forged).unwrap_err(),
            "file_snapshot_changed"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "outside edit");
        fs::rename(&path, directory.path().join("old.txt")).unwrap();
        fs::write(&path, "before").unwrap();
        assert!(owner
            .open(None, open_request(Path::new(&opened.path)))
            .is_err());
        assert!(owner.delete(None, action(&opened)).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "before");
    }
    #[test]
    fn project_documents_require_the_same_fresh_context_and_physical_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir(&root).unwrap();
        let path = root.join("file.txt");
        fs::write(&path, "before").unwrap();
        let lease = crate::platform::project_probe::probe_fixture(&root).unwrap();
        let context = context(&lease);
        let mut owner = FileOwner::default();
        let opened = owner
            .open(Some((&context, &lease)), open_request(&path))
            .unwrap();
        let mut changed = context.clone();
        changed.revision += 1;
        assert!(owner
            .save(Some((&changed, &lease)), save_request(&opened, "stale"))
            .is_err());
        assert!(owner.save(None, save_request(&opened, "unscoped")).is_err());
        let outside = directory.path().join("outside.txt");
        fs::write(&outside, "outside").unwrap();
        assert!(owner
            .open(Some((&context, &lease)), open_request(&outside))
            .is_err());
        owner
            .save(Some((&context, &lease)), save_request(&opened, "accepted"))
            .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "accepted");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside");
    }
    #[test]
    fn rename_delete_and_close_use_only_the_native_opened_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("before.txt");
        fs::write(&path, "original").unwrap();
        let mut owner = FileOwner::default();
        let chosen = owner.approve_native_selection(&path).unwrap();
        let opened = owner.open(None, open_request(Path::new(&chosen))).unwrap();
        assert!(owner
            .rename(
                None,
                RenameFileRequest {
                    file: action(&opened),
                    new_name: "../escape.txt".into()
                }
            )
            .is_err());
        let renamed = owner
            .rename(
                None,
                RenameFileRequest {
                    file: action(&opened),
                    new_name: "after.txt".into(),
                },
            )
            .unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&renamed.path).unwrap(), "original");
        let reopened = owner
            .open(None, open_request(Path::new(&renamed.path)))
            .unwrap();
        owner.delete(None, action(&reopened)).unwrap();
        assert!(!Path::new(&renamed.path).exists());
        owner.close(&renamed.path).unwrap();
        assert!(owner
            .open(None, open_request(Path::new(&renamed.path)))
            .is_err());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_substitution_is_rejected_and_case_distinct_documents_stay_distinct() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let upper = directory.path().join("Case.txt");
        let lower = directory.path().join("case.txt");
        fs::write(&upper, "upper").unwrap();
        fs::write(&lower, "lower").unwrap();
        let mut owner = FileOwner::default();
        owner.approve_native_selection(&upper).unwrap();
        owner.approve_native_selection(&lower).unwrap();
        let first = owner.open(None, open_request(&upper)).unwrap();
        let second = owner.open(None, open_request(&lower)).unwrap();
        owner
            .save(None, save_request(&first, "updated upper"))
            .unwrap();
        owner
            .save(None, save_request(&second, "updated lower"))
            .unwrap();
        assert_eq!(fs::read_to_string(&upper).unwrap(), "updated upper");
        assert_eq!(fs::read_to_string(&lower).unwrap(), "updated lower");
        fs::remove_file(&upper).unwrap();
        symlink(&lower, &upper).unwrap();
        assert!(owner.open(None, open_request(&upper)).is_err());
        assert!(owner.approve_native_selection(&upper).is_err());
    }
}
