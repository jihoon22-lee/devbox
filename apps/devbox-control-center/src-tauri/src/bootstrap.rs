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
    if arguments.len() != 3
        || !["--stage", "--prepare-install"]
            .iter()
            .any(|mode| arguments[0] == *mode)
    {
        return Err("bootstrap_arguments_invalid");
    }
    let root = PathBuf::from(&arguments[1]);
    let payload = PathBuf::from(&arguments[2]);
    let image = std::env::current_exe().map_err(|_| "bootstrap_identity_unavailable")?;
    if arguments[0] == "--prepare-install" {
        prepare_install(&root, &payload, &image)
    } else {
        stage_impl(&root, &payload, &image)
    }
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallOwner {
    schema_version: u32,
    root_identity: (u64, u64),
    installation_id: String,
    generation: String,
    operation_id: String,
    payload_revision: String,
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
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, root_identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
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
    let gate_path = root.join("suite-writers.lock");
    let gate = match OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&gate_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_no_links(&gate_path).map_err(|_| "bootstrap_gate_unsafe")?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&gate_path)
                .map_err(|_| "bootstrap_gate_unavailable")?
        }
        Err(_) => return Err("bootstrap_gate_unavailable"),
    };
    if !devbox_filesystem::try_lock_exclusive(&gate).map_err(|_| "bootstrap_gate_unavailable")? {
        return Err("suite_writers_must_close");
    }
    let _gate = Lock(gate);
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
            let digest = store.write(None, &journal)?;
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
    let result = stage_impl(&generation, payload_path, own_image)?;
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
        return Err("bootstrap_existing_installation_conflict");
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
        state: "migrationRequired",
        ..result
    })
}
