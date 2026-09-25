//! Rehydrate the exact last package after a data-preserving removal. The same
//! installation identity keeps its existing stores; a different package must
//! first recover this version, then use the generation updater.
use super::*;
use crate::core::{
    data_checkpoint,
    delivery::{Journal, Phase, Proof},
    delivery_store::Store,
};
use product_contract::activation::{Activation, Phase as ActivePhase};
#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    id: String,
    installation_key: String,
    payload_revision: String,
    original: Journal,
    state: String,
}
fn intent(root: &Path) -> Result<Intent> {
    let value: Intent =
        serde_json::from_slice(&read(&root.join("suite-reinstall.json"), 16 * 1024 * 1024)?)
            .map_err(|_| "reinstall_plan_invalid")?;
    value.original.validate()?;
    if value.schema_version != 1
        || !uuid::Uuid::parse_str(&value.id).is_ok_and(|id| id.to_string() == value.id)
        || value.installation_key != value.original.installation_key
        || !product_contract::commands::revision(&value.payload_revision)
        || !matches!(value.state.as_str(), "staging" | "health" | "done")
    {
        return Err("reinstall_plan_invalid");
    }
    Ok(value)
}
fn save(root: &Path, value: &Intent) -> Result<()> {
    devbox_filesystem::atomic_write(
        root.join("suite-reinstall.json"),
        &serde_json::to_vec(value).map_err(|_| "reinstall_plan_invalid")?,
    )
    .map_err(|_| "reinstall_plan_unavailable")
}
fn archive(root: &Path, name: &str, suffix: &str) -> Result<()> {
    let source = root.join(name);
    if matches!(fs::symlink_metadata(&source),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(());
    }
    let bytes = read(&source, 16 * 1024 * 1024)?;
    let destination = root.join(format!("{name}.{suffix}.retained"));
    match fs::hard_link(&source, &destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if read(&destination, 16 * 1024 * 1024)? != bytes {
                return Err("reinstall_archive_changed");
            }
        }
        Err(_) => return Err("reinstall_archive_unavailable"),
    }
    if read(&source, 16 * 1024 * 1024)? != bytes {
        return Err("reinstall_archive_changed");
    }
    fs::remove_file(source).map_err(|_| "reinstall_archive_unavailable")
}
#[cfg(windows)]
pub(super) fn pending(root: &Path) -> Result<bool> {
    if matches!(fs::symlink_metadata(root.join("suite-reinstall.json")),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
    {
        return Ok(false);
    }
    Ok(intent(root)?.original.committed)
}
pub(super) fn execute(
    root: &Path,
    payload_path: &Path,
    image: &Path,
    commit: bool,
) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = installation_tools::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let identity = filesystem_identity(&root, true)
        .map_err(|_| "bootstrap_root_changed")?
        .components();
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate_for_restore(&root, false)?;
    for name in ["suite-update.json", "suite-data-restore.json"] {
        if !matches!(fs::symlink_metadata(root.join(name)),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("reinstall_recovery_pending");
        }
    }
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    if owner.schema_version != 1
        || owner.root_identity != identity
        || owner.generation != format!("g-{}", owner.operation_id)
    {
        return Err("bootstrap_owner_changed");
    }
    if owner.payload_revision != revision {
        return Err("reinstall_original_package_required");
    }
    let key = hash(
        &serde_json::to_vec(&(identity, &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    let parent = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
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
    if journal.installation_key != key
        || journal.operation_id != owner.operation_id
        || journal.candidate.installation_id != owner.installation_id
        || journal.candidate.generation != owner.generation
        || journal.candidate.suite_version != payload.suite_version
    {
        return Err("bootstrap_journal_changed");
    }
    let mut plan = if root
        .join("suite-reinstall.json")
        .try_exists()
        .map_err(|_| "reinstall_plan_unavailable")?
    {
        intent(&root)?
    } else {
        if commit {
            return Err("reinstall_plan_missing");
        }
        let complete: (u32, String, String, String) =
            serde_json::from_slice(&read(&root.join("uninstall-complete.json"), 4096)?)
                .map_err(|_| "reinstall_removal_incomplete")?;
        let removal_bytes = read(&root.join("uninstall-plan.json"), 2 * 1024 * 1024)?;
        let removal: super::uninstall::Receipt =
            serde_json::from_slice(&removal_bytes).map_err(|_| "reinstall_removal_incomplete")?;
        removal.plan.validate()?;
        if complete!=(1,key.clone(),revision.clone(),hash(&removal_bytes)) || removal.schema_version!=1 || removal.payload_revision!=revision || removal.plan.installation_key!=key || removal.plan.root_identity!=identity
            || removal.plan.files.iter().any(|file|!matches!(fs::symlink_metadata(root.join(&file.relative)),Err(error) if error.kind()==std::io::ErrorKind::NotFound)){return Err("reinstall_removal_incomplete");}
        #[cfg(windows)]
        super::registration::prepare_reinstall(&root, &key, &revision)?;
        if journal.data_checkpoints.len() >= 32 {
            return Err("checkpoint_retention_review_required");
        }
        let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
        create_directory(&backup)?;
        require_space(&parent, data_checkpoint::required_space(&sources)?)?;
        let checkpoint = data_checkpoint::acquire_quiesced(
            &sources,
            &backup,
            &key,
            &owner.generation,
            &AtomicBool::new(false),
        )?;
        journal.record_checkpoint(journal.revision, checkpoint)?;
        digest = store.write(Some(&digest), &journal)?;
        let value = Intent {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            installation_key: key.clone(),
            payload_revision: revision.clone(),
            original: journal.clone(),
            state: "staging".into(),
        };
        save(&root, &value)?;
        value
    };
    if plan.installation_key != key
        || plan.payload_revision != revision
        || plan.original.candidate != journal.candidate
    {
        return Err("reinstall_plan_changed");
    }
    if commit {
        if !plan.original.committed
            || !matches!(plan.state.as_str(), "health" | "done")
            || !matches!(
                journal.phase,
                Phase::Health | Phase::Commit | Phase::Cleanup | Phase::Complete
            )
        {
            return Err("reinstall_health_required");
        }
        for product in &payload.products {
            let directory = root
                .join("generations")
                .join(&owner.generation)
                .join("products")
                .join(&product.id);
            exact_closure(&directory, product)?;
            for file in product
                .files
                .iter()
                .filter(|file| file.name != "devbox-installation.json")
            {
                verified_file(&directory.join(&file.name), file)?;
            }
        }
        if !journal.committed {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "bootstrap_clock_invalid")?
                .as_millis() as u64;
            journal.require_recent_health(now)?;
        }
        while matches!(journal.phase, Phase::Health | Phase::Commit) {
            journal.advance(
                journal.revision,
                Proof {
                    phase: journal.phase,
                    generation: owner.generation.clone(),
                    revision: revision.clone(),
                },
            )?;
            digest = store.write(Some(&digest), &journal)?;
        }
    } else {
        archive(&root, "uninstall-plan.json", &plan.id)?;
        archive(&root, "uninstall-complete.json", &plan.id)?;
        let retained = retain_setup_inputs(&root, payload_path, image, &payload, &bytes)?;
        create_directory(&root.join("generations"))?;
        let generation = root.join("generations").join(&owner.generation);
        create_directory(&generation)?;
        stage_impl(&generation, &retained, image)?;
        if plan.state == "staging" {
            // Same package/schema, same stores. No importer is run on reinstall.
            journal.phase = if plan.original.committed {
                Phase::Health
            } else {
                Phase::Snapshot
            };
            journal.committed = false;
            journal.health_checks.clear();
            if !plan.original.committed {
                journal.owner_evidence.clear();
            }
            journal.revision = journal
                .revision
                .checked_add(1)
                .ok_or("bootstrap_revision_exhausted")?;
            journal.validate()?;
            digest = store.write(Some(&digest), &journal)?;
            plan.state = "health".into();
            save(&root, &plan)?;
        }
        devbox_filesystem::atomic_write(root.join("suite-payload.json"), &bytes)
            .map_err(|_| "bootstrap_payload_unavailable")?;
        devbox_filesystem::atomic_write(
            root.join("devbox-installation.json"),
            &serde_json::to_vec(&journal.candidate).map_err(|_| "bootstrap_manifest_invalid")?,
        )
        .map_err(|_| "bootstrap_manifest_write_failed")?;
    }
    let marker = Activation {
        schema_version: 1,
        installation_id: owner.installation_id,
        generation: owner.generation.clone(),
        operation_id: owner.operation_id,
        revision: journal.revision,
        phase: if commit {
            ActivePhase::Committed
        } else if plan.original.committed {
            ActivePhase::Health
        } else {
            ActivePhase::Import
        },
    };
    marker.validate(&journal.candidate)?;
    devbox_filesystem::atomic_write(
        root.join("devbox-activation.json"),
        &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
    )
    .map_err(|_| "bootstrap_marker_write_failed")?;
    if commit && journal.phase == Phase::Cleanup && journal.cleanup_pending.is_empty() {
        journal.advance(
            journal.revision,
            Proof {
                phase: journal.phase,
                generation: owner.generation,
                revision: revision.clone(),
            },
        )?;
        store.write(Some(&digest), &journal)?;
    }
    if commit || !plan.original.committed {
        plan.state = "done".into();
        save(&root, &plan)?;
        archive(&root, "suite-reinstall.json", &plan.id)?;
    }
    Ok(StageResult {
        state: if commit {
            "reinstallationCommitted"
        } else {
            "reinstallationPrepared"
        },
        operation_id: None,
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
