//! Native component entry points; the product host admits caller and operation.
//! Calling these does not start the standalone application or select its stores.

mod imports;
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Emitter, Manager};

#[cfg(feature = "desktop")]
mod control;
pub use crate::core::runtime_controls::legacy_control_method;
mod logs;
pub use logs::OwnedRunLog;

pub async fn owning_task(
    app: &tauri::AppHandle,
    process: crate::scheduler::ObservedProcess,
) -> Result<Option<String>, String> {
    data_root(app)?;
    let runtime = app
        .try_state::<Arc<crate::lifecycle::RuntimeState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    let result = runtime
        .coordinator()
        .owning_task(process)
        .await
        .map_err(|_| "process_owner_unsettled")?;
    data_root(app)?;
    Ok(result)
}

#[derive(Clone)]
pub struct ProcessOwner {
    pub task_id: String,
    pub run_id: String,
    pub label: String,
    pub logs_available: bool,
}
pub async fn process_owner(
    app: &tauri::AppHandle,
    process: crate::scheduler::ObservedProcess,
) -> Result<Option<ProcessOwner>, String> {
    data_root(app)?;
    let runtime = app
        .try_state::<Arc<crate::lifecycle::RuntimeState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    let Some((task_id, run_id)) = runtime
        .coordinator()
        .owning_run(process)
        .await
        .map_err(|_| "process_owner_unsettled")?
    else {
        return Ok(None);
    };
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    let run = database
        .get_run(&run_id)
        .map_err(|_| "process_owner_unsettled")?
        .ok_or("process_owner_unsettled")?;
    if run.job_id != task_id
        || !matches!(
            run.status,
            crate::core::models::RunStatus::Starting
                | crate::core::models::RunStatus::Running
                | crate::core::models::RunStatus::Stopping
        )
    {
        return Err("process_owner_unsettled".into());
    }
    let job = database
        .get_run_job(&task_id)
        .map_err(|_| "process_owner_unsettled")?
        .ok_or("process_owner_unsettled")?;
    let label = if job.name.len() <= 256
        && !job.name.chars().any(char::is_control)
        && !devbox_applink::contains_sensitive_value(&job.name)
    {
        job.name
    } else {
        "Runtime task".into()
    };
    data_root(app)?;
    Ok(Some(ProcessOwner {
        task_id,
        run_id,
        label,
        logs_available: run.log_dir.is_some() && run.logs_deleted_at.is_none(),
    }))
}

pub struct DiagnosticTarget {
    pub path: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
    pub run_id: String,
    pub revision: String,
}
pub async fn diagnostic_target(
    app: &tauri::AppHandle,
    run_id: &str,
    index: u32,
) -> Result<DiagnosticTarget, String> {
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    let lease = log_descriptor(app, run_id)?;
    let (execution, diagnostics) =
        crate::commands::workspace_task_diagnostics_for_run(app, &database, run_id).await?;
    let diagnostic = diagnostics
        .items
        .iter()
        .find(|item| item.index == index)
        .ok_or("runtime_diagnostic_invalid")?;
    let path = crate::core::workspace_diagnostics::resolve_workspace_diagnostic_path(
        &execution.source_root,
        &diagnostic.file,
    )
    .map_err(str::to_owned)?;
    crate::core::workspace_tasks::verify_workspace_task_execution(&execution)
        .map_err(|_| "workspace-task-source-changed")?;
    lease.revalidate()?;
    Ok(DiagnosticTarget {
        path,
        line: diagnostic.line,
        column: diagnostic.column,
        run_id: run_id.into(),
        revision: lease.revision().into(),
    })
}

pub fn port_bindings(
    app: &tauri::AppHandle,
) -> Result<Vec<devbox_integration::PortBindingEntry>, String> {
    if !is_initialized(app) {
        return Err("component_state_unavailable".into());
    }
    data_root(app)?;
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    let services = database
        .list_services()
        .map_err(|_| "runtime_observation_unavailable")?;
    let entries = crate::integration::build_port_binding_entries(&database, services)?;
    data_root(app)?;
    Ok(entries)
}

