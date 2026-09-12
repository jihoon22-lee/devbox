use std::path::PathBuf;
/// Environment or registry WebView2 overrides must never redirect the exporter
/// into the live source or another product profile. Verify the actual COM value.
#[cfg(windows)]
pub fn verify_profile(
    window: &tauri::WebviewWindow,
    expected: PathBuf,
    complete: impl FnOnce(Result<(), String>) + Send + 'static,
) -> tauri::Result<()> {
    window.with_webview(move |view| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment7;
        use windows::core::{Interface, PWSTR};
        let result = (|| {
            let environment = view
                .environment()
                .cast::<ICoreWebView2Environment7>()
                .map_err(|_| "legacy_profile_verification_unavailable")?;
            let mut raw = PWSTR::null();
            unsafe { environment.UserDataFolder(&mut raw) }
                .map_err(|_| "legacy_profile_verification_unavailable")?;
            if raw.is_null() {
                return Err("legacy_profile_verification_unavailable".into());
            }
            let actual = PathBuf::from(webview2_com::take_pwstr(raw));
            devbox_filesystem::ensure_no_links(&actual).map_err(|_| "legacy_profile_redirected")?;
            let actual_id = devbox_filesystem::filesystem_identity(&actual, true)
                .map_err(|_| "legacy_profile_verification_unavailable")?;
            let expected_id = devbox_filesystem::filesystem_identity(&expected, true)
                .map_err(|_| "legacy_profile_verification_unavailable")?;
            if actual_id != expected_id {
                return Err("legacy_profile_redirected".into());
            }
            Ok(())
        })();
        complete(result);
    })
}
#[cfg(not(windows))]
pub fn verify_profile(
    _: &tauri::WebviewWindow,
    _: PathBuf,
    complete: impl FnOnce(Result<(), String>) + Send + 'static,
) -> tauri::Result<()> {
    complete(Err("legacy_browser_export_requires_windows".into()));
    Ok(())
}
