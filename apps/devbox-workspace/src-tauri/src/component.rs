//! Native product command admission. Route names do not grant Registry writes.
use crate::{host::Host, project_owner::RegistrationAction};
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;
use tauri::{Manager, State, WebviewWindow};

#[derive(Clone, Default)]
struct Pool(Arc<AtomicUsize>);
struct Permit(Arc<AtomicUsize>);
impl Pool {
    fn reserve(&self) -> Result<Permit, &'static str> {
        self.reserve_with_limit(2)
    }
    fn reserve_with_limit(&self, limit: usize) -> Result<Permit, &'static str> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < limit).then_some(n + 1)
            })
            .map_err(|_| "busy")?;
        Ok(Permit(self.0.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Clone)]
struct Runtime {
    shutdown_started: Arc<AtomicBool>,
    exit_authorized: Arc<AtomicBool>,
    context_activity: crate::core::context_activity::ContextActivity,
    context_waiters: Pool,
    filesystem_activity: crate::core::context_activity::ContextActivity,
    definitions: Arc<Mutex<crate::definitions::Definitions>>,
    source: Arc<Mutex<crate::source_host::SourceHost>>,
    source_requests: Pool,
    source_operations: crate::core::source_operations::Operations,
    source_workers: Arc<tokio::sync::Semaphore>,
    host: Arc<OnceLock<Result<Arc<Host>, &'static str>>>,
    metadata: Pool,
    window_imports: Arc<Mutex<crate::window_import::WindowImports>>,
    probes: Pool,
    files: Arc<Mutex<crate::files_host::FilesHost>>,
    file_requests: Pool,
    file_workers: Arc<tokio::sync::Semaphore>,
    dialogs: Pool,
    lsp: Arc<Mutex<Option<Arc<crate::lsp_host::LspHost>>>>,
    lsp_requests: Pool,
    lsp_operations: crate::core::source_operations::Operations,
    lsp_workers: Arc<tokio::sync::Semaphore>,
    lsp_shutdown: code_pad_lib::lsp::RequestCancellation,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            shutdown_started: Arc::default(),
            exit_authorized: Arc::default(),
            context_activity: Default::default(),
            context_waiters: Pool::default(),
            filesystem_activity: Default::default(),
            definitions: Arc::default(),
            source: Arc::default(),
            source_requests: Pool::default(),
            source_operations: Default::default(),
            source_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            host: Arc::default(),
            metadata: Pool::default(),
            window_imports: Arc::default(),
            probes: Pool::default(),
            files: Arc::default(),
            file_requests: Pool::default(),
            file_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            dialogs: Pool::default(),
            lsp: Arc::default(),
            lsp_requests: Pool::default(),
            lsp_operations: Default::default(),
            lsp_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            lsp_shutdown: Default::default(),
        }
    }
}
impl Runtime {
    /// A file write waits before entering a worker or taking the Files mutex.
    /// The existing bounded request pool owns the waiter; admission is not an
    /// IO retry and never repeats authentication or a partially executed save.
    async fn filesystem_permit(
        &self,
        write: bool,
        deadline: u64,
    ) -> Result<crate::core::context_activity::ContextPermit, &'static str> {
        crate::files_host::current_deadline(deadline)?;
        if self.shutdown_started.load(Ordering::Acquire) {
            return Err("request_cancelled");
        }
        if !write {
            return self.filesystem_activity.enter(false);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis();
        let remaining = u128::from(deadline).saturating_sub(now).min(30_000) as u64;
        self.filesystem_activity
            .enter_change(
                std::time::Instant::now() + Duration::from_millis(remaining),
                &self.shutdown_started,
            )
            .await
            .map_err(|issue| {
                if issue == "context_expired" {
                    "request_expired"
                } else {
                    "request_cancelled"
                }
            })
    }
    #[cfg(windows)]
    fn start_wsl_poll(&self, app: tauri::AppHandle) {
        use tauri::Emitter;
        let runtime = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if runtime.shutdown_started.load(Ordering::Acquire) {
                    continue;
                }
                let (Ok(host), Some(window)) = (runtime.host(), app.get_webview_window("main"))
                else {
                    continue;
                };
                let Ok(context_permit) = runtime.context_activity.enter(false) else {
                    continue;
                };
                let Ok(Some(context)) = product_shell_tauri::workspace_context(&window) else {
                    continue;
                };
                if !matches!(
                    context.target,
                    product_contract::ExecutionTarget::Wsl { .. }
                ) {
                    continue;
                }
                let (Ok(queued), Ok(worker), Ok(filesystem)) = (
                    runtime.file_requests.reserve_with_limit(1),
                    runtime.file_workers.clone().try_acquire_owned(),
                    runtime.filesystem_activity.enter(false),
                ) else {
                    continue;
                };
                let files = runtime.files.clone();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    let _retained = (context_permit, queued, worker, filesystem);
                    let Ok(mut files) = files.try_lock() else { return };
                    let result = files.poll_wsl(&host, &context);
                    if product_shell_tauri::workspace_context(&window).ok().flatten().as_ref() != Some(&context) { return; }
                    let Ok(context_key) = serde_json::to_string(&context) else { return };
                    match result {
                        Ok(snapshots) => for snapshot in snapshots {
                            let _ = window.emit("file-changed", json!({"path":snapshot.path,"mtimeNanos":snapshot.mtime_nanos,"size":snapshot.size,"contentHash":snapshot.content_hash,"contextKey":context_key}));
                        },
                        Err(issue) => { let _ = window.emit("workspace-file-watch-issue", json!({"contextKey":context_key,"issue":issue})); },
                    }
                }).await;
            }
        });
    }
    #[cfg(windows)]
    async fn retire_files(&self) -> Result<(), &'static str> {
        tokio::time::timeout(Duration::from_secs(30), async {
            while self.file_requests.0.load(Ordering::Acquire) != 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "files_busy")?;
        let files = self.files.clone();
        tauri::async_runtime::spawn_blocking(move || {
            files
                .lock()
                .map_err(|_| "files_unavailable")?
                .retire_wsl()?;
            Ok(())
        })
        .await
        .map_err(|_| "worker_unavailable")?
    }
    async fn retire_lsp(&self) -> Result<(), &'static str> {
        let owner = self.lsp.lock().map_err(|_| "lsp_unavailable")?.clone();
        if let Some(owner) = owner {
            owner.retire().await?;
        }
        Ok(())
    }
    fn host(&self) -> Result<Arc<Host>, &'static str> {
        // Initialization publishes once. Concurrent metadata readers must not
        // compete for a mutable lock or become spurious admission failures.
        self.host.get().cloned().unwrap_or(Err("initializing"))
    }
    fn status(&self) -> Value {
        match self.host().and_then(|host| host.status()) {
            Ok(status) => {
                json!({"phase": if status["selected"] == true {"selected"} else {"setup"}})
            }
            Err("initializing") => json!({"phase":"loading"}),
            Err(issue) => json!({"phase":"failed", "issue":issue}),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    header: RouteRequest,
    component: String,
    method: String,
    args: Value,
}
#[derive(Serialize)]
struct Response {
    operation: Operation,
    value: Value,
}
fn allowed(component: &str, route: &str, method: &str) -> bool {
    if !matches!(route, "overview" | "source" | "files" | "dependencies") {
        return false;
    }
    match component {
        "workspace.source" => {
            route == "source"
                && (crate::source_host::management(method)
                    || repo_manager_lib::component::SOURCE_COMMANDS.contains(&method))
        }
        "workspace.definitions" => {
            route == "overview"
                && matches!(
                    method,
                    "load"
                        | "preview_trust"
                        | "approve_trust"
                        | "revoke_trust"
                        | "cancel"
                        | "preview_edit"
                        | "apply_edit"
                )
        }
        "workspace.dependencies" => {
            route == "dependencies"
                && matches!(
                    method,
                    "dependency_inventory"
                        | "dependency_enrichment_preview"
                        | "dependency_enrichment_execute"
                        | "dependency_enrichment_cancel"
                )
        }
        "workspace.files" => route == "files" && crate::files_host::allowed(component, method),
        "workspace.lsp" => route == "files" && crate::lsp_host::allowed(method),
        "workspace.migration" => {
            matches!(method, "status" | "start_empty")
                || (route == "overview" && crate::window_import::WindowImports::allowed(method))
                || (route == "overview"
                    && matches!(
                        method,
                        "prepare_legacy_snapshot"
                            | "legacy_snapshot_job"
                            | "cancel_legacy_snapshot"
                            | "list_legacy_snapshots"
                            | "verify_legacy_snapshot"
                            | "preview_profile_import"
                            | "cancel_profile_import"
                            | "apply_profile_import"
                            | "preview_template_import"
                            | "cancel_template_import"
                            | "apply_template_import"
                            | "legacy_workspace"
                    ))
        }
        "workspace.registry" => {
            (route == "overview"
                && matches!(
                    method,
                    "save_template"
                        | "archive_template"
                        | "list_wsl_distros"
                        | "preview_wsl"
                        | "preview_template_profile_wsl"
                        | "preview_imported_profile_wsl"
                ))
                || matches!(
                    method,
                    "snapshot"
                        | "preview_windows"
                        | "preview_imported_profile_windows"
                        | "preview_template_profile_windows"
                        | "preview_legacy_workspace_windows"
                        | "unbind_imported_profile"
                        | "cancel_registration"
                        | "apply_registration"
                        | "rename"
                        | "remove"
                        | "select_project"
                        | "clear_project"
                )
        }
        _ => false,
    }
}

async fn execute_lsp(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: Option<crate::core::context_activity::ContextPermit>,
) -> Result<Value, &'static str> {
    use tauri_plugin_dialog::DialogExt;
    let queued =
        runtime
            .lsp_requests
            .reserve_with_limit(if crate::lsp_host::stops(&request.method) {
                10
            } else {
                8
            })?;
    let (operation_id, cancelled) =
        crate::lsp_host::admission(&request.method, request.args.clone())?;
    let context_key =
        serde_json::to_string(&request.header.context).map_err(|_| "invalid_request")?;
    for id in cancelled {
        runtime.lsp_operations.cancel(&context_key, &id)?;
    }
    let start = operation_id
        .as_deref()
        .map(|id| runtime.lsp_operations.register(&context_key, Some(id)))
        .transpose()
        .map_err(|issue| {
            if issue == "source_cancelled" {
                "lsp_operation_cancelled"
            } else {
                issue
            }
        })?;
    let _cancel = start.as_ref().map(|start| start.cancel_on_drop());
    if runtime.lsp_shutdown.is_cancelled() {
        return Err("lsp_operation_cancelled");
    }
    let mut deadline = request.header.deadline_ms;
    let chosen = if request.method == "pick_lsp_archives" {
        empty(&request.args)?;
        let dialog = runtime.dialogs.reserve_with_limit(1)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(window)
            .set_title("관리형 LSP archive 선택")
            .add_filter("LSP archive", &["zip", "tgz", "gz"])
            .pick_files(move |paths| {
                let _dialog = dialog;
                let _ = sender.send(paths);
            });
        let Some(paths) = receiver.await.map_err(|_| "file_dialog_unavailable")? else {
            return Ok(json!([]));
        };
        let mut admitted = request.header.clone();
        admitted.request_id = uuid::Uuid::new_v4().to_string();
        admitted.deadline_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64
            + 29_000;
        product_shell_tauri::authorize(window, &admitted, "workspace.lsp")
            .map_err(|_| "file_selection_expired")?;
        deadline = admitted.deadline_ms;
        Some(
            paths
                .into_iter()
                .map(|path| path.into_path().map_err(|_| "invalid_file_path"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let worker = if crate::lsp_host::worker_required(&request.method) {
        Some(
            runtime
                .lsp_workers
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| "worker_unavailable")?,
        )
    } else {
        None
    };
    let runtime = runtime.clone();
    let app = window.app_handle().clone();
    let host = runtime.host()?;
    tauri::async_runtime::spawn_blocking(move || {
        let (_queued, _worker, _context) = (queued, worker, context_permit);
        crate::files_host::current_deadline(deadline)?;
        if runtime.lsp_shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        let owner = {
            let mut slot = runtime.lsp.lock().map_err(|_| "lsp_unavailable")?;
            if slot.is_none() {
                // Files and LSP share only initialization. No installer/server
                // wait holds the editor's mutex or filesystem/context permit.
                let mut files = runtime.files.try_lock().map_err(|_| "files_unavailable")?;
                files.initialize(&app, &host)?;
                *slot = Some(Arc::new(crate::lsp_host::LspHost::new(
                    &app,
                    &host,
                    runtime.context_activity.clone(),
                    runtime.filesystem_activity.clone(),
                    runtime.files.clone(),
                )?));
            }
            slot.as_ref().ok_or("lsp_unavailable")?.clone()
        };
        crate::files_host::current_deadline(deadline)?;
        if runtime.lsp_shutdown.is_cancelled() {
            return Err("lsp_operation_cancelled");
        }
        if let Some(paths) = chosen {
            owner.choose(&host, &paths, deadline)
        } else if crate::lsp_host::review_method(&request.method) {
            owner.execute_review(
                &app,
                &host,
                &request.method,
                request.args,
                request.header.context.as_ref(),
                deadline,
            )
        } else {
            tauri::async_runtime::block_on(owner.execute(
                &app,
                &host,
                crate::lsp_host::Invocation {
                    method: &request.method,
                    args: request.args,
                    context: request.header.context.as_ref(),
                    deadline,
                    start,
                },
                &runtime.lsp_shutdown,
            ))
        }
    })
    .await
    .unwrap_or(Err("worker_unavailable"))
}

async fn execute_files(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    use tauri_plugin_dialog::DialogExt;
    let queued = runtime.file_requests.reserve_with_limit(16)?;
    let mut deadline = request.header.deadline_ms;
    let chosen = if request.method == "pick_files" {
        empty(&request.args)?;
        let dialog = runtime.dialogs.reserve_with_limit(1)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(window)
            .pick_files(move |paths| {
                // Retain the sole dialog slot until the native chooser closes.
                let _dialog = dialog;
                let _ = sender.send(paths);
            });
        let Some(paths) = receiver.await.map_err(|_| "file_dialog_unavailable")? else {
            return Ok(json!([]));
        };
        // The explicit native user choice can outlive the original RPC's
        // deadline. Re-admit the same installation/session/context/window with
        // a native request before using any returned path or saving a choice.
        let mut admitted = request.header.clone();
        admitted.request_id = uuid::Uuid::new_v4().to_string();
        admitted.deadline_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64
            + 5000;
        product_shell_tauri::authorize(window, &admitted, "workspace.files")
            .map_err(|_| "file_selection_expired")?;
        deadline = admitted.deadline_ms;
        Some(
            paths
                .into_iter()
                .map(|path| path.into_path().map_err(|_| "invalid_file_path"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let filesystem = match file_access(&request.method) {
        Some(write) => Some(runtime.filesystem_permit(write, deadline).await?),
        None => None,
    };
    // The context permit prevents a project switch while waiting. A closed or
    // replaced window must still not admit a delayed file operation.
    if product_shell_tauri::workspace_context(window).map_err(|_| "file_context_changed")?
        != request.header.context
    {
        return Err("file_context_changed");
    }
    crate::files_host::current_deadline(deadline)?;
    let worker = runtime
        .file_workers
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "worker_unavailable")?;
    let files = runtime.files.clone();
    let host = runtime.host()?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_queued, _worker, _context, _filesystem) =
            (queued, worker, context_permit, filesystem);
        let mut files = files.lock().map_err(|_| "files_unavailable")?;
        crate::files_host::current_deadline(deadline)?;
        files.initialize(&app, &host)?;
        if let Some(paths) = chosen {
            files.approve_native_selection(&paths)
        } else {
            files.execute(
                &app,
                &host,
                crate::files_host::Invocation {
                    context: request.header.context.as_ref(),
                    component: &request.component,
                    method: &request.method,
                    args: request.args,
                    deadline,
                },
            )
        }
    })
    .await
    .unwrap_or(Err("worker_unavailable"))
}
async fn execute_dependencies(
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    let host = runtime.host()?;
    let permit = runtime.probes.reserve()?;
    let deadline = request.header.deadline_ms;
    let context = request.header.context.ok_or("project_selection_required")?;
    let access = tauri::async_runtime::spawn_blocking(move || {
        crate::dependencies_host::access(host, context, deadline, (permit, context_permit))
    })
    .await
    .unwrap_or(Err("worker_unavailable"))?;
    crate::files_host::current_deadline(deadline)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis() as u64;
    let remaining = deadline.checked_sub(now).ok_or("request_expired")?;
    match tokio::time::timeout(
        Duration::from_millis(remaining),
        repo_manager_lib::component::dispatch_dependencies(access, &request.method, request.args),
    )
    .await
    {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(repo_manager_lib::component::dependency_issue(&error)),
        Err(_) => Err("request_expired"),
    }
}

async fn execute_source(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    let context = request.header.context.ok_or("project_selection_required")?;
    let deadline = request.header.deadline_ms;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis() as u64;
    let span = deadline
        .checked_sub(now)
        .ok_or("request_expired")?
        .min(product_contract::MAX_DEADLINE_MS);
    let budget = crate::source_host::Budget {
        deadline_ms: deadline,
        expires: std::time::Instant::now() + Duration::from_millis(span),
    };
    let remaining = || -> Result<Duration, &'static str> {
        budget.check()?;
        budget
            .expires
            .checked_duration_since(std::time::Instant::now())
            .ok_or("request_expired")
    };
    let app = window.app_handle().clone();
    let operation_key = serde_json::to_string(&context).map_err(|_| "invalid_context")?;
    if repo_manager_lib::component::source_cancel(&request.method) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct CancelId {
            operation_id: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Cancel {
            request: CancelId,
        }
        let cancel: Cancel = input(request.args.clone())?;
        let admitted = runtime
            .source_operations
            .cancel(&operation_key, &cancel.request.operation_id)?;
        let running = repo_manager_lib::component::dispatch_source_cancel(
            &app,
            &operation_key,
            &request.method,
            request.args,
        )
        .await
        .map_err(|error| crate::source_host::issue(&error))?;
        return Ok(json!(admitted || running.as_bool() == Some(true)));
    }
    let filesystem = if matches!(
        request.method.as_str(),
        "cancel_trust"
            | "revoke_trust"
            | "cancel_worktree"
            | "cancel_cleanup_scope"
            | "revoke_cleanup_scope"
            | "cleanup_scope_status"
    ) {
        None
    } else {
        Some(
            runtime
                .filesystem_activity
                .enter(source_mutation(&request.method))?,
        )
    };
    let queued = runtime.source_requests.reserve_with_limit(16)?;
    let operation_id = if request.method == "create_worktree" {
        request.args.get("operationId")
    } else {
        request
            .args
            .get("request")
            .and_then(|value| value.get("operationId"))
    };
    let operation_id = operation_id
        .map(|value| value.as_str().ok_or("invalid_request"))
        .transpose()?;
    let admitted = runtime
        .source_operations
        .register(&operation_key, operation_id)?;
    let _cancel_on_drop = admitted.cancel_on_drop();
    let worker_slot = tokio::time::timeout(
        remaining()?,
        admitted.until_cancelled(runtime.source_workers.clone().acquire_owned()),
    )
    .await
    .map_err(|_| "request_expired")??
    .map_err(|_| "source_owner_unavailable")?;
    let host = runtime.host()?;
    let source = runtime.source.clone();
    let definitions = runtime.definitions.clone();
    let worker_app = app.clone();
    let method = request.method;
    let args = request.args;
    if crate::source_host::management(&method) {
        let worker_admission = admitted.clone();
        let worker = tauri::async_runtime::spawn_blocking(move || {
            let _retained = (queued, worker_slot, context_permit, filesystem);
            worker_admission.check()?;
            crate::files_host::current_deadline(deadline)?;
            let mut source = source.lock().map_err(|_| "source_owner_unavailable")?;
            let mut definitions = definitions.lock().map_err(|_| "definition_owner_busy")?;
            crate::files_host::current_deadline(deadline)?;
            source.initialize(&worker_app, &host)?;
            let result = source.manage(&host, &mut definitions, &context, &method, args, budget)?;
            worker_admission.check()?;
            Ok(result)
        });
        return tokio::time::timeout(remaining()?, admitted.until_cancelled(worker))
            .await
            .map_err(|_| "request_expired")??
            .unwrap_or(Err("worker_unavailable"))
            .map_err(crate::source_host::issue);
    }
    let worker_admission = admitted.clone();
    let worker_method = method.clone();
    let files = runtime.files.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        worker_admission.check()?;
        let retained = (
            queued,
            worker_slot,
            context_permit,
            filesystem,
            worker_admission.clone(),
        );
        crate::files_host::current_deadline(deadline)?;
        let prepared = {
            let mut source = source.lock().map_err(|_| "source_owner_unavailable")?;
            let mut definitions = definitions.lock().map_err(|_| "definition_owner_busy")?;
            crate::files_host::current_deadline(deadline)?;
            source.initialize(&worker_app, &host)?;
            source.access(
                host,
                &mut definitions,
                crate::source_host::Invocation {
                    files,
                    context,
                    method: worker_method,
                    args,
                    budget,
                    admitted: worker_admission,
                },
                retained,
            )?
        };
        prepared.finish_on_worker()
    });
    let prepared = tokio::time::timeout(remaining()?, admitted.until_cancelled(worker))
        .await
        .map_err(|_| "request_expired")??
        .unwrap_or(Err("worker_unavailable"))
        .map_err(crate::source_host::issue)?;
    let (access, args) = match prepared {
        crate::source_host::ReadySource::Native(access) => *access,
        #[cfg(windows)]
        crate::source_host::ReadySource::Complete(value) => return Ok(value),
    };
    match tokio::time::timeout(
        remaining()?,
        repo_manager_lib::component::dispatch_source(&app, access, &method, args),
    )
    .await
    {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(crate::source_host::issue(&error)),
        Err(_) => Err("request_expired"),
    }
}

