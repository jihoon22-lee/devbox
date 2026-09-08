//! Reuse the existing native engine without starting its standalone application.
//! Product initialization and native caller/owner checks precede every dispatch.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;

// One product and one immutable generation per process. Only native startup
// configures these paths; there is no renderer setter or fallback after setup.
struct ProductPaths {
    common: PathBuf,
}
static PRODUCT_PATHS: OnceLock<ProductPaths> = OnceLock::new();

pub fn is_product() -> bool {
    PRODUCT_PATHS.get().is_some()
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

pub fn initialize(app: &tauri::AppHandle, common: &Path) -> Result<(), String> {
    if !common.is_absolute()
        || !common.is_dir()
        || devbox_filesystem::ensure_no_links(common).is_err()
        || app.try_state::<crate::applink::PendingOpen>().is_some()
    {
        return Err("component_state_conflict".into());
    }
    PRODUCT_PATHS
        .set(ProductPaths {
            common: common.to_path_buf(),
        })
        .map_err(|_| "component_state_conflict")?;
    app.manage(crate::applink::PendingOpen::new());
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
    "scan_root",
    "prepare_inbound_repository",
    "repo_status",
    "worktrees",
    "create_worktree",
    "worktree_clean",
    "repo_cleanup_preview",
    "repo_cleanup",
    "repo_cleanup_cancel",
    "repo_preflight",
    "repo_history",
    "repo_commit_detail",
    "repo_diff",
    "dependency_inventory",
    "dependency_enrichment_preview",
    "dependency_enrichment_execute",
    "repo_changes",
    "repo_stage",
    "repo_unstage",
    "repo_commit",
    "repo_local_cancel",
    "repo_remote_status",
    "repo_fetch",
    "repo_pull",
    "repo_push",
    "repo_remote_cancel",
    "open_targets",
    "open_in",
    "repository_copy_path",
    "open_repository_folder",
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if is_product() && matches!(method, "open_targets" | "open_in") {
        return Err("provider_unavailable".into());
    }
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "scan_root" => crate::commands::__component_scan_root(app, args).await,
        "prepare_inbound_repository" => {
            crate::commands::__component_prepare_inbound_repository(app, args).await
        }
        "repo_status" => crate::commands::__component_repo_status(app, args).await,
        "worktrees" => crate::commands::__component_worktrees(app, args).await,
        "create_worktree" => crate::commands::__component_create_worktree(app, args).await,
        "worktree_clean" => crate::commands::__component_worktree_clean(app, args).await,
        "repo_cleanup_preview" => {
            crate::commands::__component_repo_cleanup_preview(app, args).await
        }
        "repo_cleanup" => crate::commands::__component_repo_cleanup(app, args).await,
        "repo_cleanup_cancel" => crate::commands::__component_repo_cleanup_cancel(app, args).await,
        "repo_preflight" => crate::commands::__component_repo_preflight(app, args).await,
        "repo_history" => crate::commands::__component_repo_history(app, args).await,
        "repo_commit_detail" => crate::commands::__component_repo_commit_detail(app, args).await,
        "repo_diff" => crate::commands::__component_repo_diff(app, args).await,
        "dependency_inventory" => {
            crate::commands::__component_dependency_inventory(app, args).await
        }
        "dependency_enrichment_preview" => {
            crate::commands::dependency_enrichment::__component_dependency_enrichment_preview(
                app, args,
            )
            .await
        }
        "dependency_enrichment_execute" => {
            crate::commands::dependency_enrichment::__component_dependency_enrichment_execute(
                app, args,
            )
            .await
        }
        "repo_changes" => crate::commands::__component_repo_changes(app, args).await,
        "repo_stage" => crate::commands::__component_repo_stage(app, args).await,
        "repo_unstage" => crate::commands::__component_repo_unstage(app, args).await,
        "repo_commit" => crate::commands::__component_repo_commit(app, args).await,
        "repo_local_cancel" => crate::commands::__component_repo_local_cancel(app, args).await,
        "repo_remote_status" => crate::commands::__component_repo_remote_status(app, args).await,
        "repo_fetch" => crate::commands::__component_repo_fetch(app, args).await,
        "repo_pull" => crate::commands::__component_repo_pull(app, args).await,
        "repo_push" => crate::commands::__component_repo_push(app, args).await,
        "repo_remote_cancel" => crate::commands::__component_repo_remote_cancel(app, args).await,
        "open_targets" => crate::commands::__component_open_targets(app, args).await,
        "open_in" => crate::commands::__component_open_in(app, args).await,
        "repository_copy_path" => {
            crate::commands::__component_repository_copy_path(app, args).await
        }
        "open_repository_folder" => {
            crate::commands::__component_open_repository_folder(app, args).await
        }
        _ => Err("component_method_invalid".into()),
    }
}

