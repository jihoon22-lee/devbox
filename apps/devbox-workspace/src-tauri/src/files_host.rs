//! Files command owner. Called only after shell admission and inside the
//! bounded Files IO queue; the renderer cannot restore native picker choices.
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
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

type Result<T> = std::result::Result<T, &'static str>;
const CHOICES: &str = "native-file-choices.json";

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
#[derive(Default)]
pub struct FilesHost {
    owner: FileOwner,
    data: Option<MetadataRoot>,
    views: Option<MetadataRoot>,
    previews: HashMap<String, RecoveryPreview>,
    watched: HashMap<String, PathBuf>,
}
impl FilesHost {
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
                && self.owner.document_revision(&preview.path).ok().as_deref()
                    == Some(&preview.document_revision)
            {
                let _ = self.owner.close(&preview.path);
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
        self.owner.protect_native_storage(app, host)?;
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
        if paths.len() > 16 {
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
        let Some(bytes) = view.read("recovery.json")? else {
            return Ok(RecoveryFile::empty());
        };
        code_pad_lib::component::validate_persistent_file("recovery.json", &bytes)?;
        serde_json::from_slice(&bytes).map_err(|_| "invalid_files_store")
    }
    fn prepare_recovery(
        &mut self,
        host: &Host,
        context: Option<&ProjectContext>,
        raw: &str,
        deadline: u64,
    ) -> Result<Value> {
        let projects = host.projects()?;
        let lease = if self.owner.needs_project(raw)? {
            Some(projects.admit(context.ok_or("project_selection_required")?)?)
        } else {
            None
        };
        let scope = context.zip(lease.as_ref());
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
        let temporary = !self.owner.has_document(raw);
        let opened = self.owner.open(
            scope,
            file::OpenFileRequest {
                path: raw.to_owned(),
                encoding: None,
            },
        )?;
        let id = uuid::Uuid::new_v4().to_string();
        let result =
            json!({"previewId":id,"path":opened.path,"before":opened.text,"after":source.content});
        self.previews.insert(
            id,
            RecoveryPreview {
                path: opened.path.clone(),
                content: source.content.clone(),
                document_revision: self.owner.document_revision(&opened.path)?,
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
            && self.owner.document_revision(&preview.path).ok().as_deref()
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
            let project_lease = if self.owner.needs_project(&preview.path)? {
                Some(projects.admit(context.ok_or("project_selection_required")?)?)
            } else {
                None
            };
            current_deadline(deadline)?;
            self.owner.apply_recovery(
                context.zip(project_lease.as_ref()),
                &preview.path,
                &preview.content,
                &preview.document_revision,
            )
        })();
        if owns_temporary {
            let _ = self.owner.close(&preview.path);
        }
        let saved = result?;
        let _ = self.persist_choices();
        value(saved)
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
            "watch_file" | "reveal_file_action" | "render_preview" => {
                args.get("path").and_then(Value::as_str)
            }
            _ => None,
        };
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
        let scope = context.zip(lease.as_ref());
        current_deadline(deadline)?;
        match method {
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
            "unwatch_file" => {
                let request: PathArg = input(args)?;
                // Cleanup must not depend on a root still being online/current.
                // Resolve a native registration before any filesystem lookup;
                // an arbitrary cleanup path must not start a stopped distro.
                if let Some(known) = self.watched.remove(&request.path) {
                    app.state::<Arc<code_pad_lib::watcher::WatcherManager>>()
                        .unregister(&known)
                        .map_err(|_| "file_action_unavailable")?;
                }
                self.owner.close(&request.path)?;
                Ok(Value::Null)
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
                let mut result = tauri::async_runtime::block_on(code_pad_lib::component::dispatch(
                    app,
                    method,
                    json!({"path":lease.binding().root}),
                ))
                .map_err(|_| "file_listing_unavailable")?;
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
                let result = tauri::async_runtime::block_on(
                    code_pad_lib::commands::preview::render_preview(
                        path.to_string_lossy().into_owned(),
                        request.content,
                        lease.binding().root.clone(),
                    ),
                )
                .map_err(|_| "file_preview_unavailable")?;
                self.owner.admitted_path(scope, &request.path)?;
                lease.revalidate()?;
                value(result)
            }
            "load_session" => {
                empty(&args)?;
                let view = self.view(host, context)?;
                let mut session = if let Some(bytes) = view.read("session.json")? {
                    code_pad_lib::component::validate_persistent_file("session.json", &bytes)?;
                    Session::from_json(
                        std::str::from_utf8(&bytes).map_err(|_| "invalid_files_store")?,
                    )
                    .map_err(|_| "invalid_files_store")?
                } else {
                    Session::empty()
                };
                session.workspace_folder = context
                    .map(|context| projects.binding(context).map(|binding| binding.root))
                    .transpose()?;
                Ok(json!({"session":session,"persistAllowed":true}))
            }
            "save_session" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    session: Session,
                }
                let request: Input = input(args)?;
                self.owner.validate_open_paths(
                    context,
                    &request
                        .session
                        .docs
                        .iter()
                        .map(|doc| doc.path.clone())
                        .collect::<Vec<_>>(),
                )?;
                let bytes =
                    serde_json::to_vec(&request.session).map_err(|_| "invalid_files_store")?;
                code_pad_lib::component::validate_persistent_file("session.json", &bytes)?;
                self.view(host, context)?.write("session.json", &bytes)?;
                Ok(Value::Null)
            }
            "load_recovery" => {
                empty(&args)?;
                value(Self::recovery(&self.view(host, context)?)?.entries)
            }
            "save_recovery" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    entries: Vec<RecoveryEntry>,
                }
                let request: Input = input(args)?;
                self.owner.validate_open_paths(
                    context,
                    &request
                        .entries
                        .iter()
                        .map(|entry| entry.path.clone())
                        .collect::<Vec<_>>(),
                )?;
                let view = self.view(host, context)?;
                let mut recovery = Self::recovery(&view)?;
                for entry in request.entries {
                    recovery.upsert(entry);
                }
                let bytes = serde_json::to_vec(&recovery).map_err(|_| "invalid_files_store")?;
                code_pad_lib::component::validate_persistent_file("recovery.json", &bytes)?;
                view.write("recovery.json", &bytes)?;
                Ok(Value::Null)
            }
            "discard_recovery" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    path: Option<String>,
                }
                let request: Input = input(args)?;
                let view = self.view(host, context)?;
                let mut recovery = Self::recovery(&view)?;
                if let Some(path) = &request.path {
                    recovery.remove(path);
                } else {
                    recovery = RecoveryFile::empty();
                }
                view.write(
                    "recovery.json",
                    &serde_json::to_vec(&recovery).map_err(|_| "invalid_files_store")?,
                )?;
                let invalidated = self
                    .previews
                    .iter()
                    .filter(|(_, preview)| {
                        request
                            .path
                            .as_ref()
                            .is_none_or(|path| &preview.source.path == path)
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in invalidated {
                    self.cancel_preview(&id);
                }
                Ok(Value::Null)
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
