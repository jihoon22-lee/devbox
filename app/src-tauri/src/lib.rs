mod core;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
use platform::{connection, window};
#[cfg(windows)]
use tauri::Manager;
#[cfg(all(windows, debug_assertions))]
use tauri_plugin_deep_link::DeepLinkExt;

pub fn run() {
    // This path deliberately precedes Tauri/WebView2 initialization.
    if std::env::args_os().any(|arg| arg == "--self-check") {
        std::process::exit(core::self_check::run());
    }
    #[cfg(windows)]
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !platform::notify::handle_arguments(app, &args, true)
                && !args.iter().any(|arg| arg == "--minimized")
            {
                window::focus_main(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("io.github.jihoon22lee.devbox")
                .arg("--minimized")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(platform::shortcuts::pressed)
                .build(),
        )
        .manage(platform::notify::Notifications::default())
        .manage(platform::shortcuts::Shortcuts::default())
        .manage(platform::intents::Intents::default())
        .manage(connection::Connection::default())
        .invoke_handler(tauri::generate_handler![
            connection::gate_status,
            connection::gate_retry,
            connection::rpc_attach,
            connection::rpc_send,
            connection::rpc_detach,
            platform::external::open_external,
            platform::external::open_file_location,
            platform::notify::notify_attention,
            platform::notify::set_badge,
            platform::notify::pending_navigation,
            platform::notify::acknowledge_navigation,
            platform::preferences::app_preferences,
            platform::preferences::set_autostart,
            platform::shortcuts::shortcut_status,
            platform::shortcuts::configure_shortcuts,
            platform::intents::app_intents_ready,
            platform::intents::app_intents_not_ready,
            platform::intents::hide_main,
            platform::intents::quit_app
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.deep_link().register_all()?;
            if let Err(error) = platform::tray::initialize(app.handle()) {
                eprintln!("Cannot create tray; closing main will exit: {error}");
            }
            platform::shortcuts::initialize(app.handle());
            let focus = platform::notify::handle_arguments(
                app.handle(),
                &std::env::args().collect::<Vec<_>>(),
                false,
            );
            let minimized = std::env::args_os().any(|arg| arg == "--minimized");
            window::configure_main(app.handle(), focus, minimized)?;
            connection::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && platform::tray::available(window.app_handle(), window.label())
            {
                match window.hide() {
                    Ok(()) => {
                        api.prevent_close();
                        platform::tray::show_notice(window.app_handle());
                    }
                    Err(error) => eprintln!("Cannot hide main window: {error}"),
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                if window.label() == "main" {
                    platform::intents::disconnect(window.app_handle());
                }
                window
                    .app_handle()
                    .state::<connection::Connection>()
                    .detach_window(window.app_handle(), window.label());
            }
        })
        .build(tauri::generate_context!())
        .expect("devbox app failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event
                && app
                    .state::<connection::Connection>()
                    .begin_exit(app.clone())
            {
                api.prevent_exit();
            }
        });
    #[cfg(not(windows))]
    {
        eprintln!("The Devbox desktop app requires Windows.");
        std::process::exit(1);
    }
}