fn source_mutation(method: &str) -> bool {
    matches!(
        method,
        "repo_stage"
            | "repo_unstage"
            | "repo_commit"
            | "repo_fetch"
            | "repo_pull"
            | "repo_push"
            | "repo_cleanup"
            | "create_worktree"
    )
}
/// Private editor recovery/session writes remain available while Git owns the
/// worktree. User-file reads/writes coordinate with Git until workers retire.
fn definition_access(method: &str) -> Option<bool> {
    match method {
        "apply_edit" => Some(true),
        "load" | "preview_edit" | "preview_trust" | "approve_trust" => Some(false),
        _ => None,
    }
}
fn file_access(method: &str) -> Option<bool> {
    match method {
        "save_file" | "rename_file_action" | "delete_file_action" | "apply_recovery_preview" => {
            Some(true)
        }
        "sync_editor_document"
        | "load_session"
        | "save_session"
        | "preview_session_import"
        | "apply_session_import"
        | "cancel_session_import"
        | "list_session_history"
        | "preview_session_restore"
        | "preview_recovery_import"
        | "apply_recovery_import"
        | "cancel_recovery_import"
        | "list_recovery_history"
        | "preview_recovery_restore"
        | "load_recovery"
        | "save_recovery"
        | "discard_recovery"
        | "cancel_recovery_preview"
        | "take_pending_open"
        | "read_clipboard_text"
        | "validate_encoding"
        | "lsp_catalog"
        | "lsp_installed"
        | "load_lsp_config"
        | "preview_lsp_config_import"
        | "apply_lsp_config_import"
        | "cancel_lsp_config_import"
        | "list_lsp_config_history"
        | "preview_lsp_config_restore" => None,
        _ => Some(false),
    }
}

