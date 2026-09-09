//! Reuse the existing native engine without starting its standalone application.
//! Product initialization and native caller/owner checks precede every dispatch.

// Explicit importer/discovery consumers reuse the original bounded validators.
pub use crate::commands::workspace::{absorb_life_log_projects_in, LifeLogAbsorbReport};
pub use crate::core::profile::{ProfileStore, ProjectProfile, WslProfile};
pub use crate::core::templates::{ProfileTemplate, ProfileTemplateStore};

/// Validate copied settings before a product generation can be selected.
/// This parses bytes only; profile paths, services and distro names are not run.
pub fn validate_persistent_file(name: &str, bytes: &[u8]) -> Result<(), &'static str> {
    let input = std::str::from_utf8(bytes).map_err(|_| "invalid_overview_store")?;
    match name {
        "project-profiles.json" => ProfileStore::load(input).map(|_| ()),
        "profile-templates.json" => ProfileTemplateStore::load(input).map(|_| ()),
        _ => return Err("unknown_overview_store"),
    }
    .map_err(|_| "invalid_overview_store")
}

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;

// One product and one immutable generation per process. Only native startup
// configures these paths; there is no renderer setter or fallback after setup.
struct ProductPaths {
    data: PathBuf,
    common: PathBuf,
}
static PRODUCT_PATHS: OnceLock<ProductPaths> = OnceLock::new();

pub fn is_product() -> bool {
    PRODUCT_PATHS.get().is_some()
}
pub(crate) fn data_root(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    match PRODUCT_PATHS.get() {
        Some(paths) => Ok(paths.data.clone()),
        None => app.path().app_local_data_dir(),
    }
}
pub(crate) fn common_root() -> PathBuf {
    PRODUCT_PATHS
        .get()
        .map(|paths| paths.common.clone())
        .unwrap_or_else(devbox_integration::common_root)
}
pub(crate) fn integration_root() -> PathBuf {
    common_root().join("integration")
}

pub fn initialize(app: &tauri::AppHandle, data: &Path, common: &Path) -> Result<(), String> {
    if !data.is_absolute()
        || !common.is_absolute()
        || !data.is_dir()
        || !common.is_dir()
        || devbox_filesystem::ensure_no_links(data).is_err()
        || devbox_filesystem::ensure_no_links(common).is_err()
        || app.try_state::<crate::applink::PendingOpen>().is_some()
    {
        return Err("component_state_conflict".into());
    }
    PRODUCT_PATHS
        .set(ProductPaths {
            data: data.to_path_buf(),
            common: common.to_path_buf(),
        })
        .map_err(|_| "component_state_conflict")?;
    app.manage(crate::applink::PendingOpen::new());
    app.manage(crate::commands::workspace::run_registry());
    let profiles = crate::commands::workspace::profile_store_state();
    app.manage(profiles.clone());
    crate::integration::spawn_profile_snapshot_writer(app.clone(), profiles);
    Ok(())
}

