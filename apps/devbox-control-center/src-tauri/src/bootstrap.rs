//! Minimal package staging entrypoint. No Tauri app, service, registry mutation,
//! active package replacement or user-data migration starts from this operation.
use crate::core::{
    package_stage,
    suite_package::{Asset, Payload, MAX_RELEASE_BYTES},
};
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
type Result<T> = std::result::Result<T, &'static str>;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    ensure_no_links(path).map_err(|_| "bootstrap_input_unsafe")?;
    let (mut file, identity) =
        open_filesystem_object(path, false).map_err(|_| "bootstrap_input_unavailable")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "bootstrap_input_unavailable")?;
    if bytes.len() as u64 > limit
        || filesystem_identity(path, false).map_err(|_| "bootstrap_input_changed")? != identity
    {
        return Err("bootstrap_input_changed");
    }
    Ok(bytes)
}
fn verified_file(path: &Path, expected: &Asset) -> Result<()> {
    ensure_no_links(path).map_err(|_| "bootstrap_file_unsafe")?;
    let (mut file, identity) =
        open_filesystem_object(path, false).map_err(|_| "bootstrap_file_unavailable")?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|_| "bootstrap_file_unavailable")?;
        if size == 0 {
            break;
        }
        total += size as u64;
        if total > expected.size {
            return Err("bootstrap_file_changed");
        }
        hasher.update(&buffer[..size]);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if total != expected.size
        || digest != expected.sha256
        || filesystem_identity(path, false).map_err(|_| "bootstrap_file_changed")? != identity
    {
        return Err("bootstrap_file_changed");
    }
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageResult {
    pub state: &'static str,
    pub source_sha: String,
    pub suite_version: String,
    pub payload_revision: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Owner<'a> {
    schema_version: u32,
    root_identity: (u64, u64),
    payload_revision: &'a str,
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = devbox_filesystem::unlock_exclusive(&self.0);
    }
}
fn exact_closure(root: &Path, package: &crate::core::suite_package::ProductPackage) -> Result<()> {
    let expected = package
        .files
        .iter()
        .filter(|file| file.name != "devbox-installation.json")
        .map(|file| file.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut found = std::collections::BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    let mut count = 0;
    while let Some(directory) = pending.pop() {
        ensure_no_links(&directory).map_err(|_| "bootstrap_stage_unsafe")?;
        for entry in fs::read_dir(&directory).map_err(|_| "bootstrap_stage_unavailable")? {
            let entry = entry.map_err(|_| "bootstrap_stage_unavailable")?;
            count += 1;
            if count > 16 {
                return Err("bootstrap_stage_unrecognized");
            }
            let path = entry.path();
            ensure_no_links(&path).map_err(|_| "bootstrap_stage_unsafe")?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "bootstrap_stage_unsafe")?
                .to_str()
                .ok_or("bootstrap_stage_unsafe")?
                .replace('\\', "/");
            let kind = entry
                .file_type()
                .map_err(|_| "bootstrap_stage_unavailable")?;
            if kind.is_dir()
                && expected
                    .iter()
                    .any(|file| file.starts_with(&(relative.clone() + "/")))
            {
                pending.push(path);
            } else if kind.is_file() && expected.contains(relative.as_str()) {
                found.insert(relative);
            } else {
                return Err("bootstrap_stage_unrecognized");
            }
        }
    }
    if expected.len() != found.len() {
        return Err("bootstrap_stage_incomplete");
    }
    Ok(())
}

fn stage_impl(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    let revision = hash(&bytes);
    let helper = payload
        .products
        .iter()
        .find(|p| p.id == "control-center")
        .and_then(|p| {
            p.files
                .iter()
                .find(|f| f.name == "resources/suite/devbox-suite-bootstrap.exe")
        })
        .ok_or("bootstrap_identity_missing")?;
    verified_file(own_image, helper)?;
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let owner = serde_json::to_vec(&Owner {
        schema_version: 1,
        root_identity: identity.components(),
        payload_revision: &revision,
    })
    .map_err(|_| "bootstrap_owner_invalid")?;
    let owner_path = root.join("stage-owner.json");
    if owner_path.exists() {
        if read(&owner_path, 4096)? != owner {
            return Err("bootstrap_owner_changed");
        }
    } else {
        if fs::read_dir(&root)
            .map_err(|_| "bootstrap_root_unavailable")?
            .next()
            .is_some()
        {
            return Err("bootstrap_root_not_empty");
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&owner_path)
            .map_err(|_| "bootstrap_owner_unavailable")?;
        file.write_all(&owner)
            .and_then(|_| file.sync_all())
            .map_err(|_| "bootstrap_owner_unavailable")?;
    }
    let lock_path = root.join("stage.lock");
    ensure_no_links(&root).map_err(|_| "bootstrap_root_changed")?;
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_no_links(&lock_path).map_err(|_| "bootstrap_lock_unsafe")?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(|_| "bootstrap_busy")?
        }
        Err(_) => return Err("bootstrap_busy"),
    };
    if !devbox_filesystem::try_lock_exclusive(&lock).map_err(|_| "bootstrap_busy")? {
        return Err("bootstrap_busy");
    }
    let _lock = Lock(lock);
    let products = root.join("products");
    match fs::create_dir(&products) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("bootstrap_stage_unavailable"),
    }
    ensure_no_links(&products).map_err(|_| "bootstrap_stage_unsafe")?;
    let source = payload_path.parent().ok_or("bootstrap_input_unavailable")?;
    for package in &payload.products {
        let product = products.join(&package.id);
        if product.exists() {
            #[cfg(windows)]
            let _product_directories =
                crate::suite::platform::component_scope::pin_directories(&product)?;
            exact_closure(&product, package)?;
            // A crash after the last verified file can be resumed without rewriting it.
            // Partial/unrecognized content remains preserved for reviewed recovery.
            for file in package
                .files
                .iter()
                .filter(|f| f.name != "devbox-installation.json")
            {
                verified_file(&product.join(&file.name), file)
                    .map_err(|_| "bootstrap_stage_recovery_required")?;
            }
            if product.join("devbox-installation.json").exists() {
                return Err("bootstrap_stage_recovery_required");
            }
        } else {
            package_stage::stage(
                &source.join(&package.portable.name),
                &products,
                package,
                &AtomicBool::new(false),
            )?;
        }
    }
    if filesystem_identity(&root, true).map_err(|_| "bootstrap_root_changed")? != identity
        || read(&owner_path, 4096)? != owner
    {
        return Err("bootstrap_root_changed");
    }
    let result = StageResult {
        state: "staged",
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    };
    devbox_filesystem::atomic_write(
        root.join("stage-receipt.json"),
        &serde_json::to_vec(&result).map_err(|_| "bootstrap_receipt_invalid")?,
    )
    .map_err(|_| "bootstrap_receipt_unavailable")?;
    Ok(result)
}
/// The installer/update owner passes its newly prepared generation directory.
/// An existing generation is usable only with the exact retained stage receipt.
pub fn run(arguments: Vec<std::ffi::OsString>) -> Result<StageResult> {
    if !cfg!(windows) {
        return Err("bootstrap_windows_required");
    }
    if arguments.len() != 3 || arguments[0] != "--stage" {
        return Err("bootstrap_arguments_invalid");
    }
    let root = PathBuf::from(&arguments[1]);
    let payload = PathBuf::from(&arguments[2]);
    stage_impl(
        &root,
        &payload,
        &std::env::current_exe().map_err(|_| "bootstrap_identity_unavailable")?,
    )
}
