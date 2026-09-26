pub mod identity;
pub mod remote;
pub mod routes;
pub mod runtime;
pub mod server;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?.canonicalize()?;
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
        let routes =
            routes::Routes::with_runtime(scope.manifest.generation.clone(), runtime.clone());
        server::start(
            app.handle().clone(),
            scope.clone(),
            identity::pipe_name(&suffix),
            routes,
        )?;
        // Resume schedules on agent startup even when no Workspace UI is open.
        // Missing stores/activation leave the owner cold until a later request.
        tauri::async_runtime::spawn_blocking(move || {
            let _ = runtime.initialize();
        });
        Ok(())
    });
    let app = builder.build(context)?;
    app.run(|_, event| {
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
