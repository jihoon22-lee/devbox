//! Editor grants bind renderer document commands to native open-file snapshots.
use super::{actor::control_error, approval::Snapshot, input};
use crate::{file_owner::EditorSnapshot, files_host::FilesHost};
use code_pad_lib::lsp::{LspManager, LspPosition};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
type Result<T> = std::result::Result<T, &'static str>;

pub(super) fn allowed(method: &str) -> bool {
    matches!(
        method,
        "open_lsp_document"
            | "change_lsp_document"
            | "reload_lsp_document"
            | "save_lsp_document"
            | "close_lsp_document"
            | "pull_lsp_diagnostics"
            | "request_lsp_completion"
            | "request_lsp_hover"
            | "request_lsp_definition"
            | "request_lsp_references"
            | "request_lsp_formatting"
            | "request_lsp_rename"
            | "apply_lsp_rename"
            | "cancel_lsp_rename"
            | "discard_lsp_rename"
    )
}
pub(super) fn text_request(method: &str) -> bool {
    matches!(
        method,
        "open_lsp_document" | "change_lsp_document" | "reload_lsp_document" | "save_lsp_document"
    )
}

#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(super) enum Method {
    RequestLspRename {
        language_id: String,
        uri: String,
        position: LspPosition,
        new_name: String,
    },
    ApplyLspRename {
        plan_id: String,
    },
    CancelLspRename {
        plan_id: String,
    },
    DiscardLspRename {
        plan_id: String,
    },
    OpenLspDocument {
        language_id: String,
        path: String,
        text: String,
        native_revision: String,
    },
    ChangeLspDocument {
        language_id: String,
        uri: String,
        text: String,
        dirty: bool,
        native_revision: String,
    },
    ReloadLspDocument {
        language_id: String,
        uri: String,
        text: String,
        native_revision: String,
    },
    SaveLspDocument {
        language_id: String,
        uri: String,
        native_revision: String,
        #[serde(default)]
        text: Option<String>,
    },
    CloseLspDocument {
        language_id: String,
        uri: String,
    },
    PullLspDiagnostics {
        language_id: String,
        uri: String,
    },
    RequestLspCompletion {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspHover {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspDefinition {
        language_id: String,
        uri: String,
        position: LspPosition,
    },
    RequestLspReferences {
        language_id: String,
        uri: String,
        position: LspPosition,
        include_declaration: bool,
    },
    RequestLspFormatting {
        language_id: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    },
}
impl Method {
    pub(super) fn control(&self) -> bool {
        matches!(
            self,
            Self::CancelLspRename { .. } | Self::DiscardLspRename { .. }
        )
    }
    pub(super) fn parse(method: &str, args: Value) -> Result<Self> {
        let method: Self = input(json!({"method":method,"args":args}))?;
        let (language, target) = method.target();
        if language.is_empty()
            || language.len() > 64
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
            || target.is_empty()
            || target.len() > 32 * 1024
        {
            return Err("invalid_request");
        }
        match &method {
            Self::OpenLspDocument {
                text,
                native_revision,
                ..
            }
            | Self::ChangeLspDocument {
                text,
                native_revision,
                ..
            }
            | Self::ReloadLspDocument {
                text,
                native_revision,
                ..
            } if text.len() > 16 * 1024 * 1024
                || native_revision.len() > 128
                || native_revision.is_empty() =>
            {
                return Err("invalid_request")
            }
            Self::SaveLspDocument {
                text: Some(text),
                native_revision,
                ..
            } if text.len() > 16 * 1024 * 1024
                || native_revision.is_empty()
                || native_revision.len() > 128 =>
            {
                return Err("invalid_request");
            }
            _ => {}
        }
        Ok(method)
    }
    fn target(&self) -> (&str, &str) {
        match self {
            Self::ApplyLspRename { plan_id }
            | Self::CancelLspRename { plan_id }
            | Self::DiscardLspRename { plan_id } => ("native", plan_id),
            Self::RequestLspRename {
                language_id, uri, ..
            } => (language_id, uri),
            Self::OpenLspDocument {
                language_id, path, ..
            } => (language_id, path),
            Self::ChangeLspDocument {
                language_id, uri, ..
            }
            | Self::ReloadLspDocument {
                language_id, uri, ..
            }
            | Self::SaveLspDocument {
                language_id, uri, ..
            }
            | Self::CloseLspDocument { language_id, uri }
            | Self::PullLspDiagnostics { language_id, uri }
            | Self::RequestLspCompletion {
                language_id, uri, ..
            }
            | Self::RequestLspHover {
                language_id, uri, ..
            }
            | Self::RequestLspDefinition {
                language_id, uri, ..
            }
            | Self::RequestLspReferences {
                language_id, uri, ..
            }
            | Self::RequestLspFormatting {
                language_id, uri, ..
            } => (language_id, uri),
        }
    }
}
struct Binding {
    snapshot: EditorSnapshot,
    text: String,
}
pub(super) struct Documents {
    files: Arc<Mutex<FilesHost>>,
    snapshot: Arc<Snapshot>,
    bindings: BTreeMap<(String, String), Binding>,
}
impl Documents {
    pub(super) fn new(files: Arc<Mutex<FilesHost>>, snapshot: Arc<Snapshot>) -> Self {
        Self {
            files,
            snapshot,
            bindings: Default::default(),
        }
    }
    fn native(&self, path: &str, revision: &str, verify_disk: bool) -> Result<EditorSnapshot> {
        self.snapshot.document_snapshot(
            &*self.files.try_lock().map_err(|_| "files_unavailable")?,
            path,
            revision,
            verify_disk,
        )
    }
    fn prepare_document(
        &self,
        path: &str,
        revision: &str,
        verify_disk: bool,
    ) -> Result<(crate::core::context_activity::ContextPermit, EditorSnapshot)> {
        let permit = self.snapshot.document_permit(false)?;
        let files = self.files.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => "lsp_busy",
            std::sync::TryLockError::Poisoned(_) => "files_unavailable",
        })?;
        let native = self
            .snapshot
            .document_snapshot(&files, path, revision, verify_disk)?;
        // NativeEditorMirror is the only writer of UI buffer hashes. LSP
        // queues own server text and may lag behind a newer editor transaction.
        Ok((permit, native))
    }
    async fn admit_document(
        &self,
        path: &str,
        revision: &str,
        verify_disk: bool,
        deadline: u64,
    ) -> Result<(crate::core::context_activity::ContextPermit, EditorSnapshot)> {
        loop {
            crate::files_host::current_deadline(deadline)?;
            if !self
                .snapshot
                .active
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return Err("lsp_operation_cancelled");
            }
            // Retry only admission before any LSP notification or mutation.
            // Both the permit and metadata lock are released before sleeping.
            match self.prepare_document(path, revision, verify_disk) {
                Err("lsp_busy") => tokio::time::sleep(std::time::Duration::from_millis(10)).await,
                result => return result,
            }
        }
    }
    async fn apply_rename(&mut self, manager: &LspManager, plan_id: &str) -> Result<Value> {
        let _permit = self.snapshot.document_permit(true)?;
        let result = manager.apply_rename(plan_id).await.map_err(control_error)?;
        let mut wire = super::actor::value(&result)?;
        if result.error.is_some() {
            wire["error"] =
                json!("이름 변경을 완료하지 못했습니다. 파일 결과와 복구 상태를 확인하세요.");
        }
        if !result.success {
            return Ok(wire);
        }
        let mut documents = Vec::new();
        for (index, file) in result.files.iter().enumerate() {
            if file.status != code_pad_lib::lsp::RenameFileStatus::Applied {
                continue;
            }
            let relative = std::path::Path::new(&file.path);
            if relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                wire["error"] = json!("이름 변경은 저장됐지만 편집 문서를 다시 열어야 합니다.");
                continue;
            }
            let path = std::path::Path::new(&self.snapshot.config().workspace_root).join(relative);
            let refreshed = (|| {
                let raw = path.to_str().ok_or("lsp_document_denied")?;
                let mut files = self.files.try_lock().map_err(|_| "files_unavailable")?;
                self.snapshot.refresh_after_rename(&mut files, raw, file)
            })();
            let (opened, revision) = match refreshed {
                Ok(Some(refreshed)) => refreshed,
                Ok(None) => continue,
                Err(_) => {
                    wire["files"][index]["nativeRevision"] = Value::Null;
                    wire["files"][index]["error"] =
                        json!("저장은 완료됐지만 파일을 다시 열어야 합니다.");
                    wire["error"] =
                        json!("이름 변경은 저장됐지만 일부 편집 문서를 다시 열어야 합니다.");
                    continue;
                }
            };
            wire["files"][index]["nativeRevision"] = json!(revision);
            let mut version = result
                .documents
                .iter()
                .find(|document| document.path == file.path)
                .map(|document| document.version)
                .unwrap_or(0);
            let keys = self
                .bindings
                .iter()
                .filter(|(_, binding)| binding.snapshot.path == path)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in keys {
                if !result
                    .documents
                    .iter()
                    .any(|document| document.path == file.path)
                {
                    match manager
                        .reload_document(&key.0, &key.1, opened.text.clone())
                        .await
                    {
                        Ok(changed) => version = changed.version,
                        Err(_) => {
                            let _ = manager.stop(&key.0).await;
                            self.bindings.remove(&key);
                            continue;
                        }
                    }
                }
                match self.native(&opened.path, &revision, false) {
                    Ok(snapshot) => {
                        self.bindings.insert(
                            key,
                            Binding {
                                snapshot,
                                text: opened.text.clone(),
                            },
                        );
                    }
                    Err(_) => {
                        self.bindings.remove(&key);
                        wire["error"] =
                            json!("이름 변경은 저장됐지만 LSP 문서를 다시 열어야 합니다.");
                    }
                }
            }
            documents.push(json!({"path":file.path,"version":version,"text":opened.text,"nativeRevision":revision}));
        }
        wire["documents"] = json!(documents);
        Ok(wire)
    }
    pub(super) async fn execute(
        &mut self,
        manager: &LspManager,
        method: Method,
        deadline: u64,
    ) -> Result<Value> {
        if let Method::ApplyLspRename { plan_id } = method {
            return self.apply_rename(manager, &plan_id).await;
        }
        let (language, target) = method.target();
        let key = (language.to_owned(), target.to_owned());
        if let Method::CloseLspDocument { language_id, uri } = method {
            self.bindings.remove(&key);
            return super::actor::value(
                manager
                    .close_document(&language_id, &uri)
                    .await
                    .map_err(control_error)?,
            );
        }
        if let Method::OpenLspDocument {
            language_id,
            path,
            text,
            native_revision,
        } = method
        {
            let (_permit, native) = self
                .admit_document(&path, &native_revision, true, deadline)
                .await?;
            let dirty = native.dirty(&text);
            let mut opened = manager
                .open_document(&language_id, &native.path, text.clone())
                .await
                .map_err(control_error)?;
            if dirty {
                opened.version = manager
                    .change_document(&language_id, &opened.uri, text.clone(), true)
                    .await
                    .map_err(control_error)?
                    .version;
            }
            self.bindings.insert(
                (language_id, opened.uri.clone()),
                Binding {
                    snapshot: native,
                    text,
                },
            );
            return super::actor::value(opened);
        }
        let binding = self.bindings.get(&key).ok_or("lsp_document_denied")?;
        let path = binding
            .snapshot
            .path
            .to_str()
            .ok_or("lsp_document_denied")?
            .to_owned();
        let revision = match &method {
            Method::ChangeLspDocument {
                native_revision, ..
            }
            | Method::ReloadLspDocument {
                native_revision, ..
            }
            | Method::SaveLspDocument {
                native_revision, ..
            } => native_revision,
            _ => &binding.snapshot.revision,
        };
        let (_permit, native) = self
            .admit_document(
                &path,
                revision,
                matches!(
                    method,
                    Method::ReloadLspDocument { .. } | Method::SaveLspDocument { .. }
                ),
                deadline,
            )
            .await?;
        match method {
            Method::RequestLspRename {
                language_id,
                uri,
                position,
                new_name,
            } => super::actor::value(
                manager
                    .rename(&language_id, &uri, position, new_name)
                    .await
                    .map_err(control_error)?,
            ),
            Method::ChangeLspDocument {
                language_id,
                uri,
                text,
                dirty,
                ..
            } => {
                let _ = dirty; // Only the native disk baseline decides dirty state.
                let changed = manager
                    .change_document(&language_id, &uri, text.clone(), native.dirty(&text))
                    .await
                    .map_err(control_error)?;
                self.bindings.insert(
                    key,
                    Binding {
                        snapshot: native,
                        text,
                    },
                );
                super::actor::value(changed)
            }
            Method::ReloadLspDocument {
                language_id,
                uri,
                text,
                ..
            } => {
                if native.dirty(&text) {
                    return Err("file_snapshot_changed");
                }
                let changed = manager
                    .reload_document(&language_id, &uri, text.clone())
                    .await
                    .map_err(control_error)?;
                self.bindings.insert(
                    key,
                    Binding {
                        snapshot: native,
                        text,
                    },
                );
                super::actor::value(changed)
            }
            Method::SaveLspDocument {
                language_id,
                uri,
                text,
                ..
            } => {
                // A disk save can rotate its native revision before an older
                // queued didChange arrives. Only exact saved baseline text may
                // repair that LSP snapshot. Keep the Files owner's newer buffer
                // acknowledgement intact while publishing this earlier save.
                let text = text.unwrap_or_else(|| binding.text.clone());
                if native.dirty(&text) {
                    return Err("file_snapshot_changed");
                }
                if binding.text != text {
                    manager
                        .reload_document(&language_id, &uri, text.clone())
                        .await
                        .map_err(control_error)?;
                }
                let saved = manager
                    .save_document(&language_id, &uri)
                    .await
                    .map_err(control_error)?;
                self.bindings.insert(
                    key,
                    Binding {
                        snapshot: native,
                        text,
                    },
                );
                super::actor::value(saved)
            }
            Method::PullLspDiagnostics { language_id, uri } => super::actor::value(
                manager
                    .pull_diagnostics(&language_id, &uri)
                    .await
                    .map_err(control_error)?,
            ),
            Method::RequestLspCompletion {
                language_id,
                uri,
                position,
            } => super::actor::value(
                manager
                    .completion(&language_id, &uri, position)
                    .await
                    .map_err(control_error)?,
            ),
            Method::RequestLspHover {
                language_id,
                uri,
                position,
            } => super::actor::value(
                manager
                    .hover(&language_id, &uri, position)
                    .await
                    .map_err(control_error)?,
            ),
            Method::RequestLspDefinition {
                language_id,
                uri,
                position,
            } => super::actor::value(
                manager
                    .definition(&language_id, &uri, position)
                    .await
                    .map_err(control_error)?,
            ),
            Method::RequestLspReferences {
                language_id,
                uri,
                position,
                include_declaration,
            } => super::actor::value(
                manager
                    .references(&language_id, &uri, position, include_declaration)
                    .await
                    .map_err(control_error)?,
            ),
            Method::RequestLspFormatting {
                language_id,
                uri,
                tab_size,
                insert_spaces,
            } => {
                let result = manager
                    .formatting(&language_id, &uri, tab_size, insert_spaces)
                    .await
                    .map_err(control_error)?;
                for document in &result.documents {
                    if document.uri != uri {
                        return Err("lsp_document_denied");
                    }
                    self.bindings.insert(
                        key.clone(),
                        Binding {
                            snapshot: self.native(&path, &native.revision, false)?,
                            text: document.text.clone(),
                        },
                    );
                }
                super::actor::value(result)
            }
            _ => Err("invalid_request"),
        }
    }
}
