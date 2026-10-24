//! A preloaded hidden window; showing capture never focuses the main application.
use crate::core::connection::IpcError;
use std::{sync::Mutex, time::Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "popout-capture-main";
#[derive(Default)]
pub struct Capture(Mutex<Option<Instant>>);

pub fn initialize(app: &AppHandle) {
    if let Err(error) = ensure(app) {
        eprintln!("Cannot preload quick capture: {error}");
    }
}
fn ensure(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("/popout/capture".into()))
        .title("빠른 캡처")
        .inner_size(520.0, 220.0)
        .visible(false)
        .always_on_top(true)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .build()?;
    super::window::disable_browser_accelerators_and_show(&window, false, false)?;
    Ok(window)
}
pub fn show(app: &AppHandle) {
    *app.state::<Capture>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    let result = ensure(app).and_then(|window| {
        window.center()?;
        super::window::disable_browser_accelerators_and_show(&window, true, true)?;
        app.emit_to(LABEL, "capture://show", ())
    });
    if let Err(error) = result {
        eprintln!("Cannot show quick capture: {error}");
    }
}
fn only_capture(window: &WebviewWindow) -> Result<(), IpcError> {
    if window.label() != LABEL {
        return Err(IpcError::new("빠른 캡처 창에서만 사용할 수 있습니다"));
    }
    Ok(())
}
#[tauri::command]
pub fn capture_hide(window: WebviewWindow) -> Result<(), IpcError> {
    only_capture(&window)?;
    window.hide().map_err(|e| IpcError::new(e.to_string()))
}
#[tauri::command]
pub fn capture_ready(window: WebviewWindow, app: AppHandle) -> Result<(), IpcError> {
    only_capture(&window)?;
    if window
        .is_visible()
        .map_err(|e| IpcError::new(e.to_string()))?
        && let Some(start) = app
            .state::<Capture>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    {
        eprintln!("capture-ready elapsed_ms={}", start.elapsed().as_millis());
    }
    Ok(())
}
