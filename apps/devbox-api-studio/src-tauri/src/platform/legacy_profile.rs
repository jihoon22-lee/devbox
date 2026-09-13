//! Windows WebView2 profile acquisition. No legacy app is launched or upgraded.
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[cfg(all(test, windows))]
fn exclusive_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(path)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn actual_windows_delete_lock_is_retried_only_for_the_owned_export_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let repo =
            crate::core::import_repository::Repository::open(root.path(), "fixture").unwrap();
        let (id, stage) = repo.new_stage().unwrap();
        let copy = stage.join("webview-copy");
        std::fs::create_dir(&copy).unwrap();
        let file = copy.join("fixture-data");
        std::fs::write(&file, b"owned copy").unwrap();
        let original = root.path().join("original");
        std::fs::write(&original, b"unchanged").unwrap();
        let guard = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(file)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            drop(guard);
        });
        repo.clear_export_copy(&id).unwrap();
        release.join().unwrap();
        assert!(!copy.exists());
        assert_eq!(std::fs::read(original).unwrap(), b"unchanged");
    }
    #[test]
    fn actual_exclusive_windows_handles_allow_identity_checks_and_preserve_source() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let leveldb = source.join("EBWebView/Default/Local Storage/leveldb");
        std::fs::create_dir_all(&leveldb).unwrap();
        std::fs::write(leveldb.join("LOCK"), []).unwrap();
        std::fs::write(leveldb.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
        std::fs::write(leveldb.join("MANIFEST-000001"), b"synthetic manifest").unwrap();
        std::fs::write(leveldb.join("000003.log"), b"synthetic store data").unwrap();
        let before =
            devbox_filesystem::filesystem_identity(leveldb.join("CURRENT"), false).unwrap();
        let guard = exclusive_read(&leveldb.join("CURRENT")).unwrap();
        assert!(std::fs::File::open(leveldb.join("CURRENT")).is_err());
        assert_eq!(
            devbox_filesystem::filesystem_identity(leveldb.join("CURRENT"), false).unwrap(),
            before
        );
        drop(guard);
        let stage = root.path().join("stage");
        std::fs::create_dir(&stage).unwrap();
        let (_, receipt) = snapshot(&source, &stage, &AtomicBool::new(false)).unwrap();
        assert_eq!(receipt.files.len(), 3);
        assert_eq!(
            std::fs::read(leveldb.join("CURRENT")).unwrap(),
            b"MANIFEST-000001\n"
        );
        assert_eq!(
            std::fs::read(leveldb.join("000003.log")).unwrap(),
            b"synthetic store data"
        );
    }
}

pub use super::browser_profile::verify_profile;
/// Returns the new owned WebView2 data directory plus a consistent-copy receipt.
pub use super::browser_snapshot::snapshot;

/// A separate owned process fixes WEBVIEW2_USER_DATA_FOLDER to the copy without
/// mutating this running product's environment. That override takes precedence
/// over registry UserDataFolder policies. The worker also checks the COM result.
#[cfg(windows)]
pub async fn run_export_worker(
    stage: &Path,
    stage_id: &str,
    nonce: &str,
    cancelled: &AtomicBool,
    progress: impl Fn(&'static str),
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
    progress("api-export-launch");
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
        if let Some(stage_name) = crate::migration_export::worker_progress(stage) {
            progress(stage_name);
        }
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
    _: impl Fn(&'static str),
) -> Result<crate::migration_export::LegacyApiExport, String> {
    Err("legacy_browser_export_requires_windows".into())
}
