//! Reuse the existing native engine without starting its standalone application.
//! Product initialization and native caller/owner checks precede every dispatch.

use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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
#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if is_product() && matches!(method, "open_targets" | "open_in") {
        return Err("provider_unavailable".into());
    }
    if SOURCE_COMMANDS.contains(&method) {
        return dispatch_source_method(method, args).await;
    }
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "scan_root" => crate::commands::__component_scan_root(args).await,
        "prepare_inbound_repository" => {
            crate::commands::__component_prepare_inbound_repository(args).await
        }
        "dependency_inventory" => crate::commands::__component_dependency_inventory(args).await,
        "dependency_enrichment_preview" => {
            crate::commands::dependency_enrichment::__component_dependency_enrichment_preview(args)
                .await
        }
        "dependency_enrichment_execute" => {
            crate::commands::dependency_enrichment::__component_dependency_enrichment_execute(args)
                .await
        }
        "open_targets" => crate::commands::__component_open_targets(args).await,
        "open_in" => crate::commands::__component_open_in(args).await,
        "repository_copy_path" => crate::commands::__component_repository_copy_path(args).await,
        "open_repository_folder" => {
            crate::commands::__component_open_repository_folder(app, args).await
        }
        _ => Err("component_method_invalid".into()),
    }
}

async fn dispatch_source_method(
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "create_worktree" => crate::commands::__component_create_worktree(args).await,
        "repo_status" => crate::commands::__component_repo_status(args).await,
        "worktrees" => crate::commands::__component_worktrees(args).await,
        "worktree_clean" => crate::commands::__component_worktree_clean(args).await,
        "repo_preflight" => crate::commands::__component_repo_preflight(args).await,
        "repo_history" => crate::commands::__component_repo_history(args).await,
        "repo_commit_detail" => crate::commands::__component_repo_commit_detail(args).await,
        "repo_diff" => crate::commands::__component_repo_diff(args).await,
        "repo_changes" => crate::commands::__component_repo_changes(args).await,
        "repo_stage" => crate::commands::__component_repo_stage(args).await,
        "repo_unstage" => crate::commands::__component_repo_unstage(args).await,
        "repo_commit" => crate::commands::__component_repo_commit(args).await,
        "repo_local_cancel" => crate::commands::__component_repo_local_cancel(args).await,
        "repo_remote_status" => crate::commands::__component_repo_remote_status(args).await,
        "repo_fetch" => crate::commands::__component_repo_fetch(args).await,
        "repo_pull" => crate::commands::__component_repo_pull(args).await,
        "repo_push" => crate::commands::__component_repo_push(args).await,
        "repo_remote_cancel" => crate::commands::__component_repo_remote_cancel(args).await,
        "repo_cleanup_preview" => crate::commands::__component_repo_cleanup_preview(args).await,
        "repo_cleanup" => crate::commands::__component_repo_cleanup(args).await,
        "repo_cleanup_cancel" => crate::commands::__component_repo_cleanup_cancel(args).await,
        _ => Err("component_method_invalid".into()),
    }
}

pub const SOURCE_COMMANDS: &[&str] = &[
    "create_worktree",
    "repo_status",
    "worktrees",
    "worktree_clean",
    "repo_preflight",
    "repo_history",
    "repo_commit_detail",
    "repo_diff",
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
    "repo_cleanup_preview",
    "repo_cleanup",
    "repo_cleanup_cancel",
];
pub fn source_cancel(method: &str) -> bool {
    matches!(
        method,
        "repo_local_cancel" | "repo_remote_cancel" | "repo_cleanup_cancel"
    )
}
fn bind_source_operation(key: &str, args: &mut serde_json::Value) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let Some(request) = args
        .get_mut("request")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    if let Some(id) = request.get("operationId") {
        let id = id
            .as_str()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
            .ok_or("component_args_invalid")?;
        let mut hash = Sha256::new();
        hash.update((key.len() as u64).to_be_bytes());
        hash.update(key.as_bytes());
        hash.update(id.as_bytes());
        let id: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        request.insert("operationId".into(), id.into());
    }
    Ok(())
}
pub struct SourceAccess {
    root: PathBuf,
    key: String,
    policy: std::sync::Arc<devbox_git::execution::ExecutionPolicy>,
    creation: Option<SourceCreation>,
}
pub(crate) type SourceTargetValidation =
    std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
