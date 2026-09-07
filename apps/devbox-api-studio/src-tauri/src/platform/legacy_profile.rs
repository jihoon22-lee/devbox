//! Windows WebView2 profile acquisition. No legacy app is launched or upgraded.
use crate::core::source_snapshot::ClosedStoreSnapshot;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

#[cfg(windows)]
fn exclusive_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(path)
}

/// Returns the new owned WebView2 data directory plus a consistent-copy receipt.
#[cfg(windows)]
pub fn snapshot(
    source_data: &Path,
    owned_stage: &Path,
    cancelled: &AtomicBool,
) -> Result<(PathBuf, ClosedStoreSnapshot), String> {
    use std::os::windows::fs::OpenOptionsExt;
    // Prevent replacement of the source directory while its individual files
    // and LevelDB LOCK are held. No write/create access is requested.
    devbox_filesystem::ensure_no_links(source_data).map_err(|_| "legacy_store_links_forbidden")?;
    let _directory = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x0200_0000 | 0x0020_0000)
        .open(source_data)
        .map_err(|_| "legacy_app_must_be_closed")?;
    let candidates = [
        "EBWebView/Default/Local Storage/leveldb",
        "Default/Local Storage/leveldb",
    ];
    let existing: Vec<_> = candidates
        .iter()
        .filter(|relative| source_data.join(relative).exists())
        .collect();
    if existing.len() != 1 {
        return Err("legacy_browser_store_missing_or_ambiguous".into());
    }
    let data = owned_stage.join("webview-copy");
    std::fs::create_dir(&data).map_err(|_| "legacy_snapshot_target_must_be_new")?;
    let target = data.join(existing[0]);
    std::fs::create_dir_all(target.parent().ok_or("legacy_snapshot_target_invalid")?)
        .map_err(|_| "legacy_snapshot_write_failed")?;
    let receipt = crate::core::source_snapshot::copy_closed_store(
        &source_data.join(existing[0]),
        &target,
        cancelled,
        exclusive_read,
    )?;
    Ok((data, receipt))
}
#[cfg(not(windows))]
pub fn snapshot(
    _: &Path,
    _: &Path,
    _: &AtomicBool,
) -> Result<(PathBuf, ClosedStoreSnapshot), String> {
    Err("legacy_browser_export_requires_windows".into())
}

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

/// A separate owned process fixes WEBVIEW2_USER_DATA_FOLDER to the copy without
/// mutating this running product's environment. That override takes precedence
/// over registry UserDataFolder policies. The worker also checks the COM result.
#[cfg(windows)]
pub async fn run_export_worker(
    stage: &Path,
    stage_id: &str,
    nonce: &str,
    cancelled: &AtomicBool,
) -> Result<crate::migration_export::LegacyApiExport, String> {
    use std::process::Stdio;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let executable = std::env::current_exe()
        .map_err(|_| "legacy_worker_unavailable")?
        .canonicalize()
        .map_err(|_| "legacy_worker_unavailable")?;
    let mut command = tokio::process::Command::new(executable);
    command
        .arg("--migration-export-worker")
        .arg(stage_id)
        .env_clear();
    for name in [
        "SystemRoot",
        "WINDIR",
        "LOCALAPPDATA",
        "APPDATA",
        "TEMP",
        "TMP",
        "USERPROFILE",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("WEBVIEW2_USER_DATA_FOLDER", stage.join("webview-copy"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .creation_flags(0x0000_0004 | 0x0800_0000);
    let mut child = command.spawn().map_err(|_| "legacy_worker_unavailable")?;
    let mut tree = match api_playground_lib::component::OwnedProcessTree::assign(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Relaxed) {
            break Err("legacy_snapshot_cancelled".to_string());
        }
        if started.elapsed() > Duration::from_secs(60) {
            break Err("legacy_worker_timeout".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("legacy_worker_failed".into())
                }
            }
            Err(_) => break Err("legacy_worker_failed".into()),
            Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };
    // Success also closes the Job, waiting for all remaining WebView2 children.
    if !tree.terminate(&mut child).await {
        return Err("legacy_worker_cleanup_failed".into());
    }
    status?;
    crate::migration_export::read_result(stage, nonce)
}
#[cfg(not(windows))]
pub async fn run_export_worker(
    _: &Path,
    _: &str,
    _: &str,
    _: &AtomicBool,
) -> Result<crate::migration_export::LegacyApiExport, String> {
    Err("legacy_browser_export_requires_windows".into())
}
