//! Private fingerprints of fixed legacy namespaces. Never follow a vault,
//! repository, managed install root or any other path stored inside a file.
use devbox_filesystem::{ensure_no_links, opened_filesystem_identity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_ENTRIES: usize = 50_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Namespace {
    pub identifier: String,
    pub present: bool,
    pub revision: String,
    pub files: usize,
    pub bytes: u64,
}
pub struct Guard {
    pub namespaces: Vec<Namespace>,
    base: PathBuf,
    inventory: Vec<(String, Vec<String>, Vec<String>)>,
    files: Vec<(File, String)>,
    _directories: Vec<File>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn check(start: Instant) -> Result<()> {
    if start.elapsed() > Duration::from_secs(25) {
        Err("source_inventory_expired")
    } else {
        Ok(())
    }
}
fn open(path: &Path, directory: bool, exclusive: bool) -> Result<File> {
    ensure_no_links(path).map_err(|_| "source_inventory_unsafe")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .share_mode(if directory {
                3
            } else if exclusive {
                0
            } else {
                7
            })
            .custom_flags(if directory { 0x0220_0000 } else { 0x0020_0000 })
            .open(path)
            .map_err(|_| "legacy_writers_must_close")
    }
    #[cfg(not(windows))]
    {
        let _ = (directory, exclusive);
        OpenOptions::new()
            .read(true)
            .open(path)
            .map_err(|_| "source_inventory_unavailable")
    }
}
fn listing(root: &Path, start: Instant) -> Result<(Vec<String>, Vec<String>)> {
    ensure_no_links(root).map_err(|_| "source_inventory_unsafe")?;
    let mut directories = vec![];
    let mut files = vec![];
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        check(start)?;
        for entry in fs::read_dir(path).map_err(|_| "source_inventory_unavailable")? {
            let path = entry.map_err(|_| "source_inventory_unavailable")?.path();
            ensure_no_links(&path).map_err(|_| "source_inventory_unsafe")?;
            let metadata =
                fs::symlink_metadata(&path).map_err(|_| "source_inventory_unavailable")?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "source_inventory_unsafe")?
                .to_str()
                .ok_or("source_inventory_unsafe")?
                .replace('\\', "/");
            if relative.len() > 4096
                || relative.split('/').any(|part| {
                    part.is_empty() || part == "." || part == ".." || part.contains(':')
                })
            {
                return Err("source_inventory_unsafe");
            }
            if metadata.is_dir() {
                directories.push(relative);
                pending.push(path);
            } else if metadata.is_file() {
                // SQLite creates/removes shared-memory bookkeeping during a
                // read-only WAL snapshot. WAL and database bytes remain covered.
                if !relative.ends_with(".db-shm") {
                    files.push(relative);
                }
            } else {
                return Err("source_inventory_unsafe");
            }
            if files.len() + directories.len() > MAX_ENTRIES {
                return Err("source_inventory_limit");
            }
        }
    }
    files.sort();
    directories.sort();
    Ok((directories, files))
}
fn hash_file(file: &mut File, start: Instant, total: &mut u64) -> Result<String> {
    let before = file
        .metadata()
        .map_err(|_| "source_inventory_unavailable")?;
    if before.len() > MAX_BYTES {
        return Err("source_inventory_limit");
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "source_inventory_unavailable")?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        check(start)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| "source_inventory_unavailable")?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        *total += count as u64;
        if *total > MAX_BYTES {
            return Err("source_inventory_limit");
        }
        hash.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| "source_inventory_unavailable")?;
    if before.len() != bytes
        || after.len() != bytes
        || before.modified().ok() != after.modified().ok()
    {
        return Err("source_inventory_changed");
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
pub fn capture(base: &Path, identifiers: &[&str], exclusive: bool) -> Result<Guard> {
    let start = Instant::now();
    let known = product_contract::installation::PRODUCTS
        .iter()
        .flat_map(|owner| product_contract::migration_source::identifiers(owner))
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if identifiers.iter().any(|id| !known.contains(id))
        || identifiers
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != identifiers.len()
    {
        return Err("source_inventory_invalid");
    }
    let mut guard = Guard {
        namespaces: vec![],
        base: base.into(),
        inventory: vec![],
        files: vec![],
        _directories: vec![open(base, true, false)?],
    };
    let mut total = 0;
    for id in identifiers {
        let root = base.join(id);
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                guard.namespaces.push(Namespace {
                    identifier: (*id).into(),
                    present: false,
                    revision: digest(b"absent"),
                    files: 0,
                    bytes: 0,
                });
                continue;
            }
            Err(_) => return Err("source_inventory_unavailable"),
            Ok(_) => {}
        }
        let directory = open(&root, true, false)?;
        let identity = opened_filesystem_identity(&directory, true)
            .map_err(|_| "source_inventory_unavailable")?
            .components();
        guard._directories.push(directory);
        let (directories, files) = listing(&root, start)?;
        for relative in &directories {
            guard
                ._directories
                .push(open(&root.join(relative), true, false)?);
        }
        let begin = total;
        let mut entries = vec![];
        for relative in &files {
            let mut input = open(&root.join(relative), false, exclusive)?;
            let identity = opened_filesystem_identity(&input, false)
                .map_err(|_| "source_inventory_unavailable")?
                .components();
            let sha = hash_file(&mut input, start, &mut total)?;
            entries.push((relative, identity, sha.clone()));
            if exclusive {
                guard.files.push((input, sha));
            }
        }
        if listing(&root, start)? != (directories.clone(), files.clone()) {
            return Err("source_inventory_changed");
        }
        let revision = digest(
            &serde_json::to_vec(&(identity, &directories, &entries))
                .map_err(|_| "source_inventory_invalid")?,
        );
        guard.namespaces.push(Namespace {
            identifier: (*id).into(),
            present: true,
            revision,
            files: files.len(),
            bytes: total - begin,
        });
        guard.inventory.push(((*id).into(), directories, files));
    }
    Ok(guard)
}
impl Guard {
    /// Use only on the helper's exclusive capture, after all owner processes
    /// have closed. Retain handles until activation metadata has been published.
    pub fn revalidate(&mut self) -> Result<()> {
        let start = Instant::now();
        let mut total = 0;
        for (file, expected) in &mut self.files {
            if hash_file(file, start, &mut total)? != *expected {
                return Err("source_inventory_changed");
            }
        }
        for (id, directories, files) in &self.inventory {
            if listing(&self.base.join(id), start)? != (directories.clone(), files.clone()) {
                return Err("source_inventory_changed");
            }
        }
        for row in self.namespaces.iter().filter(|row| !row.present) {
            if !matches!(fs::symlink_metadata(self.base.join(&row.identifier)),Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("source_inventory_changed");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn wal_changes_and_new_namespaces_invalidate_review_but_sqlite_shared_memory_does_not() {
        let root =
            std::env::temp_dir().join(format!("devbox-source-fixture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let _scratch = Scratch(root.clone());
        let ids = ["com.devbox.runmanager"];
        let absent = capture(&root, &ids, false).unwrap().namespaces;
        let data = root.join(ids[0]);
        fs::create_dir(&data).unwrap();
        fs::write(data.join("data.db"), b"database").unwrap();
        fs::write(data.join("data.db-wal"), b"committed WAL").unwrap();
        let before = capture(&root, &ids, false).unwrap().namespaces;
        assert_ne!(before, absent);
        fs::write(data.join("data.db-shm"), b"ephemeral").unwrap();
        assert_eq!(capture(&root, &ids, false).unwrap().namespaces, before);
        fs::write(data.join("data.db-wal"), b"new WAL row").unwrap();
        assert_ne!(capture(&root, &ids, false).unwrap().namespaces, before);
        assert!(capture(&root, &["../outside"], false).is_err());
        let mut guard = capture(&root, &["com.devbox.devboxlauncher"], true).unwrap();
        fs::create_dir(root.join("com.devbox.devboxlauncher")).unwrap();
        assert!(guard.revalidate().is_err());
    }
}
