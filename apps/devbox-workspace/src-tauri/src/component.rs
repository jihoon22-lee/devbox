//! Native product command admission. Route names do not grant Registry writes.
use crate::ipc::terminal::terminal_worker;
use crate::{
    host::Host,
    ipc::lanes::{Lane, Lanes},
};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;
use tauri::{Manager, State, WebviewWindow};

#[derive(Clone)]
pub(crate) struct Runtime {
    pub(crate) lanes: Lanes,
    pub(crate) shutdown_started: Arc<AtomicBool>,
    pub(crate) ui_ready: Arc<AtomicBool>,
    pub(crate) engines: Arc<crate::runtime_host::Owners>,
    pub(crate) terminals: Arc<crate::terminal_host::Terminals>,
    pub(crate) sessions: Arc<crate::development_host::Sessions>,
    pub(crate) exit_authorized: Arc<AtomicBool>,
    pub(crate) context_activity: crate::core::context_activity::ContextActivity,
    pub(crate) filesystem_activity: crate::core::context_activity::ContextActivity,
    pub(crate) definitions: Arc<Mutex<crate::definitions::Definitions>>,
    pub(crate) source: Arc<Mutex<crate::source_host::SourceHost>>,
    pub(crate) source_operations: crate::core::source_operations::Operations,
    pub(crate) host: Arc<OnceLock<Result<Arc<Host>, &'static str>>>,
    pub(crate) files: Arc<Mutex<crate::files_host::FilesHost>>,
    pub(crate) lsp: Arc<Mutex<Option<Arc<crate::lsp_host::LspHost>>>>,
    pub(crate) lsp_operations: crate::core::source_operations::Operations,
    pub(crate) lsp_shutdown: editor_engine::lsp::RequestCancellation,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            lanes: Lanes::default(),
            shutdown_started: Arc::default(),
            ui_ready: Arc::default(),
            engines: Arc::default(),
            terminals: Arc::default(),
            sessions: Arc::default(),
            exit_authorized: Arc::default(),
            context_activity: Default::default(),
            filesystem_activity: Default::default(),
            definitions: Arc::default(),
            source: Arc::default(),
            source_operations: Default::default(),
            host: Arc::default(),
            files: Arc::default(),
            lsp: Arc::default(),
            lsp_operations: Default::default(),
            lsp_shutdown: Default::default(),
        }
    }
}
impl Runtime {
    pub(crate) fn shutting_down(&self) -> bool {
        self.shutdown_started.load(Ordering::Acquire)
    }
    /// A file write waits before entering a worker or taking the Files mutex.
    /// The existing bounded request pool owns the waiter; admission is not an
    /// IO retry and never repeats authentication or a partially executed save.
    pub(crate) async fn filesystem_permit(
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
                    runtime.lanes.try_enter(Lane::FilesWatch),
                    runtime.lanes.workers(Lane::Files).try_acquire_owned(),
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
            while self.lanes.active(Lane::Files) != 0 {
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
    pub(crate) async fn retire_lsp(&self) -> Result<(), &'static str> {
        let owner = self.lsp.lock().map_err(|_| "lsp_unavailable")?.clone();
        if let Some(owner) = owner {
            owner.retire().await?;
        }
        Ok(())
    }
    pub(crate) fn host(&self) -> Result<Arc<Host>, &'static str> {
        // Initialization publishes once. Concurrent metadata readers must not
        // compete for a mutable lock or become spurious admission failures.
        self.host.get().cloned().unwrap_or(Err("initializing"))
    }
    pub(crate) fn status(&self) -> Value {
        match self.host().and_then(|host| host.status()) {
            Ok(status) => {
                json!(if status["selected"] == true {
                    crate::ipc::setup::SetupStatus::Selected
                } else {
                    crate::ipc::setup::SetupStatus::Setup
                })
            }
            Err("initializing") => json!(crate::ipc::setup::SetupStatus::Loading),
            Err(issue) => json!(crate::ipc::setup::SetupStatus::Failed {
                issue: issue.into()
            }),
        }
    }
}

