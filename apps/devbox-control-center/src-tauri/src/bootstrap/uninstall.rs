//! Package-only uninstall. The NSIS owner runs the verified helper from its
//! private temporary directory, never from a tree being removed.
use super::*;
use crate::core::suite_removal::Plan;

#[derive(serde::Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    payload_revision: String,
    plan: Plan,
}
pub(super) fn remove(root: &Path, payload_path: &Path, image: &Path) -> Result<StageResult> {
    let bytes = read(payload_path, MAX_RELEASE_BYTES as u64)?;
    let payload = Payload::parse(&bytes)?;
    verify_payload_owner(&payload, image)?;
    let revision = hash(&bytes);
    let root = devbox_manager_lib::core::custom_root::verify_suite_directory(root)
        .map_err(|_| "bootstrap_root_unsafe")?;
    // A running executable cannot remove itself. The installer/uninstaller must
    // retain its independently verified inputs outside the owned package root.
    if image
        .canonicalize()
        .map_err(|_| "bootstrap_identity_unavailable")?
        .starts_with(&root)
        || payload_path
            .canonicalize()
            .map_err(|_| "bootstrap_input_unavailable")?
            .starts_with(&root)
    {
        return Err("bootstrap_uninstall_external_helper_required");
    }
    let (_root, identity) =
        open_filesystem_object(&root, true).map_err(|_| "bootstrap_root_unavailable")?;
    #[cfg(windows)]
    let _root_pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let _gate = writer_gate_for_restore(&root, false)?;
    if !matches!(fs::symlink_metadata(root.join("suite-data-restore.json")), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return Err("bootstrap_data_restore_pending");
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
    let key = hash(
        &serde_json::to_vec(&(identity.components(), &owner.installation_id))
            .map_err(|_| "bootstrap_owner_invalid")?,
    );
    #[cfg(windows)]
    super::registration::remove(&root, &key, false)?;
    let plan_path = root.join("uninstall-plan.json");
    let receipt = match fs::symlink_metadata(&plan_path) {
        Ok(_) => serde_json::from_slice::<Receipt>(&read(&plan_path, 2 * 1024 * 1024)?)
            .map_err(|_| "suite_remove_plan_invalid")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let manifest = product_contract::installation::Manifest::parse(
                &read(&root.join("devbox-installation.json"), 64 * 1024)?,
                &payload.suite_version,
            )?;
            let mut marker: product_contract::activation::Activation =
                serde_json::from_slice(&read(&root.join("devbox-activation.json"), 4096)?)
                    .map_err(|_| "bootstrap_marker_invalid")?;
            marker.validate(&manifest)?;
            if manifest.installation_id != owner.installation_id
                || manifest.generation != owner.generation
                || marker.operation_id != owner.operation_id
            {
                return Err("bootstrap_owner_changed");
            }
            let mut files = Vec::new();
            let mut revisions = std::collections::BTreeSet::new();
            let generations = root.join("generations");
            ensure_no_links(&generations).map_err(|_| "bootstrap_stage_unsafe")?;
            let entries = fs::read_dir(&generations)
                .map_err(|_| "bootstrap_stage_unavailable")?
                .take(33)
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|_| "bootstrap_stage_unavailable")?;
            if entries.is_empty() || entries.len() > 32 {
                return Err("bootstrap_generation_review_required");
            }
            for entry in entries {
                let generation = entry
                    .file_name()
                    .to_str()
                    .ok_or("bootstrap_owner_invalid")?
                    .to_owned();
                if !generation.strip_prefix("g-").is_some_and(|id| {
                    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
                }) {
                    return Err("bootstrap_generation_review_required");
                }
                let path = entry.path();
                ensure_no_links(&path).map_err(|_| "bootstrap_stage_unsafe")?;
                let stage_owner: serde_json::Value =
                    serde_json::from_slice(&read(&path.join("stage-owner.json"), 4096)?)
                        .map_err(|_| "bootstrap_owner_invalid")?;
                let stage_revision = stage_owner["payloadRevision"]
                    .as_str()
                    .filter(|value| product_contract::commands::revision(value))
                    .ok_or("bootstrap_owner_invalid")?;
                let actual = filesystem_identity(&path, true)
                    .map_err(|_| "bootstrap_stage_unsafe")?
                    .components();
                if stage_owner["schemaVersion"] != 1
                    || stage_owner["rootIdentity"] != serde_json::json!(actual)
                {
                    return Err("bootstrap_owner_changed");
                }
                let cached = root.join("setup").join(stage_revision);
                let cached_bytes =
                    read(&cached.join("suite-payload.json"), MAX_RELEASE_BYTES as u64)?;
                if hash(&cached_bytes) != stage_revision {
                    return Err("bootstrap_payload_changed");
                }
                let generation_payload = Payload::parse(&cached_bytes)?;
                for product in &generation_payload.products {
                    let prefix = format!("generations/{generation}/products/{}", product.id);
                    exact_closure(&root.join(&prefix), product)?;
                    for file in product
                        .files
                        .iter()
                        .filter(|file| file.name != "devbox-installation.json")
                    {
                        verified_file(&root.join(&prefix).join(&file.name), file)?;
                        files.push(format!("{prefix}/{}", file.name));
                    }
                }
                files.push(format!("generations/{generation}/stage-owner.json"));
                files.push(format!("generations/{generation}/stage-receipt.json"));
                if revisions.insert(stage_revision.to_owned()) {
                    for product in &generation_payload.products {
                        verified_file(&cached.join(&product.portable.name), &product.portable)?;
                        files.push(format!("setup/{stage_revision}/{}", product.portable.name));
                    }
                    verify_payload_owner(
                        &generation_payload,
                        &cached.join("devbox-suite-bootstrap.exe"),
                    )?;
                    files.push(format!("setup/{stage_revision}/devbox-suite-bootstrap.exe"));
                    files.push(format!("setup/{stage_revision}/suite-payload.json"));
                }
            }
            if !revisions.contains(&revision) {
                return Err("bootstrap_payload_changed");
            }
            if hash(&read(
                &root.join("suite-payload.json"),
                MAX_RELEASE_BYTES as u64,
            )?) != revision
            {
                return Err("bootstrap_payload_changed");
            }
            // Native proof acquisition is complete. Block ordinary use before
            // persisting a deletion plan; a crash retains every original store.
            marker.phase = product_contract::activation::Phase::Recover;
            marker.revision = marker
                .revision
                .checked_add(1)
                .ok_or("bootstrap_revision_exhausted")?;
            devbox_filesystem::atomic_write(
                root.join("devbox-activation.json"),
                &serde_json::to_vec(&marker).map_err(|_| "bootstrap_marker_invalid")?,
            )
            .map_err(|_| "bootstrap_marker_write_failed")?;
            files.extend([
                "suite-payload.json".into(),
                "devbox-installation.json".into(),
                "devbox-activation.json".into(),
            ]);
            let receipt = Receipt {
                schema_version: 1,
                payload_revision: revision.clone(),
                plan: Plan::capture(&root, &key, &files)?,
            };
            devbox_filesystem::atomic_write(
                &plan_path,
                &serde_json::to_vec(&receipt).map_err(|_| "suite_remove_plan_invalid")?,
            )
            .map_err(|_| "suite_remove_plan_unavailable")?;
            receipt
        }
        Err(_) => return Err("suite_remove_plan_unavailable"),
    };
    if receipt.schema_version != 1
        || receipt.payload_revision != revision
        || receipt.plan.installation_key != key
        || receipt.plan.root_identity != identity.components()
    {
        return Err("suite_remove_plan_invalid");
    }
    receipt.plan.remove(&root)?;
    #[cfg(windows)]
    super::registration::remove(&root, &key, true)?;
    // Retain the small ownership/removal records for a failed shortcut or ARP
    // cleanup to resume. NSIS removes only its own remaining registration files.
    Ok(StageResult {
        state: "packagesRemovedDataPreserved",
        operation_id: None,
        checkpoint: None,
        source_sha: payload.source_sha,
        suite_version: payload.suite_version,
        payload_revision: revision,
    })
}
