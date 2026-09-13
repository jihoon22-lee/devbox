//! Generation replacement preserves a verified physical installation's data
//! namespace. Portable/direct development binaries retain their location key.
use product_contract::installation::{Manifest, MAX_MANIFEST_BYTES};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
type Result<T> = std::result::Result<T, &'static str>;
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn read(path: &Path, limit: u64) -> Result<(Vec<u8>, File)> {
    devbox_filesystem::ensure_no_links(path).map_err(|_| "installation_path_unsafe")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "installation_file_unavailable")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "installation_file_unavailable")?;
    if bytes.len() as u64 > limit
        || devbox_filesystem::filesystem_identity(path, false)
            .map_err(|_| "installation_file_changed")?
            != identity
    {
        return Err("installation_file_changed");
    }
    Ok((bytes, file))
}
pub(crate) fn namespace(executable: &Path, product: &str, version: &str) -> Result<String> {
    let product_dir = executable.parent().ok_or("installation_path_invalid")?;
    let Some(products) = product_dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "products"))
    else {
        return Ok(digest(executable.to_string_lossy().as_bytes()));
    };
    let generation = products.parent().ok_or("installation_path_invalid")?;
    let Some(generations) = generation
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "generations"))
    else {
        return Ok(digest(executable.to_string_lossy().as_bytes()));
    };
    let root = generations.parent().ok_or("installation_path_invalid")?;
    devbox_filesystem::ensure_no_links(root).map_err(|_| "installation_path_unsafe")?;
    let (_root, identity) = devbox_filesystem::open_filesystem_object(root, true)
        .map_err(|_| "installation_root_unavailable")?;
    let (bytes, _manifest) = read(
        &root.join("devbox-installation.json"),
        MAX_MANIFEST_BYTES as u64,
    )?;
    let manifest = Manifest::parse(&bytes, version)?;
    let member = manifest
        .members
        .iter()
        .find(|m| m.product == product)
        .ok_or("installation_member_missing")?;
    let expected = format!(
        "generations/{}/products/{}/devbox-{}.exe",
        manifest.generation, product, product
    );
    if member.executable != expected || root.join(&expected) != executable {
        return Err("installation_generation_mismatch");
    }
    devbox_filesystem::ensure_no_links(executable).map_err(|_| "installation_path_unsafe")?;
    let (mut image, image_identity) = devbox_filesystem::open_filesystem_object(executable, false)
        .map_err(|_| "installation_file_unavailable")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut total = 0_u64;
    loop {
        let count = image
            .read(&mut buffer)
            .map_err(|_| "installation_file_unavailable")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 512 * 1024 * 1024 {
            return Err("installation_file_limit");
        }
        hasher.update(&buffer[..count]);
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != member.sha256
        || devbox_filesystem::filesystem_identity(executable, false)
            .map_err(|_| "installation_file_changed")?
            != image_identity
    {
        return Err("installation_digest_mismatch");
    }
    if devbox_filesystem::filesystem_identity(root, true)
        .map_err(|_| "installation_root_changed")?
        != identity
    {
        return Err("installation_root_changed");
    }
    // Matches the native component scope's installation key; product identifiers
    // separately isolate data. Manifest IDs alone cannot select another root.
    Ok(digest(
        &serde_json::to_vec(&(identity.components(), &manifest.installation_id))
            .map_err(|_| "installation_identity_invalid")?,
    ))
}