#[tauri::command]
fn terminal_describe(window: WebviewWindow, runtime: State<'_, Runtime>) -> Result<Value, String> {
    if runtime.shutdown_started.load(Ordering::Acquire) {
        return Err("request_cancelled".into());
    }
    runtime.terminals.describe(&window).map_err(str::to_owned)
}
#[tauri::command]
async fn terminal_execute(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<Value, String> {
    if runtime.shutdown_started.load(Ordering::Acquire) {
        return Err("request_cancelled".into());
    }
    use product_ipc::ComponentCall;
    let args = request.args.clone();
    let request = request
        .decode::<crate::ipc::companion::CompanionCall>()
        .map_err(|_| "terminal_args_invalid")?;
    let method = request.call.method().to_owned();
    let lane = request.call.lane();
    runtime
        .terminals
        .authorize(&window, &request.header)
        .map_err(str::to_owned)?;
    terminal_worker(
        window,
        runtime.inner().clone(),
        request.header,
        method,
        args,
        true,
        lane,
        None,
    )
    .await
    .map_err(str::to_owned)
}

/// Private editor recovery/session writes remain available while Git owns the
/// worktree. User-file reads/writes coordinate with Git until workers retire.

fn setup_runtime_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let show = MenuItem::with_id(app, "workspace-show", "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "workspace-quit", "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::Io(std::io::Error::other("missing product icon")))?;
    TrayIconBuilder::with_id("workspace-runtime-tray")
        .icon(icon)
        .tooltip("Devbox Workspace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "workspace-show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "workspace-quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("workspace")
        .invoke_handler(tauri::generate_handler![
            crate::ipc::files::files,
            crate::ipc::lsp::lsp,
            crate::ipc::source::source,
            crate::ipc::registry::registry,
            crate::ipc::setup::setup,
            crate::ipc::definitions::definitions,
            crate::ipc::dependencies::dependencies,
            crate::ipc::runtime::runtime,
            crate::ipc::processes::processes,
            crate::ipc::process_actions::process_actions,
            crate::ipc::logs::logs,
            crate::ipc::terminal::terminal,
            crate::ipc::problems::problems,
            crate::ipc::commands::commands,
            terminal_describe,
            terminal_execute
        ])
        .setup(|app, _| {
            let runtime = Runtime::default();
            app.manage(runtime.clone());
            app.manage(Arc::new(crate::problems_host::Problems::default()));
            app.manage(runtime.sessions.clone());
            app.manage(runtime.terminals.clone());
            setup_runtime_tray(app)?;
            #[cfg(windows)]
            runtime.start_wsl_poll(app.clone());
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let storage_app = app.clone();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let app = storage_app;
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
                while !runtime.ui_ready.load(Ordering::Acquire)
                    && !runtime.shutdown_started.load(Ordering::Acquire)
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                if !runtime.shutdown_started.load(Ordering::Acquire) {
                    if let (Ok(host), Ok(permit)) = (
                        runtime.host(),
                        runtime.lanes.try_enter(Lane::EngineBackground),
                    ) {
                        let owners = runtime.engines.clone();
                        let shutdown = runtime.shutdown_started.clone();
                        let worker_app = app.clone();
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            let _permit = permit;
                            if !shutdown.load(Ordering::Acquire)
                                && host.component("runtime").is_ok()
                            {
                                // Persisted product jobs keep their scheduler while
                                // Tasks/Runtime/Logs views have never been opened.
                                owners.initialize_runtime(&worker_app, &host)?;
                            }
                            Ok::<_, &'static str>(())
                        })
                        .await;
                    }
                }
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if let (Ok(host), Ok(permit)) =
                        (runtime.host(), runtime.lanes.try_enter(Lane::Metadata))
                    {
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
        .on_window_ready(|window| {
            if window.label() != "main" {
                if window.state::<Runtime>().terminals.owns(window.label()) {
                    let owner = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = owner.hide();
                        }
                    });
                }
                return;
            }
            let owner = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if (runtime_engine::component::is_initialized(owner.app_handle())
                        || terminal_engine::component::is_product(owner.app_handle()))
                        && !owner
                            .state::<Runtime>()
                            .exit_authorized
                            .load(Ordering::Acquire)
                    {
                        let _ = owner.hide();
                        api.prevent_close();
                    }
                }
            });
        })
        .on_event(|app, event| {
            if matches!(event, tauri::RunEvent::Ready) {
                app.state::<Runtime>()
                    .ui_ready
                    .store(true, Ordering::Release);
                return;
            }
            let tauri::RunEvent::ExitRequested { api, code, .. } = event else {
                return;
            };
            let runtime = app.state::<Runtime>();
            if runtime.exit_authorized.load(Ordering::Acquire) {
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
                let _ = runtime.sessions.request_shutdown();
                if runtime_engine::component::is_initialized(&app) {
                    runtime_engine::component::request_shutdown(&app);
                }
                if logs_engine::component::is_initialized(&app) {
                    let _ = logs_engine::component::request_shutdown(&app);
                }
                // Cancellation interrupts downloads; the LSP worker retains its
                // request permit until archive IO/index work actually retires.
                let retired = tokio::time::timeout(Duration::from_secs(5), async {
                    while runtime.lanes.active(Lane::Lsp) != 0
                        || runtime.lanes.active(Lane::EngineBackground) != 0
                        || runtime.lanes.active(Lane::Terminal) != 0
                        || runtime.lanes.active(Lane::TerminalIo) != 0
                        || runtime.lanes.active(Lane::TerminalStop) != 0
                    {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .is_ok();
                let manager = app
                    .try_state::<Arc<editor_engine::lsp::LspManager>>()
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
                let sessions_stopped = runtime.sessions.shutdown().await.is_ok();
                let runtime_stopped = if runtime_engine::component::is_initialized(&app) {
                    runtime_engine::component::shutdown(&app).await.is_ok()
                } else {
                    true
                };
                let logs_stopped = if logs_engine::component::is_initialized(&app) {
                    logs_engine::component::shutdown(&app).await.is_ok()
                } else {
                    true
                };
                let terminal_owner = runtime.terminals.clone();
                let terminal_app = app.clone();
                let terminals_stopped = tauri::async_runtime::spawn_blocking(move || {
                    terminal_owner.shutdown(&terminal_app)
                })
                .await
                .is_ok_and(|result| result.is_ok());
                let retired = retired
                    || (runtime.lanes.active(Lane::Lsp) == 0
                        && runtime.lanes.active(Lane::EngineBackground) == 0
                        && runtime.lanes.active(Lane::Terminal) == 0
                        && runtime.lanes.active(Lane::TerminalIo) == 0
                        && runtime.lanes.active(Lane::TerminalStop) == 0);
                if retired
                    && sessions_stopped
                    && terminals_stopped
                    && stopped
                    && actor_stopped
                    && files_stopped
                    && runtime_stopped
                    && logs_stopped
                {
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
pub(crate) fn provider_context(
    app: &tauri::AppHandle,
    context: &product_contract::ProjectContext,
) -> Result<(), &'static str> {
    let runtime = app
        .try_state::<Runtime>()
        .ok_or("session_summary_unavailable")?;
    runtime.host()?.projects()?.binding(context)?;
    Ok(())
}

pub(crate) fn terminal_owner(
    app: &tauri::AppHandle,
) -> Result<Arc<crate::terminal_host::Terminals>, &'static str> {
    Ok(app
        .try_state::<Runtime>()
        .ok_or("initializing")?
        .terminals
        .clone())
}

pub(crate) fn operation_rows(
    app: &tauri::AppHandle,
) -> Result<Vec<product_contract::operations::Row>, &'static str> {
    let runtime = app.try_state::<Runtime>().ok_or("initializing")?;
    let mut rows = runtime.sessions.operation_rows()?;
    if let Ok(host) = runtime.host() {
        if let Ok(root) = host.component("runtime") {
            use product_contract::operations::{Phase, Row};
            match runtime_engine::component::search::read(
                &root,
                runtime_engine::component::search::Source::Runs,
            ) {
                Ok(snapshot) => {
                    for entry in snapshot.entries.into_iter().take(63) {
                        let phase = match entry.revision[1].as_str() {
                            Some("queued" | "starting" | "running") => Phase::Running,
                            Some("stopping") => Phase::CancelRequested,
                            Some("succeeded") => Phase::Succeeded,
                            Some("cancelled") => Phase::Cancelled,
                            Some("failed") => Phase::Failed,
                            _ => Phase::Unknown,
                        };
                        rows.push(Row::new(
                            "workspace",
                            "workspace.runtime",
                            "tasks",
                            &format!("run-{}", entry.id),
                            "작업·서비스 실행",
                            phase,
                            &entry.revision,
                        )?);
                    }
                }
                Err(_) => rows.push(Row::new(
                    "workspace",
                    "workspace.runtime",
                    "tasks",
                    "runtime-unavailable",
                    "작업 실행 상태 확인 필요",
                    Phase::Unknown,
                    &0,
                )?),
            }
        }
    }
    Ok(rows)
}
pub(crate) fn provider_host(app: &tauri::AppHandle) -> Result<Arc<Host>, &'static str> {
    app.try_state::<Runtime>().ok_or("initializing")?.host()
}

pub(crate) async fn validate_log_selection(
    app: &tauri::AppHandle,
    context: Option<&product_contract::ProjectContext>,
    proof: &crate::selection_logs::Proof,
    deadline: u64,
) -> Result<(), &'static str> {
    let runtime = app.try_state::<Runtime>().ok_or("initializing")?;
    let _context = runtime.context_activity.enter(false)?;
    let window = app.get_webview_window("main").ok_or("window_unavailable")?;
    if product_shell_tauri::workspace_context(&window)?.as_ref() != context {
        return Err("selection_stale");
    }
    crate::selection_logs::revalidate(app, proof, deadline).await
}
pub(crate) async fn editor_selection_proof(
    app: &tauri::AppHandle,
    input: crate::selection_send::Editor,
    deadline: u64,
) -> Result<(Option<product_contract::ProjectContext>, [u8; 32]), &'static str> {
    let runtime = app
        .try_state::<Runtime>()
        .ok_or("initializing")?
        .inner()
        .clone();
    let context_permit = runtime.context_activity.enter(false)?;
    let window = app.get_webview_window("main").ok_or("window_unavailable")?;
    let context = product_shell_tauri::workspace_context(&window)?;
    let filesystem = runtime.filesystem_permit(false, deadline).await?;
    let worker = runtime
        .lanes
        .workers(Lane::Files)
        .try_acquire_owned()
        .map_err(|_| "file_busy")?;
    tokio::task::spawn_blocking(move || {
        let _retained = (context_permit, worker, filesystem);
        let host = runtime.host()?;
        let hash = runtime
            .files
            .lock()
            .map_err(|_| "file_busy")?
            .selection_hash(&host, context.as_ref(), &input, deadline)?;
        Ok((context, hash))
    })
    .await
    .map_err(|_| "file_unavailable")?
}
pub(crate) async fn approve_received_file(
    app: &tauri::AppHandle,
    proof: product_contract::file_reference::Proof,
    deadline: u64,
) -> Result<Value, &'static str> {
    let runtime = app
        .try_state::<Runtime>()
        .ok_or("initializing")?
        .inner()
        .clone();
    let _context = runtime.context_activity.enter(false)?;
    let window = app.get_webview_window("main").ok_or("window_unavailable")?;
    let context = product_shell_tauri::workspace_context(&window)?;
    let permit = runtime
        .lanes
        .workers(Lane::Files)
        .try_acquire_owned()
        .map_err(|_| "file_busy")?;
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let (_permit, _context) = (permit, _context);
        let host = runtime.host()?;
        let path = runtime
            .files
            .lock()
            .map_err(|_| "file_busy")?
            .approve_received(&app, &host, context.as_ref(), proof, deadline)?;
        Ok(json!({"path":path,"context":context}))
    })
    .await
    .map_err(|_| "file_unavailable")?
}