fn input<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, &'static str> {
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
fn empty(value: &Value) -> Result<(), &'static str> {
    if value.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("invalid_request")
    }
}
fn dispatch(host: &Host, method: &str, args: Value) -> Result<Value, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Root {
        root: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PreviewId {
        preview_id: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Apply {
        preview_id: String,
        name: String,
        action: RegistrationAction,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Rename {
        revision: u64,
        project_id: String,
        name: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Remove {
        revision: u64,
        context: product_contract::ProjectContext,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Select {
        context: product_contract::ProjectContext,
    }
    match method {
        "prepare_legacy_snapshot" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Source {
                source: crate::core::legacy_inventory::Source,
            }
            let value: Source = input(args)?;
            Ok(json!(host.legacy.start(value.source)?))
        }
        "legacy_snapshot_job" => {
            empty(&args)?;
            Ok(json!(host.legacy.status()?))
        }
        "list_legacy_snapshots" => {
            empty(&args)?;
            Ok(json!(host.legacy.catalog()?))
        }
        "preview_profile_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Job {
                job_id: String,
            }
            let value: Job = input(args)?;
            let (snapshot_id, profiles) = host.legacy.profile_source(&value.job_id)?;
            Ok(json!(host
                .projects()?
                .preview_profile_import(snapshot_id, profiles)?))
        }
        "preview_template_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Job {
                job_id: String,
            }
            let value: Job = input(args)?;
            let (snapshot_id, templates) = host.legacy.template_source(&value.job_id)?;
            Ok(json!(host
                .projects()?
                .preview_template_import(snapshot_id, templates)?))
        }
        "legacy_workspace" | "preview_legacy_workspace_windows" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Job {
                job_id: String,
            }
            let value: Job = input(args)?;
            let proposal = host.legacy.workspace(&value.job_id)?;
            if method == "legacy_workspace" {
                return Ok(json!(proposal));
            }
            let proposal = proposal
                .filter(|proposal| {
                    proposal.target == crate::core::legacy_workspace::Target::Windows
                })
                .ok_or("legacy_workspace_unsupported")?;
            Ok(json!(host.projects()?.preview_windows(&proposal.path)?))
        }
        "save_template" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct SaveTemplate {
                revision: u64,
                id: Option<String>,
                template: workbench_lib::component::ProfileTemplate,
            }
            let value: SaveTemplate = input(args)?;
            Ok(json!(host.projects()?.save_template(
                value.revision,
                value.id.as_deref(),
                value.template
            )?))
        }
        "archive_template" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct ArchiveTemplate {
                revision: u64,
                id: String,
            }
            let value: ArchiveTemplate = input(args)?;
            Ok(json!(host
                .projects()?
                .archive_template(value.revision, &value.id)?))
        }
        "preview_template_profile_wsl" => Ok(json!(host
            .projects()?
            .preview_template_profile_wsl(host.helper_directory()?, input(args)?)?)),
        "preview_imported_profile_wsl" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Profile {
                imported_id: String,
                distro_id: String,
                start_stopped: bool,
            }
            let value: Profile = input(args)?;
            Ok(json!(host.projects()?.preview_imported_profile_wsl(
                host.helper_directory()?,
                &value.imported_id,
                &value.distro_id,
                value.start_stopped
            )?))
        }
        "preview_template_profile_windows" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Template {
                template_id: String,
                root: String,
                name: String,
            }
            let value: Template = input(args)?;
            Ok(json!(host.projects()?.preview_template_profile_windows(
                &value.template_id,
                &value.root,
                &value.name
            )?))
        }
        "preview_imported_profile_windows" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Profile {
                imported_id: String,
            }
            let value: Profile = input(args)?;
            Ok(json!(host
                .projects()?
                .preview_imported_profile_windows(&value.imported_id)?))
        }
        "unbind_imported_profile" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Unbind {
                revision: u64,
                imported_id: String,
                target: crate::core::legacy_profiles::ProfileTarget,
            }
            let value: Unbind = input(args)?;
            Ok(json!(host.projects()?.unbind_imported_profile(
                value.revision,
                &value.imported_id,
                value.target
            )?))
        }
        "apply_profile_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Apply {
                preview_id: String,
                choices: Vec<crate::core::legacy_profiles::Choice>,
            }
            let value: Apply = input(args)?;
            let (registry, result) = host
                .projects()?
                .apply_profile_import(&value.preview_id, value.choices)?;
            Ok(json!({"registry":registry,"result":result}))
        }
        "cancel_profile_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Cancel {
                preview_id: String,
            }
            let value: Cancel = input(args)?;
            host.projects()?.cancel_profile_import(&value.preview_id)?;
            Ok(Value::Null)
        }
        "apply_template_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Apply {
                preview_id: String,
                choices: Vec<crate::core::legacy_profiles::Choice>,
            }
            let value: Apply = input(args)?;
            let (registry, result) = host
                .projects()?
                .apply_template_import(&value.preview_id, value.choices)?;
            Ok(json!({"registry":registry,"result":result}))
        }
        "cancel_template_import" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Cancel {
                preview_id: String,
            }
            let value: Cancel = input(args)?;
            host.projects()?.cancel_template_import(&value.preview_id)?;
            Ok(Value::Null)
        }
        "verify_legacy_snapshot" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Snapshot {
                snapshot_id: String,
            }
            let value: Snapshot = input(args)?;
            Ok(json!(host.legacy.verify(value.snapshot_id)?))
        }
        "cancel_legacy_snapshot" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Job {
                job_id: String,
            }
            let value: Job = input(args)?;
            host.legacy.cancel(&value.job_id)?;
            Ok(Value::Null)
        }
        "start_empty" => {
            empty(&args)?;
            host.start_empty()?;
            Ok(json!({}))
        }
        "snapshot" => {
            empty(&args)?;
            Ok(json!(host.projects()?.snapshot()?))
        }
        "select_project" => {
            let value: Select = input(args)?;
            let binding = if cfg!(windows)
                && matches!(
                    value.context.target,
                    product_contract::ExecutionTarget::Wsl { .. }
                ) {
                host.projects()?
                    .admit_selection(host.helper_directory()?, &value.context)?
            } else {
                host.projects()?.admit(&value.context)?.binding().clone()
            };
            Ok(json!({"context":value.context,"binding":binding}))
        }
        "list_wsl_distros" => {
            empty(&args)?;
            Ok(json!(crate::platform::wsl_distro::list()?))
        }
        "preview_wsl" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct WslRoot {
                distro_id: String,
                root: String,
                start_stopped: bool,
            }
            let value: WslRoot = input(args)?;
            Ok(json!(host.projects()?.preview_wsl(
                host.helper_directory()?,
                &value.distro_id,
                &value.root,
                value.start_stopped
            )?))
        }
        "preview_windows" => {
            let value: Root = input(args)?;
            Ok(json!(host.projects()?.preview_windows(&value.root)?))
        }
        "cancel_registration" => {
            let value: PreviewId = input(args)?;
            host.projects()?.cancel(&value.preview_id)?;
            Ok(json!({}))
        }
        "apply_registration" => {
            let value: Apply = input(args)?;
            let (registry, context) =
                host.projects()?
                    .apply(&value.preview_id, &value.name, value.action)?;
            Ok(json!({"registry":registry,"context":context}))
        }
        "rename" => {
            let value: Rename = input(args)?;
            Ok(json!(host.projects()?.rename(
                value.revision,
                &value.project_id,
                &value.name
            )?))
        }
        "remove" => {
            let value: Remove = input(args)?;
            Ok(json!(host
                .projects()?
                .remove(value.revision, &value.context)?))
        }
        _ => Err("invalid_request"),
    }
}
fn project_probe(method: &str) -> bool {
    matches!(
        method,
        "preview_template_profile_wsl"
            | "preview_imported_profile_wsl"
            | "preview_windows"
            | "list_wsl_distros"
            | "preview_wsl"
            | "preview_imported_profile_windows"
            | "preview_template_profile_windows"
            | "preview_legacy_workspace_windows"
            | "select_project"
    )
}
fn changes_context(method: &str) -> bool {
    matches!(
        method,
        "select_project"
            | "clear_project"
            | "apply_registration"
            | "remove"
            | "apply_edit"
            | "approve_cleanup_scope"
            | "revoke_cleanup_scope"
            | "approve_trust"
            | "revoke_trust"
            | "save_lsp_config"
            | "apply_lsp_config_import"
            | "lsp_execution_approve"
            | "lsp_execution_revoke"
    )
}
#[tauri::command]
async fn execute(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    request: Request,
) -> Result<Response, Problem> {
    let rejected = |code| Problem {
        code,
        provenance: Provenance {
            product: "workspace".into(),
            component: "workspace.dispatch".into(),
            request_id: "rejected".into(),
            revision: 1,
        },
    };
    if runtime.shutdown_started.load(Ordering::Acquire) {
        return Err(rejected(ProblemCode::Unavailable));
    }
    if !allowed(&request.component, &request.header.route, &request.method)
        || !request.args.is_object()
        || serde_json::to_vec(&request.args).map_or(true, |bytes| {
            bytes.len()
                > if request.component == "workspace.files"
                    || (request.component == "workspace.lsp"
                        && crate::lsp_host::text_request(&request.method))
                {
                    64 * 1024 * 1024
                } else if request.component == "workspace.definitions" {
                    2 * 1024 * 1024
                } else {
                    64 * 1024
                }
        })
    {
        return Err(rejected(ProblemCode::InvalidRequest));
    }
    let files = request.component == "workspace.files";
    let lsp = request.component == "workspace.lsp";
    let definitions = request.component == "workspace.definitions";
    let dependencies = request.component == "workspace.dependencies";
    let source = request.component == "workspace.source";
    let context_change = changes_context(&request.method);
    // Authenticate/replay-check once before waiting. A single bounded waiter
    // holds no context permit, so active file/metadata workers can retire.
    let provenance = product_shell_tauri::authorize(&window, &request.header, &request.component)?;
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    let context_permit = if files
        || definitions
        || dependencies
        || source
        || context_change
        || (lsp && crate::lsp_host::contextual(&request.method))
    {
        if context_change {
            let _waiting = runtime
                .context_waiters
                .reserve_with_limit(1)
                .map_err(|_| problem(ProblemCode::Overloaded))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| problem(ProblemCode::Expired))?
                .as_millis();
            let remaining = u128::from(request.header.deadline_ms)
                .saturating_sub(now)
                .min(30_000) as u64;
            let deadline = std::time::Instant::now() + Duration::from_millis(remaining);
            Some(
                runtime
                    .context_activity
                    .enter_change(deadline, &runtime.shutdown_started)
                    .await
                    .map_err(|issue| {
                        problem(if issue == "context_expired" {
                            ProblemCode::Expired
                        } else {
                            ProblemCode::Unavailable
                        })
                    })?,
            )
        } else {
            Some(
                runtime
                    .context_activity
                    .enter(false)
                    .map_err(|_| problem(ProblemCode::Unavailable))?,
            )
        }
    } else {
        None
    };
    if context_permit.is_some() {
        // Selection may have changed between authentication and admission.
        // Recheck under the retained permit without replaying authorization.
        if product_shell_tauri::workspace_context(&window)
            .map_err(|_| problem(ProblemCode::Unavailable))?
            != request.header.context
        {
            return Err(problem(ProblemCode::StaleContext));
        }
        crate::files_host::current_deadline(request.header.deadline_ms)
            .map_err(|_| problem(ProblemCode::Expired))?;
        if runtime.shutdown_started.load(Ordering::Acquire) {
            return Err(problem(ProblemCode::Unavailable));
        }
    }
    let select = request.method == "select_project";
    let expected_context = request.header.context.clone();
    let deadline = request.header.deadline_ms;
    let retired = if matches!(
        request.method.as_str(),
        "select_project" | "clear_project" | "apply_registration" | "remove" | "apply_edit"
    ) {
        runtime.retire_lsp().await
    } else {
        Ok(())
    };
    let mut result = if let Err(issue) = retired {
        Err(issue)
    } else if lsp {
        execute_lsp(&window, &runtime, request, context_permit.clone()).await
    } else if files {
        execute_files(
            &window,
            &runtime,
            request,
            context_permit.clone().expect("file context permit"),
        )
        .await
    } else if source {
        execute_source(
            &window,
            &runtime,
            request,
            context_permit.clone().expect("source context permit"),
        )
        .await
    } else if dependencies {
        execute_dependencies(
            &runtime,
            request,
            context_permit.clone().expect("dependency context permit"),
        )
        .await
    } else if definitions {
        let host = runtime.host();
        let owner = runtime.definitions.clone();
        let permit = runtime.probes.reserve();
        let filesystem = match definition_access(&request.method) {
            Some(write) => runtime.filesystem_permit(write, deadline).await.map(Some),
            None => Ok(None),
        }
        .and_then(|permit| {
            if product_shell_tauri::workspace_context(&window).map_err(|_| "stale_context")?
                != request.header.context
            {
                return Err("stale_context");
            }
            Ok(permit)
        });
        match (host, permit, filesystem) {
            (Ok(host), Ok(permit), Ok(filesystem)) => {
                let worker_context = context_permit.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let (_permit, _context, _filesystem) = (permit, worker_context, filesystem);
                    crate::files_host::current_deadline(deadline)?;
                    let mut owner = owner.lock().map_err(|_| "definition_owner_busy")?;
                    crate::files_host::current_deadline(deadline)?;
                    let context = request
                        .header
                        .context
                        .as_ref()
                        .ok_or("project_selection_required")?;
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Token {
                        preview_id: String,
                    }
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Revision {
                        revision: u64,
                    }
                    match request.method.as_str() {
                        "load" => {
                            empty(&request.args)?;
                            Ok(json!(owner.load(&host, context, deadline)?))
                        }
                        "preview_edit" => Ok(json!(owner.preview_edit(
                            &host,
                            context,
                            input(request.args)?,
                            deadline
                        )?)),
                        "apply_edit" => {
                            let token: Token = input(request.args)?;
                            Ok(json!(owner.apply_edit(
                                &host,
                                context,
                                &token.preview_id,
                                deadline
                            )?))
                        }
                        "preview_trust" => {
                            empty(&request.args)?;
                            Ok(json!(owner.preview_trust(&host, context, deadline)?))
                        }
                        "approve_trust" => {
                            let token: Token = input(request.args)?;
                            Ok(json!(owner.approve_trust(
                                &host,
                                context,
                                &token.preview_id,
                                deadline
                            )?))
                        }
                        "revoke_trust" => {
                            let revision: Revision = input(request.args)?;
                            Ok(json!(owner.revoke_trust(
                                &host,
                                context,
                                revision.revision
                            )?))
                        }
                        "cancel" => {
                            let token: Token = input(request.args)?;
                            owner.cancel(&token.preview_id);
                            Ok(Value::Null)
                        }
                        _ => Err("invalid_request"),
                    }
                })
                .await
                .unwrap_or(Err("worker_unavailable"))
            }
            (Err(issue), _, _) | (_, Err(issue), _) | (_, _, Err(issue)) => Err(issue),
        }
    } else if request.component == "workspace.migration"
        && crate::window_import::WindowImports::allowed(&request.method)
    {
        match (runtime.host(), runtime.metadata.reserve()) {
            (Ok(host), Ok(permit)) => {
                let owner = runtime.window_imports.clone();
                let geometry = crate::window_import::NativeGeometry {
                    window: window.clone(),
                    deadline,
                };
                tauri::async_runtime::spawn_blocking(move || {
                    let _permit = permit;
                    crate::files_host::current_deadline(deadline)?;
                    owner.lock().map_err(|_| "busy")?.dispatch(
                        &host,
                        &geometry,
                        &request.method,
                        request.args,
                        deadline,
                    )
                })
                .await
                .unwrap_or(Err("worker_unavailable"))
            }
            (Err(issue), _) | (_, Err(issue)) => Err(issue),
        }
    } else if request.method == "clear_project" {
        empty(&request.args).and_then(|()| {
            product_shell_tauri::replace_project_context(
                &window,
                expected_context.as_ref(),
                None,
                deadline,
            )?;
            Ok(json!({"context":null}))
        })
    } else if request.method == "status" {
        empty(&request.args).map(|()| runtime.status())
    } else {
        let preview = project_probe(&request.method);
        let pool = if preview {
            &runtime.probes
        } else {
            &runtime.metadata
        };
        match (runtime.host(), pool.reserve()) {
            (Ok(host), Ok(permit)) => {
                let worker_context = context_permit.clone();
                let worker = tauri::async_runtime::spawn_blocking(move || {
                    // A timed-out probe keeps its permit until the OS returns.
                    // A late preview cannot register or grant trust by itself.
                    let (_permit, _context) = (permit, worker_context);
                    dispatch(&host, &request.method, request.args)
                });
                if preview {
                    match tokio::time::timeout(Duration::from_secs(15), worker).await {
                        Ok(result) => result.unwrap_or(Err("worker_unavailable")),
                        Err(_) => Err("project_probe_timeout"),
                    }
                } else {
                    worker.await.unwrap_or(Err("worker_unavailable"))
                }
            }
            (Err(issue), _) | (_, Err(issue)) => Err(issue),
        }
    };
    if select {
        result = result.and_then(|value| {
            // The worker produced this context after native Registry/object
            // admission. A timed-out worker never reaches session mutation.
            let context = serde_json::from_value(value["context"].clone())
                .map_err(|_| "context_unavailable")?;
            product_shell_tauri::replace_project_context(
                &window,
                expected_context.as_ref(),
                Some(context),
                deadline,
            )?;
            Ok(value)
        });
    }
    let (outcome, value) = match result {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(issue) => (
            OperationState::Failed {
                code: ProblemCode::Unavailable,
            },
            json!({"issue":issue}),
        ),
    };
    Ok(Response {
        operation: Operation {
            provenance,
            outcome,
        },
        value,
    })
}
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("workspace")
        .invoke_handler(tauri::generate_handler![execute])
        .setup(|app, _| {
            let runtime = Runtime::default();
            app.manage(runtime.clone());
            #[cfg(windows)]
            runtime.start_wsl_poll(app.clone());
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let root = app
                        .path()
                        .app_local_data_dir()
                        .map_err(|_| "store_unavailable")?;
                    std::fs::create_dir_all(&root).map_err(|_| "store_unavailable")?;
                    let resources = app.path().resource_dir().map_err(|_| "store_unavailable")?;
                    Host::open_with_resources(&root, resources).map(Arc::new)
                })
                .await
                .unwrap_or(Err("worker_unavailable"));
                let _ = runtime.host.set(result);
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if let (Ok(host), Ok(permit)) = (runtime.host(), runtime.metadata.reserve()) {
                        let definitions = runtime.definitions.clone();
                        let source = runtime.source.clone();
                        let lsp = runtime.lsp.clone();
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            let _permit = permit;
                            if let Ok(projects) = host.projects() {
                                let _ = projects.expire();
                            }
                            if let Ok(mut definitions) = definitions.try_lock() {
                                definitions.expire();
                            }
                            if let Ok(mut source) = source.try_lock() {
                                source.expire();
                            }
                            if let Ok(lsp) = lsp.try_lock() {
                                if let Some(lsp) = lsp.as_ref() {
                                    lsp.expire();
                                }
                            }
                        })
                        .await;
                    }
                }
            });
            Ok(())
        })
        .on_event(|app, event| {
            let tauri::RunEvent::ExitRequested { api, code, .. } = event else {
                return;
            };
            let runtime = app.state::<Runtime>();
            if runtime.exit_authorized.load(Ordering::Acquire) {
                return;
            }
            if app
                .try_state::<Arc<code_pad_lib::lsp::LspManager>>()
                .is_none()
                && runtime.lsp_requests.0.load(Ordering::Acquire) == 0
            {
                runtime.shutdown_started.store(true, Ordering::Release);
                runtime.lsp_shutdown.cancel();
                return;
            }
            api.prevent_exit();
            if runtime
                .shutdown_started
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return;
            }
            let runtime = runtime.inner().clone();
            runtime.lsp_shutdown.cancel();
            let app = app.clone();
            let exit_code = code.unwrap_or(0);
            tauri::async_runtime::spawn(async move {
                // Cancellation interrupts downloads; the LSP worker retains its
                // request permit until archive IO/index work actually retires.
                let retired = tokio::time::timeout(Duration::from_secs(5), async {
                    while runtime.lsp_requests.0.load(Ordering::Acquire) != 0 {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .is_ok();
                let manager = app
                    .try_state::<Arc<code_pad_lib::lsp::LspManager>>()
                    .map(|manager| manager.inner().clone());
                let actor_stopped = runtime.retire_lsp().await.is_ok();
                let stopped = match manager {
                    Some(manager) => manager.shutdown_for_exit().await.is_ok(),
                    None => true,
                };
                #[cfg(windows)]
                let files_stopped = runtime.retire_files().await.is_ok();
                #[cfg(not(windows))]
                let files_stopped = true;
                if retired && stopped && actor_stopped && files_stopped {
                    runtime.exit_authorized.store(true, Ordering::Release);
                    app.exit(exit_code);
                } else {
                    // Keep the owner alive if a child has not been confirmed
                    // terminated. A later exit request retries shutdown.
                    runtime.shutdown_started.store(false, Ordering::Release);
                }
            });
        })
        .build()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn after_ms(milliseconds: u64) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + milliseconds
    }
    #[tokio::test]
    async fn a_file_write_waits_for_the_retained_reader_and_enters_only_once() {
        let runtime = Runtime::default();
        let caller = runtime.filesystem_activity.enter(false).unwrap();
        let worker = caller.clone();
        drop(caller);
        let executions = std::sync::atomic::AtomicUsize::new(0);
        let mut write = Box::pin(async {
            let permit = runtime.filesystem_permit(true, after_ms(2_000)).await?;
            executions.fetch_add(1, Ordering::SeqCst);
            Ok::<_, &'static str>(permit)
        });
        assert!(tokio::time::timeout(Duration::from_millis(15), &mut write)
            .await
            .is_err());
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        drop(worker);
        let permit = write.await.unwrap();
        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert!(runtime.filesystem_activity.enter(false).is_err());
        assert!(runtime.filesystem_activity.enter(true).is_err());
        drop(permit);
        assert!(runtime.filesystem_activity.enter(false).is_ok());
    }
    #[tokio::test]
    async fn an_expired_or_cancelled_file_wait_does_not_admit_a_write() {
        let runtime = Runtime::default();
        let reader = runtime.filesystem_activity.enter(false).unwrap();
        assert!(matches!(
            runtime.filesystem_permit(true, after_ms(15)).await,
            Err("request_expired")
        ));
        runtime.shutdown_started.store(true, Ordering::Release);
        assert!(matches!(
            runtime.filesystem_permit(true, after_ms(2_000)).await,
            Err("request_cancelled")
        ));
        drop(reader);
        assert!(matches!(
            runtime.filesystem_permit(true, after_ms(2_000)).await,
            Err("request_cancelled")
        ));
        assert!(runtime.filesystem_activity.enter(true).is_ok());
    }
    #[test]
    fn template_writes_are_overview_registry_only() {
        for method in ["save_template", "archive_template"] {
            assert!(allowed("workspace.registry", "overview", method));
            for route in ["source", "files", "dependencies", "runtime"] {
                assert!(!allowed("workspace.registry", route, method));
            }
            for component in ["workspace.migration", "workspace.source", "workspace.files"] {
                assert!(!allowed(component, "overview", method));
            }
        }
    }
    #[test]
    fn initialized_host_is_shared_without_rejecting_concurrent_metadata_readers() {
        let runtime = Runtime::default();
        assert!(matches!(runtime.host(), Err("initializing")));
        let directory = tempfile::tempdir().unwrap();
        let host = Arc::new(Host::open(directory.path()).unwrap());
        assert!(runtime.host.set(Ok(host.clone())).is_ok());
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let workers = (0..4)
            .map(|_| {
                let runtime = runtime.clone();
                let expected = host.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..128 {
                        assert!(Arc::ptr_eq(&runtime.host().unwrap(), &expected));
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(runtime.host.set(Err("store_unavailable")).is_err());
        assert!(Arc::ptr_eq(&runtime.host().unwrap(), &host));
    }

    #[test]
    fn lsp_settings_write_excludes_another_save_and_running_context_operation() {
        let runtime = Runtime::default();
        let reading = runtime.context_activity.enter(false).unwrap();
        assert!(runtime
            .context_activity
            .enter(changes_context("save_lsp_config"))
            .is_err());
        drop(reading);
        for method in [
            "preview_lsp_config_import",
            "apply_lsp_config_import",
            "cancel_lsp_config_import",
            "list_lsp_config_history",
            "preview_lsp_config_restore",
        ] {
            assert!(crate::lsp_host::allowed(method));
            assert!(crate::lsp_host::contextual(method));
            assert_eq!(file_access(method), None);
        }
        assert!(changes_context("apply_lsp_config_import"));
        let writing = runtime
            .context_activity
            .enter(changes_context("save_lsp_config"))
            .unwrap();
        assert!(runtime
            .context_activity
            .enter(changes_context("save_lsp_config"))
            .is_err());
        assert!(runtime.context_activity.enter(false).is_err());
        assert!(runtime
            .context_activity
            .enter(changes_context("apply_lsp_config_import"))
            .is_err());
        drop(writing);
        assert!(runtime.context_activity.enter(false).is_ok());
    }
    #[test]
    fn saturated_lsp_workers_leave_files_and_project_selection_available() {
        let runtime = Runtime::default();
        let _first = runtime.lsp_workers.clone().try_acquire_owned().unwrap();
        let _second = runtime.lsp_workers.clone().try_acquire_owned().unwrap();
        assert!(runtime.lsp_workers.clone().try_acquire_owned().is_err());
        let _editor = runtime.file_workers.clone().try_acquire_owned().unwrap();
        let _file_owner = runtime.files.try_lock().unwrap();
        let _selection = runtime.context_activity.enter(true).unwrap();
        let _private_install = runtime.lsp.lock().unwrap();
        let request = runtime.lsp_requests.reserve().unwrap();
        assert_eq!(runtime.lsp_requests.0.load(Ordering::Acquire), 1);
        runtime.lsp_shutdown.cancel();
        assert_eq!(runtime.lsp_requests.0.load(Ordering::Acquire), 1);
        drop(request);
        assert_eq!(runtime.lsp_requests.0.load(Ordering::Acquire), 0);
    }

    #[test]
    fn git_and_editor_io_exclude_writes_without_blocking_private_recovery() {
        let runtime = Runtime::default();
        let context = runtime.context_activity.enter(false).unwrap();
        let git = runtime
            .filesystem_activity
            .enter(source_mutation("repo_pull"))
            .unwrap();
        assert!(runtime
            .filesystem_activity
            .enter(file_access("open_file").unwrap())
            .is_err());
        assert!(runtime
            .filesystem_activity
            .enter(file_access("save_file").unwrap())
            .is_err());
        assert!(file_access("save_recovery").is_none());
        assert!(runtime
            .filesystem_activity
            .enter(definition_access("load").unwrap())
            .is_err());
        assert!(runtime
            .filesystem_activity
            .enter(definition_access("apply_edit").unwrap())
            .is_err());
        assert!(definition_access("cancel").is_none());
        assert!(definition_access("revoke_trust").is_none());
        let recovery = runtime.context_activity.enter(false).unwrap();
        assert!(runtime.context_activity.enter(true).is_err());
        drop(git);
        let status = runtime
            .filesystem_activity
            .enter(source_mutation("repo_changes"))
            .unwrap();
        assert!(runtime.filesystem_activity.enter(true).is_err());
        drop(status);
        let definition = runtime
            .filesystem_activity
            .enter(definition_access("apply_edit").unwrap())
            .unwrap();
        assert!(runtime
            .filesystem_activity
            .enter(file_access("open_file").unwrap())
            .is_err());
        assert!(runtime
            .filesystem_activity
            .enter(source_mutation("repo_stage"))
            .is_err());
        drop(definition);
        drop(recovery);
        drop(context);
        assert!(runtime.filesystem_activity.enter(true).is_ok());
        assert!(runtime.context_activity.enter(true).is_ok());
    }
    #[test]
    fn admitted_native_features_have_a_matching_product_component() {
        let catalog: Value = serde_json::from_str(include_str!("../../../products.json")).unwrap();
        let components = catalog["components"].as_array().unwrap();
        for (component, route, method) in [
            ("workspace.registry", "overview", "snapshot"),
            ("workspace.migration", "overview", "status"),
            ("workspace.migration", "overview", "prepare_legacy_snapshot"),
            ("workspace.definitions", "overview", "load"),
            (
                "workspace.dependencies",
                "dependencies",
                "dependency_inventory",
            ),
            ("workspace.source", "source", "trust_status"),
            ("workspace.source", "source", "repo_changes"),
            ("workspace.files", "files", "open_file"),
        ] {
            assert!(allowed(component, route, method), "{component} {method}");
            assert!(
                components
                    .iter()
                    .any(|entry| entry["id"] == component && entry["owner"] == "workspace"),
                "the product shell would reject {component} before its domain adapter"
            );
        }
    }
    #[test]
    fn window_geometry_changes_require_the_migration_overview_role() {
        for method in [
            "preview_window_import",
            "preview_window_restore",
            "apply_window_import",
            "cancel_window_import",
            "list_window_history",
        ] {
            assert!(allowed("workspace.migration", "overview", method));
            assert!(!allowed("workspace.migration", "files", method));
            assert!(!allowed("workspace.files", "overview", method));
            assert!(!allowed("workspace.registry", "overview", method));
        }
    }
    #[test]
    fn legacy_snapshot_admission_rejects_renderer_paths_and_foreign_roles() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        for args in [
            json!({"source":"workbench","path":"C:\\foreign"}),
            json!({"source":"unknown"}),
            json!({"source":"code-pad","destination":"C:\\foreign"}),
        ] {
            assert_eq!(
                dispatch(&host, "prepare_legacy_snapshot", args),
                Err("invalid_request")
            );
        }
        assert!(host.legacy.status().unwrap().is_none());
        assert_eq!(
            dispatch(&host, "list_legacy_snapshots", json!({"path":"foreign"})),
            Err("invalid_request")
        );
        assert_eq!(
            dispatch(
                &host,
                "verify_legacy_snapshot",
                json!({"snapshotId":"../foreign"})
            ),
            Err("invalid_legacy_snapshot")
        );
        assert!(!allowed(
            "workspace.files",
            "files",
            "verify_legacy_snapshot"
        ));
        assert!(!allowed(
            "workspace.migration",
            "source",
            "list_legacy_snapshots"
        ));
        assert!(!root.path().join("legacy-imports").exists());
        assert!(!allowed(
            "workspace.source",
            "overview",
            "prepare_legacy_snapshot"
        ));
        assert!(!allowed(
            "workspace.migration",
            "files",
            "prepare_legacy_snapshot"
        ));
        assert!(!allowed(
            "workspace.migration",
            "overview",
            "apply_registration"
        ));
    }
    #[test]
    fn registry_and_activation_roles_are_closed_and_probes_remain_bounded() {
        assert!(allowed("workspace.registry", "overview", "preview_windows"));
        for method in [
            "list_wsl_distros",
            "preview_wsl",
            "preview_template_profile_wsl",
            "preview_imported_profile_wsl",
        ] {
            assert!(allowed("workspace.registry", "overview", method));
            assert!(!allowed("workspace.registry", "files", method));
            assert!(!allowed("workspace.registry", "source", method));
            assert!(project_probe(method));
        }
        for method in [
            "preview_windows",
            "preview_imported_profile_windows",
            "preview_template_profile_windows",
            "preview_legacy_workspace_windows",
            "select_project",
        ] {
            assert!(project_probe(method));
            assert!(allowed("workspace.registry", "overview", method));
        }
        for method in [
            "preview_template_import",
            "apply_template_import",
            "cancel_template_import",
        ] {
            assert!(allowed("workspace.migration", "overview", method));
            assert!(!allowed("workspace.migration", "files", method));
            assert!(!allowed("workspace.registry", "overview", method));
            assert!(!project_probe(method));
        }
        assert!(!project_probe("legacy_workspace"));
        assert!(allowed(
            "workspace.migration",
            "overview",
            "legacy_workspace"
        ));
        assert!(!allowed("workspace.migration", "files", "legacy_workspace"));
        assert!(!allowed(
            "workspace.migration",
            "overview",
            "apply_registration"
        ));
        assert!(!allowed("workspace.registry", "runtime", "snapshot"));
        assert!(!allowed("workspace.shell", "overview", "start_empty"));
        let pool = Pool::default();
        let one = pool.reserve().unwrap();
        let two = pool.reserve().unwrap();
        assert!(pool.reserve().is_err());
        drop(one);
        assert!(pool.reserve().is_ok());
        drop(two);
    }
}
