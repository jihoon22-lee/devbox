//! Files command owner. Called only after shell admission and inside the
//! bounded Files IO queue; the renderer cannot restore native picker choices.
use crate::core::legacy_sessions::{self, Candidate, StoredSession};
use crate::{file_owner::FileOwner, host::Host, private_metadata::MetadataRoot};
use code_pad_lib::{
    commands::file,
    core::{
        recovery::{RecoveryEntry, RecoveryFile},
        session::Session,
    },
};
use product_contract::ProjectContext;
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

mod recovery_import;
#[cfg(windows)]
mod wsl;

type Result<T> = std::result::Result<T, &'static str>;
const CHOICES: &str = "native-file-choices.json";

// All session readers, writers and importer commits run under the FilesHost
// mutex. The revision binds exact persisted bytes to this native view, including
// the distinction between an absent file and a serialized empty session.
fn session_revision(view: &MetadataRoot, bytes: Option<&[u8]>) -> String {
    let mut digest = Sha256::new();
    digest.update(b"workspace-files-session-v1\0");
    digest.update(view.path().as_os_str().as_encoded_bytes());
    digest.update([0, u8::from(bytes.is_some())]);
    if let Some(bytes) = bytes {
        digest.update(bytes);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn read_session(view: &MetadataRoot) -> Result<(Session, String)> {
    let bytes = view.read("session.json")?;
    let revision = session_revision(view, bytes.as_deref());
    let session = bytes
        .as_deref()
        .map(StoredSession::decode)
        .transpose()?
        .unwrap_or_default()
        .session;
    Ok((session, revision))
}
fn write_session(view: &MetadataRoot, session: &Session, revision: &str) -> Result<String> {
    let before = view.read("session.json")?;
    if session_revision(view, before.as_deref()) != revision {
        return Err("files_session_changed");
    }
    let mut stored = before
        .as_deref()
        .map(StoredSession::decode)
        .transpose()?
        .unwrap_or_default();
    stored.session = session.clone();
    let bytes = stored.encode()?;
    view.write("session.json", &bytes)?;
    Ok(session_revision(view, Some(&bytes)))
}
fn session_history_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn read_session_history(history: &MetadataRoot, id: &str) -> Result<StoredSession> {
    if !session_history_id(id) {
        return Err("invalid_request");
    }
    let bytes = history
        .read(&format!("{id}.json"))?
        .ok_or("files_store_unavailable")?;
    if crate::core::legacy_inventory::digest(&bytes) != id {
        return Err("files_store_changed");
    }
    StoredSession::decode(&bytes)
}

pub struct Invocation<'a> {
    pub context: Option<&'a ProjectContext>,
    pub component: &'a str,
    pub method: &'a str,
    pub args: Value,
    pub deadline: u64,
}

pub fn allowed(component: &str, method: &str) -> bool {
    match component {
        "workspace.files" => matches!(
            method,
            "pick_files"
                | "open_file"
                | "sync_editor_document"
                | "save_file"
                | "rename_file_action"
                | "delete_file_action"
                | "reveal_file_action"
                | "validate_encoding"
                | "read_clipboard_text"
                | "list_workspace_files"
                | "canonicalize_workspace"
                | "workspace_capabilities"
                | "render_preview"
                | "load_session"
                | "save_session"
                | "preview_session_import"
                | "apply_session_import"
                | "cancel_session_import"
                | "list_session_history"
                | "preview_session_restore"
                | "preview_recovery_import"
                | "apply_recovery_import"
                | "cancel_recovery_import"
                | "list_recovery_history"
                | "preview_recovery_restore"
                | "load_recovery"
                | "save_recovery"
                | "discard_recovery"
                | "prepare_recovery"
                | "apply_recovery_preview"
                | "cancel_recovery_preview"
                | "watch_file"
                | "unwatch_file"
                | "take_pending_open"
        ),
        _ => false,
    }
}
fn input<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
fn file_input<T: DeserializeOwned>(value: Value, fields: &[&str]) -> Result<T> {
    if value
        .as_object()
        .is_none_or(|object| object.keys().any(|key| !fields.contains(&key.as_str())))
    {
        return Err("invalid_request");
    }
    input(value)
}
fn empty(value: &Value) -> Result<()> {
    if value.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("invalid_request")
    }
}
fn value<T: serde::Serialize>(item: T) -> Result<Value> {
    serde_json::to_value(item).map_err(|_| "files_response_invalid")
}
pub(crate) fn current_deadline(deadline: u64) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis();
    if now >= u128::from(deadline) {
        Err("request_expired")
    } else {
        Ok(())
    }
}
struct RecoveryPreview {
    path: String,
    content: String,
    document_revision: String,
    source: RecoveryEntry,
    context: Option<ProjectContext>,
    temporary: bool,
    created: Instant,
}
struct SessionImportPreview {
    candidate: Candidate,
    revision: String,
    context: Option<ProjectContext>,
    created: Instant,
    restore: Option<StoredSession>,
}
#[derive(Default)]
pub struct FilesHost {
    owner: FileOwner,
    #[cfg(windows)]
    wsl: Vec<crate::platform::wsl_files::WslFiles>,
    data: Option<MetadataRoot>,
    views: Option<MetadataRoot>,
    previews: HashMap<String, RecoveryPreview>,
    watched: HashMap<String, PathBuf>,
    session_imports: HashMap<String, SessionImportPreview>,
    recovery_imports: HashMap<String, recovery_import::ImportPreview>,
}
impl FilesHost {
    pub(crate) fn has_documents(&self) -> bool {
        self.document_count() != 0
    }
    fn document_count(&self) -> usize {
        let count = self.owner.document_count();
        #[cfg(windows)]
        let count = count
            + self
                .wsl
                .iter()
                .map(|owner| owner.documents.len())
                .sum::<usize>();
        count
    }
    fn has_document(&self, context: Option<&ProjectContext>, path: &str) -> bool {
        #[cfg(windows)]
        if wsl::posix(path) {
            return self
                .wsl_owner(context)
                .is_ok_and(|owner| owner.documents.has(path));
        }
        let _ = context;
        self.owner.has_document(path)
    }
    fn revision_for(&self, context: Option<&ProjectContext>, path: &str) -> Result<String> {
        #[cfg(windows)]
        if wsl::posix(path) {
            return self
                .wsl_owner(context)?
                .documents
                .revision(path)
                .map(str::to_owned);
        }
        let _ = context;
        self.owner.document_revision(path)
    }
    fn close_document(&mut self, context: Option<&ProjectContext>, path: &str) -> Result<()> {
        #[cfg(windows)]
        if wsl::posix(path) {
            return self.close_wsl(context, path);
        }
        self.owner.close_for_context(context, path).map(|_| ())
    }
    fn session_path_eligible(&self, root: Option<&str>, path: &str) -> bool {
        #[cfg(windows)]
        if wsl::posix(path) {
            return root.is_some_and(|root| crate::core::wsl_files::eligible(root, path));
        }
        self.owner.session_path_eligible(root, path)
    }
    fn validate_metadata_paths(
        &self,
        context: Option<&ProjectContext>,
        paths: &[String],
    ) -> Result<()> {
        #[cfg(windows)]
        {
            let (linux, windows): (Vec<_>, Vec<_>) =
                paths.iter().cloned().partition(|path| wsl::posix(path));
            if !linux.is_empty() {
                self.wsl_owner(context)?.documents.validate_paths(&linux)?;
            }
            self.owner.validate_open_paths(context, &windows)
        }
        #[cfg(not(windows))]
        self.owner.validate_open_paths(context, paths)
    }
    #[cfg(test)]
    pub(crate) fn from_editor_fixture(owner: FileOwner) -> Self {
        Self {
            owner,
            ..Self::default()
        }
    }
    pub(crate) fn has_documents_under(&self, root: devbox_filesystem::FilesystemIdentity) -> bool {
        self.owner.has_documents_under(root)
    }
    #[cfg(test)]
    pub(crate) fn execute_editor_save_fixture(
        &mut self,
        scope: crate::file_owner::Scope<'_>,
        request: file::SaveFileRequest,
    ) -> Result<String> {
        let saved = self.owner.save(scope, request)?;
        self.owner.document_revision(&saved.path)
    }
    pub(crate) fn refresh_after_rename(
        &mut self,
        scope: crate::file_owner::Scope<'_>,
        path: &str,
        file: &code_pad_lib::lsp::RenameFileResult,
    ) -> Result<Option<(file::OpenedFileWire, String)>> {
        let opened = self.owner.refresh_after_rename(
            scope,
            path,
            file.mtime_nanos.as_deref().ok_or("file_snapshot_changed")?,
            file.size.ok_or("file_snapshot_changed")?,
            file.content_hash
                .as_deref()
                .ok_or("file_snapshot_changed")?,
        )?;
        opened
            .map(|opened| {
                self.owner
                    .document_revision(&opened.path)
                    .map(|revision| (opened, revision))
            })
            .transpose()
    }
    pub(crate) fn guard_recovery_write(&self, context: &ProjectContext, path: &str) -> Result<()> {
        self.owner.guard_editor_write(context, path)?;
        if self.owner.has_document(path) {
            return Err("lsp_recovery_document_open");
        }
        Ok(())
    }
    pub(crate) fn guard_editor_write(&self, context: &ProjectContext, path: &str) -> Result<()> {
        self.owner.guard_editor_write(context, path)
    }
    pub(crate) fn editor_snapshot(
        &self,
        scope: crate::file_owner::Scope<'_>,
        path: &str,
        revision: &str,
        verify_disk: bool,
    ) -> Result<crate::file_owner::EditorSnapshot> {
        self.owner
            .editor_snapshot(scope, path, revision, verify_disk)
    }
    #[cfg(test)]
    pub(crate) fn sync_editor(
        &mut self,
        context: &ProjectContext,
        path: &str,
        revision: &str,
        text: &str,
    ) -> Result<bool> {
        self.owner
            .sync_editor_document(Some(context), path, revision, text)
    }
    fn document_value<T: serde::Serialize>(&self, path: &str, document: T) -> Result<Value> {
        let mut result = value(document)?;
        result["nativeRevision"] = json!(self.owner.document_revision(path).ok());
        Ok(result)
    }
    fn validate_revision(&self, request: &Value) -> Result<()> {
        let path = request["path"].as_str().ok_or("invalid_request")?;
        let revision = request["nativeRevision"]
            .as_str()
            .ok_or("file_snapshot_changed")?;
        if self.owner.document_revision(path)? != revision {
            return Err("file_snapshot_changed");
        }
        Ok(())
    }

