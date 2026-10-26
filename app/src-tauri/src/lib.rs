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
                .app_name(core::channel::CHANNEL.config().identifier)
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
        .manage(platform::quit::QuitState::default())
        .manage(connection::Connection::default())
        .manage(platform::activity::Activity::default())
        .manage(platform::capture::Capture::default())
        .invoke_handler(tauri::generate_handler![
            connection::gate_status,
            connection::gate_retry,
            connection::rpc_attach,
            connection::rpc_send,
            connection::rpc_detach,
            platform::external::open_external,
            platform::install::install_windows_tool,
            platform::deploy::redeploy_wsl,
            platform::wsl_cleanup::cleanup_wsl,
            platform::external::open_file_location,
            platform::external_editor::open_external_editor,
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
            platform::quit::quit_app,
            platform::quit::quit_window_ready,
            platform::quit::quit_window_not_ready,
            platform::quit::quit_prepared,
            platform::activity::activity_state,
            platform::activity::activity_set,
            platform::activity::activity_configure,
            platform::activity::activity_drain,
            platform::activity::activity_ack,
            platform::capture::capture_hide,
            platform::capture::capture_ready,
            platform::activity::activity_tray,
            platform::update::update_status,
            platform::update::update_check,
            platform::update::update_download,
            platform::update::update_cancel
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            app.deep_link().register_all()?;
            if let Err(error) = platform::tray::initialize(app.handle()) {
                eprintln!("Cannot create tray; closing main will exit: {error}");
            }
            platform::shortcuts::initialize(app.handle());
            platform::activity::initialize(app.handle());
            platform::capture::initialize(app.handle());
            platform::update::initialize(app.handle());
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
            // Keep note buffers alive even before the renderer's listener is ready.
            // Its explicit destroy happens only after the latest save has completed.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && (window.label().starts_with("popout-note-")
                    || window.label().starts_with("popout-code-"))
            {
                api.prevent_close();
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == platform::capture::LABEL
                && window.hide().is_ok()
            {
                api.prevent_close();
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                if platform::tray::available(window.app_handle(), window.label()) {
                    match window.hide() {
                        Ok(()) => platform::tray::show_notice(window.app_handle()),
                        Err(error) => eprintln!("Cannot hide main window: {error}"),
                    }
                } else {
                    platform::intents::dispatch(
                        window.app_handle(),
                        core::intents::AppIntent::Quit,
                    );
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                platform::quit::destroyed(window.app_handle(), window.label());
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
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if !platform::quit::approved(app) {
                    api.prevent_exit();
                    platform::intents::dispatch(app, core::intents::AppIntent::Quit);
                } else if app
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
