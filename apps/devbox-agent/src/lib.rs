pub mod identity;

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
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .build(context)?;
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
