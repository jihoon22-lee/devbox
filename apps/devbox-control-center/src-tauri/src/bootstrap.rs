//! Verified suite staging, setup and recovery entrypoints.
//! Product launch resolves only a pinned member of the reviewed installation.
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
pub(crate) mod cutover;
mod data_restore;
#[cfg(windows)]
pub(crate) mod interactive;
#[cfg(windows)]
pub(crate) mod registration;
mod reinstall;
mod uninstall;
mod update;
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
fn require_space(path: &Path, bytes: u64) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{core::PCWSTR, Win32::Storage::FileSystem::GetDiskFreeSpaceExW};
        ensure_no_links(path).map_err(|_| "suite_space_unavailable")?;
        let path = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut available = 0u64;
        unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }
            .map_err(|_| "suite_space_unavailable")?;
        let needed = bytes
            .checked_add(128 * 1024 * 1024)
            .ok_or("suite_space_insufficient")?;
        if available < needed {
            return Err("suite_space_insufficient");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (path, bytes);
        Err("bootstrap_windows_required")
    }
}
fn package_space(payload: &Payload) -> Result<u64> {
    payload.products.iter().try_fold(0u64, |total, product| {
        product.files.iter().try_fold(
            total
                .checked_add(product.portable.size)
                .ok_or("suite_space_insufficient")?,
            |total, file| {
                total
                    .checked_add(file.size)
                    .ok_or("suite_space_insufficient")
            },
        )
    })
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<crate::core::data_checkpoint::Receipt>,
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

fn verify_payload_owner(payload: &Payload, own_image: &Path) -> Result<()> {
    if payload.suite_version != env!("CARGO_PKG_VERSION") {
        return Err("bootstrap_version_mismatch");
    }
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
    verified_file(own_image, helper)
}

/// Retain verified setup inputs independently of NSIS's temporary directory.
/// Completed files are immutable; an interrupted copy never replaces another file.
fn retain_input(source: &Path, destination: &Path, expected: &Asset) -> Result<()> {
    let parent = destination.parent().ok_or("bootstrap_input_unsafe")?;
    ensure_no_links(parent).map_err(|_| "bootstrap_input_unsafe")?;
    match fs::symlink_metadata(destination) {
        Ok(_) => return verified_file(destination, expected),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("bootstrap_input_unavailable"),
    }
    ensure_no_links(source).map_err(|_| "bootstrap_input_unsafe")?;
    let temporary = parent.join(format!(".copy-{}", uuid::Uuid::new_v4()));
    let (mut input, source_identity) =
        open_filesystem_object(source, false).map_err(|_| "bootstrap_input_unavailable")?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "bootstrap_input_unavailable")?;
    let temporary_identity = devbox_filesystem::opened_filesystem_identity(&output, false)
        .map_err(|_| "bootstrap_input_unavailable")?;
    let copied = (|| {
        let started = std::time::Instant::now();
        let mut total = 0_u64;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 65536];
        loop {
            if started.elapsed() > std::time::Duration::from_secs(120) {
                return Err("bootstrap_input_copy_expired");
            }
            let size = input
                .read(&mut buffer)
                .map_err(|_| "bootstrap_input_unavailable")?;
            if size == 0 {
                break;
            }
            total = total
                .checked_add(size as u64)
                .ok_or("bootstrap_input_changed")?;
            if total > expected.size {
                return Err("bootstrap_input_changed");
            }
            hasher.update(&buffer[..size]);
            output
                .write_all(&buffer[..size])
                .map_err(|_| "bootstrap_input_copy_failed")?;
        }
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if total != expected.size
            || digest != expected.sha256
            || filesystem_identity(source, false).map_err(|_| "bootstrap_input_changed")?
                != source_identity
        {
            return Err("bootstrap_input_changed");
        }
        output
            .sync_all()
            .map_err(|_| "bootstrap_input_copy_failed")?;
        Ok(())
    })();
    drop(output);
    let result = copied.and_then(|()| {
        if filesystem_identity(&temporary, false).map_err(|_| "bootstrap_input_changed")?
            != temporary_identity
        {
            return Err("bootstrap_input_changed");
        }
        // Unlike rename on Windows, creating a hard link cannot overwrite a target
        // that appeared after the initial check. Both names are in this cache.
        match fs::hard_link(&temporary, destination) {
            Ok(()) => verified_file(destination, expected),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verified_file(destination, expected)
            }
            Err(_) => Err("bootstrap_input_copy_failed"),
        }
    });
    if filesystem_identity(&temporary, false).ok() == Some(temporary_identity) {
        // Only our new temporary name is removed; completed inputs remain retained.
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn retain_setup_inputs(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    payload: &Payload,
    bytes: &[u8],
) -> Result<PathBuf> {
    require_space(root, package_space(payload)?)?;
    let parent = root.join("setup");
    create_directory(&parent)?;
    let directory = parent.join(hash(bytes));
    create_directory(&directory)?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&directory)?;
    let (_directory, identity) =
        open_filesystem_object(&directory, true).map_err(|_| "bootstrap_input_unavailable")?;
    let source = payload_path.parent().ok_or("bootstrap_input_unsafe")?;
    for product in &payload.products {
        retain_input(
            &source.join(&product.portable.name),
            &directory.join(&product.portable.name),
            &product.portable,
        )?;
    }
    let helper = payload
        .products
        .iter()
        .find(|product| product.id == "control-center")
        .and_then(|product| {
            product
                .files
                .iter()
                .find(|file| file.name == "resources/suite/devbox-suite-bootstrap.exe")
        })
        .ok_or("bootstrap_identity_missing")?;
    retain_input(
        own_image,
        &directory.join("devbox-suite-bootstrap.exe"),
        helper,
    )?;
    let retained = directory.join("suite-payload.json");
    retain_input(
        payload_path,
        &retained,
        &Asset {
            name: "suite-payload.json".into(),
            sha256: hash(bytes),
            size: bytes.len() as u64,
        },
    )?;
    if filesystem_identity(&directory, true).map_err(|_| "bootstrap_input_changed")? != identity {
        return Err("bootstrap_input_changed");
    }
    Ok(retained)
}

