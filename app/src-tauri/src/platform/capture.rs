//! A preloaded hidden window; showing capture never focuses the main application.
use crate::core::connection::IpcError;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "popout-capture-main";
#[derive(Default)]
pub struct Capture {
    shown: Mutex<Option<Instant>>,
    /// The capture window holds text the inbox does not have. Memory only: a guard, not storage.
    draft: AtomicBool,
}
impl Capture {
    pub(crate) fn has_draft(&self) -> bool {
        self.draft.load(Ordering::SeqCst)
    }
}

pub fn initialize(app: &AppHandle) {
    if let Err(error) = ensure(app) {
        eprintln!("Cannot preload quick capture: {error}");
    }
}
fn ensure(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    super::quit::window_admission(app, || {
        let window =
            WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("/popout/capture".into()))
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
    })
}
pub fn show(app: &AppHandle) {
    if app
        .state::<super::quit::QuitState>()
        .state
        .lock()
        .expect("quit state")
        .sealed()
    {
        return;
    }
    *app.state::<Capture>()
        .shown
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
            .shown
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    {
        eprintln!("capture-ready elapsed_ms={}", start.elapsed().as_millis());
    }
    Ok(())
}
/// The capture window says whether it holds a draft, only when that changes.
#[tauri::command]
pub fn capture_draft(window: WebviewWindow, has_draft: bool) -> Result<(), IpcError> {
    only_capture(&window)?;
    window
        .state::<Capture>()
        .draft
        .store(has_draft, Ordering::SeqCst);
    Ok(())
}
/// The main window asks before a full quit; the update fence reads the same flag.
#[tauri::command]
pub fn capture_has_draft(window: WebviewWindow) -> Result<bool, IpcError> {
    super::preferences::main_only(&window)?;
    Ok(window.state::<Capture>().has_draft())
}
/// [캡처 열기] in the quit confirmation. Async: showing may build the window, which deadlocks
/// in a synchronous command on Windows.
#[tauri::command]
pub async fn capture_open(window: WebviewWindow, app: AppHandle) -> Result<(), IpcError> {
    super::preferences::main_only(&window)?;
    show(&app);
    Ok(())
}
