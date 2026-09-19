//! Fresh legacy checks for the native import acceptance boundary. These handles
//! are not suite activation permission and never start or stop a legacy app.
use crate::core::import_repository::Bundle;
use std::{path::Path, sync::atomic::AtomicBool};
#[cfg(windows)]
pub struct Sources {
    native: crate::core::native_source_backup::SourceGuard,
    browser: Option<data_migration::core::source_snapshot::ClosedSourceGuard>,
    _directories: Vec<std::fs::File>,
}
#[cfg(windows)]
impl Sources {
    pub fn revalidate(&mut self, cancelled: &AtomicBool) -> Result<(), String> {
        self.native.revalidate(cancelled)?;
        if let Some(browser) = &mut self.browser {
            browser.revalidate(cancelled)?;
        }
        Ok(())
    }
}
#[cfg(not(windows))]
pub struct Sources;
#[cfg(not(windows))]
impl Sources {
    pub fn revalidate(&mut self, _: &AtomicBool) -> Result<(), String> {
        Err("migration_source_guard_requires_windows".into())
    }
}
#[cfg(windows)]
pub fn acquire(
    base: &Path,
    stage: &Path,
    bundle: &Bundle,
    cancelled: &AtomicBool,
) -> Result<Sources, String> {
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeSet,
        fs::{File, OpenOptions},
        os::windows::fs::OpenOptionsExt,
    };
    // Pin each existing ancestor when opening a source, so a rename/junction
    // cannot redirect later paths. Existing files deny all competing opens.
    let mut directories = Vec::new();
    let mut pinned = BTreeSet::new();
    let mut open = |path: &Path| -> std::io::Result<File> {
        devbox_filesystem::ensure_no_links(path)?;
        let ancestors = path
            .parent()
            .ok_or_else(|| std::io::Error::other("source parent"))?
            .ancestors()
            .collect::<Vec<_>>();
        for directory in ancestors.into_iter().rev() {
            if pinned.insert(directory.to_owned()) {
                directories.push(
                    OpenOptions::new()
                        .read(true)
                        .share_mode(3)
                        .custom_flags(0x0220_0000)
                        .open(directory)?,
                );
            }
        }
        OpenOptions::new()
            .read(true)
            .share_mode(0)
            .custom_flags(0x0020_0000)
            .open(path)
    };
    let native = crate::core::native_source_backup::hold_source(
        base,
        &stage.join("retained-native"),
        bundle
            .native_backup_revision
            .as_deref()
            .ok_or("migration_backup_binding_missing")?,
        cancelled,
        &mut open,
    )?;
    let browser = if let Some(revision) = &bundle.browser_backup_revision {
        let raw = crate::core::import_repository::read_file(
            &stage.join("closed-source.json"),
            512 * 1024,
        )?
        .ok_or("migration_backup_missing")?;
        if format!("{:x}", Sha256::digest(raw.as_bytes())) != *revision {
            return Err("migration_backup_changed".into());
        }
        let receipt = serde_json::from_str(&raw).map_err(|_| "migration_backup_invalid")?;
        data_migration::core::source_snapshot::verify_closed_copy(
            &stage.join("retained-leveldb"),
            &receipt,
            cancelled,
        )?;
        let source = base.join("com.devbox.apiplayground");
        let candidates = [
            "EBWebView/Default/Local Storage/leveldb",
            "Default/Local Storage/leveldb",
        ];
        let found = candidates
            .iter()
            .map(|relative| source.join(relative))
            .filter(|path| path.exists())
            .collect::<Vec<_>>();
        if found.len() != 1 {
            return Err("legacy_browser_store_missing_or_ambiguous".into());
        }
        Some(data_migration::core::source_snapshot::hold_closed_source(
            &found[0], &receipt, cancelled, &mut open,
        )?)
    } else {
        if native.selected("com.devbox.apiplayground")
            && [
                "EBWebView/Default/Local Storage/leveldb",
                "Default/Local Storage/leveldb",
            ]
            .iter()
            .any(|relative| {
                base.join("com.devbox.apiplayground")
                    .join(relative)
                    .exists()
            })
        {
            return Err("migration_source_changed".into());
        }
        None
    };
    Ok(Sources {
        native,
        browser,
        _directories: directories,
    })
}
#[cfg(not(windows))]
pub fn acquire(_: &Path, _: &Path, _: &Bundle, _: &AtomicBool) -> Result<Sources, String> {
    Err("migration_source_guard_requires_windows".into())
}
