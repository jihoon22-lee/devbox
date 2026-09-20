use super::*;
use crate::core::{cutover::Plan, delivery::Journal, legacy_sources};
#[cfg(windows)]
use crate::core::{cutover::Request, delivery_store::Store};
const FILE: &str = "suite-cutover.json";
pub(super) fn return_to_import(
    root: &Path,
    payload_path: &Path,
    image: &Path,
) -> Result<StageResult> {
    use crate::core::{
        data_checkpoint,
        delivery::{Journal, Phase},
        delivery_store::Store,
    };
    use product_contract::activation::{Activation, Phase as ActivePhase};
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    let _gate = writer_gate(&root, false)?;
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let owner: InstallOwner = serde_json::from_slice(&read(&root.join("suite-owner.json"), 4096)?)
        .map_err(|_| "bootstrap_owner_invalid")?;
    let identity = filesystem_identity(&root, true)
        .map_err(|_| "bootstrap_root_changed")?
        .components();
    if owner.root_identity != identity || owner.payload_revision != revision {
        return Err("bootstrap_owner_changed");
    }
    let manifest = product_contract::installation::Manifest::parse(
        &read(&root.join("devbox-installation.json"), 64 * 1024)?,
        &payload.suite_version,
    )?;
    let mut marker: Activation =
        serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
            .map_err(|_| "bootstrap_marker_invalid")?;
    marker.validate(&manifest)?;
    if marker.operation_id != owner.operation_id
        || manifest.installation_id != owner.installation_id
        || manifest.generation != owner.generation
    {
        return Err("bootstrap_owner_changed");
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
    let (mut journal, digest): (Journal, String) =
        store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.candidate != manifest
        || journal.installation_key != key
        || journal.operation_id != owner.operation_id
        || journal.committed
        || journal.previous.is_some()
        || !matches!(
            journal.phase,
            Phase::Snapshot
                | Phase::Import
                | Phase::Validate
                | Phase::Quiesce
                | Phase::Activate
                | Phase::Health
        )
        || !matches!(marker.phase, ActivePhase::Import | ActivePhase::Health)
    {
        return Err("cutover_phase_invalid");
    }
    if journal.phase != Phase::Snapshot || !journal.owner_evidence.is_empty() {
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
            &manifest.generation,
            &AtomicBool::new(false),
        )?;
        journal.data_checkpoints.push(checkpoint);
        journal.phase = Phase::Snapshot;
        journal.health_checks.clear();
        journal.owner_evidence.clear();
        journal.revision = journal
            .revision
            .checked_add(1)
            .ok_or("bootstrap_revision_exhausted")?;
        journal.validate()?;
        store.write(Some(&digest), &journal)?;
    }
    marker.phase = ActivePhase::Import;
    marker.revision = journal.revision;
    devbox_filesystem::atomic_write(
        root.join("devbox-activation.json"),
        &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
    )
    .map_err(|_| "bootstrap_marker_write_failed")?;
    Ok(StageResult {
        state: "importReviewRequired",
        operation_id: None,
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
pub(super) fn plan(data: &Path, journal: &Journal) -> Result<Plan> {
    let plan: Plan = serde_json::from_slice(&read(&data.join(FILE), 1024 * 1024)?)
        .map_err(|_| "cutover_plan_invalid")?;
    plan.validate(journal)?;
    Ok(plan)
}
#[cfg(windows)]
pub(crate) fn review(request: Option<Request>) -> Result<serde_json::Value> {
    let scope = crate::suite::capture_own("control-center")?;
    let base = dirs::data_local_dir().ok_or("bootstrap_data_unavailable")?;
    let data = base.join(format!(
        "com.devbox.v08.controlcenter.i{}",
        scope.installation_key
    ));
    let store = Store::open(&data)?;
    let (journal, _) = store.read()?.ok_or("bootstrap_journal_missing")?;
    if journal.installation_key != scope.installation_key || journal.candidate != scope.manifest {
        return Err("cutover_owner_changed");
    }
    let mut review = crate::core::cutover::review(&journal)?;
    if let Some(request) = request {
        if !matches!(
            journal.phase,
            crate::core::delivery::Phase::Snapshot
                | crate::core::delivery::Phase::Import
                | crate::core::delivery::Phase::Validate
        ) {
            return Err("cutover_phase_invalid");
        }
        let plan = crate::core::cutover::prepare(&journal, request)?;
        let ids = plan
            .namespaces
            .iter()
            .map(|row| row.identifier.as_str())
            .collect::<Vec<_>>();
        if legacy_sources::capture(&base, &ids, false)?.namespaces != plan.namespaces {
            return Err("cutover_source_changed");
        }
        scope.revalidate()?;
        devbox_filesystem::atomic_write(
            data.join(FILE),
            &serde_json::to_vec(&plan).map_err(|_| "cutover_plan_invalid")?,
        )
        .map_err(|_| "cutover_plan_unavailable")?;
    }
    if let Ok(plan) = plan(&data, &journal) {
        review.prepared = true;
        review.choices = plan.choices;
    }
    serde_json::to_value(review).map_err(|_| "cutover_plan_invalid")
}
pub(super) fn hold(data: &Path, journal: &Journal) -> Result<legacy_sources::Guard> {
    let plan = plan(data, journal)?;
    let base = data.parent().ok_or("bootstrap_data_unavailable")?;
    require_closed()?;
    let ids = plan
        .namespaces
        .iter()
        .map(|row| row.identifier.as_str())
        .collect::<Vec<_>>();
    let mut guard = legacy_sources::capture(base, &ids, true)?;
    if guard.namespaces != plan.namespaces {
        return Err("cutover_source_changed");
    }
    guard.revalidate()?;
    require_closed()?;
    Ok(guard)
}
#[cfg(windows)]
pub(crate) fn require_closed() -> Result<()> {
    use windows::Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
    };
    let catalog = devbox_catalog::parse_catalog(include_str!("../../../../legacy-v0.7-catalog.json"))
        .map_err(|_| "bootstrap_catalog_invalid")?;
    let names = catalog
        .apps
        .iter()
        .filter(|app| !app.identifier.starts_with("com.devbox.v08."))
        .map(|app| format!("{}.exe", app.id))
        .collect::<std::collections::BTreeSet<_>>();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|_| "legacy_process_inventory_unavailable")?;
    struct Snapshot(windows::Win32::Foundation::HANDLE);
    impl Drop for Snapshot {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let snapshot = Snapshot(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe { Process32FirstW(snapshot.0, &mut entry) }
        .map_err(|_| "legacy_process_inventory_unavailable")?;
    for _ in 0..32768 {
        let length = entry
            .szExeFile
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase();
        if names.contains(&name) {
            return Err("legacy_writers_must_close");
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            return if unsafe { GetLastError() } == ERROR_NO_MORE_FILES {
                Ok(())
            } else {
                Err("legacy_process_inventory_unavailable")
            };
        }
    }
    Err("legacy_process_inventory_limit")
}
#[cfg(not(windows))]
pub(crate) fn require_closed() -> Result<()> {
    Err("suite_windows_required")
}
