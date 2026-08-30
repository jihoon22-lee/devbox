mod commands;
mod core;
mod hotkey;

use tauri::{Manager, State, WindowEvent};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutView {
    pub accelerator: String,
    pub enabled: bool,
    pub registration: hotkey::RegistrationState,
    pub alternatives: Vec<String>,
}

impl ShortcutView {
    fn from_config(value: hotkey::ShortcutConfig, runtime: &hotkey::RuntimeState) -> Self {
        Self {
            accelerator: value.accelerator.clone(),
            enabled: value.enabled,
            registration: runtime.registration(),
            alternatives: hotkey::SHORTCUTS
                .iter()
                .filter(|shortcut| **shortcut != value.accelerator)
                .map(|shortcut| (*shortcut).into())
                .collect(),
        }
    }
}

#[tauri::command]
fn shortcut_config(
    app: tauri::AppHandle,
    runtime: State<'_, hotkey::RuntimeState>,
) -> ShortcutView {
    ShortcutView::from_config(hotkey::load(&app), runtime.inner())
}

#[tauri::command]
fn set_shortcut(
    app: tauri::AppHandle,
    config: hotkey::ShortcutConfig,
    runtime: State<'_, hotkey::RuntimeState>,
) -> Result<ShortcutView, String> {
    let saved = hotkey::save(&app, config)?;
    runtime.stop_global_listener();
    hotkey::start_global_listener(&app, &saved, runtime.inner().clone());
    Ok(ShortcutView::from_config(saved, runtime.inner()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(hotkey::RuntimeState::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let config = hotkey::load(app.handle());
            hotkey::start_global_listener(
                app.handle(),
                &config,
                app.state::<hotkey::RuntimeState>().inner().clone(),
            );
            // A successfully registered shortcut owns discovery of the
            // transient window. If registration failed or is unsupported,
            // keep the initial window visible so the user can choose another
            // allow-listed shortcut instead of being stranded with a hidden
            // process.
            if let Some(window) = app.get_webview_window("main") {
                if app.state::<hotkey::RuntimeState>().registration()
                    == hotkey::RegistrationState::Registered
                {
                    let _ = window.hide();
                } else {
                    // `tauri.conf.json` starts this transient window hidden to
                    // avoid a successful registration flashing on screen.
                    // Registration failure therefore has to show it
                    // explicitly or the user cannot choose an alternative.
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::search,
            commands::launch_result,
            commands::preview_text_action,
            commands::perform_text_action,
            commands::set_favorite,
            commands::clear_recents,
            shortcut_config,
            set_shortcut,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Devbox Launcher");

    app.run(|handle, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            handle
                .state::<hotkey::RuntimeState>()
                .stop_global_listener();
        }
    });
}
