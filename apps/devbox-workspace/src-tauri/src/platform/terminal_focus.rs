//! WebView2's focused child can leave Tao's top-level focus cache false. Quick
//! Summon needs the actual foreground top-level HWND, not keyboard-child focus.
pub(crate) fn focused(window: &tauri::WebviewWindow) -> Result<bool, &'static str> {
    #[cfg(windows)]
    {
        let handle = window.hwnd().map_err(|_| "terminal_window_unavailable")?;
        Ok(unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 == handle.0)
    }
    #[cfg(not(windows))]
    window
        .is_focused()
        .map_err(|_| "terminal_window_unavailable")
}
