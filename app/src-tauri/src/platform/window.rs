use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use windows::core::Interface;

use crate::core::geometry::{initial_size, minimum_size};

pub fn configure_main(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| tauri::Error::Io(std::io::Error::other("main window is missing")))?;
    if let Some(monitor) = window.current_monitor()? {
        let area = monitor.work_area();
        let scale = monitor.scale_factor();
        // Tauri size setters use client bounds; work area and position use outer bounds.
        let outer = window.outer_size()?;
        let inner = window.inner_size()?;
        let frame = (
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        );
        let (width, height) = initial_size(area.size.width, area.size.height, scale, frame);
        let (min_width, min_height) = minimum_size(area.size.width, area.size.height, scale, frame);
        window.set_min_size(Some(PhysicalSize::new(min_width, min_height)))?;
        window.set_size(PhysicalSize::new(width, height))?;
        window.set_position(PhysicalPosition::new(
            area.position.x + ((area.size.width.saturating_sub(width + frame.0)) / 2) as i32,
            area.position.y + ((area.size.height.saturating_sub(height + frame.1)) / 2) as i32,
        ))?;
    }
    disable_browser_accelerators_and_show(&window)
}

fn disable_browser_accelerators_and_show(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let ready_window = window.clone();
    // with_webview dispatches onto the UI thread; waiting for it here can deadlock.
    // Only reveal the window after the actual COM operation succeeds.
    window.with_webview(move |webview| {
        // SAFETY: Tauri supplies this window's live controller on its UI thread.
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.Settings())
                .and_then(|settings| settings.cast::<ICoreWebView2Settings3>())
                .and_then(|settings| settings.SetAreBrowserAcceleratorKeysEnabled(false))
        };
        if let Err(error) = result {
            eprintln!("Cannot disable WebView2 browser accelerators: {error}");
            ready_window.app_handle().exit(1);
            return;
        }
        if let Err(error) = ready_window.show() {
            eprintln!("Cannot show main window: {error}");
            ready_window.app_handle().exit(1);
        }
    })
}

pub fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main")
        && let Err(error) = window
            .unminimize()
            .and_then(|()| window.show())
            .and_then(|()| window.set_focus())
    {
        eprintln!("Cannot focus main window: {error}");
    }
}
