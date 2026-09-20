//! Closed generation updates preserve both package versions and the original
//! physical data directories. Only the helper owns the multi-record switch.
use super::data_restore::{claim, directory_identity, move_directory, persist, release};
use super::*;
use crate::core::{
    data_checkpoint,
    data_restore::{Namespace, Step},
    delivery::{Journal, Phase, Proof},
    delivery_store::Store,
};
use product_contract::{
    activation::{Activation, Phase as ActivePhase},
    installation::{Manifest, Member},
};
use std::collections::BTreeMap;

#[derive(Clone, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Records {
    owner: InstallOwner,
    manifest: Manifest,
    activation: Activation,
    payload: Vec<u8>,
}

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Plan {
    schema_version: u32,
    id: String,
    key: String,
    root_identity: (u64, u64),
    original: Records,
    candidate: Records,
    checkpoint: data_checkpoint::Receipt,
    namespaces: BTreeMap<String, Namespace>,
}
#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum State {
    Applying,
    Health,
    Committing,
    Committed,
    RollingBack,
    RolledBack,
}
#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Progress {
    revision: String,
    state: State,
}

fn sources(parent: &Path, key: &str) -> Result<BTreeMap<String, PathBuf>> {
    Ok(
        devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE)?
            .products
            .into_iter()
            .map(|product| {
                (
                    product.id,
                    parent.join(format!("{}.i{key}", product.identifier)),
                )
            })
            .collect(),
    )
}
fn records(root: &Path) -> Result<Records> {
    let payload = read(&root.join("suite-payload.json"), MAX_RELEASE_BYTES as u64)?;
    let package = Payload::parse(&payload)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "update_owner_invalid")?;
    let manifest = Manifest::parse(
        &read(&root.join("devbox-installation.json"), 64 * 1024)?,
        &package.suite_version,
    )?;
    let activation: Activation =
        serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
            .map_err(|_| "update_activation_invalid")?;
    activation.validate(&manifest)?;
    if owner.schema_version != 1
        || owner.root_identity
            != filesystem_identity(root, true)
                .map_err(|_| "update_root_changed")?
                .components()
        || owner.payload_revision != hash(&payload)
        || owner.installation_id != manifest.installation_id
        || owner.generation != manifest.generation
        || owner.generation != format!("g-{}", owner.operation_id)
        || activation.operation_id != owner.operation_id
    {
        return Err("update_owner_changed");
    }
    Ok(Records {
        owner,
        manifest,
        activation,
        payload,
    })
}
fn verify_packages(root: &Path, records: &Records) -> Result<()> {
    let payload = Payload::parse(&records.payload)?;
    for package in &payload.products {
        let directory = root
            .join("generations")
            .join(&records.owner.generation)
            .join("products")
            .join(&package.id);
        exact_closure(&directory, package)?;
        for asset in package
            .files
            .iter()
            .filter(|asset| asset.name != "devbox-installation.json")
        {
            verified_file(&directory.join(&asset.name), asset)?;
        }
        let name = format!("devbox-{}.exe", package.id);
        let asset = package
            .files
            .iter()
            .find(|asset| asset.name == name)
            .ok_or("bootstrap_payload_incomplete")?;
        if !records.manifest.members.iter().any(|member| {
            member.product == package.id
                && member.sha256 == asset.sha256
                && member.executable
                    == format!(
                        "generations/{}/products/{}/{name}",
                        records.owner.generation, package.id
                    )
        }) {
            return Err("update_manifest_changed");
        }
    }
    Ok(())
}
fn marker(records: &Records, phase: ActivePhase, epoch: u64) -> Result<Activation> {
    let mut marker = records.activation.clone();
    marker.phase = phase;
    marker.revision = epoch;
    marker.validate(&records.manifest)?;
    Ok(marker)
}
fn same_json(actual: &[u8], expected: &impl Serialize) -> Result<bool> {
    Ok(
        serde_json::from_slice::<serde_json::Value>(actual).map_err(|_| "update_record_invalid")?
            == serde_json::to_value(expected).map_err(|_| "update_record_invalid")?,
    )
}
fn check_records(root: &Path, plan: &Plan, state: Option<State>) -> Result<()> {
    let original = &plan.original;
    let candidate = &plan.candidate;
    let original_epoch = original.activation.revision;
    let health = marker(
        candidate,
        ActivePhase::Health,
        original_epoch
            .checked_add(2)
            .ok_or("update_revision_exhausted")?,
    )?;
    let committed = marker(
        candidate,
        ActivePhase::Committed,
        original_epoch
            .checked_add(3)
            .ok_or("update_revision_exhausted")?,
    )?;
    let recovered = marker(
        original,
        ActivePhase::Committed,
        original_epoch
            .checked_add(4)
            .ok_or("update_revision_exhausted")?,
    )?;
    let owner = read(&root.join("suite-owner.json"), 4096)?;
    let manifest = read(&root.join("devbox-installation.json"), 64 * 1024)?;
    let activation = read(&root.join("devbox-activation.json"), 4096)?;
    let payload = read(&root.join("suite-payload.json"), MAX_RELEASE_BYTES as u64)?;
    let old = same_json(&owner, &original.owner)?
        && same_json(&manifest, &original.manifest)?
        && payload == original.payload;
    let new = same_json(&owner, &candidate.owner)?
        && same_json(&manifest, &candidate.manifest)?
        && payload == candidate.payload;
    let valid = match state {
        None | Some(State::Applying) => old && same_json(&activation, &original.activation)?,
        Some(State::Committed) => new && same_json(&activation, &committed)?,
        Some(State::RolledBack) => old && same_json(&activation, &recovered)?,
        Some(State::Committing) => {
            new && (same_json(&activation, &health)? || same_json(&activation, &committed)?)
        }
        Some(State::Health | State::RollingBack) => {
            // The durable blocker covers a crash between these independent
            // records. Each record must still be one of the two pinned values.
            (same_json(&owner, &original.owner)? || same_json(&owner, &candidate.owner)?)
                && (same_json(&manifest, &original.manifest)?
                    || same_json(&manifest, &candidate.manifest)?)
                && (payload == original.payload || payload == candidate.payload)
                && (same_json(&activation, &original.activation)?
                    || same_json(&activation, &health)?
                    || same_json(&activation, &recovered)?)
        }
    };
    if valid {
        Ok(())
    } else {
        Err("update_records_changed")
    }
}
fn write_records(root: &Path, records: &Records, activation: &Activation) -> Result<()> {
    persist(&root.join("suite-owner.json"), &records.owner)?;
    persist(&root.join("devbox-installation.json"), &records.manifest)?;
    devbox_filesystem::atomic_write(root.join("suite-payload.json"), &records.payload)
        .map_err(|_| "update_record_write_failed")?;
    persist(&root.join("devbox-activation.json"), activation)
}
fn operation_path(parent: &Path, key: &str, id: &str) -> Result<PathBuf> {
    if !product_contract::commands::revision(key)
        || !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
    {
        return Err("update_operation_invalid");
    }
    Ok(parent
        .join(format!("com.devbox.v08.suite-updates.i{key}"))
        .join(id))
}
pub(super) fn status(root: &Path, key: &str) -> Result<Option<serde_json::Value>> {
    let path = root.join("suite-update.json");
    if matches!(fs::symlink_metadata(&path),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let (id, recorded_key, revision, payload): (String, String, String, String) =
        serde_json::from_slice(&read(&path, 4096)?).map_err(|_| "update_claim_invalid")?;
    if recorded_key != key || !product_contract::commands::revision(&payload) {
        return Err("update_claim_changed");
    }
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    let operation = operation_path(&parent, key, &id)?;
    let bytes = read(&operation.join("plan.json"), 2 * 1024 * 1024)?;
    if hash(&bytes) != revision {
        return Err("update_plan_changed");
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|_| "update_plan_invalid")?;
    if plan.id != id
        || plan.key != key
        || plan.candidate.owner.payload_revision != payload
        || plan.root_identity
            != filesystem_identity(root, true)
                .map_err(|_| "update_root_changed")?
                .components()
    {
        return Err("update_plan_changed");
    }
    let progress_path = operation.join("progress.json");
    let state = if matches!(fs::symlink_metadata(&progress_path),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        State::Applying
    } else {
        let progress: Progress = serde_json::from_slice(&read(&progress_path, 4096)?)
            .map_err(|_| "update_progress_invalid")?;
        if progress.revision != revision {
            return Err("update_plan_changed");
        }
        progress.state
    };
    Ok(Some(
        serde_json::json!({"id":id,"state":state,"payloadRevision":payload,
        "previousVersion":plan.original.manifest.suite_version,"version":plan.candidate.manifest.suite_version,"checkpointId":plan.checkpoint.id}),
    ))
}

#[cfg(windows)]
pub(super) fn open_current(root: &Path, product: &str) -> Result<StageResult> {
    // Only called after a completed, verified helper operation or a pinned
    // dispatcher decision. The current helper still revalidates the whole scope.
    let current = records(root)?;
    let payload = Payload::parse(&current.payload)?;
    let directory = root.join("setup").join(&current.owner.payload_revision);
    let helper = directory.join("devbox-suite-bootstrap.exe");
    verify_payload_owner(&payload, &helper)?;
    super::open_install(
        root,
        &directory.join("suite-payload.json"),
        &helper,
        product,
    )
}

#[cfg(windows)]
pub(super) fn dispatch(
    root: &Path,
    supplied: &Payload,
    supplied_revision: &str,
    product: &str,
) -> Result<Option<StageResult>> {
    use std::os::windows::process::CommandExt;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    let identity = filesystem_identity(root, true)
        .map_err(|_| "bootstrap_root_changed")?
        .components();
    if owner.root_identity != identity {
        return Err("bootstrap_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity, &owner.installation_id))
            .map_err(|_| "update_owner_invalid")?,
    );
    let pending = status(root, &key)?;
    let blocked = !matches!(fs::symlink_metadata(root.join("suite-update.block")),Err(error) if error.kind()==std::io::ErrorKind::NotFound);
    if owner.payload_revision == supplied_revision && !blocked {
        return Ok(None);
    }
    let next_revision = if blocked {
        pending
            .as_ref()
            .and_then(|value| value["payloadRevision"].as_str())
            .ok_or("update_claim_invalid")?
    } else {
        owner.payload_revision.as_str()
    }
    .to_owned();
    let mut trusted = super::registration::trusts_dispatcher(root, supplied_revision)?;
    if let Some(pending) = &pending {
        let operation = operation_path(
            &dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?,
            &key,
            pending["id"].as_str().ok_or("update_claim_invalid")?,
        )?;
        let plan: Plan =
            serde_json::from_slice(&read(&operation.join("plan.json"), 2 * 1024 * 1024)?)
                .map_err(|_| "update_plan_invalid")?;
        trusted |= plan.original.owner.payload_revision == supplied_revision
            || plan.candidate.owner.payload_revision == supplied_revision;
    }
    if !trusted {
        return Err("bootstrap_dispatcher_untrusted");
    }
    if !product_contract::commands::revision(&next_revision) {
        return Err("update_claim_invalid");
    }
    let directory = root.join("setup").join(&next_revision);
    let payload_path = directory.join("suite-payload.json");
    let bytes = read(&payload_path, MAX_RELEASE_BYTES as u64)?;
    if hash(&bytes) != next_revision {
        return Err("bootstrap_payload_changed");
    }
    let payload = Payload::parse(&bytes)?;
    let helper = directory.join("devbox-suite-bootstrap.exe");
    verify_payload_owner(&payload, &helper)?;
    let mut command = std::process::Command::new(helper);
    if blocked {
        let pending = pending.ok_or("update_claim_invalid")?;
        let action = match pending["state"].as_str() {
            Some("committing" | "committed") => "updateCommit",
            Some("rollingBack" | "rolledBack") => "updateRollback",
            Some("applying" | "health") => "updateResume",
            _ => return Err("update_progress_invalid"),
        };
        command
            .arg("--reviewed-data-action")
            .arg(root)
            .arg(payload_path)
            .arg(action)
            .arg(pending["id"].as_str().ok_or("update_claim_invalid")?);
    } else {
        command
            .arg(format!("--open-{product}"))
            .arg(root)
            .arg(payload_path);
    }
    command
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|_| "bootstrap_launch_failed")?;
    let _ = supplied;
    Ok(Some(StageResult {
        state: if blocked {
            "updateRecoveryOpened"
        } else {
            "currentProductOpened"
        },
        operation_id: None,
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: next_revision,
    }))
}

#[cfg(windows)]
pub(super) fn resume_before_shell(image: &Path) -> Result<bool> {
    let Some(root) = image
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .filter(|path| path.file_name().is_some_and(|name| name == "generations"))
        .and_then(|path| path.parent())
    else {
        return Ok(false);
    };
    if matches!(fs::symlink_metadata(root.join("suite-update.block")),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(false);
    }
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "update_owner_invalid")?;
    let key = hash(
        &serde_json::to_vec(&(
            filesystem_identity(root, true)
                .map_err(|_| "update_root_changed")?
                .components(),
            &owner.installation_id,
        ))
        .map_err(|_| "update_owner_invalid")?,
    );
    let pending = status(root, &key)?.ok_or("update_claim_invalid")?;
    let operation = operation_path(
        &dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?,
        &key,
        pending["id"].as_str().ok_or("update_claim_invalid")?,
    )?;
    let plan: Plan = serde_json::from_slice(&read(&operation.join("plan.json"), 2 * 1024 * 1024)?)
        .map_err(|_| "update_plan_invalid")?;
    let canonical = image
        .canonicalize()
        .map_err(|_| "bootstrap_identity_unavailable")?;
    for records in [&plan.original, &plan.candidate] {
        if let Some(member) = records.manifest.members.iter().find(|member| {
            member.product == "control-center"
                && root.join(&member.executable).canonicalize().ok().as_ref() == Some(&canonical)
        }) {
            let payload = Payload::parse(&records.payload)?;
            let package = payload
                .products
                .iter()
                .find(|package| package.id == "control-center")
                .ok_or("bootstrap_payload_incomplete")?;
            let asset = package
                .files
                .iter()
                .find(|asset| {
                    asset.name == "devbox-control-center.exe" && asset.sha256 == member.sha256
                })
                .ok_or("bootstrap_payload_incomplete")?;
            verified_file(image, asset)?;
            dispatch(
                root,
                &payload,
                &records.owner.payload_revision,
                "control-center",
            )?;
            return Ok(true);
        }
    }
    Err("update_caller_untrusted")
}
pub(super) fn prepare(root: &Path, payload_path: &Path, image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate(&root, false)?;
    let original = records(&root)?;
    if original.activation.phase != ActivePhase::Committed {
        return Err("update_committed_installation_required");
    }
    let version = |value: &str| -> Result<Vec<u32>> {
        value
            .split('.')
            .map(|part| part.parse::<u32>().map_err(|_| "update_version_invalid"))
            .collect()
    };
    if version(&payload.suite_version)? < version(&original.manifest.suite_version)? {
        return Err("downgrade_requires_backup_export");
    }
    if revision == original.owner.payload_revision {
        return Err("update_already_installed");
    }
    verify_packages(&root, &original)?;
    let identity = original.owner.root_identity;
    let key = hash(
        &serde_json::to_vec(&(identity, &original.owner.installation_id))
            .map_err(|_| "update_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    ensure_no_links(&parent).map_err(|_| "bootstrap_data_unsafe")?;
    let live = sources(&parent, &key)?;
    let (journal, _) =
        Store::inspect(&live["control-center"])?.ok_or("bootstrap_journal_missing")?;
    if journal.phase != Phase::Complete
        || !journal.committed
        || journal.candidate != original.manifest
        || journal.installation_key != key
    {
        return Err("update_previous_operation_incomplete");
    }
    require_space(
        &parent,
        data_checkpoint::required_space(&live)?
            .checked_mul(2)
            .ok_or("suite_space_insufficient")?,
    )?;
    require_space(&root, package_space(&payload)?)?;
    let updates = parent.join(format!("com.devbox.v08.suite-updates.i{key}"));
    create_directory(&updates)?;
    if fs::read_dir(&updates)
        .map_err(|_| "update_store_unavailable")?
        .take(32)
        .count()
        >= 32
    {
        return Err("update_retention_review_required");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let operation = operation_path(&parent, &key, &id)?;
    fs::create_dir(&operation).map_err(|_| "update_operation_conflict")?;
    let retained = retain_setup_inputs(&root, payload_path, image, &payload, &bytes)?;
    let generation = format!("g-{id}");
    let directory = root.join("generations").join(&generation);
    create_directory(&directory)?;
    stage_impl(&directory, &retained, image)?;
    let manifest = Manifest {
        schema_version: 1,
        installation_id: original.owner.installation_id.clone(),
        generation: generation.clone(),
        suite_version: payload.suite_version.clone(),
        protocol_version: 1,
        members: payload
            .products
            .iter()
            .map(|product| {
                let name = format!("devbox-{}.exe", product.id);
                let asset = product
                    .files
                    .iter()
                    .find(|asset| asset.name == name)
                    .ok_or("bootstrap_payload_incomplete")?;
                Ok(Member {
                    product: product.id.clone(),
                    executable: format!("generations/{generation}/products/{}/{name}", product.id),
                    sha256: asset.sha256.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };
    manifest.validate(&payload.suite_version)?;
    let candidate = Records {
        owner: InstallOwner {
            schema_version: 1,
            root_identity: identity,
            installation_id: original.owner.installation_id.clone(),
            generation: generation.clone(),
            operation_id: id.clone(),
            payload_revision: revision.clone(),
            restart_from: None,
        },
        activation: Activation {
            schema_version: 1,
            installation_id: original.owner.installation_id.clone(),
            generation,
            operation_id: id.clone(),
            revision: 0,
            phase: ActivePhase::Health,
        },
        manifest,
        payload: bytes,
    };
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    create_directory(&backup)?;
    let checkpoint = data_checkpoint::acquire_quiesced(
        &live,
        &backup,
        &key,
        &original.manifest.generation,
        &AtomicBool::new(false),
    )?;
    let prepared = operation.join("prepared");
    create_directory(&prepared)?;
    data_checkpoint::materialize(
        &backup,
        &checkpoint,
        &key,
        &original.manifest.generation,
        &prepared.join(&checkpoint.id),
        &AtomicBool::new(false),
    )?;
    let namespaces = live
        .iter()
        .map(|(owner, path)| {
            Ok((
                owner.clone(),
                Namespace {
                    original: directory_identity(path)?,
                    prepared: directory_identity(&prepared.join(&checkpoint.id).join(owner))?,
                },
            ))
        })
        .collect::<Result<_>>()?;
    let plan = Plan {
        schema_version: 1,
        id: id.clone(),
        key,
        root_identity: identity,
        original,
        candidate,
        checkpoint: checkpoint.clone(),
        namespaces,
    };
    persist(&operation.join("plan.json"), &plan)?;
    Ok(StageResult {
        state: "updatePrepared",
        operation_id: Some(id),
        checkpoint: Some(checkpoint),
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
pub(super) fn install(root: &Path, payload_path: &Path, image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    verify_payload_owner(&Payload::parse(&bytes)?, image)?;
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "update_owner_invalid")?;
    let identity = filesystem_identity(&root, true)
        .map_err(|_| "update_root_changed")?
        .components();
    if owner.root_identity != identity {
        return Err("update_owner_changed");
    }
    let key = hash(
        &serde_json::to_vec(&(identity, &owner.installation_id))
            .map_err(|_| "update_owner_invalid")?,
    );
    if let Some(pending) = status(&root, &key)? {
        if pending["payloadRevision"].as_str() != Some(hash(&bytes).as_str()) {
            return Err("update_other_generation_pending");
        }
        let mode = match pending["state"].as_str() {
            Some("committing" | "committed") => "--commit-update",
            Some("rollingBack" | "rolledBack") => "--rollback-update",
            Some("applying" | "health") => "--apply-update",
            _ => return Err("update_progress_invalid"),
        };
        return execute(
            &root,
            payload_path,
            image,
            pending["id"].as_str().ok_or("update_claim_invalid")?,
            mode,
        );
    }
    let prepared = prepare(&root, payload_path, image)?;
    execute(
        &root,
        payload_path,
        image,
        prepared
            .operation_id
            .as_deref()
            .ok_or("update_operation_invalid")?,
        "--apply-update",
    )
}

pub(super) fn execute(
    root: &Path,
    payload_path: &Path,
    image: &Path,
    id: &str,
    mode: &str,
) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate_for_restore(&root, false)?;
    for forbidden in ["suite-data-restore.json", "uninstall-plan.json"] {
        if !matches!(fs::symlink_metadata(root.join(forbidden)),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("update_other_recovery_pending");
        }
    }
    // Installation ID is stable across the two owner records, including a
    // partially published metadata switch protected by suite-update.block.
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "update_owner_invalid")?;
    let identity = filesystem_identity(&root, true)
        .map_err(|_| "update_root_changed")?
        .components();
    let key = hash(
        &serde_json::to_vec(&(identity, &owner.installation_id))
            .map_err(|_| "update_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    let operation = operation_path(&parent, &key, id)?;
    #[cfg(windows)]
    let _operation_pins = crate::suite::platform::component_scope::pin_directories(&operation)?;
    let plan_bytes = read(&operation.join("plan.json"), 2 * 1024 * 1024)?;
    let plan: Plan = serde_json::from_slice(&plan_bytes).map_err(|_| "update_plan_invalid")?;
    let plan_revision = hash(&plan_bytes);
    if plan.schema_version != 1
        || plan.id != id
        || plan.key != key
        || plan.root_identity != identity
        || plan.candidate.payload != bytes
        || plan.candidate.owner.payload_revision != revision
        || plan.candidate.owner.operation_id != id
        || plan.original.owner.installation_id != owner.installation_id
        || plan.original.owner.root_identity != identity
        || plan.candidate.owner.root_identity != identity
    {
        return Err("update_plan_changed");
    }
    plan.original
        .manifest
        .validate(&Payload::parse(&plan.original.payload)?.suite_version)?;
    plan.candidate.manifest.validate(&payload.suite_version)?;
    verify_packages(&root, &plan.original)?;
    verify_packages(&root, &plan.candidate)?;
    let progress_path = operation.join("progress.json");
    let mut progress: Option<Progress> = match fs::symlink_metadata(&progress_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        _ => Some(
            serde_json::from_slice(&read(&progress_path, 4096)?)
                .map_err(|_| "update_progress_invalid")?,
        ),
    };
    if progress
        .as_ref()
        .is_some_and(|progress| progress.revision != plan_revision)
    {
        return Err("update_plan_changed");
    }
    check_records(&root, &plan, progress.as_ref().map(|value| value.state))?;
    let claim_path = root.join("suite-update.json");
    let block_path = root.join("suite-update.block");
    let claim_bytes = serde_json::to_vec(&(id, &key, &plan_revision, &revision))
        .map_err(|_| "update_plan_invalid")?;
    let result = |state| StageResult {
        state,
        operation_id: Some(id.into()),
        checkpoint: Some(plan.checkpoint.clone()),
        source_sha: payload.source_sha.clone(),
        suite_version: payload.suite_version.clone(),
        payload_revision: revision.clone(),
    };
    if progress
        .as_ref()
        .is_some_and(|value| matches!(value.state, State::Committed | State::RolledBack))
    {
        release(&block_path, &claim_bytes)?;
        release(&claim_path, &claim_bytes)?;
        return Ok(result(if progress.unwrap().state == State::Committed {
            "updateCommitted"
        } else {
            "updateRolledBack"
        }));
    }
    let commit = mode == "--commit-update";
    let rollback = mode == "--rollback-update";
    if commit
        && !progress
            .as_ref()
            .is_some_and(|value| matches!(value.state, State::Health | State::Committing))
    {
        return Err("suite_health_required");
    }
    if rollback
        && progress
            .as_ref()
            .is_some_and(|value| value.state == State::Committing)
    {
        return Err("update_commit_started");
    }
    if !rollback
        && !commit
        && progress
            .as_ref()
            .is_some_and(|value| matches!(value.state, State::Committing | State::RollingBack))
    {
        return Err("update_resume_selected_action");
    }
    let live = sources(&parent, &key)?;
    if live.keys().ne(plan.namespaces.keys()) {
        return Err("update_plan_invalid");
    }
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    let prepared = operation.join("prepared").join(&plan.checkpoint.id);
    let displaced = operation.join("original");
    let retained = operation.join("retained-after-update");
    if progress.is_none() {
        data_checkpoint::matches_quiesced_sources(
            &live,
            &backup,
            &plan.checkpoint,
            &key,
            &plan.original.manifest.generation,
            &AtomicBool::new(false),
        )?;
        data_checkpoint::verify(
            &operation.join("prepared"),
            &plan.checkpoint,
            &key,
            &plan.original.manifest.generation,
            &AtomicBool::new(false),
        )?;
        claim(&claim_path, &claim_bytes)?;
        claim(&block_path, &claim_bytes)?;
        let value = Progress {
            revision: plan_revision.clone(),
            state: State::Applying,
        };
        persist(&progress_path, &value)?;
        progress = Some(value);
    } else if read(&claim_path, 4096)? != claim_bytes {
        return Err("update_operation_conflict");
    }
    let mut progress = progress.ok_or("update_progress_invalid")?;
    let epoch = plan.original.activation.revision;
    if rollback {
        claim(&block_path, &claim_bytes)?;
        progress.state = State::RollingBack;
        persist(&progress_path, &progress)?;
        create_directory(&displaced)?;
        create_directory(&retained)?;
        for (owner, namespace) in &plan.namespaces {
            for _ in 0..3 {
                match namespace.rollback(
                    directory_identity(&live[owner])?,
                    directory_identity(&displaced.join(owner))?,
                    directory_identity(&prepared.join(owner))?,
                    directory_identity(&retained.join(owner))?,
                )? {
                    Step::Preserve => move_directory(
                        &live[owner],
                        &retained.join(owner),
                        namespace.prepared.ok_or("update_plan_invalid")?,
                    )?,
                    Step::Publish => move_directory(
                        &displaced.join(owner),
                        &live[owner],
                        namespace.original.ok_or("update_plan_invalid")?,
                    )?,
                    Step::Complete => break,
                }
            }
        }
        data_checkpoint::matches_quiesced_sources(
            &live,
            &backup,
            &plan.checkpoint,
            &key,
            &plan.original.manifest.generation,
            &AtomicBool::new(false),
        )?;
        write_records(
            &root,
            &plan.original,
            &marker(
                &plan.original,
                ActivePhase::Committed,
                epoch.checked_add(4).ok_or("update_revision_exhausted")?,
            )?,
        )?;
        progress.state = State::RolledBack;
        persist(&progress_path, &progress)?;
        release(&block_path, &claim_bytes)?;
        release(&claim_path, &claim_bytes)?;
        return Ok(result("updateRolledBack"));
    }
    if progress.state == State::Applying {
        claim(&block_path, &claim_bytes)?;
        create_directory(&displaced)?;
        let mut original = BTreeMap::new();
        let mut candidate = BTreeMap::new();
        for (owner, namespace) in &plan.namespaces {
            let old = displaced.join(owner);
            let staged = prepared.join(owner);
            namespace.apply(
                directory_identity(&live[owner])?,
                directory_identity(&old)?,
                directory_identity(&staged)?,
            )?;
            original.insert(
                owner.clone(),
                if namespace.original.is_some() && directory_identity(&old)? == namespace.original {
                    old
                } else if namespace.original.is_some() {
                    live[owner].clone()
                } else {
                    displaced.join(owner)
                },
            );
            candidate.insert(
                owner.clone(),
                if namespace.prepared.is_some()
                    && directory_identity(&live[owner])? == namespace.prepared
                {
                    live[owner].clone()
                } else {
                    staged
                },
            );
        }
        for set in [&original, &candidate] {
            data_checkpoint::matches_quiesced_sources(
                set,
                &backup,
                &plan.checkpoint,
                &key,
                &plan.original.manifest.generation,
                &AtomicBool::new(false),
            )?;
        }
        for (owner, namespace) in &plan.namespaces {
            for _ in 0..3 {
                match namespace.apply(
                    directory_identity(&live[owner])?,
                    directory_identity(&displaced.join(owner))?,
                    directory_identity(&prepared.join(owner))?,
                )? {
                    Step::Preserve => move_directory(
                        &live[owner],
                        &displaced.join(owner),
                        namespace.original.ok_or("update_plan_invalid")?,
                    )?,
                    Step::Publish => move_directory(
                        &prepared.join(owner),
                        &live[owner],
                        namespace.prepared.ok_or("update_plan_invalid")?,
                    )?,
                    Step::Complete => break,
                }
            }
        }
        progress.state = State::Health;
        persist(&progress_path, &progress)?;
    }
    if progress.state == State::Health {
        let store = Store::open(&live["control-center"])?;
        let (mut journal, mut digest) = store.read()?.ok_or("bootstrap_journal_missing")?;
        if journal.candidate == plan.original.manifest && journal.phase == Phase::Complete {
            journal = Journal::begin(
                id.into(),
                key.clone(),
                Some(plan.original.manifest.clone()),
                plan.candidate.manifest.clone(),
            )?;
            digest = store.begin(Some(&digest), &journal)?;
        }
        if journal.candidate != plan.candidate.manifest
            || journal.operation_id != id
            || journal.installation_key != key
        {
            return Err("update_journal_changed");
        }
        while matches!(
            journal.phase,
            Phase::Inventory
                | Phase::Stage
                | Phase::Verify
                | Phase::Snapshot
                | Phase::Import
                | Phase::Validate
                | Phase::Quiesce
                | Phase::Activate
        ) {
            journal.advance(
                journal.revision,
                Proof {
                    phase: journal.phase,
                    generation: plan.candidate.manifest.generation.clone(),
                    revision: plan_revision.clone(),
                },
            )?;
            digest = store.write(Some(&digest), &journal)?;
        }
        if !matches!(journal.phase, Phase::Health | Phase::Commit) {
            return Err("update_journal_changed");
        }
        write_records(
            &root,
            &plan.candidate,
            &marker(
                &plan.candidate,
                ActivePhase::Health,
                epoch.checked_add(2).ok_or("update_revision_exhausted")?,
            )?,
        )?;
        release(&block_path, &claim_bytes)?;
        if !commit {
            return Ok(result("updateHealthRequired"));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "bootstrap_clock_invalid")?
            .as_millis() as u64;
        journal.require_recent_health(now)?;
        claim(&block_path, &claim_bytes)?;
        progress.state = State::Committing;
        persist(&progress_path, &progress)?;
    }
    let store = Store::open(&live["control-center"])?;
    let (mut journal, mut digest) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.candidate != plan.candidate.manifest
        || journal.operation_id != id
        || journal.installation_key != key
    {
        return Err("update_journal_changed");
    }
    while matches!(
        journal.phase,
        Phase::Health | Phase::Commit | Phase::Cleanup
    ) {
        journal.advance(
            journal.revision,
            Proof {
                phase: journal.phase,
                generation: plan.candidate.manifest.generation.clone(),
                revision: plan_revision.clone(),
            },
        )?;
        digest = store.write(Some(&digest), &journal)?;
    }
    if !journal.committed || journal.phase != Phase::Complete {
        return Err("update_journal_changed");
    }
    #[cfg(windows)]
    super::registration::update_version(&root, &key, &payload.suite_version)?;
    write_records(
        &root,
        &plan.candidate,
        &marker(
            &plan.candidate,
            ActivePhase::Committed,
            epoch.checked_add(3).ok_or("update_revision_exhausted")?,
        )?,
    )?;
    progress.state = State::Committed;
    persist(&progress_path, &progress)?;
    release(&block_path, &claim_bytes)?;
    release(&claim_path, &claim_bytes)?;
    Ok(result("updateCommitted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> (Temp, Plan) {
        let root =
            std::env::temp_dir().join(format!("devbox-update-records-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let identity = filesystem_identity(&root, true).unwrap().components();
        let records = |id: &str| {
            let generation = format!("g-{id}");
            let manifest = Manifest {
                schema_version: 1,
                installation_id: "fixture-installation".into(),
                generation: generation.clone(),
                suite_version: "0.8.0".into(),
                protocol_version: 1,
                members: product_contract::installation::PRODUCTS
                    .iter()
                    .map(|product| Member {
                        product: (*product).into(),
                        executable: format!(
                            "generations/{generation}/products/{product}/devbox-{product}.exe"
                        ),
                        sha256: "a".repeat(64),
                    })
                    .collect(),
            };
            Records {
                owner: InstallOwner {
                    schema_version: 1,
                    root_identity: identity,
                    installation_id: manifest.installation_id.clone(),
                    generation: generation.clone(),
                    operation_id: id.into(),
                    payload_revision: "b".repeat(64),
                    restart_from: None,
                },
                activation: Activation {
                    schema_version: 1,
                    installation_id: manifest.installation_id.clone(),
                    generation,
                    operation_id: id.into(),
                    revision: 10,
                    phase: ActivePhase::Committed,
                },
                manifest,
                payload: format!("{{\"fixture\":\"{id}\"}}").into_bytes(),
            }
        };
        let plan = Plan {
            schema_version: 1,
            id: "next".into(),
            key: "c".repeat(64),
            root_identity: identity,
            original: records("old"),
            candidate: records("next"),
            checkpoint: data_checkpoint::Receipt {
                id: uuid::Uuid::new_v4().to_string(),
                revision: "d".repeat(64),
                bytes: 1,
                files: 1,
            },
            namespaces: BTreeMap::new(),
        };
        write_records(&root, &plan.original, &plan.original.activation).unwrap();
        (Temp(root), plan)
    }
    #[test]
    fn interrupted_metadata_publication_accepts_only_the_two_pinned_generations() {
        let (root, plan) = fixture();
        check_records(&root.0, &plan, None).unwrap();
        persist(&root.0.join("suite-owner.json"), &plan.candidate.owner).unwrap();
        assert!(check_records(&root.0, &plan, Some(State::Applying)).is_err());
        check_records(&root.0, &plan, Some(State::Health)).unwrap();
        persist(
            &root.0.join("devbox-installation.json"),
            &plan.candidate.manifest,
        )
        .unwrap();
        check_records(&root.0, &plan, Some(State::Health)).unwrap();
        let mut foreign = plan.candidate.manifest.clone();
        foreign.installation_id = "another-installation".into();
        persist(&root.0.join("devbox-installation.json"), &foreign).unwrap();
        assert!(check_records(&root.0, &plan, Some(State::Health)).is_err());
        assert!(check_records(&root.0, &plan, Some(State::RollingBack)).is_err());
    }
    #[test]
    fn completed_update_and_rollback_reject_stale_marker_replay() {
        let (root, plan) = fixture();
        let health = marker(&plan.candidate, ActivePhase::Health, 12).unwrap();
        write_records(&root.0, &plan.candidate, &health).unwrap();
        check_records(&root.0, &plan, Some(State::Health)).unwrap();
        assert!(check_records(&root.0, &plan, Some(State::Committed)).is_err());
        write_records(
            &root.0,
            &plan.candidate,
            &marker(&plan.candidate, ActivePhase::Committed, 13).unwrap(),
        )
        .unwrap();
        check_records(&root.0, &plan, Some(State::Committed)).unwrap();
        assert!(check_records(&root.0, &plan, None).is_err());
        write_records(
            &root.0,
            &plan.original,
            &marker(&plan.original, ActivePhase::Committed, 14).unwrap(),
        )
        .unwrap();
        check_records(&root.0, &plan, Some(State::RolledBack)).unwrap();
        assert!(check_records(&root.0, &plan, Some(State::Committed)).is_err());
    }
}
