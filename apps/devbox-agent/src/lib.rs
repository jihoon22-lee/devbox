pub mod identity;
pub mod routes;
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
    let builder =
        tauri::Builder::default().plugin(tauri_plugin_single_instance::init(|_, _, _| {}));
    #[cfg(windows)]
    let builder = builder.setup(move |app| {
        let routes = routes::Routes::new(scope.manifest.generation.clone());
        server::start(
            app.handle().clone(),
            scope.clone(),
            identity::pipe_name(&suffix),
            routes,
        )?;
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