/// Held by every product UI and its dedicated browser/service worker through
/// shutdown. The updater's exclusive lease prevents a late writer from starting.
#[must_use = "the writer lease must be retained through product shutdown"]
pub struct WriterGuard(Option<File>);
impl WriterGuard {
    pub(crate) fn acquire(executable: &Path) -> Result<Self> {
        let parent = executable.parent().ok_or("installation_path_invalid")?;
        let Some(products) = parent
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "products"))
        else {
            return Ok(Self(None));
        };
        let generation = products.parent().ok_or("installation_path_invalid")?;
        let Some(generations) = generation
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "generations"))
        else {
            return Ok(Self(None));
        };
        let root = generations.parent().ok_or("installation_path_invalid")?;
        let path = root.join("suite-writers.lock");
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "suite_writer_gate_unavailable")?;
        #[cfg(windows)]
        let (file, identity) = {
            use std::os::windows::fs::OpenOptionsExt;
            // A live writer also prevents deletion/replacement of the lock file.
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0020_0000)
                .open(&path)
                .map_err(|_| "suite_writer_gate_unavailable")?;
            let identity = devbox_filesystem::opened_filesystem_identity(&file, false)
                .map_err(|_| "suite_writer_gate_unavailable")?;
            (file, identity)
        };
        #[cfg(not(windows))]
        let (file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
            .map_err(|_| "suite_writer_gate_unavailable")?;
        if !devbox_filesystem::try_lock_shared(&file)
            .map_err(|_| "suite_writer_gate_unavailable")?
        {
            return Err("suite_update_in_progress");
        }
        if devbox_filesystem::filesystem_identity(&path, false)
            .map_err(|_| "suite_writer_gate_changed")?
            != identity
        {
            return Err("suite_writer_gate_changed");
        }
        Ok(Self(Some(file)))
    }
}
impl Drop for WriterGuard {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = devbox_filesystem::unlock_exclusive(file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("devbox-namespace-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn generation(&self, id: &str) -> std::path::PathBuf {
            let relative = format!("generations/{id}/products/workspace/devbox-workspace.exe");
            let path = self.0.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"synthetic package bytes").unwrap();
            let manifest = Manifest {
                schema_version: 1,
                installation_id: "fixture".into(),
                generation: id.into(),
                suite_version: "0.8.0".into(),
                protocol_version: 1,
                members: vec![product_contract::installation::Member {
                    product: "workspace".into(),
                    executable: relative,
                    sha256: digest(b"synthetic package bytes"),
                }],
            };
            fs::write(
                self.0.join("devbox-installation.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn package_replacement_keeps_data_key_and_copied_installation_id_does_not_alias() {
        let root = Fixture::new();
        let first = root.generation("one");
        let old = namespace(&first, "workspace", "0.8.0").unwrap();
        let second = root.generation("two");
        assert_eq!(namespace(&second, "workspace", "0.8.0").unwrap(), old);
        assert!(namespace(&first, "workspace", "0.8.0").is_err());
        let foreign = Fixture::new();
        assert_ne!(
            namespace(&foreign.generation("two"), "workspace", "0.8.0").unwrap(),
            old
        );
        fs::write(&second, b"changed package").unwrap();
        assert!(namespace(&second, "workspace", "0.8.0").is_err());
    }
    #[test]
    fn all_products_and_workers_hold_shared_leases_until_update_can_exclude_new_writers() {
        let root = Fixture::new();
        let executable = root.generation("one");
        let gate = root.0.join("suite-writers.lock");
        fs::write(&gate, b"").unwrap();
        let first = WriterGuard::acquire(&executable).unwrap();
        let second = WriterGuard::acquire(&executable).unwrap();
        let update = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&gate)
            .unwrap();
        assert!(!devbox_filesystem::try_lock_exclusive(&update).unwrap());
        drop(first);
        assert!(!devbox_filesystem::try_lock_exclusive(&update).unwrap());
        drop(second);
        assert!(devbox_filesystem::try_lock_exclusive(&update).unwrap());
        assert!(WriterGuard::acquire(&executable).is_err());
        devbox_filesystem::unlock_exclusive(&update).unwrap();
        assert!(WriterGuard::acquire(&executable).is_ok());
    }
    #[test]
    fn portable_key_is_unchanged_and_generation_without_manifest_is_rejected() {
        let root = Fixture::new();
        let direct = root.0.join("devbox-workspace.exe");
        fs::write(&direct, b"fixture").unwrap();
        assert_eq!(
            namespace(&direct, "workspace", "0.8.0").unwrap(),
            digest(direct.to_string_lossy().as_bytes())
        );
        let staged = root.generation("next");
        fs::remove_file(root.0.join("devbox-installation.json")).unwrap();
        assert!(namespace(&staged, "workspace", "0.8.0").is_err());
    }
}