pub struct SourceCreation {
    pub(crate) branch: String,
    pub(crate) target_dir: String,
    pub(crate) operation_id: String,
    pub(crate) validate: SourceTargetValidation,
}
impl SourceCreation {
    pub fn new(
        branch: String,
        target_dir: String,
        operation_id: String,
        validate: impl Fn() -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if !source_branch_valid(&branch) {
            return Err("worktree_branch_invalid".into());
        }
        Ok(Self {
            branch,
            target_dir,
            operation_id,
            validate: std::sync::Arc::new(validate),
        })
    }
}
pub fn source_branch_valid(branch: &str) -> bool {
    crate::commands::valid_worktree_branch(branch)
}
impl SourceAccess {
    pub fn for_project(
        root: PathBuf,
        key: String,
        policy: std::sync::Arc<devbox_git::execution::ExecutionPolicy>,
    ) -> Self {
        Self {
            root,
            key,
            policy,
            creation: None,
        }
    }
    pub fn with_creation(mut self, creation: SourceCreation) -> Self {
        self.creation = Some(creation);
        self
    }
}
pub async fn dispatch_source_cancel_native(
    key: &str,
    method: &str,
    mut args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !source_cancel(method) {
        return Err("component_method_invalid".into());
    }
    bind_source_operation(key, &mut args)?;
    dispatch_source_method(method, args).await
}
pub async fn dispatch_source_native(
    mut access: SourceAccess,
    method: &str,
    mut args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !SOURCE_COMMANDS.contains(&method) || source_cancel(method) {
        return Err("component_method_invalid".into());
    }
    if method == "create_worktree" {
        if !args.as_object().is_some_and(|object| object.is_empty()) {
            return Err("component_args_invalid".into());
        }
        let mut creation = access.creation.take().ok_or("worktree_review_required")?;
        let mut operation = serde_json::json!({"request":{"operationId":creation.operation_id}});
        bind_source_operation(&access.key, &mut operation)?;
        creation.operation_id = operation["request"]["operationId"]
            .as_str()
            .ok_or("component_args_invalid")?
            .to_owned();
        let _cancel = access.policy.cancel_on_drop();
        let root = access
            .root
            .to_str()
            .ok_or("source_context_changed")?
            .to_owned();
        let result = access
            .policy
            .scope_future(crate::commands::create_reviewed_worktree(root, creation))
            .await?;
        return serde_json::to_value(result).map_err(|_| "component_response_invalid".into());
    }
    let path = if matches!(method, "repo_status" | "worktrees" | "worktree_clean") {
        args.get("path")
    } else {
        args.get("request").and_then(|request| request.get("path"))
    };
    if path.and_then(serde_json::Value::as_str) != access.root.to_str() {
        return Err("source_context_changed".into());
    }
    bind_source_operation(&access.key, &mut args)?;
    let _cancel = access.policy.cancel_on_drop();
    access
        .policy
        .scope_future(dispatch_source_method(method, args))
        .await
}

/// Desktop startup and its immutable generation remain mandatory on Windows.
#[cfg(feature = "desktop")]
pub async fn dispatch_source(
    _app: &tauri::AppHandle,
    access: SourceAccess,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !is_product() {
        return Err("component_method_invalid".into());
    }
    dispatch_source_native(access, method, args).await
}
#[cfg(feature = "desktop")]
pub async fn dispatch_source_cancel(
    _app: &tauri::AppHandle,
    key: &str,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    dispatch_source_cancel_native(key, method, args).await
}

/// Dependencies accept only a native-created project capability. This adapter
/// never resolves a renderer path or invokes Git to admit a product request.
#[derive(Clone)]
pub struct DependencyAccess {
    pub(crate) root: PathBuf,
    pub(crate) key: String,
    pub(crate) common: PathBuf,
    pub(crate) strict_cache: bool,
    inventory: Option<std::sync::Arc<NativeInventory>>,
    verify: std::sync::Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}
type NativeInventory =
    dyn Fn(std::time::Duration) -> Result<serde_json::Value, String> + Send + Sync;