pub(crate) fn suite_migration_status(app: &tauri::AppHandle) -> Result<Value, &'static str> {
    let runtime = app.try_state::<Runtime>().ok_or("migration_unavailable")?;
    let host = runtime.host()?;
    let status = host.status()?;
    let busy = false;
    let selected = status
        .get("selected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Read the selected Registry and revalidate every owned component directory.
    // No repository, task, terminal or scheduler is started by this observation.
    let registry = if selected {
        for component in crate::core::stores::COMPONENTS {
            host.component(component)?;
        }
        Some(host.projects()?.snapshot()?)
    } else {
        None
    };
    let native = serde_json::to_vec(&(status, registry)).map_err(|_| "migration_unavailable")?;
    let summary = product_contract::migration_status::Summary::new(
        "workspace",
        env!("CARGO_PKG_VERSION"),
        busy,
        selected,
        !selected,
        &native,
    )?;
    serde_json::to_value(summary).map_err(|_| "migration_unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{
        allow_table::permits as allowed, changes_context, definitions::definition_access,
        files::file_access, source::source_mutation,
    };
    #[test]
    fn retired_import_methods_are_denied_and_current_templates_remain() {
        for (component, route, parts) in [
            (
                "workspace.setup",
                "overview",
                vec!["prepare", "legacy", "snapshot"],
            ),
            (
                "workspace.files",
                "files",
                vec!["preview", "session", "import"],
            ),
            (
                "workspace.lsp",
                "files",
                vec!["preview", "lsp", "config", "import"],
            ),
            (
                "workspace.terminal",
                "terminal",
                vec!["start", "terminal", "import"],
            ),
            (
                "workspace.runtime",
                "tasks",
                vec!["runtime", "import", "prepare"],
            ),
        ] {
            let method = parts.join("_");
            assert!(!allowed(component, route, &method));
        }
        assert!(allowed("workspace.setup", "overview", "start_empty"));
        assert!(!<crate::ipc::registry::RegistryCall as product_ipc::ComponentCall>::IMPORT_PHASE);
        assert!(allowed("workspace.registry", "overview", "save_template"));
        assert!(allowed(
            "workspace.registry",
            "overview",
            "preview_template_profile_wsl"
        ));
    }
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
    fn terminal_restore_saturation_preserves_live_input_and_stop_capacity() {
        let runtime = Runtime::default();
        let _restores = (0..4)
            .map(|_| {
                runtime
                    .lanes
                    .workers(Lane::Terminal)
                    .try_acquire_owned()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let _queued = (0..64)
            .map(|_| runtime.lanes.try_enter(Lane::Terminal).unwrap())
            .collect::<Vec<_>>();
        assert!(runtime.lanes.try_enter(Lane::Terminal).is_err());
        let _input = runtime.lanes.try_enter(Lane::TerminalIo).unwrap();
        let _input_worker = runtime
            .lanes
            .workers(Lane::TerminalIo)
            .try_acquire_owned()
            .unwrap();
        let _stop = runtime.lanes.try_enter(Lane::TerminalStop).unwrap();
        let _stop_worker = runtime
            .lanes
            .workers(Lane::TerminalStop)
            .try_acquire_owned()
            .unwrap();
    }

    #[test]
    fn runtime_wsl_controls_do_not_grant_raw_terminal_io() {
        for method in [
            "dashboard_snapshot",
            "docker_action",
            "open_wsl_file_in_log_lens",
            "open_distro_terminal",
        ] {
            assert!(allowed("workspace.terminal", "runtime", method));
            assert!(!allowed("workspace.terminal", "files", method));
        }
        for method in [
            "write_session",
            "start_session",
            "broadcast",
            "run_wsl_command",
        ] {
            assert!(!allowed("workspace.terminal", "runtime", method));
        }
    }

    #[test]
    fn template_writes_are_overview_registry_only() {
        for method in ["save_template", "archive_template"] {
            assert!(allowed("workspace.registry", "overview", method));
            for route in ["source", "files", "dependencies", "runtime"] {
                assert!(!allowed("workspace.registry", route, method));
            }
            for component in ["workspace.setup", "workspace.source", "workspace.files"] {
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
            .enter(changes_context("save_lsp_config"))
            .is_err());
        drop(writing);
        assert!(runtime.context_activity.enter(false).is_ok());
    }
    #[test]
    fn saturated_lsp_workers_leave_files_and_project_selection_available() {
        let runtime = Runtime::default();
        let _first = runtime
            .lanes
            .workers(Lane::Lsp)
            .try_acquire_owned()
            .unwrap();
        let _second = runtime
            .lanes
            .workers(Lane::Lsp)
            .try_acquire_owned()
            .unwrap();
        assert!(runtime
            .lanes
            .workers(Lane::Lsp)
            .try_acquire_owned()
            .is_err());
        let _editor = runtime
            .lanes
            .workers(Lane::Files)
            .try_acquire_owned()
            .unwrap();
        let _file_owner = runtime.files.try_lock().unwrap();
        let _selection = runtime.context_activity.enter(true).unwrap();
        let _private_install = runtime.lsp.lock().unwrap();
        let request = runtime.lanes.try_enter(Lane::Lsp).unwrap();
        assert_eq!(runtime.lanes.active(Lane::Lsp), 1);
        runtime.lsp_shutdown.cancel();
        assert_eq!(runtime.lanes.active(Lane::Lsp), 1);
        drop(request);
        assert_eq!(runtime.lanes.active(Lane::Lsp), 0);
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
            ("workspace.setup", "overview", "status"),
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
}
