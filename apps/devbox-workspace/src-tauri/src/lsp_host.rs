//! Independent native LSP metadata/installer owner. Installing a reviewed
//! artifact never grants language-server execution or editor file access.
mod archives;
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
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct Invocation<'a> {
    pub method: &'a str,
    pub args: Value,
    pub context: Option<&'a product_contract::ProjectContext>,
    pub deadline: u64,
}

pub(crate) fn allowed(method: &str) -> bool {
    matches!(
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
            | "language_server_statuses"
            | "language_server_logs"
            | "stop_language_server"
            | "stop_all_language_servers"
            | "close_lsp_document"
    )
}
pub(crate) fn contextual(method: &str) -> bool {
    matches!(method, "load_lsp_config" | "save_lsp_config")
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
}
impl LspHost {
    /// The shared component manager/installer must already have been initialized
    /// through the short Files initialization boundary before constructing this.
    pub(crate) fn new(app: &tauri::AppHandle, host: &Host) -> Result<Self> {
        let data = Arc::new(MetadataRoot::open(&host.component("files")?)?);
        let archives = Arc::new(data.child("native-lsp-archives")?);
        Ok(Self {
            storage: Storage { data, archives },
            selections: Mutex::new(archives::Selections::new(ProtectedStorage::from_host(
                app, host,
            )?)),
        })
    }
    pub(crate) fn expire(&self) {
        if let Ok(mut selections) = self.selections.try_lock() {
            selections.expire();
        }
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
        host: &Host,
        invocation: Invocation<'_>,
        shutdown: &RequestCancellation,
    ) -> Result<Value> {
        let Invocation {
            method,
            args,
            context,
            deadline,
        } = invocation;
        self.storage.revalidate(host)?;
        if shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        let result = match method {
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
            _ if allowed(method) && method != "pick_lsp_archives" => {
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
    fn installation_and_cleanup_do_not_admit_execution_or_renderer_manifests() {
        for method in [
            "lsp_install",
            "lsp_import_archive",
            "lsp_recover_installed",
            "stop_language_server",
            "save_lsp_config",
        ] {
            assert!(allowed(method));
        }
        for method in [
            "start_language_server",
            "restart_language_server",
            "apply_lsp_rename",
        ] {
            assert!(!allowed(method));
        }
        assert!(input::<Key>(json!({"manifestId":"rust-analyzer", "version":"1", "platform":"windows-x86_64", "url":"https://unreviewed.invalid"})).is_err());
        assert!(input::<Import>(json!({"manifestId":"rust-analyzer", "version":"1", "platform":"windows-x86_64", "archivePaths":[], "destination":"C:/foreign"})).is_err());
    }
}
