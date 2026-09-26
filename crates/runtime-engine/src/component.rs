//! Native component entry points; the product host admits caller and operation.
//! Calling these does not start the standalone application or select its stores.

pub mod search;
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Emitter, Manager};

#[cfg(feature = "desktop")]
pub(crate) mod control;
mod logs;
#[cfg(feature = "desktop")]
pub mod sessions;
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

/// Native metadata for binding an already-owned operation's diagnostics.
pub fn diagnostic_scope(app: &tauri::AppHandle, run_id: &str) -> Result<(String, String), String> {
    data_root(app)?;
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    let source = database
        .get_workspace_task_execution_for_operation_run(run_id)
        .map_err(|_| "runtime_diagnostic_invalid")?
        .ok_or("runtime_diagnostic_invalid")?;
    Ok((source.source_root, source.project_identity))
}

pub fn diagnostic_identity(
    app: &tauri::AppHandle,
    run_id: &str,
) -> Result<(String, String, i64), String> {
    data_root(app)?;
    let database = app
        .try_state::<Arc<crate::storage::DatabaseState>>()
        .ok_or("component_state_unavailable")?;
    let run = database
        .get_run(run_id)
        .map_err(|_| "runtime_diagnostic_invalid")?
        .ok_or("runtime_diagnostic_invalid")?;
    let source = database
        .get_workspace_task_execution_for_operation_run(run_id)
        .map_err(|_| "runtime_diagnostic_invalid")?
        .ok_or("runtime_diagnostic_invalid")?;
    Ok((source.source_root, run.job_id, run.queue_sequence))
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
    let path = if execution.target_kind == crate::core::models::TargetKind::Wsl
        && execution.source_root.starts_with('/')
        && database.task_sources.get().is_some()
    {
        // The product's native Files owner admits the POSIX path at actual navigation.
        let relative =
            crate::core::workspace_diagnostics::relative_diagnostic_file(&diagnostic.file)
                .map_err(str::to_owned)?;
        PathBuf::from(&execution.source_root).join(relative)
    } else {
        crate::core::workspace_diagnostics::resolve_workspace_diagnostic_path(
            &execution.source_root,
            &diagnostic.file,
        )
        .map_err(str::to_owned)?
    };
    crate::workspace_sources::verify(&database, std::slice::from_ref(&execution), false)
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
pub fn initialize(app: &tauri::AppHandle, data: &Path, common: &Path) -> Result<(), String> {
    initialize_with_sources(app, data, common, None)
}

pub fn initialize_with_sources(
    app: &tauri::AppHandle,
    data: &Path,
    common: &Path,
    sources: Option<Arc<dyn crate::workspace_sources::NativeTaskSources>>,
) -> Result<(), String> {
    initialize_owner(app, data, common, sources, true)
}

/// Import-only Workspace initialization does not start cron, services or the
/// maintenance worker. Commit requires a clean process restart with normal mode.
pub fn initialize_import_only_with_sources(
    app: &tauri::AppHandle,
    data: &Path,
    common: &Path,
    sources: Option<Arc<dyn crate::workspace_sources::NativeTaskSources>>,
) -> Result<(), String> {
    initialize_owner(app, data, common, sources, false)
}
fn initialize_owner(
    app: &tauri::AppHandle,
    data: &Path,
    common: &Path,
    sources: Option<Arc<dyn crate::workspace_sources::NativeTaskSources>>,
    background_work: bool,
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
    if let Some(sources) = sources {
        database
            .task_sources
            .set(sources)
            .map_err(|_| "component_state_conflict")?;
    }
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
    app.manage(paths);
    app.manage(database.clone());
    app.manage(Arc::new(ImportOperationRegistry::default()));
    app.manage(protector);
    app.manage(crate::applink::PendingOpen::new());
    app.manage(crate::task_control::PendingTaskControl::new());
    app.manage(runtime.clone());
    if background_work {
        crate::lifecycle::spawn_scheduler(runtime.clone());
        crate::lifecycle::spawn_maintenance(runtime, database, app.clone(), data.to_path_buf());
    }
    Ok(())
}

pub fn is_initialized(app: &tauri::AppHandle) -> bool {
    app.try_state::<ProductPaths>().is_some()
}

/// Resolves only after owned process trees and scheduler writes are retired.
/// The Workspace exit owner must also finish its other components before exit.
pub fn request_shutdown(app: &tauri::AppHandle) {
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