    fn cancel_preview(&mut self, id: &str) {
        if let Some(preview) = self.previews.remove(id) {
            if preview.temporary
                && self
                    .revision_for(preview.context.as_ref(), &preview.path)
                    .ok()
                    .as_deref()
                    == Some(&preview.document_revision)
            {
                let _ = self.close_document(preview.context.as_ref(), &preview.path);
            }
        }
    }
    pub fn initialize(&mut self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let path = host.component("files")?;
        if let Some(data) = &self.data {
            if data.path() != path {
                return Err("files_store_changed");
            }
            return data.revalidate();
        }
        let data = MetadataRoot::open(&path)?;
        self.owner
            .protect_storage(crate::platform::storage_paths::from_host(app, host)?);
        if let Some(bytes) = data.read(CHOICES)? {
            self.owner.restore_native_choices(&bytes)?;
        }
        code_pad_lib::component::initialize(app, &path).map_err(|_| "files_initialize_failed")?;
        self.data = Some(data);
        Ok(())
    }
    fn persist_choices(&self) -> Result<()> {
        self.data
            .as_ref()
            .ok_or("files_initialize_failed")?
            .write(CHOICES, &self.owner.native_choices()?)
    }
    pub fn approve_native_selection(&mut self, paths: &[PathBuf]) -> Result<Value> {
        if paths.len() > 16 || paths.len() + self.document_count() > 64 {
            return Err("file_limit");
        }
        let mut selected = Vec::new();
        for path in paths {
            selected.push(self.owner.approve_native_selection(path)?);
        }
        self.persist_choices()?;
        Ok(json!(selected))
    }
    fn view(&mut self, host: &Host, context: Option<&ProjectContext>) -> Result<MetadataRoot> {
        let name = if let Some(context) = context {
            host.projects()?.binding(context)?;
            format!("worktree-{}", context.worktree_id)
        } else {
            "single-file".into()
        };
        if self.views.is_none() {
            self.views = Some(
                self.data
                    .as_ref()
                    .ok_or("files_initialize_failed")?
                    .child("views")?,
            );
        }
        self.views
            .as_ref()
            .ok_or("files_initialize_failed")?
            .child(&name)
    }
    fn recovery(view: &MetadataRoot) -> Result<RecoveryFile> {
        Ok(recovery_import::read_recovery(view)?.0.recovery)
    }

