//! Independent native LSP metadata/installer owner. Installing a reviewed
//! artifact never grants language-server execution or editor file access.
mod actor;
mod approval;
mod archives;
mod evidence;
mod settings;
use crate::{
    host::Host, platform::storage_paths::ProtectedStorage, private_metadata::MetadataRoot,
};
use code_pad_lib::lsp::{InstallError, ManagedInstaller, RequestCancellation};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct Invocation<'a> {
    pub method: &'a str,
    pub args: Value,
    pub context: Option<&'a product_contract::ProjectContext>,
    pub deadline: u64,
    pub start: Option<crate::core::source_operations::Request>,
}

pub(crate) fn worker_required(method: &str) -> bool {
    !actor::allowed(method) || actor::starts(method)
}
pub(crate) fn stops(method: &str) -> bool {
    actor::stops(method)
}
pub(crate) fn admission(method: &str, args: Value) -> Result<(Option<String>, Vec<String>)> {
    if actor::allowed(method) {
        actor::admission(method, args)
    } else {
        Ok((None, vec![]))
    }
}

pub(crate) fn allowed(method: &str) -> bool {
    actor::allowed(method)
        || matches!(
            method,
            "lsp_catalog"
                | "lsp_installed"
                | "lsp_recover_installed"
                | "lsp_install"
                | "lsp_import_archive"
                | "lsp_uninstall"
                | "pick_lsp_archives"
                | "discard_lsp_archives"
                | "load_lsp_config"
                | "save_lsp_config"
                | "lsp_execution_preview"
                | "lsp_execution_approve"
                | "lsp_execution_cancel"
                | "lsp_execution_revoke"
                | "language_server_statuses"
                | "language_server_logs"
                | "stop_language_server"
                | "stop_all_language_servers"
                | "close_lsp_document"
        )
}
pub(crate) fn contextual(method: &str) -> bool {
    actor::allowed(method)
        || matches!(
            method,
            "load_lsp_config"
                | "save_lsp_config"
                | "lsp_execution_preview"
                | "lsp_execution_approve"
                | "lsp_execution_cancel"
                | "lsp_execution_revoke"
        )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Key {
    manifest_id: String,
    version: String,
    platform: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Import {
    manifest_id: String,
    version: String,
    platform: String,
    archive_paths: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Choices {
    archive_paths: Vec<String>,
}
fn input<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
fn installer_error(error: InstallError) -> &'static str {
    match error {
        InstallError::Cancelled => "lsp_operation_cancelled",
        InstallError::InstallBusy => "lsp_install_busy",
        InstallError::IndexCorrupt => "lsp_index_corrupt",
        _ => "lsp_install_failed",
    }
}
struct Storage {
    data: Arc<MetadataRoot>,
    archives: Arc<MetadataRoot>,
}
impl Storage {
    fn revalidate(&self, host: &Host) -> Result<()> {
        if host.component("files")? != self.data.path() {
            return Err("files_store_changed");
        }
        self.data.revalidate()?;
        self.archives.revalidate()
    }
}
pub(crate) struct LspHost {
    storage: Storage,
    selections: Mutex<archives::Selections>,
    approvals: Mutex<approval::Approvals>,
    protected: ProtectedStorage,
    actor: Mutex<Option<Arc<actor::Actor>>>,
    preparing: std::sync::atomic::AtomicBool,
    activities: approval::Activities,
}
impl LspHost {
    /// The shared component manager/installer must already have been initialized
    /// through the short Files initialization boundary before constructing this.
    pub(crate) fn new(
        app: &tauri::AppHandle,
        host: &Host,
        context: crate::core::context_activity::ContextActivity,
        filesystem: crate::core::context_activity::ContextActivity,
    ) -> Result<Self> {
        let data = Arc::new(MetadataRoot::open(&host.component("files")?)?);
        let archives = Arc::new(data.child("native-lsp-archives")?);
        let protected = ProtectedStorage::from_host(app, host)?;
        Ok(Self {
            storage: Storage { data, archives },
            selections: Mutex::new(archives::Selections::new(protected.clone())),
            approvals: Mutex::new(Default::default()),
            protected,
            actor: Mutex::new(None),
            preparing: Default::default(),
            activities: approval::Activities {
                context,
                filesystem,
                installation: Default::default(),
            },
        })
    }
    pub(crate) fn expire(&self) {
        if let Ok(mut selections) = self.selections.try_lock() {
            selections.expire();
        }
        if let Ok(mut approvals) = self.approvals.try_lock() {
            approvals.expire();
        }
    }
    pub(crate) async fn retire(&self) -> Result<()> {
        let actor = self.actor.lock().map_err(|_| "lsp_unavailable")?.clone();
        if let Some(actor) = actor {
            actor.retire().await?;
            let mut slot = self.actor.lock().map_err(|_| "lsp_unavailable")?;
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &actor))
            {
                *slot = None;
            }
        }
        if self.preparing.load(std::sync::atomic::Ordering::Acquire) {
            return Err("lsp_busy");
        }
        Ok(())
    }
    async fn retire_installation(&self, id: &str, version: &str) -> Result<()> {
        let affected = self
            .actor
            .lock()
            .map_err(|_| "lsp_unavailable")?
            .as_ref()
            .is_some_and(|actor| actor.uses_installation(id, version));
        if affected {
            self.retire().await?;
        }
        Ok(())
    }
    async fn execute_live(
        &self,
        app: &tauri::AppHandle,
        host: &Arc<Host>,
        invocation: Invocation<'_>,
        shutdown: &RequestCancellation,
    ) -> Result<Value> {
        let Invocation {
            method,
            args,
            context,
            deadline,
            start,
        } = invocation;
        let _installation = if actor::starts(method) {
            Some(
                self.activities
                    .installation
                    .enter(false)
                    .map_err(|_| "lsp_install_busy")?,
            )
        } else {
            None
        };
        let Some(context) = context else {
            return actor::idle(method, args);
        };
        let existing = self.actor.lock().map_err(|_| "lsp_unavailable")?.clone();
        let instance = if let Some(instance) = existing {
            instance
        } else {
            if !actor::starts(method) {
                return actor::idle(method, args);
            }
            use std::sync::atomic::{AtomicBool, Ordering};
            struct Preparing<'a>(&'a AtomicBool);
            impl Drop for Preparing<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            self.preparing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| "lsp_busy")?;
            let _preparing = Preparing(&self.preparing);
            let current = self.actor.lock().map_err(|_| "lsp_unavailable")?.clone();
            if let Some(current) = current {
                drop(_preparing);
                if current.context() != context {
                    return Err("lsp_context_changed");
                }
                return current
                    .request(
                        method,
                        args,
                        deadline,
                        start.as_ref().map(|start| start.flag()),
                    )
                    .await;
            }
            let start = start.as_ref().ok_or("invalid_request")?;
            let installer = app.state::<Arc<ManagedInstaller>>().inner().clone();
            let snapshot = actor::start_scope(start.flag(), shutdown.clone(), deadline, async {
                approval::Snapshot::capture(
                    host.clone(),
                    context,
                    &mut Default::default(),
                    installer.clone(),
                    self.protected.clone(),
                    deadline,
                    true,
                )
            })
            .await?;
            if shutdown.is_cancelled() || start.check().is_err() {
                return Err("lsp_operation_cancelled");
            }
            let event_app = app.clone();
            snapshot.bind_activities(self.activities.clone());
            let sink: actor::Events = Arc::new(move |name, value| {
                let _ = event_app.emit_to("main", name, value);
            });
            let instance = Arc::new(actor::Actor::spawn(
                sink,
                snapshot,
                installer,
                shutdown.clone(),
            )?);
            *self.actor.lock().map_err(|_| "lsp_unavailable")? = Some(instance.clone());
            instance
        };
        if instance.context() != context {
            return Err("lsp_context_changed");
        }
        instance
            .request(
                method,
                args,
                deadline,
                start.as_ref().map(|start| start.flag()),
            )
            .await
    }
    pub(crate) fn choose(&self, host: &Host, paths: &[PathBuf], deadline: u64) -> Result<Value> {
        self.storage.revalidate(host)?;
        let tokens = self
            .selections
            .lock()
            .map_err(|_| "lsp_unavailable")?
            .choose(paths, deadline)?;
        self.storage.revalidate(host)?;
        Ok(json!(tokens))
    }
    pub(crate) async fn execute(
        &self,
        app: &tauri::AppHandle,
        host: &Arc<Host>,
        invocation: Invocation<'_>,
        shutdown: &RequestCancellation,
    ) -> Result<Value> {
        let Invocation {
            method,
            args,
            context,
            deadline,
            start,
        } = invocation;
        self.storage.revalidate(host)?;
        if shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        let result = match method {
            "lsp_execution_preview" => {
                if args.as_object().is_none_or(|value| !value.is_empty()) {
                    return Err("invalid_request");
                }
                let snapshot = approval::Snapshot::capture(
                    host.clone(),
                    context.ok_or("project_selection_required")?,
                    &mut Default::default(),
                    app.state::<Arc<ManagedInstaller>>().inner().clone(),
                    self.protected.clone(),
                    deadline,
                    false,
                )?;
                self.approvals
                    .lock()
                    .map_err(|_| "lsp_unavailable")?
                    .preview(snapshot)
            }
            "lsp_execution_approve" | "lsp_execution_cancel" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Token {
                    preview_id: String,
                }
                let token: Token = input(args)?;
                let context = context.ok_or("project_selection_required")?;
                if method == "lsp_execution_approve" {
                    self.retire().await?;
                }
                let mut approvals = self.approvals.lock().map_err(|_| "lsp_unavailable")?;
                if method == "lsp_execution_approve" {
                    approvals.approve(context, &token.preview_id, deadline)
                } else {
                    approvals.cancel(context, &token.preview_id)
                }
            }
            "lsp_execution_revoke" => {
                if args.as_object().is_none_or(|value| !value.is_empty()) {
                    return Err("invalid_request");
                }
                self.retire().await?;
                self.approvals
                    .lock()
                    .map_err(|_| "lsp_unavailable")?
                    .revoke(host, context.ok_or("project_selection_required")?, deadline)
            }
            "load_lsp_config" => {
                if args.as_object().is_none_or(|value| !value.is_empty()) {
                    return Err("invalid_request");
                }
                if let Some(context) = context {
                    settings::Settings::open(host, context)?.view()
                } else {
                    Ok(
                        json!({"config":code_pad_lib::lsp::LspConfig::default(),"persist_allowed":false,"recoveryAllowed":false,"error":null,"nativeRevision":null}),
                    )
                }
            }
            "save_lsp_config" => {
                self.retire().await?;
                settings::Settings::open(host, context.ok_or("project_selection_required")?)?
                    .save(host, args, deadline)
            }
            "discard_lsp_archives" => {
                let choices: Choices = input(args)?;
                self.selections
                    .lock()
                    .map_err(|_| "lsp_unavailable")?
                    .discard(&choices.archive_paths)?;
                Ok(Value::Null)
            }
            "lsp_install" => {
                let key: Key = input(args)?;
                let _installation = self
                    .activities
                    .installation
                    .enter(true)
                    .map_err(|_| "lsp_install_busy")?;
                self.retire_installation(&key.manifest_id, &key.version)
                    .await?;
                let installer = app
                    .try_state::<Arc<ManagedInstaller>>()
                    .ok_or("lsp_unavailable")?
                    .inner()
                    .clone();
                let cancellation = RequestCancellation::new();
                let operation = installer.install_catalog_cancellable(
                    &key.manifest_id,
                    &key.version,
                    &key.platform,
                    &cancellation,
                );
                tokio::pin!(operation);
                let result = tokio::select! {
                    result = &mut operation => result,
                    _ = shutdown.cancelled() => { cancellation.cancel(); operation.await },
                    _ = tokio::time::sleep(Duration::from_secs(600)) => { cancellation.cancel(); operation.await },
                };
                result.map(|_| Value::Null).map_err(installer_error)
            }
            "lsp_import_archive" => {
                let selection: Import = input(args)?;
                let _installation = self
                    .activities
                    .installation
                    .enter(true)
                    .map_err(|_| "lsp_install_busy")?;
                self.retire_installation(&selection.manifest_id, &selection.version)
                    .await?;
                // Resolve trusted keys before consuming or reading chosen files.
                ManagedInstaller::catalog_manifest(
                    &selection.manifest_id,
                    &selection.version,
                    &selection.platform,
                )
                .map_err(installer_error)?;
                let archives = self
                    .selections
                    .lock()
                    .map_err(|_| "lsp_unavailable")?
                    .take(&selection.archive_paths)?;
                let snapshots =
                    archives::Snapshots::create(self.storage.archives.clone(), archives, shutdown)?;
                self.storage.revalidate(host)?;
                if shutdown.is_cancelled() {
                    return Err("lsp_operation_cancelled");
                }
                app.try_state::<Arc<ManagedInstaller>>()
                    .ok_or("lsp_unavailable")?
                    .import_catalog_archives(
                        &selection.manifest_id,
                        &selection.version,
                        &selection.platform,
                        &snapshots.paths(),
                    )
                    .map(|_| Value::Null)
                    .map_err(installer_error)
            }
            _ if actor::allowed(method) => {
                self.execute_live(
                    app,
                    host,
                    Invocation {
                        method,
                        args,
                        context,
                        deadline,
                        start,
                    },
                    shutdown,
                )
                .await
            }
            _ if allowed(method) && method != "pick_lsp_archives" => {
                let _installation = if matches!(method, "lsp_uninstall" | "lsp_recover_installed") {
                    Some(
                        self.activities
                            .installation
                            .enter(true)
                            .map_err(|_| "lsp_install_busy")?,
                    )
                } else {
                    None
                };
                if method == "lsp_uninstall" {
                    let key: Key = input(args.clone())?;
                    self.retire_installation(&key.manifest_id, &key.version)
                        .await?;
                } else if method == "lsp_recover_installed" {
                    self.retire().await?;
                }
                code_pad_lib::component::dispatch(app, method, args)
                    .await
                    .map_err(|_| "lsp_unavailable")
            }
            _ => Err("invalid_request"),
        };
        self.storage.revalidate(host)?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_inputs_remain_closed_and_session_calls_have_separate_admission() {
        for method in [
            "lsp_install",
            "lsp_import_archive",
            "lsp_recover_installed",
            "stop_language_server",
            "save_lsp_config",
            "start_language_server",
            "restart_language_server",
        ] {
            assert!(allowed(method));
        }
        for method in ["apply_lsp_rename", "unknown_lsp_command"] {
            assert!(!allowed(method));
        }
        assert!(input::<Key>(json!({"manifestId":"rust-analyzer", "version":"1", "platform":"windows-x86_64", "url":"https://unreviewed.invalid"})).is_err());
        assert!(input::<Import>(json!({"manifestId":"rust-analyzer", "version":"1", "platform":"windows-x86_64", "archivePaths":[], "destination":"C:/foreign"})).is_err());
    }
}
