use crate::core::source_snapshot::ClosedStoreSnapshot;
use std::{
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
#[cfg(windows)]
fn exclusive_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(path)
}

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