/// The native helper supplies filesystem admission; this function only parses
/// local files. It never publishes Windows metadata or enables remote queries.
pub fn native_dependency_inventory(
    root: &Path,
    budget: std::time::Duration,
    admit: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    let report =
        crate::core::dependency_lens::analyze_repository_with_admission(root, budget, admit)?;
    serde_json::to_value(report).map_err(|_| "dependency_operation_failed".into())
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
            inventory: None,
            verify: std::sync::Arc::new(verify),
        };
        access.verify()?;
        Ok(access)
    }
    /// A POSIX root is only a UI selection key on Windows. The mandatory native
    /// reader owns all filesystem access; the common directory stays on Windows.
    pub fn for_native_inventory(
        root: String,
        key: String,
        common: PathBuf,
        verify: impl Fn() -> Result<(), String> + Send + Sync + 'static,
        read: impl Fn(std::time::Duration) -> Result<serde_json::Value, String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if !devbox_filesystem::parse_safe_project_path(&root)
            .is_some_and(|path| path.kind() == devbox_filesystem::ProjectPathKind::Posix)
            || !common.is_absolute()
            || key.is_empty()
        {
            return Err("dependency_access_invalid".into());
        }
        let access = Self {
            root: root.into(),
            key,
            common,
            strict_cache: true,
            inventory: Some(std::sync::Arc::new(read)),
            verify: std::sync::Arc::new(verify),
        };
        access.verify()?;
        Ok(access)
    }
    pub(crate) fn analyze(
        &self,
        budget: std::time::Duration,
    ) -> Result<crate::core::dependency_lens::DependencyReport, String> {
        self.verify()?;
        let report = match &self.inventory {
            Some(read) => crate::core::dependency_lens::decode_native_report(read(budget)?)?,
            None => crate::core::dependency_lens::analyze_repository(&self.root, budget)?,
        };
        self.verify()?;
        Ok(report)
    }
    #[cfg(any(feature = "desktop", test))]
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
            inventory: None,
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
            crate::runtime::spawn_blocking(move || {
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

#[cfg(test)]
mod source_tests {
    use super::*;
    #[test]
    fn native_source_dispatch_requires_the_exact_root_and_its_execution_admission() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let root = tempfile::tempdir().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let access = || {
            let observed = count.clone();
            SourceAccess::for_project(
                root.path().into(),
                "native-scope".into(),
                devbox_git::execution::ExecutionPolicy::new(
                    std::env::current_exe().unwrap(),
                    vec![],
                    std::time::Instant::now() + std::time::Duration::from_secs(5),
                    move |_| {
                        observed.fetch_add(1, Ordering::SeqCst);
                        Err("fixture admission denied".into())
                    },
                )
                .unwrap(),
            )
        };
        assert_eq!(
            crate::runtime::block_on(dispatch_source_native(
                access(),
                "repo_changes",
                serde_json::json!({"request":{"path":"unregistered"}})
            ))
            .unwrap_err(),
            "source_context_changed"
        );
        assert_eq!(
            crate::runtime::block_on(dispatch_source_native(
                access(),
                "scan_root",
                serde_json::json!({"root":root.path()})
            ))
            .unwrap_err(),
            "component_method_invalid"
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(crate::runtime::block_on(dispatch_source_native(
            access(),
            "repo_changes",
            serde_json::json!({"request":{"path":root.path()}})
        ))
        .is_err());
        assert!(count.load(Ordering::SeqCst) > 0);
        assert!(devbox_git::execution::current().is_none());
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }
    #[test]
    fn cancellation_ids_are_bound_to_the_native_context_and_other_arguments_stay_literal() {
        let original = serde_json::json!({"request":{"path":"C:\\selected","paths":["literal.txt"],"operationId":"operation-1"}});
        let mut first = original.clone();
        let mut same = original.clone();
        let mut other = original;
        bind_source_operation("native-context-a", &mut first).unwrap();
        bind_source_operation("native-context-a", &mut same).unwrap();
        bind_source_operation("native-context-b", &mut other).unwrap();
        assert_eq!(first, same);
        assert_ne!(
            first["request"]["operationId"],
            other["request"]["operationId"]
        );
        assert_eq!(first["request"]["path"], "C:\\selected");
        assert_eq!(
            first["request"]["paths"],
            serde_json::json!(["literal.txt"])
        );
        let mut injected = serde_json::json!({"request":{"operationId":"../untrusted;value"}});
        assert_eq!(
            bind_source_operation("native-context-a", &mut injected).unwrap_err(),
            "component_args_invalid"
        );
    }
}

#[cfg(test)]
mod worktree_admission_tests {
    #[test]
    fn native_destination_revalidation_precedes_legacy_target_or_repository_io() {
        let request = super::SourceCreation::new(
            "fixture-branch".into(),
            "invalid-relative-target".into(),
            "review-order-fixture".into(),
            || Err("review changed before IO".into()),
        )
        .unwrap();
        let result = crate::runtime::block_on(crate::commands::create_reviewed_worktree(
            "invalid-relative-repository".into(),
            request,
        ));
        assert_eq!(result.unwrap_err(), "review changed before IO");
    }
}