fn stage_impl(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    let revision = hash(&bytes);
    verify_payload_owner(&payload, own_image)?;
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
            let complete = (|| {
                #[cfg(windows)]
                let _product_directories =
                    crate::suite::platform::component_scope::pin_directories(&product)?;
                exact_closure(&product, package)?;
                for file in package
                    .files
                    .iter()
                    .filter(|file| file.name != "devbox-installation.json")
                {
                    verified_file(&product.join(&file.name), file)?;
                }
                if product.join("devbox-installation.json").exists() {
                    return Err("bootstrap_stage_recovery_required");
                }
                Ok::<_, &'static str>(())
            })();
            if complete.is_ok() {
                continue;
            }
            // Only unfinished staging can preserve its partial tree and retry.
            // A completed generation's changed content is never repaired silently.
            if !matches!(fs::symlink_metadata(root.join("stage-receipt.json")), Err(error) if error.kind()==std::io::ErrorKind::NotFound)
            {
                return Err("bootstrap_stage_recovery_required");
            }
            preserve_incomplete_package(&root, &product, &package.id)?;
        }
        package_stage::stage(
            &source.join(&package.portable.name),
            &products,
            package,
            &AtomicBool::new(false),
        )?;
    }
    if filesystem_identity(&root, true).map_err(|_| "bootstrap_root_changed")? != identity
        || read(&owner_path, 4096)? != owner
    {
        return Err("bootstrap_root_changed");
    }
    let result = StageResult {
        operation_id: None,
        checkpoint: None,
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
fn preserve_incomplete_package(root: &Path, product: &Path, id: &str) -> Result<()> {
    if !product_contract::installation::PRODUCTS.contains(&id)
        || product != root.join("products").join(id)
    {
        return Err("bootstrap_stage_unsafe");
    }
    let preserved = root.join("interrupted");
    create_directory(&preserved)?;
    if fs::read_dir(&preserved)
        .map_err(|_| "bootstrap_stage_unavailable")?
        .take(32)
        .count()
        >= 32
    {
        return Err("bootstrap_stage_retention_review_required");
    }
    let identity = filesystem_identity(product, true)
        .map_err(|_| "bootstrap_stage_unsafe")?
        .components();
    let destination = preserved.join(format!("{id}.{}", uuid::Uuid::new_v4()));
    data_restore::move_directory(product, &destination, identity)
}

fn resume_or_update_install(root: &Path, payload_path: &Path, image: &Path) -> Result<StageResult> {
    use crate::core::{delivery::Phase, delivery_store::Store};
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    let identity = filesystem_identity(&root, true)
        .map_err(|_| "bootstrap_root_changed")?
        .components();
    if owner.root_identity != identity {
        return Err("bootstrap_owner_changed");
    }
    let revision = hash(&bytes);
    if owner.payload_revision == revision {
        let key = hash(
            &serde_json::to_vec(&(identity, &owner.installation_id))
                .map_err(|_| "bootstrap_owner_invalid")?,
        );
        let data = dirs::data_local_dir()
            .ok_or("bootstrap_data_unavailable")?
            .join(format!("com.devbox.v08.controlcenter.i{key}"));
        let observed = Store::inspect(&data)?;
        if observed.is_none() {
            return prepare_install(&root, payload_path, image);
        }
        if observed.as_ref().is_some_and(|(journal, _)| {
            journal.phase == Phase::Recovered
                && journal.previous.is_none()
                && owner.restart_from.as_ref() == Some(&journal.candidate)
        }) {
            return prepare_install(&root, payload_path, image);
        }
        if let Some((journal, _)) = observed.filter(|(journal, _)| {
            journal.candidate.installation_id == owner.installation_id
                && journal.candidate.generation == owner.generation
        }) {
            if journal.previous.is_none() && !journal.committed {
                if matches!(
                    journal.phase,
                    Phase::Inventory | Phase::Stage | Phase::Verify | Phase::Snapshot
                ) {
                    return prepare_install(&root, payload_path, image);
                }
                if journal.phase == Phase::Recovered {
                    return restart_install(&root, payload_path, image);
                }
                return Ok(StageResult {
                    state: "existingSetupReviewRequired",
                    operation_id: None,
                    checkpoint: None,
                    source_sha: payload.source_sha,
                    suite_version: payload.suite_version,
                    payload_revision: revision,
                });
            }
            if journal.committed && !root.join("suite-update.json").exists() {
                return Ok(StageResult {
                    state: "samePackageAlreadyInstalled",
                    operation_id: None,
                    checkpoint: None,
                    source_sha: payload.source_sha,
                    suite_version: payload.suite_version,
                    payload_revision: revision,
                });
            }
        }
    }
    update::install(&root, payload_path, image)
}
/// The installer/update owner passes its newly prepared generation directory.
/// An existing generation is usable only with the exact retained stage receipt.
pub fn run(arguments: Vec<std::ffi::OsString>) -> Result<StageResult> {
    if !cfg!(windows) {
        return Err("bootstrap_windows_required");
    }
    #[cfg(windows)]
    if arguments
        .first()
        .is_some_and(|mode| mode == "--reviewed-data-action")
    {
        return interactive::run(&arguments);
    }
    let restore = arguments.first().is_some_and(|mode| {
        [
            "--prepare-data-restore",
            "--apply-data-restore",
            "--commit-data-restore",
            "--rollback-data-restore",
        ]
        .iter()
        .any(|value| mode == *value)
    });
    let update_action = arguments.first().is_some_and(|mode| {
        ["--apply-update", "--commit-update", "--rollback-update"]
            .iter()
            .any(|value| mode == *value)
    });
    if arguments.len() != if restore || update_action { 4 } else { 3 }
        || ![
            "--stage",
            "--reinstall-install",
            "--commit-reinstall",
            "--prepare-update",
            "--prepare-and-apply-update",
            "--apply-update",
            "--commit-update",
            "--rollback-update",
            "--uninstall-install",
            "--register-install",
            "--resume-or-update-install",
            "--prepare-install",
            "--recover-install",
            "--restart-install",
            "--open-install",
            "--open-workspace",
            "--open-api-studio",
            "--open-knowledge",
            "--open-control-center",
            "--snapshot-install",
            "--verify-checkpoints",
            "--prepare-data-restore",
            "--apply-data-restore",
            "--commit-data-restore",
            "--rollback-data-restore",
            "--review-import-again",
            "--activate-reviewed-install",
            "--commit-reviewed-install",
            "--activate-clean-install",
            "--commit-clean-install",
        ]
        .iter()
        .any(|mode| arguments[0] == *mode)
    {
        return Err("bootstrap_arguments_invalid");
    }
    let root = PathBuf::from(&arguments[1]);
    let payload = PathBuf::from(&arguments[2]);
    let image = std::env::current_exe().map_err(|_| "bootstrap_identity_unavailable")?;
    if arguments[0] == "--reinstall-install" || arguments[0] == "--commit-reinstall" {
        reinstall::execute(
            &root,
            &payload,
            &image,
            arguments[0] == "--commit-reinstall",
        )
    } else if update_action {
        update::execute(
            &root,
            &payload,
            &image,
            arguments[3].to_str().ok_or("update_operation_invalid")?,
            arguments[0].to_str().ok_or("bootstrap_arguments_invalid")?,
        )
    } else if arguments[0] == "--prepare-update" || arguments[0] == "--prepare-and-apply-update" {
        if arguments[0] == "--prepare-update" {
            update::prepare(&root, &payload, &image)
        } else {
            update::install(&root, &payload, &image)
        }
    } else if restore {
        let checkpoint = arguments[3].to_str().ok_or("checkpoint_manifest_invalid")?;
        if arguments[0] == "--prepare-data-restore" {
            prepare_data_restore(&root, &payload, &image, checkpoint)
        } else {
            data_restore::execute(
                &root,
                &payload,
                &image,
                checkpoint,
                arguments[0].to_str().ok_or("bootstrap_arguments_invalid")?,
            )
        }
    } else if arguments[0] == "--uninstall-install" {
        uninstall::remove(&root, &payload, &image)
    } else if arguments[0] == "--register-install" {
        #[cfg(windows)]
        {
            registration::register(&root, &payload, &image)
        }
        #[cfg(not(windows))]
        {
            Err("bootstrap_windows_required")
        }
    } else if arguments[0] == "--resume-or-update-install" {
        resume_or_update_install(&root, &payload, &image)
    } else if arguments[0] == "--prepare-install" {
        prepare_install(&root, &payload, &image)
    } else if arguments[0] == "--review-import-again" {
        cutover::return_to_import(&root, &payload, &image)
    } else if arguments[0] == "--activate-reviewed-install"
        || arguments[0] == "--commit-reviewed-install"
    {
        activate_install(
            &root,
            &payload,
            &image,
            arguments[0] == "--commit-reviewed-install",
            true,
        )
    } else if arguments[0] == "--activate-clean-install" || arguments[0] == "--commit-clean-install"
    {
        activate_clean_install(
            &root,
            &payload,
            &image,
            arguments[0] == "--commit-clean-install",
        )
    } else if arguments[0] == "--recover-install" {
        recover_install(&root, &payload, &image)
    } else if arguments[0] == "--restart-install" {
        restart_install(&root, &payload, &image)
    } else if arguments[0] == "--snapshot-install" || arguments[0] == "--verify-checkpoints" {
        snapshot_install(
            &root,
            &payload,
            &image,
            arguments[0] == "--verify-checkpoints",
        )
    } else if let Some(product) = setup_product(&arguments[0]) {
        open_install(&root, &payload, &image, product)
    } else {
        stage_impl(&root, &payload, &image)
    }
}

fn setup_product(mode: &std::ffi::OsStr) -> Option<&'static str> {
    match mode.to_str()? {
        "--open-install" | "--open-control-center" => Some("control-center"),
        "--open-workspace" => Some("workspace"),
        "--open-api-studio" => Some("api-studio"),
        "--open-knowledge" => Some("knowledge"),
        _ => None,
    }
}

