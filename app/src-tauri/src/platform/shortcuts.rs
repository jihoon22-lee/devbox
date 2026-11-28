use crate::core::{
    connection::IpcError,
    intents::AppIntent,
    settings,
    shortcuts::{
        self, Registration, ShortcutAction, ShortcutStatus, parse_accelerator, startup::Bindings,
    },
};
use std::{io, sync::Mutex, time::Instant};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Default)]
pub struct Shortcuts(Mutex<Bindings>);
struct Backend<'a>(&'a AppHandle);
impl Registration for Backend<'_> {
    fn register(&mut self, key: &str) -> io::Result<()> {
        self.0
            .global_shortcut()
            .register(key)
            .map_err(io::Error::other)
    }
    fn unregister(&mut self, key: &str) -> io::Result<()> {
        self.0
            .global_shortcut()
            .unregister(key)
            .map_err(io::Error::other)
    }
}
pub fn pressed(
    app: &AppHandle,
    shortcut: &Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    // SC2 starts at native receipt, before the binding lookup and focusing main.
    let received = Instant::now();
    let state = app.state::<Shortcuts>();
    let bindings = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let action = bindings.owners.action(shortcut.id, &bindings.registered);
    drop(bindings);
    let intent = match action {
        Some(ShortcutAction::Capture) => {
            super::capture::show(app);
            return;
        }
        Some(ShortcutAction::Palette) => AppIntent::Palette {
            trace: Some(super::paint_trace::palette_trace(app, received)),
        },
        Some(ShortcutAction::BottomTerminal) => AppIntent::BottomTerminal {
            was_focused: app
                .get_webview_window("main")
                .is_some_and(|main| main.is_focused().unwrap_or(false)),
        },
        None => return,
    };
    super::window::focus_main(app);
    super::intents::dispatch(app, intent);
}
pub fn initialize(app: &AppHandle) {
    let settings = super::preferences::settings_path(app)
        .map_err(|error| io::Error::other(error.message))
        .and_then(|path| settings::load_with_guidance(&path))
        .map(|settings| settings.global_shortcuts);
    apply_initial(app, settings);
}
// Global shortcut registration runs on the event loop. Acquire the bindings lock there too,
// so a simultaneous shortcut press cannot block the thread needed for registration.
pub fn settings_loaded(app: &AppHandle, settings: settings::GlobalShortcuts) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || apply_initial(&handle, Ok(settings))) {
        eprintln!("Cannot apply recovered shortcut settings: {error}");
    }
}
fn apply_initial(app: &AppHandle, settings: io::Result<settings::GlobalShortcuts>) {
    let state = app.state::<Shortcuts>();
    let mut bindings = state.0.lock().unwrap_or_else(|error| error.into_inner());
    if bindings.initialize(&mut Backend(app), settings) {
        super::tray::set_shortcut_status(app, &bindings.status);
    }
}
#[tauri::command]
pub fn shortcut_status(
    window: WebviewWindow,
    state: State<'_, Shortcuts>,
) -> Result<ShortcutStatus, IpcError> {
    super::preferences::main_only(&window)?;
    Ok(state
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .status
        .clone())
}
#[tauri::command]
pub fn configure_shortcuts(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Shortcuts>,
    palette: String,
    capture: String,
    bottom_terminal: Option<String>,
) -> Result<ShortcutStatus, IpcError> {
    super::preferences::main_only(&window)?;
    let path = super::preferences::settings_path(&app)?;
    let mut bindings = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let palette = match parse_accelerator(&palette) {
        Ok(key) => key,
        Err(error) => {
            bindings.status.palette_error = Some(error.to_string());
            super::tray::set_shortcut_status(&app, &bindings.status);
            return Ok(bindings.status.clone());
        }
    };
    let bottom = match bottom_terminal
        .filter(|key| !key.trim().is_empty())
        .map(|key| parse_accelerator(&key))
        .transpose()
    {
        Ok(key) => key,
        Err(error) => {
            bindings.status.bottom_terminal_error = Some(error.to_string());
            super::tray::set_shortcut_status(&app, &bindings.status);
            return Ok(bindings.status.clone());
        }
    };
    if bottom.as_ref() == Some(&palette) {
        bindings.status.bottom_terminal_error =
            Some("팔레트와 터미널의 전역 단축키가 같습니다".into());
        super::tray::set_shortcut_status(&app, &bindings.status);
        return Ok(bindings.status.clone());
    }
    let capture = match shortcuts::capture_binding(&palette, bottom.as_deref(), &capture) {
        Ok(key) => key,
        Err(error) => {
            bindings.status.capture_error = Some(error.to_string());
            super::tray::set_shortcut_status(&app, &bindings.status);
            return Ok(bindings.status.clone());
        }
    };
    let mut next = vec![
        (palette.clone(), ShortcutAction::Palette),
        (capture.clone(), ShortcutAction::Capture),
    ];
    next.extend(
        bottom
            .iter()
            .cloned()
            .map(|key| (key, ShortcutAction::BottomTerminal)),
    );
    let mut backend = Backend(&app);
    let result = settings::with_settings(&path, |settings| {
        // A preferences command may arrive before the supervisor's recovery callback.
        bindings.initialize(&mut backend, Ok(settings.global_shortcuts.clone()));
        let Bindings {
            registered, owners, ..
        } = &mut *bindings;
        shortcuts::replace_action_bindings(&mut backend, registered, owners, &next, || {
            settings.global_shortcuts.palette = Some(palette.clone());
            settings.global_shortcuts.capture = Some(capture.clone());
            settings.global_shortcuts.bottom_terminal = bottom.clone();
            settings::save(&path, settings)
        })
    });
    match result {
        Ok(()) => {
            bindings.status = ShortcutStatus {
                palette,
                capture,
                capture_error: None,
                bottom_terminal: bottom,
                palette_error: None,
                bottom_terminal_error: None,
            };
        }
        Err(error) => {
            // Keep configured values and report failure; registered tracks the actual rollback result.
            let reason = Some(error.to_string());
            bindings.status.palette_error = reason.clone();
            bindings.status.capture_error = reason.clone();
            if bottom.is_some() || bindings.status.bottom_terminal.is_some() {
                bindings.status.bottom_terminal_error = reason;
            }
        }
    }
    super::tray::set_shortcut_status(&app, &bindings.status);
    Ok(bindings.status.clone())
}
