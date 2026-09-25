//! Closed bootstrap operations; no product process may hold the writer lease.
use super::*;
use crate::core::{
    data_checkpoint,
    data_restore::{Identity, Step},
    delivery::{Phase, Proof},
    delivery_store::Store,
};
use product_contract::activation::{Activation, Phase as ActivePhase};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum PhaseState {
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
    plan_revision: String,
    phase: PhaseState,
}
pub(super) fn directory_identity(path: &Path) -> Result<Option<Identity>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("restore_namespace_unavailable"),
        Ok(_) => {
            ensure_no_links(path).map_err(|_| "restore_namespace_unsafe")?;
            filesystem_identity(path, true)
                .map(|id| Some(id.components()))
                .map_err(|_| "restore_namespace_unsafe")
        }
    }
}
pub(super) fn persist(path: &Path, value: &impl Serialize) -> Result<()> {
    devbox_filesystem::atomic_write(
        path,
        &serde_json::to_vec(value).map_err(|_| "restore_record_invalid")?,
    )
    .map_err(|_| "restore_record_write_failed")
}
pub(super) fn claim(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) if read(path, 4096)? == bytes => Ok(()),
        Ok(_) => Err("restore_operation_conflict"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Publish a flushed, complete record without replacing another owner.
            let temporary = path.with_extension(format!("pending-{}", uuid::Uuid::new_v4()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| "restore_record_write_failed")?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| "restore_record_write_failed")?;
            fs::hard_link(&temporary, path).map_err(|_| "restore_operation_conflict")?;
            fs::remove_file(temporary).map_err(|_| "restore_record_write_failed")
        }
        Err(_) => Err("restore_record_unavailable"),
    }
}
pub(super) fn release(path: &Path, bytes: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) if read(path, 4096)? == bytes => {
            fs::remove_file(path).map_err(|_| "restore_record_write_failed")
        }
        _ => Err("restore_operation_conflict"),
    }
}
pub(super) fn move_directory(source: &Path, destination: &Path, expected: Identity) -> Result<()> {
    if directory_identity(source)? != Some(expected) || directory_identity(destination)?.is_some() {
        return Err("restore_namespace_changed");
    }
    ensure_no_links(destination.parent().ok_or("restore_namespace_unsafe")?)
        .map_err(|_| "restore_namespace_unsafe")?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
        };
        let source = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let destination = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // No REPLACE_EXISTING or COPY_ALLOWED: preserve foreign destinations and
        // reject cross-volume copies rather than weakening an atomic rename.
        unsafe {
            MoveFileExW(
                PCWSTR(source.as_ptr()),
                PCWSTR(destination.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|_| "restore_namespace_locked")?;
    }
    #[cfg(not(windows))]
    return Err("bootstrap_windows_required");
    #[cfg(windows)]
    if directory_identity(source)?.is_some() || directory_identity(destination)? != Some(expected) {
        return Err("restore_namespace_changed");
    }
    #[cfg(windows)]
    Ok(())
}
fn marker(plan: &DataRestorePlan, phase: ActivePhase, offset: u64) -> Result<Activation> {
    let mut marker = plan.original_activation.clone();
    marker.phase = phase;
    marker.revision = marker
        .revision
        .checked_add(offset)
        .ok_or("bootstrap_revision_exhausted")?;
    Ok(marker)
}
fn same_marker(left: &Activation, right: &Activation) -> bool {
    left.schema_version == right.schema_version
        && left.installation_id == right.installation_id
        && left.generation == right.generation
        && left.operation_id == right.operation_id
        && left.revision == right.revision
        && left.phase == right.phase
}
pub(super) fn execute(
    root: &Path,
    payload_path: &Path,
    image: &Path,
    id: &str,
    mode: &str,
) -> Result<StageResult> {
    if !uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id) {
        return Err("restore_operation_invalid");
    }
    let payload_bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&payload_bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&payload_bytes);
    let root = installation_tools::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _root_pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate_for_restore(&root, false)?;
    for pending in ["suite-update.json", "uninstall-plan.json"] {
        if !matches!(fs::symlink_metadata(root.join(pending)),Err(error) if error.kind()==std::io::ErrorKind::NotFound)
        {
            return Err("restore_other_operation_pending");
        }
    }
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
    let marker_path = root.join("devbox-activation.json");
    let marker_bytes = read(&marker_path, 4096)?;
    let active: Activation =
        serde_json::from_slice(&marker_bytes).map_err(|_| "bootstrap_marker_invalid")?;
    active.validate(&manifest)?;
    if manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
        || active.operation_id != owner.operation_id
    {
        return Err("bootstrap_owner_changed");
    }
    // The helper and all four pinned images must still belong to this payload.
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
        let name = format!("devbox-{}.exe", package.id);
        let file = package
            .files
            .iter()
            .find(|file| file.name == name)
            .ok_or("bootstrap_payload_incomplete")?;
        if !manifest.members.iter().any(|member| {
            member.product == package.id
                && member.sha256 == file.sha256
                && member.executable
                    == format!(
                        "generations/{}/products/{}/{name}",
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
    let operation = parent
        .join(format!("com.devbox.v08.suite-restore.i{key}"))
        .join(id);
    ensure_no_links(&operation).map_err(|_| "restore_operation_unsafe")?;
    #[cfg(windows)]
    let _operation_pins = crate::suite::platform::component_scope::pin_directories(&operation)?;
    let plan_bytes = read(&operation.join("restore-plan.json"), 16 * 1024 * 1024)?;
    let plan_revision = hash(&plan_bytes);
    let plan: DataRestorePlan =
        serde_json::from_slice(&plan_bytes).map_err(|_| "restore_plan_invalid")?;
    plan.health_journal.validate()?;
    if plan.schema_version != 1
        || plan.operation_id != id
        || plan.installation_key != key
        || plan.root_identity != identity.components()
        || plan.generation != manifest.generation
        || plan.manifest_revision != hash(&manifest_bytes)
        || plan.health_journal.candidate != manifest
        || plan.health_journal.installation_key != key
        || plan.health_journal.operation_id != owner.operation_id
        || plan.health_journal.phase != Phase::Health
        || !plan.health_journal.health_checks.is_empty()
    {
        return Err("restore_plan_changed");
    }
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
        .collect::<BTreeMap<_, _>>();
    if plan.namespaces.keys().ne(sources.keys()) {
        return Err("restore_plan_invalid");
    }
    let backup = parent.join(format!("com.devbox.v08.suite-backups.i{key}"));
    let prepared = operation.join("prepared").join(&plan.source.id);
    let displaced = operation.join("displaced");
    let retained = operation.join("retained-after-review");
    let progress_path = operation.join("restore-progress.json");
    let mut progress: Option<Progress> = match fs::symlink_metadata(&progress_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("restore_record_unavailable"),
        Ok(_) => Some(
            serde_json::from_slice(&read(&progress_path, 4096)?)
                .map_err(|_| "restore_record_invalid")?,
        ),
    };
    if progress
        .as_ref()
        .is_some_and(|value| value.plan_revision != plan_revision)
    {
        return Err("restore_plan_changed");
    }
    let claim_path = root.join("suite-data-restore.json");
    let block_path = root.join("suite-data-restore.block");
    let claim_bytes =
        serde_json::to_vec(&(id, &plan_revision)).map_err(|_| "restore_record_invalid")?;
    let rollback = mode == "--rollback-data-restore";
    let commit = mode == "--commit-data-restore";
    let recover_marker = marker(&plan, ActivePhase::Recover, 1)?;
    let health_marker = marker(&plan, ActivePhase::Health, 2)?;
    let committed_marker = marker(&plan, ActivePhase::Committed, 3)?;
    let rollback_marker = marker(&plan, plan.original_activation.phase, 4)?;
    if let Some(value) = &progress {
        let valid = match value.phase {
            PhaseState::Applying => {
                same_marker(&active, &plan.original_activation)
                    || same_marker(&active, &recover_marker)
            }
            PhaseState::Health => {
                same_marker(&active, &recover_marker) || same_marker(&active, &health_marker)
            }
            PhaseState::Committing => {
                same_marker(&active, &health_marker) || same_marker(&active, &committed_marker)
            }
            PhaseState::Committed => same_marker(&active, &committed_marker),
            PhaseState::RollingBack => [
                &plan.original_activation,
                &recover_marker,
                &health_marker,
                &rollback_marker,
            ]
            .iter()
            .any(|expected| same_marker(&active, expected)),
            PhaseState::RolledBack => same_marker(&active, &rollback_marker),
        };
        if !valid {
            return Err("restore_activation_changed");
        }
    } else if hash(&marker_bytes) != plan.activation_revision {
        return Err("restore_review_stale");
    }
    let result = |state| StageResult {
        state,
        operation_id: Some(id.into()),
        checkpoint: Some(plan.source.clone()),
        source_sha: payload.source_sha.clone(),
        suite_version: payload.suite_version.clone(),
        payload_revision: revision.clone(),
    };
    if progress
        .as_ref()
        .is_some_and(|value| matches!(value.phase, PhaseState::Committed | PhaseState::RolledBack))
    {
        // Finish an interrupted removal only for this exact, terminal operation.
        release(&block_path, &claim_bytes)?;
        release(&claim_path, &claim_bytes)?;
        return Ok(result(
            if progress.unwrap().phase == PhaseState::Committed {
                "dataRestoreCommitted"
            } else {
                "dataRestoreRolledBack"
            },
        ));
    }
    if commit
        && !progress
            .as_ref()
            .is_some_and(|value| matches!(value.phase, PhaseState::Health | PhaseState::Committing))
    {
        return Err("bootstrap_health_required");
    }
    if !rollback
        && !commit
        && progress.as_ref().is_some_and(|value| {
            matches!(
                value.phase,
                PhaseState::Committing | PhaseState::RollingBack
            )
        })
    {
        return Err("restore_resume_selected_action");
    }
    if rollback
        && progress
            .as_ref()
            .is_some_and(|value| value.phase == PhaseState::Committing)
    {
        return Err("restore_commit_already_started");
    }
    if progress.is_none() {
        data_checkpoint::matches_quiesced_sources(
            &sources,
            &backup,
            &plan.preserved,
            &key,
            &plan.generation,
            &AtomicBool::new(false),
        )?;
        data_checkpoint::verify(
            &operation.join("prepared"),
            &plan.source,
            &key,
            &plan.generation,
            &AtomicBool::new(false),
        )?;
        claim(&claim_path, &claim_bytes)?;
        claim(&block_path, &claim_bytes)?;
        let value = Progress {
            plan_revision: plan_revision.clone(),
            phase: PhaseState::Applying,
        };
        persist(&progress_path, &value)?;
        progress = Some(value);
    } else if read(&claim_path, 4096)? != claim_bytes {
        return Err("restore_operation_conflict");
    }
    let mut progress = progress.ok_or("restore_record_invalid")?;
    if rollback {
        claim(&block_path, &claim_bytes)?;
        progress.phase = PhaseState::RollingBack;
        persist(&progress_path, &progress)?;
        persist(&marker_path, &recover_marker)?;
        create_directory(&displaced)?;
        create_directory(&retained)?;
        for (owner, namespace) in &plan.namespaces {
            let live = &sources[owner];
            let old = displaced.join(owner);
            let staged = prepared.join(owner);
            let saved = retained.join(owner);
            for _ in 0..3 {
                match namespace.rollback(
                    directory_identity(live)?,
                    directory_identity(&old)?,
                    directory_identity(&staged)?,
                    directory_identity(&saved)?,
                )? {
                    Step::Preserve => move_directory(
                        live,
                        &saved,
                        namespace.prepared.ok_or("restore_plan_invalid")?,
                    )?,
                    Step::Publish => move_directory(
                        &old,
                        live,
                        namespace.original.ok_or("restore_plan_invalid")?,
                    )?,
                    Step::Complete => break,
                }
            }
        }
        data_checkpoint::matches_quiesced_sources(
            &sources,
            &backup,
            &plan.preserved,
            &key,
            &plan.generation,
            &AtomicBool::new(false),
        )?;
        persist(&marker_path, &rollback_marker)?;
        progress.phase = PhaseState::RolledBack;
        persist(&progress_path, &progress)?;
        release(&block_path, &claim_bytes)?;
        release(&claim_path, &claim_bytes)?;
        return Ok(result("dataRestoreRolledBack"));
    }
    if progress.phase == PhaseState::Applying {
        claim(&block_path, &claim_bytes)?;
        persist(&marker_path, &recover_marker)?;
        create_directory(&displaced)?;
        // Verify both complete logical trees even after some directories moved.
        let mut originals = BTreeMap::new();
        let mut replacements = BTreeMap::new();
        for (owner, namespace) in &plan.namespaces {
            let live = &sources[owner];
            let old = displaced.join(owner);
            let staged = prepared.join(owner);
            namespace.apply(
                directory_identity(live)?,
                directory_identity(&old)?,
                directory_identity(&staged)?,
            )?;
            originals.insert(
                owner.clone(),
                if namespace.original.is_some() && directory_identity(&old)? == namespace.original {
                    old
                } else if namespace.original.is_some() {
                    live.clone()
                } else {
                    displaced.join(owner)
                },
            );
            replacements.insert(
                owner.clone(),
                if namespace.prepared.is_some() && directory_identity(live)? == namespace.prepared {
                    live.clone()
                } else {
                    staged
                },
            );
        }
        data_checkpoint::matches_quiesced_sources(
            &originals,
            &backup,
            &plan.preserved,
            &key,
            &plan.generation,
            &AtomicBool::new(false),
        )?;
        data_checkpoint::matches_quiesced_sources(
            &replacements,
            &backup,
            &plan.source,
            &key,
            &plan.generation,
            &AtomicBool::new(false),
        )?;
        for (owner, namespace) in &plan.namespaces {
            let live = &sources[owner];
            let old = displaced.join(owner);
            let staged = prepared.join(owner);
            for _ in 0..3 {
                match namespace.apply(
                    directory_identity(live)?,
                    directory_identity(&old)?,
                    directory_identity(&staged)?,
                )? {
                    Step::Preserve => move_directory(
                        live,
                        &old,
                        namespace.original.ok_or("restore_plan_invalid")?,
                    )?,
                    Step::Publish => move_directory(
                        &staged,
                        live,
                        namespace.prepared.ok_or("restore_plan_invalid")?,
                    )?,
                    Step::Complete => break,
                }
            }
        }
        // Record completion before changing Control Center's restored journal;
        // a restart no longer compares that intentionally changed file to a snapshot.
        progress.phase = PhaseState::Health;
        persist(&progress_path, &progress)?;
    }
    if progress.phase == PhaseState::Health {
        let store = Store::open(&sources["control-center"])?;
        let (journal, digest) = store.read()?.ok_or("bootstrap_restore_journal_missing")?;
        if same_marker(&active, &recover_marker) || same_marker(&active, &plan.original_activation)
        {
            if journal != plan.health_journal {
                store.write(Some(&digest), &plan.health_journal)?;
            }
        } else if journal.candidate != manifest
            || journal.operation_id != owner.operation_id
            || journal.installation_key != key
            || !matches!(journal.phase, Phase::Health | Phase::Commit)
        {
            return Err("bootstrap_restore_journal_changed");
        }
        persist(&marker_path, &health_marker)?;
        release(&block_path, &claim_bytes)?;
        if !commit {
            return Ok(result("dataRestoreHealthRequired"));
        }
        let (journal, _) = store.read()?.ok_or("bootstrap_restore_journal_missing")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "bootstrap_clock_invalid")?
            .as_millis() as u64;
        journal.require_recent_health(now)?;
        claim(&block_path, &claim_bytes)?;
        progress.phase = PhaseState::Committing;
        persist(&progress_path, &progress)?;
    }
    claim(&block_path, &claim_bytes)?;
    let store = Store::open(&sources["control-center"])?;
    let (mut journal, mut digest) = store.read()?.ok_or("bootstrap_restore_journal_missing")?;
    if journal.candidate != manifest
        || journal.operation_id != owner.operation_id
        || journal.installation_key != key
    {
        return Err("bootstrap_restore_journal_changed");
    }
    while matches!(journal.phase, Phase::Health | Phase::Commit) {
        journal.advance(
            journal.revision,
            Proof {
                phase: journal.phase,
                generation: plan.generation.clone(),
                revision: plan_revision.clone(),
            },
        )?;
        digest = store.write(Some(&digest), &journal)?;
    }
    if !journal.committed {
        return Err("bootstrap_health_required");
    }
    persist(&marker_path, &committed_marker)?;
    progress.phase = PhaseState::Committed;
    persist(&progress_path, &progress)?;
    release(&block_path, &claim_bytes)?;
    release(&claim_path, &claim_bytes)?;
    Ok(result("dataRestoreCommitted"))
}
