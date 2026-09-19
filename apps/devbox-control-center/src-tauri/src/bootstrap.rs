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
/// The installer/update owner passes its newly prepared generation directory.
/// An existing generation is usable only with the exact retained stage receipt.
pub fn run(arguments: Vec<std::ffi::OsString>) -> Result<StageResult> {
    if !cfg!(windows) {
        return Err("bootstrap_windows_required");
    }
    if arguments.len() != 3
        || ![
            "--stage",
            "--prepare-install",
            "--recover-install",
            "--restart-install",
            "--open-install",
            "--snapshot-install",
            "--verify-checkpoints",
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
    if arguments[0] == "--prepare-install" {
        prepare_install(&root, &payload, &image)
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
    } else if arguments[0] == "--open-install" {
        open_install(&root, &payload, &image)
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
        state: "migrationRequired",
        ..result
    })
}

fn writer_gate(root: &Path, create: bool) -> Result<Lock> {
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

/// Open only this verified installation's Control Center. The helper does not
/// route to a guessed executable, grant write authority, or start other owners.
#[cfg(windows)]
fn open_install(root: &Path, payload_path: &Path, own_image: &Path) -> Result<StageResult> {
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
            .find(|member| member.product == "control-center")
            .ok_or("bootstrap_payload_incomplete")?
            .executable,
    );
    let scope = crate::suite::platform::component_scope::CapturedScope::capture(
        &root,
        "control-center",
        &image,
        &payload.suite_version,
    )?;
    let (_, image, _) = scope.member("control-center")?;
    let mut command = std::process::Command::new(image);
    command
        .arg("--suite-setup")
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
        state: "controlCenterLaunched",
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision,
    })
}
#[cfg(not(windows))]
fn open_install(_: &Path, _: &Path, _: &Path) -> Result<StageResult> {
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
        || !journal.imports.is_empty()
        || !journal.backup.is_empty()
        || journal.owner_evidence.len() != 4
        || journal.owner_evidence.iter().any(|evidence| {
            !evidence.backups.is_empty()
                || !evidence.summary.setup_selected
                || evidence.summary.busy
                || evidence.summary.review_required
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
        let legacy = devbox_catalog::parse_catalog(include_str!("../../../catalog.json"))
            .map_err(|_| "bootstrap_catalog_invalid")?;
        for app in legacy.apps {
            match fs::symlink_metadata(parent.join(app.identifier)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err("bootstrap_source_cutover_required"),
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
    if commit {
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
        state: if commit {
            "cleanInstallationCommitted"
        } else {
            "nativeHealthRequired"
        },
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