pub fn imported_log_descriptor(
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<OwnedRunLog, String> {
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    if !database
        .imported_run(run_id)
        .map_err(|_| "runtime_log_unavailable")?
    {
        return Err("runtime_log_unavailable".into());
    }
    log_descriptor(app, run_id)
}

pub fn log_descriptor(app: &tauri::AppHandle, run_id: &str) -> Result<OwnedRunLog, String> {
    if !is_initialized(app) {
        return Err("component_state_unavailable".into());
    }
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    OwnedRunLog::open(database, &data_root(app)?, run_id)
}

struct ProductRoot {
    path: PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    _handle: File,
}
impl ProductRoot {
    fn open(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("component_storage_unavailable".into());
        }
        devbox_filesystem::ensure_no_links(path).map_err(|_| "component_storage_unavailable")?;
        let (handle, identity) = devbox_filesystem::open_filesystem_object(path, true)
            .map_err(|_| "component_storage_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn checked(&self) -> Result<PathBuf, String> {
        devbox_filesystem::ensure_no_links(&self.path).map_err(|_| "component_storage_changed")?;
        if devbox_filesystem::filesystem_identity(&self.path, true)
            .map_err(|_| "component_storage_changed")?
            != self.identity
        {
            return Err("component_storage_changed".into());
        }
        Ok(self.path.clone())
    }
}
struct ProductFile {
    path: PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    _handle: File,
}
impl ProductFile {
    fn open(path: &Path) -> Result<Self, String> {
        devbox_filesystem::ensure_no_links(path).map_err(|_| "component_storage_unavailable")?;
        let (handle, identity) = devbox_filesystem::open_filesystem_object(path, false)
            .map_err(|_| "component_storage_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn check(&self) -> Result<(), String> {
        devbox_filesystem::ensure_no_links(&self.path).map_err(|_| "component_storage_changed")?;
        if devbox_filesystem::filesystem_identity(&self.path, false)
            .map_err(|_| "component_storage_changed")?
            != self.identity
        {
            return Err("component_storage_changed".into());
        }
        Ok(())
    }
}
struct ProductPaths {
    data: ProductRoot,
    common: ProductRoot,
    database: Option<ProductFile>,
}

pub(crate) fn data_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(paths) = app.try_state::<ProductPaths>() {
        paths
            .database
            .as_ref()
            .ok_or("component_state_unavailable")?
            .check()?;
        return paths.data.checked();
    }
    // A partly initialized product must never fall through into its executable
    // root or a legacy namespace, even when Cargo unifies standalone features.
    if app.config().identifier != "com.devbox.runmanager" {
        return Err("component_state_unavailable".into());
    }
    app.path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable".into())
}

pub fn common_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.try_state::<ProductPaths>()
        .ok_or("component_state_unavailable")?
        .common
        .checked()
}

