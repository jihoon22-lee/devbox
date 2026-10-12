mod core;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
use platform::{connection, window};
#[cfg(windows)]
use tauri::Manager;

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
        .manage(connection::Connection::default())
        .invoke_handler(tauri::generate_handler![
            connection::gate_status,
            connection::gate_retry,
            connection::rpc_attach,
            connection::rpc_send,
            connection::rpc_detach
        ])
        .setup(|app| {
            window::configure_main(app.handle())?;
            connection::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                window
                    .app_handle()
                    .state::<connection::Connection>()
                    .detach_window(window.app_handle(), window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("devbox app failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if app
                    .state::<connection::Connection>()
                    .begin_exit(app.clone())
                {
                    api.prevent_exit();
                }
            }
        });
    #[cfg(not(windows))]
    {
        eprintln!("The Devbox desktop app requires Windows.");
        std::process::exit(1);
    }
}