#[derive(Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallOwner {
    schema_version: u32,
    root_identity: (u64, u64),
    installation_id: String,
    generation: String,
    operation_id: String,
    payload_revision: String,
    #[serde(default)]
    restart_from: Option<product_contract::installation::Manifest>,
}
fn create_directory(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("bootstrap_directory_unavailable"),
    }
    ensure_no_links(path).map_err(|_| "bootstrap_directory_unsafe")?;
    if !path.is_dir() {
        return Err("bootstrap_directory_unsafe");
    }
    Ok(())
}
fn prepare_install(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
    use crate::core::{
        delivery::{Journal, Phase, Proof},
        delivery_store::Store,
    };
    use product_contract::installation::{Manifest, Member};
    let payload_bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&payload_bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let payload_revision = hash(&payload_bytes);
    let mut created_root = None;
    // NSIS delegates even the first directory creation to this native policy.
    // In particular, it cannot create an unreviewed UNC/junction target first.
    if fs::symlink_metadata(root).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        let candidate = devbox_manager_lib::core::custom_root::preview_new_suite_directory(root)
            .map_err(|_| "bootstrap_root_unsafe")?;
        let parent = candidate.parent().ok_or("bootstrap_root_unsafe")?;
        #[cfg(windows)]
        let _parents = crate::suite::platform::component_scope::pin_directories(parent)?;
        let (_parent, identity) =
            open_filesystem_object(parent, true).map_err(|_| "bootstrap_root_unavailable")?;
        fs::create_dir(&candidate).map_err(|_| "bootstrap_root_unavailable")?;
        created_root = Some(
            open_filesystem_object(&candidate, true).map_err(|_| "bootstrap_root_unavailable")?,
        );
        if filesystem_identity(parent, true).map_err(|_| "bootstrap_root_changed")? != identity {
            return Err("bootstrap_root_changed");
        }
    }
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, root_identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    if created_root
        .as_ref()
        .is_some_and(|(_, identity)| *identity != root_identity)
    {
        return Err("bootstrap_root_changed");
    }
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let owner_path = root.join("suite-owner.json");
    let owner = if owner_path.exists() {
        let owner: InstallOwner = serde_json::from_slice(&read(&owner_path, 4096)?)
            .map_err(|_| "bootstrap_owner_invalid")?;
        if owner.schema_version != 1
            || owner.root_identity != root_identity.components()
            || owner.payload_revision != payload_revision
            || uuid::Uuid::parse_str(&owner.installation_id).is_err()
            || uuid::Uuid::parse_str(&owner.operation_id).is_err()
            || owner.generation != format!("g-{}", owner.operation_id)
        {
            return Err("bootstrap_owner_changed");
        }
        owner
    } else {
        if fs::read_dir(&root)
            .map_err(|_| "bootstrap_root_unavailable")?
            .next()
            .is_some()
        {
            return Err("bootstrap_root_not_empty");
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let owner = InstallOwner {
            schema_version: 1,
            root_identity: root_identity.components(),
            installation_id: uuid::Uuid::new_v4().to_string(),
            generation: format!("g-{operation_id}"),
            operation_id,
            payload_revision: payload_revision.clone(),
            restart_from: None,
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&owner_path)
            .map_err(|_| "bootstrap_owner_unavailable")?;
        file.write_all(&serde_json::to_vec(&owner).map_err(|_| "bootstrap_owner_invalid")?)
            .and_then(|_| file.sync_all())
            .map_err(|_| "bootstrap_owner_unavailable")?;
        owner
    };
    let _gate = writer_gate(&root, true)?;
    let retained_payload =
        retain_setup_inputs(&root, payload_path, own_image, &payload, &payload_bytes)?;
    let key = hash(
        &serde_json::to_vec(&(root_identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let data_parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    ensure_no_links(&data_parent).map_err(|_| "bootstrap_data_unsafe")?;
    let data = data_parent.join(format!("com.devbox.v08.controlcenter.i{key}"));
    create_directory(&data)?;
    let store = Store::open(&data)?;
    let candidate = Manifest {
        schema_version: 1,
        installation_id: owner.installation_id.clone(),
        generation: owner.generation.clone(),
        suite_version: payload.suite_version.clone(),
        protocol_version: 1,
        members: payload
            .products
            .iter()
            .map(|package| {
                let image = format!("devbox-{}.exe", package.id);
                let asset = package
                    .files
                    .iter()
                    .find(|asset| asset.name == image)
                    .ok_or("bootstrap_payload_incomplete")?;
                Ok(Member {
                    product: package.id.clone(),
                    executable: format!(
                        "generations/{}/products/{}/{}",
                        owner.generation, package.id, image
                    ),
                    sha256: asset.sha256.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    let (mut journal, mut digest) = match store.read()? {
        Some((old, digest))
            if old.phase == Phase::Recovered
                && old.previous.is_none()
                && owner.restart_from.as_ref() == Some(&old.candidate)
                && old.installation_key == key =>
        {
            let journal = Journal::begin(owner.operation_id.clone(), key, None, candidate.clone())?;
            let digest = store.begin(Some(&digest), &journal)?;
            (journal, digest)
        }
        Some((journal, digest)) => {
            if journal.operation_id != owner.operation_id
                || journal.candidate != candidate
                || journal.installation_key != key
            {
                return Err("bootstrap_journal_changed");
            }
            (journal, digest)
        }
        None => {
            let journal = Journal::begin(owner.operation_id.clone(), key, None, candidate.clone())?;
            let digest = store.begin(None, &journal)?;
            (journal, digest)
        }
    };
    let mut advance = |journal: &mut Journal| -> Result<()> {
        let proof = Proof {
            phase: journal.phase,
            generation: owner.generation.clone(),
            revision: payload_revision.clone(),
        };
        journal.advance(journal.revision, proof)?;
        digest = store.write(Some(&digest), journal)?;
        Ok(())
    };
    if journal.phase == Phase::Inventory {
        advance(&mut journal)?;
    }
    if !matches!(
        journal.phase,
        Phase::Stage | Phase::Verify | Phase::Snapshot
    ) {
        return Err("bootstrap_phase_requires_recovery");
    }
    let generations = root.join("generations");
    create_directory(&generations)?;
    let generation = generations.join(&owner.generation);
    create_directory(&generation)?;
    let result = stage_impl(&generation, &retained_payload, own_image)?;
    if journal.phase == Phase::Stage {
        advance(&mut journal)?;
    }
    if journal.phase == Phase::Verify {
        advance(&mut journal)?;
    }
    let manifest_path = root.join("devbox-installation.json");
    let manifest_bytes =
        serde_json::to_vec(&candidate).map_err(|_| "bootstrap_manifest_invalid")?;
    if manifest_path.exists() && read(&manifest_path, 64 * 1024)? != manifest_bytes {
        let previous = owner
            .restart_from
            .as_ref()
            .ok_or("bootstrap_existing_installation_conflict")?;
        let operation = previous
            .generation
            .strip_prefix("g-")
            .ok_or("bootstrap_owner_invalid")?;
        let archived = store.archived(operation)?;
        let marker: product_contract::activation::Activation =
            serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
                .map_err(|_| "bootstrap_marker_invalid")?;
        marker.validate(previous)?;
        if archived.phase != Phase::Recovered
            || archived.previous.is_some()
            || archived.candidate != *previous
            || archived.installation_key != journal.installation_key
            || marker.phase != product_contract::activation::Phase::Recover
            || marker.operation_id != operation
            || read(&manifest_path, 64 * 1024)?
                != serde_json::to_vec(previous).map_err(|_| "bootstrap_manifest_invalid")?
        {
            return Err("bootstrap_existing_installation_conflict");
        }
    }
    let marker = product_contract::activation::Activation {
        schema_version: 1,
        installation_id: owner.installation_id,
        generation: owner.generation,
        operation_id: owner.operation_id,
        revision: journal.revision,
        phase: product_contract::activation::Phase::Import,
    };
    marker.validate(&candidate)?;
    // Both records fail closed during an interrupted first install. No ordinary
    // product operation is allowed until a later reviewed commit owns the gate.
    devbox_filesystem::atomic_write(&manifest_path, &manifest_bytes)
        .map_err(|_| "bootstrap_manifest_write_failed")?;
    devbox_filesystem::atomic_write(
        root.join("devbox-activation.json"),
        &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
    )
    .map_err(|_| "bootstrap_marker_write_failed")?;
    devbox_filesystem::atomic_write(root.join("suite-payload.json"), &payload_bytes)
        .map_err(|_| "bootstrap_payload_write_failed")?;
    if filesystem_identity(&root, true).map_err(|_| "bootstrap_root_changed")? != root_identity {
        return Err("bootstrap_root_changed");
    }
    Ok(StageResult {
        operation_id: None,
        state: "migrationRequired",
        ..result
    })
}

fn writer_gate(root: &Path, create: bool) -> Result<Lock> {
    let guard = writer_gate_for_restore(root, create)?;
    if !matches!(fs::symlink_metadata(root.join("suite-update.json")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Err("bootstrap_update_pending");
    }
    if !matches!(fs::symlink_metadata(root.join("uninstall-plan.json")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Err("bootstrap_uninstall_pending");
    }
    match fs::symlink_metadata(root.join("suite-data-restore.json")) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(guard),
        _ => Err("bootstrap_data_restore_pending"),
    }
}
fn writer_gate_for_restore(root: &Path, create: bool) -> Result<Lock> {
    let path = root.join("suite-writers.lock");
    let open = |new: bool| {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(new);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(3).custom_flags(0x0020_0000);
        }
        options.open(&path)
    };
    let gate = if create {
        match open(true) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                ensure_no_links(&path).map_err(|_| "bootstrap_gate_unsafe")?;
                open(false).map_err(|_| "bootstrap_gate_unavailable")?
            }
            Err(_) => return Err("bootstrap_gate_unavailable"),
        }
    } else {
        ensure_no_links(&path).map_err(|_| "bootstrap_gate_unsafe")?;
        open(false).map_err(|_| "bootstrap_gate_unavailable")?
    };
    let identity = devbox_filesystem::opened_filesystem_identity(&gate, false)
        .map_err(|_| "bootstrap_gate_unsafe")?;
    if !devbox_filesystem::try_lock_exclusive(&gate).map_err(|_| "bootstrap_gate_unavailable")? {
        return Err("suite_writers_must_close");
    }
    let gate = Lock(gate);
    if filesystem_identity(path, false).map_err(|_| "bootstrap_gate_changed")? != identity {
        return Err("bootstrap_gate_changed");
    }
    Ok(gate)
}

/// Abort an uncommitted first installation without deleting imported data,
/// package bytes or legacy sources. This is not a postcommit downgrade path.
fn recover_install(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
    use crate::core::{
        delivery::{Phase, Proof},
        delivery_store::Store,
    };
    let payload_bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&payload_bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let payload_revision = hash(&payload_bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != payload_revision
        || uuid::Uuid::parse_str(&owner.installation_id).is_err()
        || uuid::Uuid::parse_str(&owner.operation_id).is_err()
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let data = dirs::data_local_dir()
        .ok_or("bootstrap_data_unavailable")?
        .join(format!("com.devbox.v08.controlcenter.i{key}"));
    let Some((observed, _)) = Store::inspect(&data)? else {
        return Err("bootstrap_journal_missing");
    };
    if observed.committed || observed.previous.is_some() {
        return Err("bootstrap_recovery_requires_data_plan");
    }
    let store = Store::open(&data)?;
    let (mut journal, mut digest) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal != observed
        || journal.operation_id != owner.operation_id
        || journal.installation_key != key
        || journal.candidate.generation != owner.generation
        || journal.candidate.installation_id != owner.installation_id
        || journal.candidate.suite_version != payload.suite_version
    {
        return Err("bootstrap_journal_changed");
    }
    let manifest_path = root.join("devbox-installation.json");
    if manifest_path.exists() {
        let manifest = product_contract::installation::Manifest::parse(
            &read(&manifest_path, 64 * 1024)?,
            &payload.suite_version,
        )?;
        if manifest != journal.candidate {
            return Err("bootstrap_existing_installation_conflict");
        }
    }
    let marker_path = root.join("devbox-activation.json");
    if marker_path.exists() {
        let marker: product_contract::activation::Activation =
            serde_json::from_slice(&read(&marker_path, 4096)?)
                .map_err(|_| "bootstrap_marker_invalid")?;
        marker.validate(&journal.candidate)?;
        if marker.operation_id != owner.operation_id
            || marker.phase == product_contract::activation::Phase::Committed
        {
            return Err("bootstrap_recovery_requires_data_plan");
        }
    }
    let marker = product_contract::activation::Activation {
        schema_version: 1,
        installation_id: owner.installation_id,
        generation: owner.generation,
        operation_id: owner.operation_id,
        revision: journal.revision,
        phase: product_contract::activation::Phase::Recover,
    };
    marker.validate(&journal.candidate)?;
    devbox_filesystem::atomic_write(
        marker_path,
        &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
    )
    .map_err(|_| "bootstrap_marker_write_failed")?;
    if journal.phase != Phase::Recovered {
        if journal.phase != Phase::Recover {
            journal.fail(journal.revision, "installation_cancelled")?;
            digest = store.write(Some(&digest), &journal)?;
        }
        journal.advance(
            journal.revision,
            Proof {
                phase: Phase::Recover,
                generation: journal.candidate.generation.clone(),
                revision: payload_revision.clone(),
            },
        )?;
        store.write(Some(&digest), &journal)?;
    }
    Ok(StageResult {
        operation_id: None,
        state: "uncommittedInstallRecovered",
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision,
    })
}

/// An explicit retry allocates a fresh package slot after recovery. Partial
/// package files and imported data from the abandoned attempt remain intact.
fn restart_install(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
    use crate::core::{delivery::Phase, delivery_store::Store};
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let gate = writer_gate(&root, false)?;
    let owner_path = root.join("suite-owner.json");
    let mut owner: InstallOwner =
        serde_json::from_slice(&read(&owner_path, 4096)?).map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != hash(&bytes)
        || uuid::Uuid::parse_str(&owner.installation_id).is_err()
        || uuid::Uuid::parse_str(&owner.operation_id).is_err()
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let data = dirs::data_local_dir()
        .ok_or("bootstrap_data_unavailable")?
        .join(format!("com.devbox.v08.controlcenter.i{key}"));
    if Store::inspect(&data)?.is_none() {
        return Err("bootstrap_journal_missing");
    }
    let store = Store::open(&data)?;
    let (journal, _) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.phase != Phase::Recovered
        || journal.previous.is_some()
        || journal.installation_key != key
        || journal.candidate.installation_id != owner.installation_id
    {
        return Err("bootstrap_restart_requires_recovery");
    }
    let marker: product_contract::activation::Activation =
        serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
            .map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&journal.candidate)?;
    if marker.phase != product_contract::activation::Phase::Recover
        || marker.operation_id != journal.operation_id
    {
        return Err("bootstrap_restart_requires_recovery");
    }
    if journal.operation_id == owner.operation_id {
        owner.operation_id = uuid::Uuid::new_v4().to_string();
        owner.generation = format!("g-{}", owner.operation_id);
        owner.restart_from = Some(journal.candidate);
        devbox_filesystem::atomic_write(
            owner_path,
            &serde_json::to_vec(&owner).map_err(|_| "bootstrap_owner_invalid")?,
        )
        .map_err(|_| "bootstrap_owner_unavailable")?;
    } else if owner.restart_from.as_ref() != Some(&journal.candidate) {
        return Err("bootstrap_journal_changed");
    }
    // A crash here resumes the retained restart intent with --prepare-install.
    // An old generation remains blocked by its Recover marker throughout.
    drop(store);
    drop(gate);
    prepare_install(&root, payload_path, own_image)
}

/// Open only the selected, verified member of this installation. Precommit
/// setup entrypoints do not grant ordinary product write authority.
#[cfg(windows)]
fn open_install(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    product: &str,
) -> Result<StageResult> {
    use std::os::windows::process::CommandExt;
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let payload_revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    if let Some(result) = update::dispatch(&root, &payload, &payload_revision, product)? {
        return Ok(result);
    }
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != payload_revision
    {
        return Err("bootstrap_owner_changed");
    }
    let manifest = product_contract::installation::Manifest::parse(
        &read(&root.join("devbox-installation.json"), 64 * 1024)?,
        &payload.suite_version,
    )?;
    if manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
        || manifest.members.len() != payload.products.len()
    {
        return Err("bootstrap_owner_changed");
    }
    for package in &payload.products {
        let name = format!("devbox-{}.exe", package.id);
        let asset = package
            .files
            .iter()
            .find(|file| file.name == name)
            .ok_or("bootstrap_payload_incomplete")?;
        if !manifest.members.iter().any(|member| {
            member.product == package.id
                && member.sha256 == asset.sha256
                && member.executable
                    == format!(
                        "generations/{}/products/{}/{name}",
                        owner.generation, package.id
                    )
        }) {
            return Err("bootstrap_manifest_invalid");
        }
    }
    let marker: product_contract::activation::Activation =
        serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
            .map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&manifest)?;
    if marker.operation_id != owner.operation_id {
        return Err("bootstrap_owner_changed");
    }
    let image = root.join(
        &manifest
            .members
            .iter()
            .find(|member| member.product == product)
            .ok_or("bootstrap_payload_incomplete")?
            .executable,
    );
    let scope = crate::suite::platform::component_scope::CapturedScope::capture(
        &root,
        product,
        &image,
        &payload.suite_version,
    )?;
    let (_, image, _) = scope.member(product)?;
    let mut command = std::process::Command::new(image);
    if marker.phase != product_contract::activation::Phase::Committed {
        // Every owner opens its own import/health surface while the native
        // activation barrier still blocks ordinary writes and scheduled work.
        command.arg("--suite-setup");
    }
    command
        .current_dir(image.parent().ok_or("bootstrap_manifest_invalid")?)
        .creation_flags(0x08000000)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("WEBVIEW2_")
        {
            command.env_remove(name);
        }
    }
    // All captured image/directory handles remain pinned through native spawn.
    scope.revalidate()?;
    // A Windows GUI process intentionally outlives setup. Null stdio prevents
    // it retaining NSIS's helper-output pipe after this process returns.
    #[allow(clippy::zombie_processes)]
    let _child = command.spawn().map_err(|_| "bootstrap_launch_failed")?;
    Ok(StageResult {
        operation_id: None,
        state: if product == "control-center" {
            "controlCenterLaunched"
        } else {
            "productLaunched"
        },
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision,
    })
}
#[cfg(not(windows))]
fn open_install(_: &Path, _: &Path, _: &Path, _: &str) -> Result<StageResult> {
    Err("bootstrap_windows_required")
}

/// Preserve only this installation's four closed namespaces. External vaults,
/// repository trees, legacy namespaces and activation markers are not changed.
fn snapshot_install(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    verify_existing: bool,
) -> Result<StageResult> {
    use crate::core::{data_checkpoint, delivery_store::Store};
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != revision
        || !uuid::Uuid::parse_str(&owner.installation_id)
            .is_ok_and(|id| id.to_string() == owner.installation_id)
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    let manifest = product_contract::installation::Manifest::parse(
        &read(&root.join("devbox-installation.json"), 64 * 1024)?,
        &payload.suite_version,
    )?;
    let marker: product_contract::activation::Activation =
        serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
            .map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&manifest)?;
    if manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
        || marker.operation_id != owner.operation_id
    {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "bootstrap_data_unsafe")?;
    #[cfg(windows)]
    let _data_directories = crate::suite::platform::component_scope::pin_directories(&parent)?;
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    let sources = catalog
        .products
        .iter()
        .map(|product| {
            (
                product.id.clone(),
                parent.join(format!("{}.i{key}", product.identifier)),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let store = Store::open(
        sources
            .get("control-center")
            .ok_or("bootstrap_data_unavailable")?,
    )?;
    let (mut journal, digest) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.installation_key != key
        || journal.operation_id != owner.operation_id
        || journal.candidate != manifest
    {
        return Err("bootstrap_journal_changed");
    }
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    if verify_existing {
        if journal.data_checkpoints.is_empty() {
            return Err("checkpoint_missing");
        }
        for checkpoint in &journal.data_checkpoints {
            data_checkpoint::verify(
                &backup,
                checkpoint,
                &key,
                &manifest.generation,
                &AtomicBool::new(false),
            )?;
        }
        return Ok(StageResult {
            operation_id: None,
            state: "dataCheckpointsVerified",
            checkpoint: None,
            source_sha: payload.source_sha,
            suite_version: payload.suite_version,
            payload_revision: revision,
        });
    }
    if journal.data_checkpoints.len() >= 32 {
        return Err("checkpoint_retention_review_required");
    }
    create_directory(&backup)?;
    require_space(&parent, data_checkpoint::required_space(&sources)?)?;
    let checkpoint = data_checkpoint::acquire_quiesced(
        &sources,
        &backup,
        &key,
        &manifest.generation,
        &AtomicBool::new(false),
    )?;
    // The helper still holds the writer gate here. Keep a completed checkpoint
    // even if journal persistence fails; never delete a possible recovery copy.
    journal.record_checkpoint(journal.revision, checkpoint.clone())?;
    store.write(Some(&digest), &journal)?;
    Ok(StageResult {
        operation_id: None,
        state: "dataCheckpointPreserved",
        checkpoint: Some(checkpoint),
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}

/// Clean first-install activation only. Any legacy data namespace or imported
/// backup keeps this path closed until a source-aware cutover plan is available.
fn activate_clean_install(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    commit: bool,
) -> Result<StageResult> {
    activate_install(root, payload_path, own_image, commit, false)
}
fn activate_install(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    commit: bool,
    reviewed: bool,
) -> Result<StageResult> {
    use crate::core::{
        data_checkpoint,
        delivery::{Phase, Proof},
        delivery_store::Store,
    };
    use product_contract::activation::{Activation, Phase as ActivePhase};
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != revision
        || !uuid::Uuid::parse_str(&owner.installation_id)
            .is_ok_and(|id| id.to_string() == owner.installation_id)
        || !uuid::Uuid::parse_str(&owner.operation_id)
            .is_ok_and(|id| id.to_string() == owner.operation_id)
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    let manifest = product_contract::installation::Manifest::parse(
        &read(&root.join("devbox-installation.json"), 64 * 1024)?,
        &payload.suite_version,
    )?;
    let marker_path = root.join("devbox-activation.json");
    let mut marker: Activation = serde_json::from_slice(&read(&marker_path, 4096)?)
        .map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&manifest)?;
    if manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
        || marker.operation_id != owner.operation_id
    {
        return Err("bootstrap_owner_changed");
    }
    for package in &payload.products {
        let product = root
            .join("generations")
            .join(&owner.generation)
            .join("products")
            .join(&package.id);
        exact_closure(&product, package)?;
        for file in package
            .files
            .iter()
            .filter(|file| file.name != "devbox-installation.json")
        {
            verified_file(&product.join(&file.name), file)?;
        }
        let executable = format!("devbox-{}.exe", package.id);
        let expected = package
            .files
            .iter()
            .find(|file| file.name == executable)
            .ok_or("bootstrap_payload_incomplete")?;
        if !manifest.members.iter().any(|member| {
            member.product == package.id
                && member.sha256 == expected.sha256
                && member.executable
                    == format!(
                        "generations/{}/products/{}/{executable}",
                        owner.generation, package.id
                    )
        }) {
            return Err("bootstrap_manifest_changed");
        }
    }
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "bootstrap_data_unsafe")?;
    #[cfg(windows)]
    let _data_directories = crate::suite::platform::component_scope::pin_directories(&parent)?;
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    let sources = catalog
        .products
        .iter()
        .map(|product| {
            (
                product.id.clone(),
                parent.join(format!("{}.i{key}", product.identifier)),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let store = Store::open(
        sources
            .get("control-center")
            .ok_or("bootstrap_data_unavailable")?,
    )?;
    let (mut journal, mut digest) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.candidate != manifest
        || journal.installation_key != key
        || journal.operation_id != owner.operation_id
        || journal.previous.is_some()
        || journal.failure.is_some()
        || journal.owner_evidence.len() != 4
        || journal.owner_evidence.iter().any(|evidence| {
            !evidence.summary.setup_selected
                || evidence.summary.busy
                || evidence.summary.review_required
        })
    {
        return Err("bootstrap_source_cutover_required");
    }
    let mut source_guard = if reviewed && !journal.committed {
        Some(cutover::hold(
            sources
                .get("control-center")
                .ok_or("bootstrap_data_unavailable")?,
            &journal,
        )?)
    } else {
        None
    };
    if !reviewed {
        if !journal.imports.is_empty()
            || !journal.backup.is_empty()
            || journal.owner_evidence.iter().any(|evidence| {
                !evidence.backups.is_empty()
                    || evidence
                        .summary
                        .mappings
                        .as_ref()
                        .is_some_and(|mapping| mapping.record_count != 0)
            })
        {
            return Err("bootstrap_source_cutover_required");
        }
        if !journal.committed {
            #[cfg(windows)]
            {
                let installed = crate::legacy_installer::inventory()?;
                if !installed.complete || !installed.entries.is_empty() {
                    return Err("bootstrap_legacy_registration_review_required");
                }
            }
            let legacy =
                devbox_catalog::parse_catalog(include_str!("../../../legacy-v0.7-catalog.json"))
                    .map_err(|_| "bootstrap_catalog_invalid")?;
            for app in legacy.apps {
                if !matches!(fs::symlink_metadata(parent.join(app.identifier)),Err(error) if error.kind() == std::io::ErrorKind::NotFound)
                {
                    return Err("bootstrap_source_cutover_required");
                }
            }
        }
    }
    if commit {
        if !matches!(
            journal.phase,
            Phase::Health | Phase::Commit | Phase::Cleanup | Phase::Complete
        ) || !matches!(marker.phase, ActivePhase::Health | ActivePhase::Committed)
        {
            return Err("bootstrap_health_required");
        }
        if !journal.committed {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "bootstrap_clock_invalid")?
                .as_millis() as u64;
            journal.require_recent_health(now)?;
        }
    } else if !matches!(
        journal.phase,
        Phase::Snapshot
            | Phase::Import
            | Phase::Validate
            | Phase::Quiesce
            | Phase::Activate
            | Phase::Health
    ) || !matches!(marker.phase, ActivePhase::Import | ActivePhase::Health)
    {
        return Err("bootstrap_phase_requires_recovery");
    }
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    // Capture immediately before each activation/commit boundary while every
    // product is closed. Existing complete snapshots are never overwritten.
    if journal.phase == Phase::Snapshot
        || (commit && matches!(journal.phase, Phase::Health | Phase::Commit))
    {
        if journal.data_checkpoints.len() >= 32 {
            return Err("checkpoint_retention_review_required");
        }
        create_directory(&backup)?;
        require_space(&parent, data_checkpoint::required_space(&sources)?)?;
        let receipt = data_checkpoint::acquire_quiesced(
            &sources,
            &backup,
            &key,
            &manifest.generation,
            &AtomicBool::new(false),
        )?;
        journal.record_checkpoint(journal.revision, receipt)?;
        digest = store.write(Some(&digest), &journal)?;
    }
    let checkpoint = journal
        .data_checkpoints
        .last()
        .ok_or("checkpoint_missing")?;
    data_checkpoint::verify(
        &backup,
        checkpoint,
        &key,
        &manifest.generation,
        &AtomicBool::new(false),
    )?;
    let proof_revision = checkpoint.revision.clone();
    let mut advance = |journal: &mut crate::core::delivery::Journal| -> Result<()> {
        journal.advance(
            journal.revision,
            Proof {
                phase: journal.phase,
                generation: manifest.generation.clone(),
                revision: proof_revision.clone(),
            },
        )?;
        digest = store.write(Some(&digest), journal)?;
        Ok(())
    };
    if let Some(guard) = &mut source_guard {
        guard.revalidate()?;
        cutover::require_closed()?;
    }
    if commit {
        if reviewed
            && !journal.committed
            && journal.health_checks.iter().any(|health| {
                !journal.owner_evidence.iter().any(|evidence| {
                    evidence.summary.owner == health.report.store.owner
                        && evidence.summary.mappings == health.report.store.mappings
                })
            })
        {
            return Err("cutover_destination_changed");
        }
        while matches!(journal.phase, Phase::Health | Phase::Commit) {
            advance(&mut journal)?;
        }
        // Durable commit intent precedes the marker. A crash between these
        // writes stays blocked and resumes without undoing committed data.
        marker.phase = ActivePhase::Committed;
    } else {
        while journal.phase != Phase::Health {
            advance(&mut journal)?;
        }
        marker.phase = ActivePhase::Health;
    }
    marker.revision = journal.revision;
    devbox_filesystem::atomic_write(
        &marker_path,
        &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
    )
    .map_err(|_| "bootstrap_marker_write_failed")?;
    if commit && journal.phase == Phase::Cleanup && journal.cleanup_pending.is_empty() {
        advance(&mut journal)?;
    }
    Ok(StageResult {
        operation_id: None,
        state: if commit {
            if reviewed {
                "reviewedInstallationCommitted"
            } else {
                "cleanInstallationCommitted"
            }
        } else {
            "nativeHealthRequired"
        },
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DataRestorePlan {
    schema_version: u32,
    operation_id: String,
    installation_key: String,
    root_identity: (u64, u64),
    generation: String,
    manifest_revision: String,
    activation_revision: String,
    original_activation: product_contract::activation::Activation,
    namespaces: std::collections::BTreeMap<String, crate::core::data_restore::Namespace>,
    health_journal: crate::core::delivery::Journal,
    source: crate::core::data_checkpoint::Receipt,
    preserved: crate::core::data_checkpoint::Receipt,
    prepared_ms: u64,
}

fn prepare_data_restore(
    root: &Path,
    payload_path: &Path,
    own_image: &Path,
    checkpoint: &str,
) -> Result<StageResult> {
    use crate::core::{data_checkpoint, delivery_store::Store};
    if !uuid::Uuid::parse_str(checkpoint).is_ok_and(|id| id.to_string() == checkpoint) {
        return Err("checkpoint_manifest_invalid");
    }
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, own_image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _directories = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity.components()
        || owner.payload_revision != revision
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    let manifest_bytes = read(&root.join("devbox-installation.json"), 64 * 1024)?;
    let manifest =
        product_contract::installation::Manifest::parse(&manifest_bytes, &payload.suite_version)?;
    let activation_bytes = read(&root.join("devbox-activation.json"), 4096)?;
    let marker: product_contract::activation::Activation =
        serde_json::from_slice(&activation_bytes).map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&manifest)?;
    if manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
        || marker.operation_id != owner.operation_id
    {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "bootstrap_data_unsafe")?;
    #[cfg(windows)]
    let _data_directories = crate::suite::platform::component_scope::pin_directories(&parent)?;
    let catalog =
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?;
    let sources = catalog
        .products
        .iter()
        .map(|product| {
            (
                product.id.clone(),
                parent.join(format!("{}.i{key}", product.identifier)),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let (journal, _) = Store::inspect(
        sources
            .get("control-center")
            .ok_or("bootstrap_data_unavailable")?,
    )?
    .ok_or("bootstrap_journal_missing")?;
    if journal.installation_key != key
        || journal.candidate != manifest
        || journal.operation_id != owner.operation_id
    {
        return Err("bootstrap_journal_changed");
    }
    let selected = journal
        .data_checkpoints
        .iter()
        .find(|receipt| receipt.id == checkpoint)
        .ok_or("checkpoint_missing")?
        .clone();
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    // Stage data beside live namespaces so later directory swaps stay on the
    // same volume even when the package installation uses a custom drive.
    let recovery = parent.join(format!("com.devbox.v08.suite-restore.i{key}"));
    create_directory(&recovery)?;
    if fs::read_dir(&recovery)
        .map_err(|_| "bootstrap_recovery_unavailable")?
        .take(32)
        .count()
        >= 32
    {
        return Err("bootstrap_restore_retention_review_required");
    }
    let operation_id = uuid::Uuid::new_v4().to_string();
    let operation = recovery.join(&operation_id);
    fs::create_dir(&operation).map_err(|_| "bootstrap_recovery_conflict")?;
    #[cfg(windows)]
    let _operation_directories =
        crate::suite::platform::component_scope::pin_directories(&operation)?;
    let prepared = operation.join("prepared");
    create_directory(&prepared)?;
    require_space(
        &parent,
        data_checkpoint::required_space(&sources)?
            .checked_add(selected.bytes)
            .ok_or("suite_space_insufficient")?,
    )?;
    data_checkpoint::materialize(
        &backup,
        &selected,
        &key,
        &manifest.generation,
        &prepared.join(&selected.id),
        &AtomicBool::new(false),
    )?;
    // This receipt belongs to the external recovery plan. Do not write it into
    // the live Control Center journal and invalidate the very preimage captured.
    let preserved = data_checkpoint::acquire_quiesced(
        &sources,
        &backup,
        &key,
        &manifest.generation,
        &AtomicBool::new(false),
    )?;
    let namespaces = sources
        .iter()
        .map(|(owner, path)| {
            Ok((
                owner.clone(),
                crate::core::data_restore::Namespace {
                    original: data_restore::directory_identity(path)?,
                    prepared: data_restore::directory_identity(
                        &prepared.join(&selected.id).join(owner),
                    )?,
                },
            ))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let (mut health_journal, _) =
        Store::inspect(&prepared.join(&selected.id).join("control-center"))?
            .ok_or("bootstrap_restore_journal_missing")?;
    if health_journal.installation_key != key
        || health_journal.candidate != manifest
        || health_journal.operation_id != owner.operation_id
    {
        return Err("bootstrap_restore_journal_changed");
    }
    health_journal.phase = crate::core::delivery::Phase::Health;
    health_journal.committed = false;
    health_journal.failure = None;
    health_journal.health_checks.clear();
    health_journal.revision = health_journal
        .revision
        .checked_add(1)
        .ok_or("bootstrap_revision_exhausted")?;
    for receipt in [&selected, &preserved] {
        if !health_journal
            .data_checkpoints
            .iter()
            .any(|old| old.id == receipt.id)
        {
            health_journal.data_checkpoints.push(receipt.clone());
        }
    }
    health_journal.validate()?;
    let plan = DataRestorePlan {
        schema_version: 1,
        operation_id: operation_id.clone(),
        installation_key: key,
        root_identity: identity.components(),
        generation: manifest.generation,
        manifest_revision: hash(&manifest_bytes),
        activation_revision: hash(&activation_bytes),
        original_activation: marker,
        namespaces,
        health_journal,
        source: selected.clone(),
        preserved,
        prepared_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "bootstrap_clock_invalid")?
            .as_millis() as u64,
    };
    let mut record = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(operation.join("restore-plan.json"))
        .map_err(|_| "bootstrap_restore_plan_failed")?;
    record
        .write_all(&serde_json::to_vec(&plan).map_err(|_| "bootstrap_restore_plan_failed")?)
        .and_then(|()| record.sync_all())
        .map_err(|_| "bootstrap_restore_plan_failed")?;
    Ok(StageResult {
        operation_id: Some(operation_id),
        state: "dataRestorePrepared",
        checkpoint: Some(selected),
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}

#[cfg(test)]
mod setup_retention_tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> std::io::Result<Temp> {
        let path =
            std::env::temp_dir().join(format!("devbox-setup-input-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Temp(path))
    }
    fn asset(bytes: &[u8]) -> Asset {
        Asset {
            name: "fixture.zip".into(),
            sha256: hash(bytes),
            size: bytes.len() as u64,
        }
    }
    #[cfg(windows)]
    #[test]
    fn insufficient_space_is_refused_without_creating_a_stage_or_mutating_data() {
        let root = tempdir().unwrap();
        fs::write(root.path().join("user.json"), b"unchanged").unwrap();
        assert_eq!(
            require_space(root.path(), u64::MAX - 128 * 1024 * 1024),
            Err("suite_space_insufficient")
        );
        assert_eq!(
            fs::read(root.path().join("user.json")).unwrap(),
            b"unchanged"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn interrupted_package_tree_is_preserved_without_replacing_unknown_files() {
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join("products")).unwrap();
        let package = root.path().join("products/workspace");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("devbox-workspace.exe"), b"incomplete bytes").unwrap();
        fs::write(package.join("user-file.txt"), b"preserve unknown").unwrap();
        preserve_incomplete_package(root.path(), &package, "workspace").unwrap();
        assert!(!package.exists());
        let archived = fs::read_dir(root.path().join("interrupted"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read(archived.join("devbox-workspace.exe")).unwrap(),
            b"incomplete bytes"
        );
        assert_eq!(
            fs::read(archived.join("user-file.txt")).unwrap(),
            b"preserve unknown"
        );
        assert!(preserve_incomplete_package(root.path(), root.path(), "workspace").is_err());
    }
    #[test]
    fn retained_inputs_survive_installer_cleanup_and_repeated_preparation() {
        let original = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source = original.path().join("fixture.zip");
        let destination = cache.path().join("fixture.zip");
        fs::write(&source, b"verified input").unwrap();
        let expected = asset(b"verified input");
        retain_input(&source, &destination, &expected).unwrap();
        retain_input(&source, &destination, &expected).unwrap();
        drop(original);
        verified_file(&destination, &expected).unwrap();
    }
    #[test]
    fn corrupt_input_is_not_published_and_existing_content_is_not_overwritten() {
        let original = tempdir().unwrap();
        let cache = tempdir().unwrap();
        let source = original.path().join("fixture.zip");
        let destination = cache.path().join("fixture.zip");
        let expected = asset(b"verified input");
        fs::write(&source, b"corrupt input").unwrap();
        assert!(retain_input(&source, &destination, &expected).is_err());
        assert!(!destination.exists());
        fs::write(&source, b"verified input").unwrap();
        fs::write(&destination, b"preserve existing file").unwrap();
        assert!(retain_input(&source, &destination, &expected).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"preserve existing file");
    }
}
