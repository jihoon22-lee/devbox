pub mod cleanup;
mod commands;
pub mod core;
mod lifecycle;
pub mod logs;
pub mod notifications;
pub mod platform;
pub mod scheduler;
pub mod storage;

use lifecycle::{is_background_launch, request_orderly_exit, RuntimeState};
use serde::Serialize;
use std::sync::Arc;
use tauri::{Emitter, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecondInstancePayload {
    args: Vec<String>,
    cwd: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The single-instance plugin must remain first: later plugins and setup may
    // otherwise initialize a second scheduler before the duplicate exits.
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let background = args.iter().any(|arg| arg == "--background");
            if app
                .try_state::<Arc<RuntimeState>>()
                .is_some_and(|state| state.shutdown_requested())
            {
                return;
            }
            let _ = app.emit("second-instance", SecondInstancePayload { args, cwd });
            if !background {
                let _ = lifecycle::show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::runtime_status,
            commands::show_main_window,
            commands::hide_main_window,
            commands::quit_app,
            commands::startup_shortcut_status,
            commands::set_startup_shortcut_enabled,
            commands::list_jobs,
            commands::get_job,
            commands::create_job,
            commands::update_job,
            commands::set_job_enabled,
            commands::delete_job,
            commands::run_job_now,
            commands::stop_active_run,
            commands::get_active_run,
            commands::list_active_runs,
            commands::list_services,
            commands::get_service,
            commands::create_service,
            commands::update_service,
            commands::delete_service,
            commands::get_service_instance,
            commands::start_service,
            commands::stop_service,
            commands::restart_service,
            commands::get_run,
            commands::list_runs,
            commands::preview_cron,
            commands::tail_log,
        ])
        .setup(|app| {
            let data_dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(data_dir.join("logs/runs"))?;
            let background = is_background_launch(&std::env::args_os().collect::<Vec<_>>());
            let database_path = data_dir.join("data.db");
            let database = Arc::new(storage::DatabaseState::open(&database_path)?);
            if !database.is_ready() {
                return Err(std::io::Error::other("database connection is not ready").into());
            }
            if let Err(error) = cleanup::RetentionCleaner::new(&data_dir)
                .run(&database, storage::current_epoch_millis())
            {
                eprintln!("Run Manager retention cleanup will retry later: {error}");
            }
            if let Err(error) = notifications::drain_pending(app.handle(), &database) {
                eprintln!("Run Manager notification outbox will retry later: {error}");
            }
            let protector = platform::environment::EnvironmentProtectorState::new();
            let adapter = Arc::new(platform::execution::PlatformExecutionAdapter::new(
                Arc::clone(&database),
                protector.clone(),
                data_dir.clone(),
            ));
            let listener = Arc::new(notifications::SchedulerNotificationListener::new(
                app.handle().clone(),
                Arc::clone(&database),
            ));
            let coordinator = scheduler::SchedulerCoordinator::with_config_and_listener(
                Arc::clone(&database),
                adapter,
                scheduler::SchedulerConfig::default()
                    .with_shutdown_timeout(std::time::Duration::from_secs(5)),
                listener,
            );
            app.manage(Arc::clone(&database));
            app.manage(protector);
            let state = Arc::new(RuntimeState::new(database_path, background, coordinator));
            app.manage(state.clone());
            let main_window = app
                .get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("main window is unavailable"))?;
            platform::install_session_end_hook(&main_window, app.handle(), state.clone())
                .map_err(std::io::Error::other)?;
            lifecycle::spawn_scheduler(Arc::clone(&state));
            lifecycle::spawn_maintenance(
                Arc::clone(&state),
                Arc::clone(&database),
                app.handle().clone(),
                data_dir,
            );
            setup_tray(app)?;
            if !background {
                lifecycle::show_main_window(app.handle()).map_err(std::io::Error::other)?;
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<RuntimeState>>();
                if !state.exit_authorized() {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(error) => {
            eprintln!("Run Manager failed to start: {error}");
            return;
        }
    };

    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            let state = handle.state::<Arc<RuntimeState>>().inner().clone();
            if !state.exit_authorized() {
                api.prevent_exit();
                request_orderly_exit(handle, state);
            }
        }
    });
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "열기", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app.default_window_icon().cloned().ok_or_else(|| {
        tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing default app icon",
        ))
    })?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("Run Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                let _ = lifecycle::show_main_window(app);
            }
            "quit" => {
                let state = app.state::<Arc<RuntimeState>>().inner().clone();
                request_orderly_exit(app, state);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
