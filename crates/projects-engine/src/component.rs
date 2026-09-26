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
    devbox_applink::validate_request(&request).map_err(|_| "component_args_invalid")?;
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "devbox://open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}