/// Dependencies accept only a native-created project capability. This adapter
/// never resolves a renderer path or invokes Git to admit a product request.
#[derive(Clone)]
pub struct DependencyAccess {
    pub(crate) root: PathBuf,
    pub(crate) key: String,
    pub(crate) common: PathBuf,
    pub(crate) strict_cache: bool,
    verify: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}
impl DependencyAccess {
    pub fn for_project(
        root: PathBuf,
        key: String,
        common: PathBuf,
        verify: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if !root.is_absolute() || !common.is_absolute() || key.is_empty() {
            return Err("dependency_access_invalid".into());
        }
        let access = Self {
            root,
            key,
            common,
            strict_cache: true,
            verify: std::sync::Arc::new(verify),
        };
        access.verify()?;
        Ok(access)
    }
    pub(crate) fn legacy(path: &str) -> Result<Self, String> {
        crate::commands::legacy_dependency_access(path)
    }
    pub(crate) fn legacy_parts(
        root: PathBuf,
        key: String,
        verify: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            root,
            key,
            common: common_root(),
            strict_cache: false,
            verify: std::sync::Arc::new(verify),
        }
    }
    pub(crate) fn retaining<T: Send + Sync + 'static>(mut self, guard: T) -> Self {
        let verify = self.verify.clone();
        self.verify = std::sync::Arc::new(move || {
            let _retained = &guard;
            verify()
        });
        self
    }
    pub(crate) fn verify(&self) -> Result<(), String> {
        (self.verify)()
    }
}

pub async fn dispatch_dependencies(
    access: DependencyAccess,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use crate::commands::dependency_enrichment as remote;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input<T> {
        request: T,
    }
    fn input<T: serde::de::DeserializeOwned>(args: serde_json::Value) -> Result<T, String> {
        serde_json::from_value::<Input<T>>(args)
            .map(|value| value.request)
            .map_err(|_| "component_args_invalid".into())
    }
    // Legacy UI paths are a projection only, checked for stale UI selection.
    let check_path = |path: &str| {
        if path == access.root.to_string_lossy() {
            Ok(())
        } else {
            Err("dependency_context_changed".to_string())
        }
    };
    match method {
        "dependency_inventory" => {
            let request: crate::commands::DependencyInventoryRequest = input(args)?;
            check_path(&request.path)?;
            serde_json::to_value(crate::commands::dependency_inventory_with_access(access).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        "dependency_enrichment_preview" => {
            let request: remote::DependencyEnrichmentPreviewRequest = input(args)?;
            check_path(&request.path)?;
            serde_json::to_value(
                remote::preview_with_access(access, request.services, request.force_refresh)
                    .await?,
            )
            .map_err(|_| "component_response_invalid".into())
        }
        "dependency_enrichment_execute" => {
            let request: remote::DependencyEnrichmentExecuteRequest = input(args)?;
            check_path(&request.path)?;
            serde_json::to_value(remote::execute_with_access(access, request.preview_token).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        "dependency_enrichment_cancel" => {
            let request: remote::DependencyEnrichmentExecuteRequest = input(args)?;
            check_path(&request.path)?;
            tauri::async_runtime::spawn_blocking(move || {
                remote::cancel_with_access(&access, &request.preview_token)
            })
            .await
            .map_err(|_| "component_worker_unavailable")??;
            Ok(serde_json::json!({}))
        }
        _ => Err("component_method_invalid".into()),
    }
}

/// Only fixed issue codes cross into the product's provenance-checked response.
pub fn dependency_issue(error: &str) -> &'static str {
    use crate::core::dependency_enrichment::{
        DEPENDENCY_ENRICHMENT_BUSY, DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED,
    };
    match error {
        DEPENDENCY_ENRICHMENT_BUSY => "dependency_busy",
        DEPENDENCY_ENRICHMENT_REVIEW_REQUIRED => "dependency_review_required",
        "request_expired" => "request_expired",
        "dependency_context_changed"
        | "project_object_changed"
        | "project_binding_changed"
        | "stale_context" => "dependency_context_changed",
        _ => "dependency_operation_failed",
    }
}