/// Native producer delivery. The receiver still resolves the target and asks
/// about dirty documents; the event is only a hint to consume this pending item.
pub fn offer_product_open(
    app: &tauri::AppHandle,
    request: devbox_applink::OpenRequest,
) -> Result<(), String> {
    use tauri::Emitter;
    if !is_product() {
        return Err("component_state_unavailable".into());
    }
    devbox_applink::build_argv(&request).map_err(|_| "component_args_invalid")?;
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "devbox://open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

pub const COMMANDS: &[&str] = &[
    "take_pending_open",
    "list_profiles",
    "create_profile",
    "update_profile",
    "delete_profile",
    "git_status",
    "project_health",
    "cancel_project_health",
    "preview_project_environment",
    "cancel_project_environment",
    "workspace_preflight",
    "dependency_health",
    "cancel_workspace_preflight",
    "cancel_dependency_health",
    "package_dependency_summary",
    "cancel_start_workspace",
    "start_workspace",
    "retry_workspace",
    "stop_workspace",
    "current_workspace_run",
    "list_profile_templates",
    "create_profile_template",
    "update_profile_template",
    "delete_profile_template",
    "create_profile_from_template",
    "wsl_runtime_suggestions",
    "profile_open_targets",
    "profile_copy_path",
    "open_profile_in",
    "dispatch_workspace_task_control",
    "list_workspace_task_controls",
    "get_workspace_task_control_receipt",
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if is_product()
        && matches!(
            method,
            "start_workspace"
                | "retry_workspace"
                | "stop_workspace"
                | "cancel_start_workspace"
                | "workspace_preflight"
                | "dependency_health"
                | "profile_open_targets"
                | "open_profile_in"
                | "dispatch_workspace_task_control"
        )
    {
        return Err("provider_unavailable".into());
    }
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "list_profiles" => crate::commands::workspace::__component_list_profiles(app, args).await,
        "create_profile" => crate::commands::workspace::__component_create_profile(app, args).await,
        "update_profile" => crate::commands::workspace::__component_update_profile(app, args).await,
        "delete_profile" => crate::commands::workspace::__component_delete_profile(app, args).await,
        "git_status" => crate::commands::workspace::__component_git_status(app, args).await,
        "project_health" => crate::commands::workspace::__component_project_health(app, args).await,
        "cancel_project_health" => {
            crate::commands::workspace::__component_cancel_project_health(app, args).await
        }
        "preview_project_environment" => {
            crate::commands::environment::__component_preview_project_environment(app, args).await
        }
        "cancel_project_environment" => {
            crate::commands::environment::__component_cancel_project_environment(app, args).await
        }
        "workspace_preflight" => {
            crate::commands::preflight::__component_workspace_preflight(app, args).await
        }
        "dependency_health" => {
            crate::commands::preflight::__component_dependency_health(app, args).await
        }
        "cancel_workspace_preflight" => {
            crate::commands::preflight::__component_cancel_workspace_preflight(app, args).await
        }
        "cancel_dependency_health" => {
            crate::commands::preflight::__component_cancel_dependency_health(app, args).await
        }
        "package_dependency_summary" => {
            crate::commands::dependencies::__component_package_dependency_summary(app, args).await
        }
        "cancel_start_workspace" => {
            crate::commands::workspace::__component_cancel_start_workspace(app, args).await
        }
        "start_workspace" => {
            crate::commands::workspace::__component_start_workspace(app, args).await
        }
        "retry_workspace" => {
            crate::commands::workspace::__component_retry_workspace(app, args).await
        }
        "stop_workspace" => crate::commands::workspace::__component_stop_workspace(app, args).await,
        "current_workspace_run" => {
            crate::commands::workspace::__component_current_workspace_run(app, args).await
        }
        "list_profile_templates" => {
            crate::commands::templates::__component_list_profile_templates(app, args).await
        }
        "create_profile_template" => {
            crate::commands::templates::__component_create_profile_template(app, args).await
        }
        "update_profile_template" => {
            crate::commands::templates::__component_update_profile_template(app, args).await
        }
        "delete_profile_template" => {
            crate::commands::templates::__component_delete_profile_template(app, args).await
        }
        "create_profile_from_template" => {
            crate::commands::templates::__component_create_profile_from_template(app, args).await
        }
        "wsl_runtime_suggestions" => {
            crate::core::runtime_suggestions::__component_wsl_runtime_suggestions(app, args).await
        }
        "profile_open_targets" => {
            crate::commands::profile_actions::__component_profile_open_targets(app, args).await
        }
        "profile_copy_path" => {
            crate::commands::profile_actions::__component_profile_copy_path(app, args).await
        }
        "open_profile_in" => {
            crate::commands::profile_actions::__component_open_profile_in(app, args).await
        }
        "dispatch_workspace_task_control" => {
            crate::commands::task_control::__component_dispatch_workspace_task_control(app, args)
                .await
        }
        "list_workspace_task_controls" => {
            crate::commands::task_control::__component_list_workspace_task_controls(app, args).await
        }
        "get_workspace_task_control_receipt" => {
            crate::commands::task_control::__component_get_workspace_task_control_receipt(app, args)
                .await
        }
        _ => Err("component_method_invalid".into()),
    }
}
