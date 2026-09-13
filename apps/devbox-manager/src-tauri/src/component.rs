//! Embedded Manager tools only. The suite's installer does not use legacy batch
//! install, cleanup_partials, startup migration or a legacy executable fallback.
use crate::commands::{dev_setup, diagnostics, doctor, local_quality, related_tools};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
pub(crate) struct EmbeddedTools;

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if !app.manage(EmbeddedTools)
        || !app.manage(diagnostics::DiagnosticsState::default())
        || !app.manage(dev_setup::DevSetupConfigurationState::default())
    {
        return Err("manager_state_conflict".into());
    }
    Ok(())
}
pub fn allowed(route: &str, method: &str) -> bool {
    match route {
        "diagnostics" | "recovery" => matches!(
            method,
            "run_diagnosis"
                | "inspect_local_quality"
                | "inspect_data_databases"
                | "preview_data_query"
                | "cancel_data_diagnostics"
                | "export_data_preview"
                | "preview_support_bundle"
                | "cancel_support_bundle"
                | "export_support_bundle"
        ),
        "environment" => matches!(
            method,
            "dev_setup_audit"
                | "import_dev_setup_configuration"
                | "discard_dev_setup_configuration"
                | "export_dev_setup_configuration"
                | "apply_dev_setup_configuration"
                | "cancel_dev_setup_apply"
        ),
        "tools" => matches!(
            method,
            "related_tools" | "install_related_tool" | "launch_related_tool" | "open_related_url"
        ),
        _ => false,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nested<T> {
    request: T,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Operation {
    operation_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preview {
    preview_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Tool {
    tool_id: String,
}
fn input<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    serde_json::from_value(args).map_err(|_| "manager_args_invalid".into())
}
fn nested<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
    Ok(input::<Nested<T>>(args)?.request)
}
fn value<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
    serde_json::to_value(result?).map_err(|_| "manager_response_invalid".into())
}
fn empty(args: Value) -> Result<(), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Empty {}
    let _: Empty = input(args)?;
    Ok(())
}
pub async fn dispatch(
    app: &tauri::AppHandle,
    route: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    if !allowed(route, method) {
        return Err("manager_method_denied".into());
    }
    match method {
        "run_diagnosis" => {
            empty(args)?;
            value(doctor::run_diagnosis(app.clone()).await)
        }
        "inspect_local_quality" => {
            empty(args)?;
            value(local_quality::inspect_local_quality(app.clone()).await)
        }
        "inspect_data_databases" => value(
            diagnostics::inspect_data_databases(
                app.state(),
                input::<Operation>(args)?.operation_id,
            )
            .await,
        ),
        "preview_data_query" => {
            value(diagnostics::preview_data_query(app.state(), nested(args)?).await)
        }
        "cancel_data_diagnostics" => value(diagnostics::cancel_data_diagnostics(
            app.state(),
            nested(args)?,
        )),
        "export_data_preview" => {
            value(diagnostics::export_data_preview(app.state(), nested(args)?).await)
        }
        "preview_support_bundle" => value(
            diagnostics::preview_support_bundle(
                app.clone(),
                app.state(),
                input::<Operation>(args)?.operation_id,
            )
            .await,
        ),
        "cancel_support_bundle" => value(diagnostics::cancel_support_bundle(
            app.state(),
            nested(args)?,
        )),
        "export_support_bundle" => value(
            diagnostics::export_support_bundle(app.state(), input::<Preview>(args)?.preview_id)
                .await,
        ),
        "open_related_url" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Url {
                url: String,
            }
            let url = input::<Url>(args)?.url;
            let allowed = crate::core::related_tools::CURATED_TOOLS
                .iter()
                .any(|tool| tool.official_url == url || tool.license_url == url);
            if !allowed {
                return Err("manager_url_denied".into());
            }
            use tauri_plugin_opener::OpenerExt;
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|_| "manager_url_unavailable")?;
            Ok(Value::Null)
        }
        "related_tools" => {
            empty(args)?;
            value(related_tools::related_tools().await)
        }
        "dev_setup_audit" => {
            empty(args)?;
            value(related_tools::dev_setup_audit().await)
        }
        "install_related_tool" => value(related_tools::install_related_tool(nested(args)?).await),
        "launch_related_tool" => {
            value(related_tools::launch_related_tool(input::<Tool>(args)?.tool_id).await)
        }
        "import_dev_setup_configuration" => {
            empty(args)?;
            value(dev_setup::import_dev_setup_configuration(app.clone(), app.state()).await)
        }
        "discard_dev_setup_configuration" => value(dev_setup::discard_dev_setup_configuration(
            app.state(),
            nested(args)?,
        )),
        "export_dev_setup_configuration" => value(dev_setup::export_dev_setup_configuration(
            app.state(),
            nested(args)?,
        )),
        "apply_dev_setup_configuration" => value(
            dev_setup::apply_dev_setup_configuration(app.clone(), app.state(), nested(args)?).await,
        ),
        "cancel_dev_setup_apply" => {
            empty(args)?;
            value(dev_setup::cancel_dev_setup_apply(app.state()))
        }
        _ => Err("manager_method_denied".into()),
    }
}

/// Existing Manager-owned registrations are read from their original namespace;
/// the Control Center host must not reinterpret its own empty app-data directory
/// as an empty legacy installation inventory.
pub(crate) fn legacy_default_root(
    app: &tauri::AppHandle,
) -> Result<Option<std::path::PathBuf>, String> {
    if app.try_state::<EmbeddedTools>().is_none() {
        return Ok(None);
    }
    app.path()
        .local_data_dir()
        .map(|root| Some(root.join("com.devbox.devboxmanager")))
        .map_err(|_| "legacy_manager_root_unavailable".into())
}
pub fn legacy_installations(app: &tauri::AppHandle) -> Result<Value, String> {
    value(crate::commands::manager::installed(app.clone()))
}

pub(crate) fn legacy_catalog_revision(app: &tauri::AppHandle) -> Option<u64> {
    app.try_state::<EmbeddedTools>().map(|_| 18)
}
