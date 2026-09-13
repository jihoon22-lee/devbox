//! Extract an exact reviewed archive into a newly owned generation product slot.
//! No existing destination is overwritten and no archive path chooses its root.
use super::suite_package::{Asset, ProductPackage, MAX_PACKAGE_BYTES};
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
type Result<T> = std::result::Result<T, &'static str>;
fn digest(file: &mut File, expected: &Asset, cancelled: &AtomicBool) -> Result<()> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "suite_archive_unavailable")?;
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    let mut hasher = Sha256::new();
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("suite_stage_cancelled");
        }
        let count = file
            .read(&mut buffer)
            .map_err(|_| "suite_archive_unavailable")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > expected.size || total > MAX_PACKAGE_BYTES {
            return Err("suite_archive_size_mismatch");
        }
        hasher.update(&buffer[..count]);
    }
    let sha: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if total != expected.size || sha != expected.sha256 {
        return Err("suite_archive_digest_mismatch");
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "suite_archive_unavailable")?;
    Ok(())
}
pub fn stage(
    archive: &Path,
    products_root: &Path,
    package: &ProductPackage,
    cancelled: &AtomicBool,
) -> Result<()> {
    if !product_contract::installation::PRODUCTS.contains(&package.id.as_str()) {
        return Err("suite_product_invalid");
    }
    ensure_no_links(archive).map_err(|_| "suite_archive_unsafe")?;
    ensure_no_links(products_root).map_err(|_| "suite_stage_unsafe")?;
    #[cfg(windows)]
    let mut directories = crate::suite::platform::component_scope::pin_directories(products_root)?;
    let (_parent, parent_identity) =
        open_filesystem_object(products_root, true).map_err(|_| "suite_stage_unavailable")?;
    let (mut file, archive_identity) =
        open_filesystem_object(archive, false).map_err(|_| "suite_archive_unavailable")?;
    digest(&mut file, &package.portable, cancelled)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "suite_archive_invalid")?;
    if zip.len() != package.files.len() {
        return Err("suite_archive_entries_mismatch");
    }
    let mut names = std::collections::BTreeSet::new();
    // Validate every entry before creating the owned target.
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|_| "suite_archive_invalid")?;
        let expected = package
            .files
            .iter()
            .find(|f| f.name == entry.name())
            .ok_or("suite_archive_entry_denied")?;
        if !super::suite_package::product_file(&package.id, entry.name())
            || !names.insert(entry.name().to_owned())
            || entry.is_dir()
            || entry.encrypted()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
            || entry.enclosed_name().is_none()
            || entry.size() != expected.size
            || entry.size() > MAX_PACKAGE_BYTES
        {
            return Err("suite_archive_entry_denied");
        }
    }
    let root = products_root.join(&package.id);
    fs::create_dir(&root).map_err(|_| "suite_stage_exists")?;
    #[cfg(windows)]
    directories.extend(crate::suite::platform::component_scope::pin_directories(
        &root,
    )?);
    let (_root, root_identity) =
        open_filesystem_object(&root, true).map_err(|_| "suite_stage_unavailable")?;
    for i in 0..zip.len() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("suite_stage_cancelled");
        }
        let mut entry = zip.by_index(i).map_err(|_| "suite_archive_invalid")?;
        let expected = package
            .files
            .iter()
            .find(|f| f.name == entry.name())
            .ok_or("suite_archive_entry_denied")?;
        let target = root.join(&expected.name);
        let mut parent = root.clone();
        for part in Path::new(&expected.name)
            .parent()
            .ok_or("suite_archive_entry_denied")?
            .components()
        {
            parent.push(part);
            match fs::create_dir(&parent) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("suite_stage_write_failed"),
            }
            ensure_no_links(&parent).map_err(|_| "suite_stage_changed")?;
            #[cfg(windows)]
            directories.extend(crate::suite::platform::component_scope::pin_directories(
                &parent,
            )?);
        }
        // The suite's manifest lives at its verified root; an embedded portable
        // declaration must not shadow it. Still verify those archive bytes exactly.
        let mut output = if expected.name == "devbox-installation.json" {
            None
        } else {
            Some(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|_| "suite_stage_write_failed")?,
            )
        };
        let mut buffer = [0u8; 65536];
        let mut total = 0u64;
        let mut hasher = Sha256::new();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err("suite_stage_cancelled");
            }
            let count = entry
                .read(&mut buffer)
                .map_err(|_| "suite_archive_invalid")?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > expected.size {
                return Err("suite_archive_size_mismatch");
            }
            hasher.update(&buffer[..count]);
            if let Some(file) = &mut output {
                file.write_all(&buffer[..count])
                    .map_err(|_| "suite_stage_write_failed")?;
            }
        }
        let actual: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if total != expected.size || actual != expected.sha256 {
            return Err("suite_file_digest_mismatch");
        }
        if let Some(file) = output {
            file.sync_all().map_err(|_| "suite_stage_write_failed")?;
        }
        ensure_no_links(&root).map_err(|_| "suite_stage_changed")?;
        if filesystem_identity(&root, true).map_err(|_| "suite_stage_changed")? != root_identity
            || filesystem_identity(products_root, true).map_err(|_| "suite_stage_changed")?
                != parent_identity
            || filesystem_identity(archive, false).map_err(|_| "suite_archive_changed")?
                != archive_identity
        {
            return Err("suite_stage_changed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("devbox-stage-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn archive(root: &Path, name: &str) -> (std::path::PathBuf, ProductPackage) {
        let content = b"synthetic package file";
        let hash = |b: &[u8]| {
            Sha256::digest(b)
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
        };
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let path = root.join("fixture.zip");
        fs::write(&path, &bytes).unwrap();
        let package = ProductPackage {
            id: "knowledge".into(),
            version: "0.8.0".into(),
            portable: Asset {
                name: "fixture.zip".into(),
                sha256: hash(&bytes),
                size: bytes.len() as u64,
            },
            files: vec![Asset {
                name: name.into(),
                sha256: hash(content),
                size: content.len() as u64,
            }],
        };
        (path, package)
    }
    #[test]
    fn archive_paths_cannot_escape_even_when_the_declaration_repeats_them() {
        for name in [
            "../outside.exe",
            "resources/../../outside.exe",
            "C:/outside.exe",
            "devbox-knowledge.exe:stream",
        ] {
            let f = Fixture::new();
            let (archive, package) = archive(&f.0, name);
            let target = f.0.join("products");
            fs::create_dir(&target).unwrap();
            assert!(stage(&archive, &target, &package, &AtomicBool::new(false)).is_err());
            assert!(!target.join("knowledge").exists());
            assert!(!f.0.join("outside.exe").exists());
        }
    }
    #[test]
    fn verified_stage_never_replaces_existing_product_or_accepts_changed_archive() {
        let f = Fixture::new();
        let (archive, package) = archive(&f.0, "devbox-knowledge.exe");
        let target = f.0.join("products");
        fs::create_dir(&target).unwrap();
        stage(&archive, &target, &package, &AtomicBool::new(false)).unwrap();
        assert!(stage(&archive, &target, &package, &AtomicBool::new(false)).is_err());
        assert_eq!(
            fs::read(target.join("knowledge/devbox-knowledge.exe")).unwrap(),
            b"synthetic package file"
        );
        let other = f.0.join("other");
        fs::create_dir(&other).unwrap();
        fs::write(&archive, b"changed archive").unwrap();
        assert!(stage(&archive, &other, &package, &AtomicBool::new(false)).is_err());
        assert!(!other.join("knowledge").exists());
    }
    #[test]
    fn cancellation_preserves_the_previous_generation_without_creating_a_stage() {
        let f = Fixture::new();
        let (archive, package) = archive(&f.0, "devbox-knowledge.exe");
        let target = f.0.join("products");
        fs::create_dir(&target).unwrap();
        assert_eq!(
            stage(&archive, &target, &package, &AtomicBool::new(true)),
            Err("suite_stage_cancelled")
        );
        assert!(!target.join("knowledge").exists());
    }
}
