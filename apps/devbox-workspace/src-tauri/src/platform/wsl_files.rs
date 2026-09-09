//! Windows owner of one selected project's Linux file connection. Renderer
//! paths are never opened by Windows; only native helper acknowledgements
//! update the metadata mirror used for private session/recovery records.
use super::wsl_project::WslProjectLease;
use crate::{
    core::{
        registry::Binding,
        wsl_files::{self, Documents},
    },
    project_owner::ProjectOwner,
};
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

type Result<T> = std::result::Result<T, &'static str>;
pub struct WslFiles {
    context: ProjectContext,
    lease: WslProjectLease,
    pub documents: Documents,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nested {
    request: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathInput {
    path: String,
}
fn input<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
fn request(value: Value, fields: &[&str]) -> Result<Value> {
    let value: Nested = input(value)?;
    let object = value.request.as_object().ok_or("invalid_request")?;
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err("invalid_request");
    }
    Ok(value.request)
}
impl WslFiles {
    pub fn open(
        projects: &ProjectOwner,
        resources: &Path,
        context: &ProjectContext,
    ) -> Result<Self> {
        Self::open_until(projects, resources, context, u64::MAX)
    }
    pub fn open_until(
        projects: &ProjectOwner,
        resources: &Path,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<Self> {
        let lease = projects.admit_wsl(resources, context)?;
        lease.file_request_until(context, "files_attach", json!({}), deadline)?;
        Ok(Self {
            context: context.clone(),
            lease,
            documents: Documents::default(),
        })
    }
    pub fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub fn reconnect_until(
        &mut self,
        projects: &ProjectOwner,
        resources: &Path,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        if context != &self.context || projects.binding(context)? != *self.binding() {
            return Err("file_context_changed");
        }
        self.shutdown()?;
        crate::files_host::current_deadline(deadline)?;
        // Failed retirement or fresh admission preserves metadata and buffers.
        // Admission never implicitly starts a stopped distribution.
        let mut replacement = Self::open_until(projects, resources, context, deadline)?;
        replacement.documents = self.documents.disconnected();
        *self = replacement;
        Ok(())
    }
    pub fn shutdown(&self) -> Result<()> {
        self.lease.shutdown()
    }
    pub fn is_open(&self) -> bool {
        self.lease.is_open()
    }
    pub fn binding(&self) -> &Binding {
        self.lease.binding()
    }
    pub fn revalidate(&self, projects: &ProjectOwner, context: &ProjectContext) -> Result<()> {
        if context != &self.context || projects.binding(context)? != *self.binding() {
            return Err("file_context_changed");
        }
        self.lease.revalidate()
    }
    pub fn close(&mut self, path: &str) -> Result<()> {
        // Always release known local metadata even when the distro disappeared.
        // A cleanup request never creates a connection or starts a distribution.
        if self.documents.has(path) {
            let _ = self
                .lease
                .file_request(&self.context, "files_close", json!({"path":path}));
            self.documents.close(path)?;
        }
        Ok(())
    }
    pub fn poll(
        &mut self,
        projects: &ProjectOwner,
        context: &ProjectContext,
    ) -> Result<Vec<workspace_wsl::files::WatchSnapshot>> {
        let paths = self.documents.watched_paths();
        if paths.is_empty() {
            return Ok(vec![]);
        }
        self.revalidate(projects, context)?;
        let result = self
            .lease
            .file_request(context, "files_poll", json!({"paths":paths}))?;
        self.revalidate(projects, context)?;
        self.documents
            .accept_poll(&self.lease.binding().root, result)
    }
    pub fn recover(
        &mut self,
        projects: &ProjectOwner,
        context: &ProjectContext,
        path: &str,
        content: &str,
        revision: &str,
        deadline: u64,
    ) -> Result<Value> {
        crate::files_host::current_deadline(deadline)?;
        self.revalidate(projects, context)?;
        if self.documents.revision(path)? != revision {
            return Err("recovery_preview_stale");
        }
        let result = self.lease.file_request_until(
            context,
            "files_recover",
            json!({"path":path,"content":content,"nativeRevision":revision}),
            deadline,
        )?;
        self.documents
            .record(&self.lease.binding().root, path, &result, false)?;
        Ok(result)
    }
    pub fn execute(
        &mut self,
        projects: &ProjectOwner,
        context: &ProjectContext,
        method: &str,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        crate::files_host::current_deadline(deadline)?;
        self.revalidate(projects, context)?;
        crate::files_host::current_deadline(deadline)?;
        match method {
            "list_workspace_files" | "canonicalize_workspace" | "workspace_capabilities" => {
                let value: PathInput = input(args)?;
                if wsl_files::path(&value.path)? != self.binding().root {
                    return Err("file_context_changed");
                }
                match method {
                    "canonicalize_workspace" => Ok(json!(self.binding().root)),
                    "workspace_capabilities" => Ok(
                        json!({"path":self.binding().root,"sourceKind":"wsl","watchMode":"polling","editSupported":true,"lspSupported":false,"lspReason":"host_lsp_wsl_unsupported"}),
                    ),
                    _ => self.lease.file_request_until(
                        context,
                        "files_list",
                        json!({"path":self.binding().root}),
                        deadline,
                    ),
                }
            }
            "open_file" => {
                let mut request = request(args, &["path", "encoding"])?;
                let path = wsl_files::path(request["path"].as_str().ok_or("invalid_request")?)?;
                if !wsl_files::eligible(&self.binding().root, &path) {
                    return Err("file_context_changed");
                }
                request["path"] = json!(path);
                let result = self.lease.file_request_until(
                    context,
                    "files_open",
                    json!({"request":request}),
                    deadline,
                )?;
                self.documents
                    .record(&self.lease.binding().root, &path, &result, true)?;
                Ok(result)
            }
            "save_file" | "rename_file_action" | "delete_file_action" => {
                let fields: &[&str] = match method {
                    "save_file" => &[
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
                    "rename_file_action" => &[
                        "path",
                        "newName",
                        "expectedMtimeNanos",
                        "expectedSize",
                        "expectedContentHash",
                        "nativeRevision",
                    ],
                    _ => &[
                        "path",
                        "expectedMtimeNanos",
                        "expectedSize",
                        "expectedContentHash",
                        "nativeRevision",
                    ],
                };
                let mut request = request(args, fields)?;
                let path = wsl_files::path(request["path"].as_str().ok_or("invalid_request")?)?;
                let revision = request["nativeRevision"]
                    .as_str()
                    .ok_or("file_snapshot_changed")?
                    .to_owned();
                if self.documents.revision(&path)? != revision {
                    return Err("file_snapshot_changed");
                }
                let new_name = request["newName"].as_str().map(str::to_owned);
                request
                    .as_object_mut()
                    .ok_or("invalid_request")?
                    .remove("nativeRevision");
                request["path"] = json!(path);
                let native = match method {
                    "save_file" => "files_save",
                    "rename_file_action" => "files_rename",
                    _ => "files_delete",
                };
                let result = self.lease.file_request_until(
                    context,
                    native,
                    json!({"request":request,"nativeRevision":revision}),
                    deadline,
                )?;
                match method {
                    "save_file" => {
                        self.documents
                            .record(&self.lease.binding().root, &path, &result, false)?
                    }
                    "rename_file_action" => self.documents.renamed(
                        &self.lease.binding().root,
                        &path,
                        new_name.as_deref().ok_or("invalid_request")?,
                        &result,
                    )?,
                    _ => {
                        if !result.is_null() {
                            return Err("wsl_protocol_invalid");
                        }
                        self.documents.close(&path)?;
                    }
                }
                Ok(result)
            }
            "watch_file" => {
                let value: PathInput = input(args)?;
                self.documents.watch(&value.path)?;
                Ok(Value::Null)
            }
            "sync_editor_document" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Buffer {
                    path: String,
                    native_revision: String,
                    text: String,
                }
                let value: Buffer = input(args)?;
                if self.documents.revision(&value.path)? != value.native_revision {
                    return Err("file_snapshot_changed");
                }
                let result = self.lease.file_request_until(context, "files_sync_editor", json!({"path":value.path,"nativeRevision":value.native_revision,"text":value.text}), deadline)?;
                result["dirty"]
                    .as_bool()
                    .map(|dirty| json!(dirty))
                    .ok_or("wsl_protocol_invalid")
            }
            "render_preview" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Preview {
                    path: String,
                    content: String,
                    workspace_root: String,
                }
                let value: Preview = input(args)?;
                if !self.documents.has(&value.path) || value.workspace_root != self.binding().root {
                    return Err("file_context_changed");
                }
                self.lease.file_request_until(context, "files_preview", json!({"path":value.path,"content":value.content,"workspaceRoot":value.workspace_root}), deadline)
            }
            "reveal_file_action" => Err("wsl_reveal_unavailable"),
            _ => Err("invalid_request"),
        }
    }
}