/// Called once by the product's serialized native initialization, before any
/// command is admitted. Importing definitions never calls this initializer.
/// No standalone migration, tray, integration writer or legacy path is used.
pub fn initialize(
    app: &tauri::AppHandle,
    data: &Path,
    common: &Path,
    legacy_base: &Path,
) -> Result<(), String> {
    use crate::{
        core::imports::ImportOperationRegistry, lifecycle::RuntimeState, storage::DatabaseState,
    };
    if app.try_state::<ProductPaths>().is_some()
        || app.try_state::<Arc<DatabaseState>>().is_some()
        || app.try_state::<Arc<RuntimeState>>().is_some()
        || app.try_state::<Arc<ImportOperationRegistry>>().is_some()
        || app
            .try_state::<crate::platform::environment::EnvironmentProtectorState>()
            .is_some()
        || app.try_state::<crate::applink::PendingOpen>().is_some()
        || app
            .try_state::<crate::task_control::PendingTaskControl>()
            .is_some()
    {
        return Err("component_state_conflict".into());
    }
    let mut paths = ProductPaths {
        data: ProductRoot::open(data)?,
        common: ProductRoot::open(common)?,
        database: None,
    };
    if paths.data.identity == paths.common.identity {
        return Err("component_storage_conflict".into());
    }
    for path in [data.join("logs"), data.join("logs/runs")] {
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err("component_storage_unavailable".into()),
        }
        ProductRoot::open(&path)?;
    }
    let database_path = data.join("data.db");
    // Reject links before SQLite can open a database, WAL or shared-memory file.
    for name in ["data.db", "data.db-wal", "data.db-shm"] {
        let path = data.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                devbox_filesystem::ensure_no_links(&path)
                    .map_err(|_| "component_storage_unavailable")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("component_storage_unavailable".into()),
        }
    }
    let database = Arc::new(
        DatabaseState::open_product(&database_path).map_err(|_| "component_storage_unavailable")?,
    );
    paths.database = Some(ProductFile::open(&database_path)?);
    if !database.is_ready() {
        return Err("component_storage_unavailable".into());
    }
    database
        .interrupt_workspace_task_operations_at(crate::storage::current_epoch_millis())
        .map_err(|_| "component_recovery_unavailable")?;
    paths.data.checked()?;
    paths.common.checked()?;
    let protector = crate::platform::environment::EnvironmentProtectorState::new();
    let adapter = Arc::new(crate::platform::execution::PlatformExecutionAdapter::new(
        database.clone(),
        protector.clone(),
        data.to_path_buf(),
    ));
    let listener = Arc::new(crate::notifications::SchedulerNotificationListener::new(
        app.clone(),
        database.clone(),
    ));
    let coordinator = crate::scheduler::SchedulerCoordinator::with_config_and_listener(
        database.clone(),
        adapter,
        crate::scheduler::SchedulerConfig::default()
            .with_shutdown_timeout(std::time::Duration::from_secs(5)),
        listener,
    );
    let background =
        crate::lifecycle::is_background_launch(&std::env::args_os().collect::<Vec<_>>());
    let runtime = Arc::new(RuntimeState::new(database_path, background, coordinator));
    // The product owns normal exit; this hook only handles Windows session end.
    let window = app
        .get_webview_window("main")
        .ok_or("component_window_unavailable")?;
    crate::platform::install_session_end_hook(&window, app, runtime.clone())
        .map_err(|_| "component_shutdown_hook_unavailable")?;
    let import_owner = Arc::new(imports::ImportOwner::new(data, legacy_base)?);
    app.manage(import_owner);
    app.manage(paths);
    app.manage(database.clone());
    app.manage(Arc::new(ImportOperationRegistry::default()));
    app.manage(protector);
    app.manage(crate::applink::PendingOpen::new());
    app.manage(crate::task_control::PendingTaskControl::new());
    app.manage(runtime.clone());
    crate::lifecycle::spawn_scheduler(runtime.clone());
    crate::lifecycle::spawn_maintenance(runtime, database, app.clone(), data.to_path_buf());
    Ok(())
}

pub fn is_initialized(app: &tauri::AppHandle) -> bool {
    app.try_state::<ProductPaths>().is_some()
}

/// Resolves only after owned process trees and scheduler writes are retired.
/// The Workspace exit owner must also finish its other components before exit.
pub fn request_shutdown(app: &tauri::AppHandle) {
    if let Some(owner) = app.try_state::<Arc<imports::ImportOwner>>() {
        owner.request_shutdown();
    }
    if let Some(runtime) = app.try_state::<Arc<crate::lifecycle::RuntimeState>>() {
        runtime.request_shutdown();
    }
}

pub async fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    let runtime = app
        .try_state::<Arc<crate::lifecycle::RuntimeState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone();
    if let Some(owner) = app.try_state::<Arc<imports::ImportOwner>>() {
        let owner = owner.inner().clone();
        tauri::async_runtime::spawn_blocking(move || owner.join())
            .await
            .map_err(|_| "runtime_import_failed")?;
    }
    crate::lifecycle::shutdown_owner(&runtime).await;
    Ok(())
}