    fn preview_session_import(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        job_id: &str,
    ) -> Result<Value> {
        let (snapshot_id, source) = host.legacy.session_source(job_id)?;
        let root = context
            .map(|context| {
                host.projects()?
                    .binding(context)
                    .map(|binding| binding.root)
            })
            .transpose()?;
        let candidate = legacy_sessions::candidate(snapshot_id, &source, root.clone(), |path| {
            self.session_path_eligible(root.as_deref(), path)
        })?;
        if candidate.session.docs.is_empty() && candidate.session.recent_files.is_empty() {
            return Err("legacy_session_no_files");
        }
        self.session_preview(host, context, candidate, None)
    }
    fn session_preview(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        candidate: Candidate,
        restore: Option<StoredSession>,
    ) -> Result<Value> {
        if self.has_documents() {
            return Err("legacy_session_documents_open");
        }
        self.session_imports
            .retain(|_, preview| preview.created.elapsed() < Duration::from_secs(180));
        if self.session_imports.len() >= 4 {
            return Err("legacy_session_review_limit");
        }
        let view = self.view(host, context)?;
        let before = view.read("session.json")?;
        let stored = before
            .as_deref()
            .map(StoredSession::decode)
            .transpose()?
            .unwrap_or_default();
        let id = uuid::Uuid::new_v4().to_string();
        let response = json!({"previewId":id,"candidate":candidate,"currentDocuments":stored.session.docs.len(),
            "currentRecentFiles":stored.session.recent_files.len(),"conflict":stored.session!=Session::empty(),"alreadyImported":restore.is_none()&&stored.imports.contains(&candidate.receipt),"restoring":restore.is_some()});
        self.session_imports.insert(
            id,
            SessionImportPreview {
                candidate,
                revision: session_revision(&view, before.as_deref()),
                context: context.cloned(),
                created: Instant::now(),
                restore,
            },
        );
        Ok(response)
    }
    fn session_history(&mut self, host: &Host, context: Option<&ProjectContext>) -> Result<Value> {
        let history = self.view(host, context)?.child("session-history")?;
        let mut items = Vec::new();
        let mut unrecognized = 0;
        for entry in fs::read_dir(history.path())
            .map_err(|_| "files_store_unavailable")?
            .take(33)
        {
            let entry = entry.map_err(|_| "files_store_unavailable")?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name
                .strip_suffix(".json")
                .filter(|id| session_history_id(id))
            else {
                unrecognized += 1;
                continue;
            };
            match read_session_history(&history,id) {
                Ok(stored)=>items.push(json!({"id":id,"documents":stored.session.docs.len(),"recentFiles":stored.session.recent_files.len(),"issue":null})),
                Err(issue)=>items.push(json!({"id":id,"documents":null,"recentFiles":null,"issue":issue})),
            }
        }
        items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        history.revalidate()?;
        Ok(json!({"items":items,"unrecognized":unrecognized}))
    }
    fn preview_session_restore(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        id: &str,
    ) -> Result<Value> {
        if !session_history_id(id) {
            return Err("invalid_request");
        }
        let history = self.view(host, context)?.child("session-history")?;
        let mut stored = read_session_history(&history, id)?;
        let root = context
            .map(|context| {
                host.projects()?
                    .binding(context)
                    .map(|binding| binding.root)
            })
            .transpose()?;
        let candidate =
            legacy_sessions::candidate(id.into(), &stored.session, root.clone(), |path| {
                self.session_path_eligible(root.as_deref(), path)
            })?;
        stored.session = candidate.session.clone();
        self.session_preview(host, context, candidate, Some(stored))
    }
    fn apply_session_import(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        id: &str,
        replace_existing: bool,
        deadline: u64,
    ) -> Result<Value> {
        let preview = self
            .session_imports
            .remove(id)
            .ok_or("legacy_session_review_stale")?;
        if preview.created.elapsed() >= Duration::from_secs(180)
            || preview.context.as_ref() != context
        {
            return Err("legacy_session_review_stale");
        }
        if self.has_documents() {
            return Err("legacy_session_documents_open");
        }
        current_deadline(deadline)?;
        let view = self.view(host, context)?;
        let before = view.read("session.json")?;
        if session_revision(&view, before.as_deref()) != preview.revision {
            return Err("files_session_changed");
        }
        let stored = before
            .as_deref()
            .map(StoredSession::decode)
            .transpose()?
            .unwrap_or_default();
        if preview.restore.is_none() && stored.imports.contains(&preview.candidate.receipt) {
            return Ok(json!({"importedDocuments":0,"importedRecentFiles":0,"reused":true}));
        }
        let restoring = preview.restore.is_some();
        let next = if let Some(restore) = preview.restore {
            if stored.session != Session::empty() && !replace_existing {
                return Err("legacy_session_conflict");
            }
            restore
        } else {
            stored.apply(&preview.candidate, replace_existing)?
        };
        let bytes = next.encode()?;
        if let Some(before) = &before {
            let history = view.child("session-history")?;
            let name = format!("{}.json", crate::core::legacy_inventory::digest(before));
            if history.read(&name)?.is_none()
                && fs::read_dir(history.path())
                    .map_err(|_| "files_store_unavailable")?
                    .take(32)
                    .count()
                    >= 32
            {
                return Err("legacy_session_limit");
            }
            history.preserve(&name, before)?;
        }
        current_deadline(deadline)?;
        if view.read("session.json")? != before {
            return Err("files_session_changed");
        }
        // One atomic publication commits both consumer metadata and its receipt.
        // The unchanged preimage is durable before this point, even on retry.
        view.write("session.json", &bytes)?;
        Ok(
            json!({"importedDocuments":preview.candidate.session.docs.len(),"importedRecentFiles":preview.candidate.session.recent_files.len(),"reused":false,"restored":restoring}),
        )
    }
    fn open_recovery_document(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        raw: &str,
        deadline: u64,
    ) -> Result<(String, String)> {
        if !self.has_document(context, raw) && self.document_count() >= 64 {
            return Err("file_limit");
        }
        #[cfg(windows)]
        if wsl::posix(raw) {
            let opened = self.execute_wsl(
                host,
                context,
                "open_file",
                json!({"request":{"path":raw,"encoding":null}}),
                deadline,
            )?;
            return Ok((
                opened["path"]
                    .as_str()
                    .ok_or("wsl_protocol_invalid")?
                    .into(),
                opened["text"]
                    .as_str()
                    .ok_or("wsl_protocol_invalid")?
                    .into(),
            ));
        }
        let projects = host.projects()?;
        let lease = if self.owner.needs_project(raw)? {
            Some(projects.admit(context.ok_or("project_selection_required")?)?)
        } else {
            None
        };
        let scope = context.zip(
            lease
                .as_ref()
                .map(|lease| lease as &dyn workspace_wsl::files::RootLease),
        );
        current_deadline(deadline)?;
        let opened = self.owner.open(
            scope,
            file::OpenFileRequest {
                path: raw.into(),
                encoding: None,
            },
        )?;
        Ok((opened.path, opened.text))
    }
    fn prepare_recovery(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        raw: &str,
        deadline: u64,
    ) -> Result<Value> {
        current_deadline(deadline)?;
        let expired = self
            .previews
            .iter()
            .filter(|(_, preview)| preview.created.elapsed() >= Duration::from_secs(180))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            self.cancel_preview(&id);
        }
        if self.previews.len() >= 64 {
            return Err("recovery_preview_limit");
        }
        let source = Self::recovery(&self.view(host, context)?)?
            .entries
            .into_iter()
            .find(|entry| entry.path == raw)
            .ok_or("recovery_unavailable")?;
        let temporary = !self.has_document(context, raw);
        let (path, text) = self.open_recovery_document(host, context, raw, deadline)?;
        let id = uuid::Uuid::new_v4().to_string();
        let result = json!({"previewId":id,"path":path,"before":text,"after":source.content});
        self.previews.insert(
            id,
            RecoveryPreview {
                path: path.clone(),
                content: source.content.clone(),
                document_revision: self.revision_for(context, &path)?,
                source,
                context: context.cloned(),
                temporary,
                created: Instant::now(),
            },
        );
        Ok(result)
    }
    fn apply_recovery_preview(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        id: &str,
        deadline: u64,
    ) -> Result<Value> {
        let projects = host.projects()?;
        let preview = self.previews.remove(id).ok_or("recovery_preview_stale")?;
        let owns_temporary = preview.temporary
            && self.revision_for(context, &preview.path).ok().as_deref()
                == Some(&preview.document_revision);
        let result = (|| {
            if preview.created.elapsed() >= Duration::from_secs(180)
                || preview.context.as_ref() != context
            {
                return Err("recovery_preview_stale");
            }
            let view = self.view(host, context)?;
            if !Self::recovery(&view)?.entries.contains(&preview.source) {
                return Err("recovery_preview_stale");
            }
            current_deadline(deadline)?;
            #[cfg(windows)]
            if wsl::posix(&preview.path) {
                return self.wsl_owner_mut(context)?.recover(
                    &projects,
                    context.ok_or("file_context_changed")?,
                    &preview.path,
                    &preview.content,
                    &preview.document_revision,
                    deadline,
                );
            }
            let project_lease = if self.owner.needs_project(&preview.path)? {
                Some(projects.admit(context.ok_or("project_selection_required")?)?)
            } else {
                None
            };
            current_deadline(deadline)?;
            value(
                self.owner.apply_recovery_guarded(
                    context.zip(
                        project_lease
                            .as_ref()
                            .map(|lease| lease as &dyn workspace_wsl::files::RootLease),
                    ),
                    &preview.path,
                    &preview.content,
                    &preview.document_revision,
                    &|| current_deadline(deadline),
                )?,
            )
        })();
        if owns_temporary {
            let _ = self.close_document(context, &preview.path);
        }
        let saved = result?;
        let _ = self.persist_choices();
        Ok(saved)
    }
    pub fn execute(
        &mut self,
        app: &tauri::AppHandle,
        host: &Host,
        invocation: Invocation<'_>,
    ) -> Result<Value> {
        let Invocation {
            context,
            component,
            method,
            args,
            deadline,
        } = invocation;
        self.initialize(app, host)?;
        current_deadline(deadline)?;
        if component != "workspace.files" {
            return Err("invalid_request");
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PathArg {
            path: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Nested {
            request: Value,
        }
        let requested_path = match method {
            "open_file" | "save_file" | "rename_file_action" | "delete_file_action" => args
                .get("request")
                .and_then(|request| request.get("path"))
                .and_then(Value::as_str),
            "watch_file" | "reveal_file_action" | "render_preview" | "sync_editor_document" => {
                args.get("path").and_then(Value::as_str)
            }
            _ => None,
        };
        if method == "open_file"
            && requested_path.is_some_and(|path| !self.has_document(context, path))
            && self.document_count() >= 64
        {
            return Err("file_limit");
        }
        if method == "unwatch_file" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Cleanup {
                path: String,
                document_context: Option<ProjectContext>,
            }
            let explicit_context = args.get("documentContext").is_some();
            let request: Cleanup = input(args)?;
            let cleanup_context = if explicit_context {
                request.document_context.as_ref()
            } else {
                context
            };
            #[cfg(windows)]
            if wsl::posix(&request.path) {
                self.close_wsl(cleanup_context, &request.path)?;
                return Ok(Value::Null);
            }
            if self
                .owner
                .close_for_context(cleanup_context, &request.path)?
            {
                if let Some(known) = self.watched.remove(&request.path) {
                    app.state::<Arc<code_pad_lib::watcher::WatcherManager>>()
                        .unregister(&known)
                        .map_err(|_| "file_action_unavailable")?;
                }
            }
            return Ok(Value::Null);
        }
        #[cfg(windows)]
        if requested_path.is_some_and(wsl::posix)
            || matches!(
                method,
                "list_workspace_files" | "canonicalize_workspace" | "workspace_capabilities"
            ) && context.is_some_and(|context| {
                matches!(
                    context.target,
                    product_contract::ExecutionTarget::Wsl { .. }
                )
            })
        {
            return self.execute_wsl(host, context, method, args, deadline);
        }
        let needs_project = matches!(
            method,
            "list_workspace_files"
                | "canonicalize_workspace"
                | "workspace_capabilities"
                | "render_preview"
        ) || requested_path
            .map(|path| self.owner.needs_project(path))
            .transpose()?
            .unwrap_or(false);
        let projects = host.projects()?;
        let lease = if needs_project {
            Some(projects.admit(context.ok_or("project_selection_required")?)?)
        } else {
            None
        };
        let scope = context.zip(
            lease
                .as_ref()
                .map(|lease| lease as &dyn workspace_wsl::files::RootLease),
        );
        current_deadline(deadline)?;
        match method {
            "list_session_history" => {
                empty(&args)?;
                self.session_history(host, context)
            }
            "preview_session_restore" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    backup_id: String,
                }
                let request: Input = input(args)?;
                self.preview_session_restore(host, context, &request.backup_id)
            }
            "preview_session_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    job_id: String,
                }
                let request: Input = input(args)?;
                self.preview_session_import(host, context, &request.job_id)
            }
            "apply_session_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                    replace_existing: bool,
                }
                let request: Input = input(args)?;
                self.apply_session_import(
                    host,
                    context,
                    &request.preview_id,
                    request.replace_existing,
                    deadline,
                )
            }
            "cancel_session_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                }
                let request: Input = input(args)?;
                Ok(json!(self
                    .session_imports
                    .remove(&request.preview_id)
                    .is_some()))
            }
            "list_recovery_history" => {
                empty(&args)?;
                self.recovery_history(host, context)
            }
            "preview_recovery_restore" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    backup_id: String,
                }
                let request: Input = input(args)?;
                self.preview_recovery_restore(host, context, &request.backup_id)
            }
            "preview_recovery_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    job_id: String,
                }
                let request: Input = input(args)?;
                self.preview_recovery_import(host, context, &request.job_id)
            }
            "apply_recovery_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                    replace_existing: bool,
                }
                let request: Input = input(args)?;
                self.apply_recovery_import(
                    host,
                    context,
                    &request.preview_id,
                    request.replace_existing,
                    deadline,
                )
            }
            "cancel_recovery_import" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                }
                let request: Input = input(args)?;
                Ok(json!(self
                    .recovery_imports
                    .remove(&request.preview_id)
                    .is_some()))
            }
            "sync_editor_document" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct EditorBuffer {
                    path: String,
                    native_revision: String,
                    text: String,
                }
                let buffer: EditorBuffer = input(args)?;
                value(self.owner.sync_editor_document(
                    context,
                    &buffer.path,
                    &buffer.native_revision,
                    &buffer.text,
                )?)
            }
            "open_file" => {
                let request: Nested = input(args)?;
                let opened = self
                    .owner
                    .open(scope, file_input(request.request, &["path", "encoding"])?)?;
                self.document_value(&opened.path, &opened)
            }
            "save_file" => {
                let request: Nested = input(args)?;
                self.validate_revision(&request.request)?;
                let saved = self.owner.save(
                    scope,
                    file_input(
                        request.request,
                        &[
                            "path",
                            "text",
                            "encoding",
                            "lineEnding",
                            "expectedMtimeNanos",
                            "expectedSize",
                            "expectedContentHash",
                            "nativeRevision",
                            "sourceLossy",
                        ],
                    )?,
                )?;
                // A committed file save remains successful if a recent-choice
                // metadata write fails; the UI receives a durability warning.
                let mut saved = saved;
                if self.persist_choices().is_err() {
                    saved.durability_warning =
                        Some("저장은 완료됐지만 최근 파일 정보를 기록하지 못했습니다.".into());
                }
                self.document_value(&saved.path, &saved)
            }
            "rename_file_action" => {
                let request: Nested = input(args)?;
                self.validate_revision(&request.request)?;
                let renamed = self.owner.rename(
                    scope,
                    file_input(
                        request.request,
                        &[
                            "path",
                            "newName",
                            "expectedMtimeNanos",
                            "expectedSize",
                            "expectedContentHash",
                            "nativeRevision",
                        ],
                    )?,
                )?;
                let _ = self.persist_choices();
                self.document_value(&renamed.path, &renamed)
            }
            "delete_file_action" => {
                let request: Nested = input(args)?;
                self.validate_revision(&request.request)?;
                self.owner.delete(
                    scope,
                    file_input(
                        request.request,
                        &[
                            "path",
                            "expectedMtimeNanos",
                            "expectedSize",
                            "expectedContentHash",
                            "nativeRevision",
                        ],
                    )?,
                )?;
                let _ = self.persist_choices();
                Ok(Value::Null)
            }
            "watch_file" | "reveal_file_action" => {
                let request: PathArg = input(args)?;
                let path = self.owner.admitted_path(scope, &request.path)?;
                if method == "watch_file" && self.watched.contains_key(&request.path) {
                    return Ok(Value::Null);
                }
                let canonical = fs::canonicalize(&path).map_err(|_| "file_changed")?;
                let result = tauri::async_runtime::block_on(code_pad_lib::component::dispatch(
                    app,
                    method,
                    json!({"path":path}),
                ))
                .map_err(|_| "file_action_unavailable")?;
                if self.owner.admitted_path(scope, &request.path).is_err() {
                    if method == "watch_file" {
                        let _ = app
                            .state::<Arc<code_pad_lib::watcher::WatcherManager>>()
                            .unregister(&canonical);
                    }
                    return Err("file_changed");
                }
                if method == "watch_file" {
                    self.watched.insert(request.path, canonical);
                }
                Ok(result)
            }
            "list_workspace_files" | "canonicalize_workspace" | "workspace_capabilities" => {
                let request: PathArg = input(args)?;
                let lease = lease.as_ref().ok_or("project_selection_required")?;
                if request.path != lease.binding().root {
                    return Err("file_context_changed");
                }
                self.owner
                    .ensure_user_path(std::path::Path::new(&lease.binding().root))?;
                if method == "canonicalize_workspace" {
                    return Ok(json!(lease.binding().root));
                }
                if method == "workspace_capabilities" {
                    return Ok(
                        json!({"path":lease.binding().root,"sourceKind":"native","watchMode":"native","editSupported":true,"lspSupported":lease.binding().target == product_contract::ExecutionTarget::Windows,"lspReason":if lease.binding().target == product_contract::ExecutionTarget::Windows {None} else {Some("host_lsp_wsl_unsupported")}}),
                    );
                }
                let mut result = value(
                    code_pad_lib::commands::folder::list_workspace_files_guarded(
                        std::path::Path::new(&lease.binding().root),
                        &|path| self.owner.ensure_user_path(path).map_err(str::to_string),
                        &|| current_deadline(deadline).map_err(str::to_string),
                    )
                    .map_err(|_| "file_listing_unavailable")?,
                )?;
                if let Some(files) = result["files"].as_array_mut() {
                    for file in files.iter_mut() {
                        if let Some(path) = file["path"].as_str() {
                            file["path"] = json!(client_path(path));
                        }
                    }
                    files.retain(|file| {
                        file["path"].as_str().is_some_and(|path| {
                            self.owner
                                .ensure_user_path(std::path::Path::new(path))
                                .is_ok()
                        })
                    });
                }
                lease.revalidate()?;
                Ok(result)
            }
            "render_preview" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Preview {
                    path: String,
                    content: String,
                    workspace_root: String,
                }
                let request: Preview = input(args)?;
                let lease = lease.as_ref().ok_or("project_selection_required")?;
                if request.workspace_root != lease.binding().root {
                    return Err("file_context_changed");
                }
                let path = self.owner.admitted_path(scope, &request.path)?;
                let result = code_pad_lib::commands::preview::render_preview_guarded(
                    &path.to_string_lossy(),
                    &request.content,
                    &lease.binding().root,
                    &|path| self.owner.ensure_user_path(path).map_err(str::to_string),
                    &|| current_deadline(deadline).map_err(str::to_string),
                )
                .map_err(|_| "file_preview_unavailable")?;
                self.owner.admitted_path(scope, &request.path)?;
                lease.revalidate()?;
                value(result)
            }
            "load_session" => {
                empty(&args)?;
                let view = self.view(host, context)?;
                let (mut session, revision) = read_session(&view)?;
                session.workspace_folder = context
                    .map(|context| projects.binding(context).map(|binding| binding.root))
                    .transpose()?;
                Ok(json!({"session":session,"persistAllowed":true,"nativeRevision":revision}))
            }
            "save_session" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    session: Session,
                    native_revision: String,
                }
                let request: Input = input(args)?;
                self.validate_metadata_paths(
                    context,
                    &request
                        .session
                        .docs
                        .iter()
                        .map(|doc| doc.path.clone())
                        .collect::<Vec<_>>(),
                )?;
                let revision = write_session(
                    &self.view(host, context)?,
                    &request.session,
                    &request.native_revision,
                )?;
                Ok(json!({"nativeRevision":revision}))
            }
            "load_recovery" => {
                empty(&args)?;
                let (stored, revision) =
                    recovery_import::read_recovery(&self.view(host, context)?)?;
                Ok(json!({"entries":stored.recovery.entries,"nativeRevision":revision}))
            }
            "save_recovery" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    entries: Vec<RecoveryEntry>,
                    native_revision: String,
                }
                let request: Input = input(args)?;
                self.validate_metadata_paths(
                    context,
                    &request
                        .entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect::<Vec<_>>(),
                )?;
                let view = self.view(host, context)?;
                let (stored, _) = recovery_import::read_recovery(&view)?;
                let next = stored.merge(&request.entries)?;
                let revision =
                    recovery_import::write_recovery(&view, &next, &request.native_revision)?;
                Ok(json!({"nativeRevision":revision}))
            }
            "discard_recovery" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    path: Option<String>,
                    native_revision: String,
                }
                let request: Input = input(args)?;
                let view = self.view(host, context)?;
                let (mut stored, _) = recovery_import::read_recovery(&view)?;
                if let Some(path) = &request.path {
                    stored.recovery.remove(path);
                } else {
                    stored.recovery.entries.clear();
                }
                let revision =
                    recovery_import::write_recovery(&view, &stored, &request.native_revision)?;
                let invalidated = self
                    .previews
                    .iter()
                    .filter(|(_, preview)| {
                        preview.context.as_ref() == context
                            && request
                                .path
                                .as_ref()
                                .is_none_or(|path| &preview.source.path == path)
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in invalidated {
                    self.cancel_preview(&id);
                }
                Ok(json!({"nativeRevision":revision}))
            }
            "prepare_recovery" => {
                let request: PathArg = input(args)?;
                self.prepare_recovery(host, context, &request.path, deadline)
            }
            "apply_recovery_preview" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                }
                let request: Input = input(args)?;
                self.apply_recovery_preview(host, context, &request.preview_id, deadline)
            }
            "cancel_recovery_preview" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    preview_id: String,
                }
                let request: Input = input(args)?;
                self.cancel_preview(&request.preview_id);
                Ok(Value::Null)
            }
            "take_pending_open" | "validate_encoding" => {
                tauri::async_runtime::block_on(code_pad_lib::component::dispatch(app, method, args))
                    .map_err(|_| "file_action_unavailable")
            }
            "read_clipboard_text" => {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                empty(&args)?;
                let text = app
                    .clipboard()
                    .read_text()
                    .map_err(|_| "clipboard_unavailable")?;
                if text.len() > 20 * 1024 * 1024 {
                    return Err("clipboard_limit");
                }
                Ok(json!(text))
            }
            _ => Err("invalid_request"),
        }
    }
}
fn client_path(path: &str) -> String {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn verified_session_import_preserves_preimage_and_receipt_and_rejects_stale_review() {
        use crate::core::legacy_inventory::Source;
        use code_pad_lib::core::session::SessionDoc;
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("workspace");
        fs::create_dir(&root).unwrap();
        let source_root = base.path().join(Source::CodePad.identifier());
        fs::create_dir(&source_root).unwrap();
        let selected_path = base.path().join("선택한 파일.txt");
        fs::write(&selected_path, b"unchanged user file").unwrap();
        let host = Host::open(&root).unwrap();
        host.start_empty().unwrap();
        let mut files = files(&host);
        let selected = files
            .owner
            .approve_native_selection(&selected_path)
            .unwrap();
        let mut source = Session::empty();
        source.docs = vec![SessionDoc {
            id: "old-document-id".into(),
            path: selected.clone(),
            cursor: 3,
            bookmarks: vec![1, 5],
        }];
        source.views[1].push("old-document-id".into());
        source.active_view = 1;
        source.active_doc_by_view[1] = Some("old-document-id".into());
        source.recent_files.push(selected);
        let source_bytes = serde_json::to_vec(&source).unwrap();
        fs::write(source_root.join("session.json"), &source_bytes).unwrap();
        let job = json!(host.legacy.start(Source::CodePad).unwrap());
        let job_id = job["id"].as_str().unwrap();
        let start = Instant::now();
        while json!(host.legacy.status().unwrap())["phase"] != "ready" {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        let view = files.view(&host, None).unwrap();
        let (_, revision) = read_session(&view).unwrap();
        let mut previous = Session::empty();
        previous.recent_files.push(source.recent_files[0].clone());
        let revision = write_session(&view, &previous, &revision).unwrap();
        let before = view.read("session.json").unwrap().unwrap();
        let preview = files.preview_session_import(&host, None, job_id).unwrap();
        assert_eq!(preview["conflict"], true);
        let token = preview["previewId"].as_str().unwrap();
        assert_eq!(
            files.apply_session_import(&host, None, token, false, u64::MAX),
            Err("legacy_session_conflict")
        );
        assert_eq!(view.read("session.json").unwrap().unwrap(), before);
        assert_eq!(
            files.apply_session_import(&host, None, token, true, u64::MAX),
            Err("legacy_session_review_stale")
        );
        let preview = files.preview_session_import(&host, None, job_id).unwrap();
        let result = files
            .apply_session_import(
                &host,
                None,
                preview["previewId"].as_str().unwrap(),
                true,
                u64::MAX,
            )
            .unwrap();
        assert_eq!(result["importedDocuments"], 1);
        assert_eq!(read_session(&view).unwrap().0, source);
        let history = view.child("session-history").unwrap();
        assert_eq!(
            history
                .read(&format!(
                    "{}.json",
                    crate::core::legacy_inventory::digest(&before)
                ))
                .unwrap()
                .unwrap(),
            before
        );
        assert_eq!(
            write_session(&view, &previous, &revision),
            Err("files_session_changed")
        );
        let (mut edited, revision) = read_session(&view).unwrap();
        edited.docs[0].cursor = 99;
        write_session(&view, &edited, &revision).unwrap();
        let preview = files.preview_session_import(&host, None, job_id).unwrap();
        assert_eq!(preview["alreadyImported"], true);
        assert_eq!(
            files
                .apply_session_import(
                    &host,
                    None,
                    preview["previewId"].as_str().unwrap(),
                    false,
                    u64::MAX
                )
                .unwrap()["reused"],
            true
        );
        assert_eq!(read_session(&view).unwrap().0.docs[0].cursor, 99);
        let preview = files.preview_session_import(&host, None, job_id).unwrap();
        let (_, revision) = read_session(&view).unwrap();
        edited.docs[0].cursor = 100;
        write_session(&view, &edited, &revision).unwrap();
        assert_eq!(
            files.apply_session_import(
                &host,
                None,
                preview["previewId"].as_str().unwrap(),
                true,
                u64::MAX
            ),
            Err("files_session_changed")
        );
        let backup_id = crate::core::legacy_inventory::digest(&before);
        assert_eq!(
            files.session_history(&host, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let restore = files
            .preview_session_restore(&host, None, &backup_id)
            .unwrap();
        assert_eq!(restore["restoring"], true);
        files
            .apply_session_import(
                &host,
                None,
                restore["previewId"].as_str().unwrap(),
                true,
                u64::MAX,
            )
            .unwrap();
        assert_eq!(read_session(&view).unwrap().0, previous);
        assert_eq!(
            files.session_history(&host, None).unwrap()["items"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        history
            .write(&format!("{backup_id}.json"), b"changed history")
            .unwrap();
        assert_eq!(
            files.preview_session_restore(&host, None, &backup_id),
            Err("files_store_changed")
        );
        assert_eq!(
            history.preserve(&format!("{backup_id}.json"), &before),
            Err("files_store_changed")
        );
        assert_eq!(
            fs::read(source_root.join("session.json")).unwrap(),
            source_bytes
        );
        assert_eq!(fs::read(selected_path).unwrap(), b"unchanged user file");
        assert!(!files.owner.has_documents());
    }
    #[test]
    fn session_revisions_preserve_imports_against_old_autosave_and_other_views() {
        let storage = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(storage.path()).unwrap();
        let first = root.child("first").unwrap();
        let second = root.child("second").unwrap();
        let (empty, original) = read_session(&first).unwrap();
        assert_ne!(original, read_session(&second).unwrap().1);
        assert_eq!(
            write_session(&second, &empty, &original),
            Err("files_session_changed")
        );
        assert!(second.read("session.json").unwrap().is_none());
        let mut imported = Session::empty();
        imported.recent_files.push("C:\\reviewed\\file.txt".into());
        let imported_revision = write_session(&first, &imported, &original).unwrap();
        let imported_bytes = first.read("session.json").unwrap().unwrap();
        assert_eq!(
            write_session(&first, &empty, &original),
            Err("files_session_changed")
        );
        assert_eq!(first.read("session.json").unwrap().unwrap(), imported_bytes);
        assert_eq!(read_session(&first).unwrap().1, imported_revision);
        let saved = write_session(&first, &empty, &imported_revision).unwrap();
        assert_ne!(saved, original);
        assert_eq!(read_session(&first).unwrap().1, saved);
        // External corruption must never be replaced using an older lease.
        first.write("session.json", b"corrupt evidence").unwrap();
        assert!(read_session(&first).is_err());
        assert_eq!(
            write_session(&first, &empty, &saved),
            Err("files_session_changed")
        );
        assert_eq!(
            first.read("session.json").unwrap().unwrap(),
            b"corrupt evidence"
        );
    }

    fn files(host: &Host) -> FilesHost {
        FilesHost {
            data: Some(MetadataRoot::open(&host.component("files").unwrap()).unwrap()),
            ..FilesHost::default()
        }
    }
    fn put_recovery(files: &mut FilesHost, host: &Host, path: &str, content: &str) {
        let record = RecoveryFile {
            version: 1,
            entries: vec![RecoveryEntry {
                path: path.into(),
                content: content.into(),
                base_hash: None,
                snapshot_at_ms: 1,
            }],
        };
        files
            .view(host, None)
            .unwrap()
            .write("recovery.json", &serde_json::to_vec(&record).unwrap())
            .unwrap();
    }
    #[test]
    fn recovery_requires_a_current_preview_and_consumes_it_once() {
        let storage = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let path = external.path().join("한글.txt");
        fs::write(&path, b"before\r\n").unwrap();
        let host = Host::open(storage.path()).unwrap();
        host.start_empty().unwrap();
        let mut files = files(&host);
        let selected = files.owner.approve_native_selection(&path).unwrap();
        put_recovery(&mut files, &host, &selected, "recovered\n");
        let preview = files
            .prepare_recovery(&host, None, &selected, u64::MAX)
            .unwrap();
        assert_eq!(preview["before"], "before\n");
        assert_eq!(fs::read(&path).unwrap(), b"before\r\n");
        let token = preview["previewId"].as_str().unwrap();
        files.cancel_preview(token);
        assert!(!files.owner.has_document(&selected));
        assert!(files
            .apply_recovery_preview(&host, None, token, u64::MAX)
            .is_err());
        let preview = files
            .prepare_recovery(&host, None, &selected, u64::MAX)
            .unwrap();
        let token = preview["previewId"].as_str().unwrap();
        files
            .apply_recovery_preview(&host, None, token, u64::MAX)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"recovered\r\n");
        assert!(!files.owner.has_document(&selected));
        assert!(files
            .apply_recovery_preview(&host, None, token, u64::MAX)
            .is_err());
        // Applying a preview never silently discards the persisted recovery.
        assert_eq!(
            FilesHost::recovery(&files.view(&host, None).unwrap())
                .unwrap()
                .entries
                .len(),
            1
        );
    }
    #[test]
    fn changed_disk_or_recovery_source_rejects_apply_and_releases_preview_only_documents() {
        let storage = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let path = external.path().join("file.txt");
        fs::write(&path, "before").unwrap();
        let host = Host::open(storage.path()).unwrap();
        host.start_empty().unwrap();
        let mut files = files(&host);
        let selected = files.owner.approve_native_selection(&path).unwrap();
        put_recovery(&mut files, &host, &selected, "recovered");
        let preview = files
            .prepare_recovery(&host, None, &selected, u64::MAX)
            .unwrap();
        fs::write(&path, "external change").unwrap();
        assert!(files
            .apply_recovery_preview(
                &host,
                None,
                preview["previewId"].as_str().unwrap(),
                u64::MAX
            )
            .is_err());
        assert!(!files.owner.has_document(&selected));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
        let preview = files
            .prepare_recovery(&host, None, &selected, u64::MAX)
            .unwrap();
        put_recovery(&mut files, &host, &selected, "new recovery");
        assert!(files
            .apply_recovery_preview(
                &host,
                None,
                preview["previewId"].as_str().unwrap(),
                u64::MAX
            )
            .is_err());
        assert!(!files.owner.has_document(&selected));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
    }
    #[test]
    fn component_directory_replacement_cannot_redirect_metadata() {
        let storage = tempfile::tempdir().unwrap();
        let host = Host::open(storage.path()).unwrap();
        host.start_empty().unwrap();
        let root = host.component("files").unwrap();
        let metadata = MetadataRoot::open(&root).unwrap();
        metadata.write("session.json", b"preserved").unwrap();
        let moved = root.with_file_name("files-before");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(host.component("files").is_err());
        assert!(metadata.write("session.json", b"wrong owner").is_err());
        assert_eq!(fs::read(moved.join("session.json")).unwrap(), b"preserved");
        assert!(!root.join("session.json").exists());
    }
    #[test]
    fn file_role_does_not_restore_choices_or_forward_raw_recovery_or_lsp_execution() {
        assert!(allowed("workspace.files", "prepare_recovery"));
        for method in [
            "restore_native_choices",
            "apply_recovery",
            "start_language_server",
            "load_lsp_config",
        ] {
            assert!(!allowed("workspace.files", method));
        }
        assert!(!allowed("workspace.lsp", "start_language_server"));
        assert!(file_input::<file::OpenFileRequest>(
            json!({"path":"C:/file.txt","encoding":null,"force":true}),
            &["path", "encoding"]
        )
        .is_err());
    }
    #[test]
    fn write_revision_survives_identical_reads_but_not_a_new_disk_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("file.txt");
        fs::write(&path, "before").unwrap();
        let mut files = FilesHost::default();
        let selected = files.owner.approve_native_selection(&path).unwrap();
        files
            .owner
            .open(
                None,
                file::OpenFileRequest {
                    path: selected.clone(),
                    encoding: None,
                },
            )
            .unwrap();
        let revision = files.owner.document_revision(&selected).unwrap();
        files
            .owner
            .open(
                None,
                file::OpenFileRequest {
                    path: selected.clone(),
                    encoding: None,
                },
            )
            .unwrap();
        assert_eq!(files.owner.document_revision(&selected).unwrap(), revision);
        files
            .validate_revision(&json!({"path":selected,"nativeRevision":revision}))
            .unwrap();
        fs::write(&path, "new disk snapshot").unwrap();
        files
            .owner
            .open(
                None,
                file::OpenFileRequest {
                    path: selected.clone(),
                    encoding: None,
                },
            )
            .unwrap();
        assert!(files
            .validate_revision(&json!({"path":selected,"nativeRevision":revision}))
            .is_err());
        assert!(files.validate_revision(&json!({"path":selected})).is_err());
    }
}
