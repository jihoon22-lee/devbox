pub mod autostart;
pub mod identity;
pub mod mcp;
pub mod remote;
pub mod routes;
pub mod runtime;
pub mod server;
pub mod session_runtime;
pub mod webhooks;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let _login_launch = autostart::login_launch(&std::env::args_os().skip(1).collect::<Vec<_>>())?;
    let executable = std::env::current_exe()?.canonicalize()?;
    // A login shortcut can race installation health/import mode. Do not keep
    // an idle writer alive while bootstrap still needs to commit activation.
    if !product_shell_tauri::component_ready(
        &executable,
        "control-center",
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(std::io::Error::other)?
    {
        return Ok(());
    }
    let _writer = product_shell_tauri::WriterGuard::acquire_component(
        &executable,
        "control-center",
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(std::io::Error::other)?;
    let suffix = product_shell_tauri::component_namespace(
        &executable,
        "control-center",
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(std::io::Error::other)?;
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = format!("com.devbox.v08.agent.i{suffix}");
    #[cfg(windows)]
    let scope = {
        use suite_runtime::platform::component_scope::CapturedScope;
        let owner_dir = executable
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .ok_or("component_path_invalid")?;
        let owner = owner_dir.join("devbox-control-center.exe");
        let root = owner_dir
            .ancestors()
            .nth(4)
            .ok_or("component_path_invalid")?;
        let scope =
            CapturedScope::capture(root, "control-center", &owner, env!("CARGO_PKG_VERSION"))?
                .capture_agent_image()?;
        std::sync::Arc::new(scope)
    };
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .plugin(tauri_plugin_notification::init());
    #[cfg(windows)]
    let builder = builder.setup(move |app| {
        let (_, workspace, _) = scope.member("workspace")?;
        let resources = workspace
            .parent()
            .ok_or("component_path_invalid")?
            .to_owned();
        let data = dirs::data_local_dir()
            .ok_or("runtime_owner_unavailable")?
            .join(format!("com.devbox.v08.workspace.i{suffix}"));
        let runtime = runtime::Runtime::new(app.handle().clone(), data, resources);
        let (_, api_executable, _) = scope.member("api-studio")?;
        let api_root = dirs::data_local_dir()
            .ok_or("component_storage_unavailable")?
            .join(format!("com.devbox.v08.apistudio.i{suffix}"))
            .join("webhooks");
        let webhooks =
            webhooks::Webhooks::new(app.handle().clone(), api_root, api_executable.to_owned());
        let knowledge_root = dirs::data_local_dir()
            .ok_or("knowledge_store_unavailable")?
            .join(format!("com.devbox.v08.knowledge.i{suffix}"));
        let collectors = collectors::Collectors::new(app.handle().clone(), knowledge_root);
        let settings = autostart::Settings::new(scope.clone(), &suffix)?;
        // A registry failure must not disable unrelated background functionality;
        // the settings query will report the fixed failure to the product UI.
        let _ = settings.reconcile();
        let routes = routes::Routes::with_runtime(
            scope.manifest.generation.clone(),
            runtime.clone(),
            webhooks.clone(),
            collectors.clone(),
            settings,
        );
        tray::initialize(
            app.handle(),
            scope.clone(),
            routes.clone(),
            collectors.clone(),
        )?;
        server::start(
            app.handle().clone(),
            scope.clone(),
            identity::pipe_name(&suffix),
            routes,
        )?;
        // This worker holds its own writer lease through copy/receipt writes,
        // including a shutdown racing startup. It cannot outlive the writer gate.
        let launcher_scope = scope.clone();
        let launcher_image = executable.clone();
        let launcher_suffix = suffix.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = (|| -> Result<(), &'static str> {
                let _writer = product_shell_tauri::WriterGuard::acquire_component(
                    &launcher_image,
                    "control-center",
                    env!("CARGO_PKG_VERSION"),
                )?;
                launcher_scope.revalidate()?;
                mcp::launcher::refresh(
                    std::path::Path::new(&launcher_scope.review_root()),
                    &launcher_image,
                )
                .map_err(|_| "mcp_launcher_refresh_failed")?;
                Ok(())
            })();
            if result.is_err() {
                if let Some(root) = dirs::data_local_dir() {
                    if let Ok(log) = product_contract::operation_log::OperationLog::open(
                        root.join(format!("com.devbox.v08.agent.i{launcher_suffix}"))
                            .join("logs"),
                    ) {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|time| time.as_millis() as u64)
                            .unwrap_or(0);
                        log.append(&product_contract::operation_log::Entry::new(
                            now,
                            env!("CARGO_PKG_VERSION"),
                            "agent",
                            "mcp",
                            "refresh_launcher",
                            0,
                            product_contract::operation_log::Outcome::Failed,
                            Some("mcp_launcher_refresh_failed"),
                        ));
                    }
                }
            }
        });
        // Resume schedules on agent startup even when no Workspace UI is open.
        // Missing stores/activation leave the owner cold until a later request.
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = runtime.initialize();
            let _ = webhooks.initialize();
            let _ = collectors.initialize(false);
            tray::refresh(&app_handle);
        });
        Ok(())
    });
    let app = builder.build(context)?;
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            runtime_engine::component::system_session_end(app);
        }
        // No windows exist; only explicit lifecycle shutdown may exit the agent.
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
    Ok(())
}

pub mod collectors;

pub mod tray;