pub fn offer_product_open(
    app: &tauri::AppHandle,
    request: devbox_applink::OpenRequest,
) -> Result<(), String> {
    data_root(app)?;
    if !is_initialized(app) || !crate::applink::is_supported_request(&request) {
        return Err("component_args_invalid".into());
    }
    app.try_state::<crate::applink::PendingOpen>()
        .ok_or("component_state_unavailable")?
        .set(request.clone());
    app.emit_to("main", "workspace://tasks-open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

pub const COMMANDS: &[&str] = &[
    "runtime_import_prepare",
    "runtime_import_resume",
    "runtime_import_apply",
    "runtime_import_cancel",
    "runtime_import_status",
    "runtime_import_catalog",
    "runtime_import_reviews",
    "runtime_control",
    "runtime_control_status",
    "list_runtime_controls",
    "review_runtime_control",
    "take_pending_open",
    "runtime_status",
    "show_main_window",
    "hide_main_window",
    "quit_app",
    "startup_shortcut_status",
    "set_startup_shortcut_enabled",
    "list_jobs",
    "get_job",
    "create_job",
    "update_job",
    "set_job_enabled",
    "delete_job",
    "list_services",
    "get_service",
    "service_observability",
    "import_definitions",
    "apply_import",
    "preview_workspace_task_import",
    "cancel_workspace_task_import",
    "apply_workspace_task_import",
    "list_workspace_tasks",
    "trust_workspace_task_source",
    "trust_workspace_task_shell_source",
    "run_workspace_task_operation",
    "get_workspace_task_operation",
    "list_workspace_task_operations",
    "stop_workspace_task_operation",
    "list_workspace_task_diagnostics",
    "open_workspace_task_diagnostic",
    "preview_project_import",
    "cancel_project_import",
    "apply_project_import",
    "create_service",
    "update_service",
    "delete_service",
    "export_definitions",
    "get_service_instance",
    "start_service",
    "stop_service",
    "restart_service",
    "get_run",
    "list_runs",
    "list_run_history",
    "run_job_now",
    "stop_active_run",
    "get_active_run",
    "list_active_runs",
    "preview_cron",
    "tail_log",
    "search_run_logs",
    "open_run_log_in_log_lens",
    "preview_workspace_task_control",
    "renew_workspace_task_control",
    "reject_workspace_task_control",
    "accept_workspace_task_control",
    "list_workspace_task_control_receipts",
];

#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    data_root(app)?;
    common_root(app)?;
    let result = match method {
        "runtime_import_prepare"
        | "runtime_import_resume"
        | "runtime_import_apply"
        | "runtime_import_cancel"
        | "runtime_import_status"
        | "runtime_import_catalog"
        | "runtime_import_reviews" => imports::dispatch(app, method, args),
        "runtime_control" => control::execute(app, args).await,
        "runtime_control_status" | "list_runtime_controls" | "review_runtime_control" => {
            control::metadata(app, method, args)
        }
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "runtime_status" => crate::commands::__component_runtime_status(app, args).await,
        "show_main_window" => crate::commands::__component_show_main_window(app, args).await,
        "hide_main_window" => crate::commands::__component_hide_main_window(app, args).await,
        "quit_app" => crate::commands::__component_quit_app(app, args).await,
        "startup_shortcut_status" => {
            crate::commands::__component_startup_shortcut_status(app, args).await
        }
        "set_startup_shortcut_enabled" => {
            crate::commands::__component_set_startup_shortcut_enabled(app, args).await
        }
        "list_jobs" => crate::commands::__component_list_jobs(app, args).await,
        "get_job" => crate::commands::__component_get_job(app, args).await,
        "create_job" => crate::commands::__component_create_job(app, args).await,
        "update_job" => crate::commands::__component_update_job(app, args).await,
        "set_job_enabled" => crate::commands::__component_set_job_enabled(app, args).await,
        "delete_job" => crate::commands::__component_delete_job(app, args).await,
        "list_services" => crate::commands::__component_list_services(app, args).await,
        "get_service" => crate::commands::__component_get_service(app, args).await,
        "service_observability" => {
            crate::commands::__component_service_observability(app, args).await
        }
        "import_definitions" => crate::commands::__component_import_definitions(app, args).await,
        "apply_import" => crate::commands::__component_apply_import(app, args).await,
        "preview_workspace_task_import" => {
            crate::commands::__component_preview_workspace_task_import(app, args).await
        }
        "cancel_workspace_task_import" => {
            crate::commands::__component_cancel_workspace_task_import(app, args).await
        }
        "apply_workspace_task_import" => {
            crate::commands::__component_apply_workspace_task_import(app, args).await
        }
        "list_workspace_tasks" => {
            crate::commands::__component_list_workspace_tasks(app, args).await
        }
        "trust_workspace_task_source" => {
            crate::commands::__component_trust_workspace_task_source(app, args).await
        }
        "trust_workspace_task_shell_source" => {
            crate::commands::__component_trust_workspace_task_shell_source(app, args).await
        }
        "run_workspace_task_operation" => {
            crate::commands::__component_run_workspace_task_operation(app, args).await
        }
        "get_workspace_task_operation" => {
            crate::commands::__component_get_workspace_task_operation(app, args).await
        }
        "list_workspace_task_operations" => {
            crate::commands::__component_list_workspace_task_operations(app, args).await
        }
        "stop_workspace_task_operation" => {
            crate::commands::__component_stop_workspace_task_operation(app, args).await
        }
        "list_workspace_task_diagnostics" => {
            crate::commands::__component_list_workspace_task_diagnostics(app, args).await
        }
        "open_workspace_task_diagnostic" => {
            crate::commands::__component_open_workspace_task_diagnostic(app, args).await
        }
        "preview_project_import" => {
            crate::commands::__component_preview_project_import(app, args).await
        }
        "cancel_project_import" => {
            crate::commands::__component_cancel_project_import(app, args).await
        }
        "apply_project_import" => {
            crate::commands::__component_apply_project_import(app, args).await
        }
        "create_service" => crate::commands::__component_create_service(app, args).await,
        "update_service" => crate::commands::__component_update_service(app, args).await,
        "delete_service" => crate::commands::__component_delete_service(app, args).await,
        "export_definitions" => crate::commands::__component_export_definitions(app, args).await,
        "get_service_instance" => {
            crate::commands::__component_get_service_instance(app, args).await
        }
        "start_service" => crate::commands::__component_start_service(app, args).await,
        "stop_service" => crate::commands::__component_stop_service(app, args).await,
        "restart_service" => crate::commands::__component_restart_service(app, args).await,
        "get_run" => crate::commands::__component_get_run(app, args).await,
        "list_runs" => crate::commands::__component_list_runs(app, args).await,
        "list_run_history" => crate::commands::__component_list_run_history(app, args).await,
        "run_job_now" => crate::commands::__component_run_job_now(app, args).await,
        "stop_active_run" => crate::commands::__component_stop_active_run(app, args).await,
        "get_active_run" => crate::commands::__component_get_active_run(app, args).await,
        "list_active_runs" => crate::commands::__component_list_active_runs(app, args).await,
        "preview_cron" => crate::commands::__component_preview_cron(app, args).await,
        "tail_log" => crate::commands::__component_tail_log(app, args).await,
        "search_run_logs" => crate::commands::__component_search_run_logs(app, args).await,
        "open_run_log_in_log_lens" => {
            crate::log_lens::__component_open_run_log_in_log_lens(app, args).await
        }
        "preview_workspace_task_control" => {
            crate::task_control::__component_preview_workspace_task_control(app, args).await
        }
        "renew_workspace_task_control" => {
            crate::task_control::__component_renew_workspace_task_control(app, args).await
        }
        "reject_workspace_task_control" => {
            crate::task_control::__component_reject_workspace_task_control(app, args).await
        }
        "accept_workspace_task_control" => {
            crate::task_control::__component_accept_workspace_task_control(app, args).await
        }
        "list_workspace_task_control_receipts" => {
            crate::task_control::__component_list_workspace_task_control_receipts(app, args).await
        }
        _ => Err("component_method_invalid".into()),
    };
    data_root(app)?;
    common_root(app)?;
    let mut value = result?;
    if matches!(
        method,
        "list_jobs"
            | "list_services"
            | "get_job"
            | "get_service"
            | "update_job"
            | "update_service"
            | "create_job"
            | "create_service"
            | "set_job_enabled"
    ) {
        let database = app
            .try_state::<Arc<crate::storage::DatabaseState>>()
            .ok_or("component_state_unavailable")?;
        let annotate = |value: &mut serde_json::Value| -> Result<(), String> {
            if let Some(object) = value.as_object_mut() {
                let id = object
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("component_response_invalid")?;
                let required = database
                    .requires_secret_review(id)
                    .map_err(|_| "runtime_import_failed")?;
                object.insert(
                    "envReconnectRequired".into(),
                    serde_json::Value::Bool(required),
                );
            }
            Ok(())
        };
        if let Some(values) = value.as_array_mut() {
            for value in values {
                annotate(value)?;
            }
        } else {
            annotate(&mut value)?;
        }
    }
    Ok(value)
}
