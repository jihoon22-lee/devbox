mod core;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
use platform::window;

pub fn run() {
    // This path deliberately precedes Tauri/WebView2 initialization.
    if std::env::args_os().any(|arg| arg == "--self-check") {
        std::process::exit(core::self_check::run());
    }
    #[cfg(windows)]
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            window::focus_main(app);
        }))
        .setup(|app| {
            window::configure_main(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("devbox app failed to start");
    #[cfg(not(windows))]
    {
        eprintln!("The Devbox desktop app requires Windows.");
        std::process::exit(1);
    }
}
