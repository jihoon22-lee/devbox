use crate::core::{
    connection::IpcError,
    intents::AppIntent,
    settings,
    shortcuts::{self, Registration, ShortcutStatus, parse_accelerator},
};
use std::{io, sync::Mutex};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Default)]
pub struct Shortcuts(Mutex<Bindings>);
#[derive(Default)]
struct Bindings {
    status: ShortcutStatus,
    registered: Vec<String>,
}
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
    let state = app.state::<Shortcuts>();
    let bindings = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let is = |key: &str| {
        key.parse::<Shortcut>()
            .is_ok_and(|key| key.id == shortcut.id)
    };
    if !bindings.registered.iter().any(|key| is(key)) {
        return;
    }
    let intent = if is(&bindings.status.palette) {
        Some(AppIntent::Palette)
    } else if bindings.status.bottom_terminal.as_deref().is_some_and(is) {
        Some(AppIntent::BottomTerminal {
            was_focused: app
                .get_webview_window("main")
                .is_some_and(|main| main.is_focused().unwrap_or(false)),
        })
    } else {
        None
    };
    drop(bindings);
    if let Some(intent) = intent {
        super::window::focus_main(app);
        super::intents::dispatch(app, intent);
    }
}
pub fn initialize(app: &AppHandle) {
    let state = app.state::<Shortcuts>();
    let mut bindings = state.0.lock().unwrap_or_else(|error| error.into_inner());
    let settings = super::preferences::settings_path(app)
        .and_then(|path| settings::load(&path).map_err(|error| IpcError::new(error.to_string())));
    let settings = match settings {
        Ok(settings) => settings,
        Err(error) => {
            bindings.status.palette_error =
                Some(format!("설정을 읽지 못했습니다: {}", error.message));
            super::tray::set_shortcut_status(app, &bindings.status);
            return;
        }
    };
    bindings.status.palette = settings
        .global_shortcuts
        .palette
        .unwrap_or_else(|| shortcuts::default_palette(cfg!(debug_assertions)).into());
    bindings.status.bottom_terminal = settings.global_shortcuts.bottom_terminal;
    let mut backend = Backend(app);
    let mut register = |key: &str| -> io::Result<String> {
        let canonical = parse_accelerator(key)?;
        backend.register(&canonical)?;
        Ok(canonical)
    };
    match register(&bindings.status.palette) {
        Ok(key) => {
            bindings.status.palette = key.clone();
            bindings.registered.push(key);
        }
        Err(error) => bindings.status.palette_error = Some(error.to_string()),
    }
    if let Some(key) = bindings.status.bottom_terminal.clone() {
        let result = if bindings
            .registered
            .contains(&parse_accelerator(&key).unwrap_or_default())
        {
            Err(io::Error::other("팔레트와 터미널의 전역 단축키가 같습니다"))
        } else {
            register(&key)
        };
        match result {
            Ok(key) => {
                bindings.status.bottom_terminal = Some(key.clone());
                bindings.registered.push(key);
            }
            Err(error) => bindings.status.bottom_terminal_error = Some(error.to_string()),
        }
    }
    super::tray::set_shortcut_status(app, &bindings.status);
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
    let mut next = vec![palette.clone()];
    next.extend(bottom.iter().cloned());
    let mut backend = Backend(&app);
    let result = settings::with_settings(&path, |settings| {
        shortcuts::replace_bindings(&mut backend, &mut bindings.registered, &next, || {
            settings.global_shortcuts.palette = Some(palette.clone());
            settings.global_shortcuts.bottom_terminal = bottom.clone();
            settings::save(&path, settings)
        })
    });
    match result {
        Ok(()) => {
            bindings.status = ShortcutStatus {
                palette,
                bottom_terminal: bottom,
                palette_error: None,
                bottom_terminal_error: None,
            };
        }
        Err(error) => {
            // Keep configured values and report failure; registered tracks the actual rollback result.
            let reason = Some(error.to_string());
            bindings.status.palette_error = reason.clone();
            if bottom.is_some() || bindings.status.bottom_terminal.is_some() {
                bindings.status.bottom_terminal_error = reason;
            }
        }
    }
    super::tray::set_shortcut_status(&app, &bindings.status);
    Ok(bindings.status.clone())
}
