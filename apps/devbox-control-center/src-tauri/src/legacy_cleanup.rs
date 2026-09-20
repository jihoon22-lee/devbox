//! Explicit, postcommit package cleanup. Private plans retain exact identities;
//! renderer requests contain only catalog/registration IDs or an operation UUID.
use crate::core::{
    delivery::{Journal, Phase},
    delivery_store::Store,
    suite_removal::Plan,
};
use devbox_filesystem::{ensure_no_links, filesystem_identity};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Target {
    Installer {
        registration: crate::legacy_installer::Registration,
        root: PathBuf,
        files: Plan,
        shortcuts: Vec<(PathBuf, Plan)>,
    },
    Portable {
        request: Value,
        version: String,
        target: PathBuf,
        identity: (u64, u64),
        target_identity: Option<(u64, u64)>,
        files: Option<Plan>,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cleanup {
    schema_version: u32,
    id: String,
    installation_key: String,
    app: String,
    version: String,
    state: String,
    issue: Option<String>,
    target: Target,
}
fn view(plan: &Cleanup) -> Value {
    json!({"id":plan.id,"app":plan.app,"version":plan.version,"state":plan.state,"issue":plan.issue,"preservesUserData":true})
}
fn read(path: &Path) -> Result<Cleanup> {
    use std::io::Read;
    ensure_no_links(path).map_err(|_| "legacy_cleanup_unsafe")?;
    let mut bytes = vec![];
    std::fs::File::open(path)
        .map_err(|_| "legacy_cleanup_unavailable")?
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "legacy_cleanup_unavailable")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("legacy_cleanup_limit");
    }
    let value: Cleanup = serde_json::from_slice(&bytes).map_err(|_| "legacy_cleanup_invalid")?;
    if value.schema_version != 1
        || !uuid::Uuid::parse_str(&value.id).is_ok_and(|id| id.to_string() == value.id)
        || !product_contract::commands::revision(&value.installation_key)
        || !matches!(
            value.state.as_str(),
            "reviewed" | "cleanupPending" | "complete"
        )
    {
        return Err("legacy_cleanup_invalid");
    }
    Ok(value)
}
fn save(directory: &Path, plan: &Cleanup) -> Result<()> {
    ensure_no_links(directory).map_err(|_| "legacy_cleanup_unsafe")?;
    devbox_filesystem::atomic_write(
        directory.join(format!("{}.json", plan.id)),
        &serde_json::to_vec(plan).map_err(|_| "legacy_cleanup_invalid")?,
    )
    .map_err(|_| "legacy_cleanup_unavailable")
}
fn acquire_lock(directory: &Path) -> Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    let path = directory.join("owner.lock");
    let open = |create| {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(create)
            .share_mode(3)
            .custom_flags(0x0020_0000)
            .open(&path)
    };
    let lock = match open(true) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_no_links(&path).map_err(|_| "legacy_cleanup_unsafe")?;
            open(false).map_err(|_| "legacy_cleanup_busy")?
        }
        Err(_) => return Err("legacy_cleanup_busy"),
    };
    let identity = devbox_filesystem::opened_filesystem_identity(&lock, false)
        .map_err(|_| "legacy_cleanup_unsafe")?;
    if lock.metadata().map_err(|_| "legacy_cleanup_unsafe")?.len() != 0
        || filesystem_identity(&path, false).map_err(|_| "legacy_cleanup_unsafe")? != identity
    {
        return Err("legacy_cleanup_unsafe");
    }
    if !devbox_filesystem::try_lock_exclusive(&lock).map_err(|_| "legacy_cleanup_busy")? {
        return Err("legacy_cleanup_busy");
    }
    Ok(lock)
}
fn journal(
    app: &tauri::AppHandle,
) -> Result<(
    crate::suite::platform::component_scope::CapturedScope,
    PathBuf,
    Journal,
)> {
    product_shell_tauri::require_suite_writable(app)?;
    let scope = crate::suite::capture_own("control-center")?;
    let data = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "legacy_cleanup_unavailable")?;
    let (journal, _) = Store::inspect(&data)?.ok_or("suite_journal_missing")?;
    if !journal.committed
        || journal.installation_key != scope.installation_key
        || journal.candidate != scope.manifest
    {
        return Err("legacy_cleanup_commit_required");
    }
    Ok((scope, data, journal))
}
fn mark(data: &Path, key: &str, id: &str, pending: bool) -> Result<()> {
    let store = Store::open(data)?;
    let (mut journal, digest) = store.read()?.ok_or("suite_journal_missing")?;
    if !journal.committed || journal.installation_key != key {
        return Err("legacy_cleanup_commit_required");
    }
    let changed = if pending {
        journal.cleanup_pending.insert(id.into())
    } else {
        journal.cleanup_pending.remove(id)
    };
    if changed {
        journal.phase = if journal.cleanup_pending.is_empty() {
            Phase::Complete
        } else {
            Phase::Cleanup
        };
        journal.revision = journal
            .revision
            .checked_add(1)
            .ok_or("suite_revision_exhausted")?;
        journal.validate()?;
        store.write(Some(&digest), &journal)?;
    }
    Ok(())
}
fn shortcut_candidates(
    registration: &crate::legacy_installer::Registration,
) -> Result<Vec<PathBuf>> {
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{
            FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
            SHGetKnownFolderPath, KF_FLAG_DEFAULT,
        },
    };
    let catalog = devbox_catalog::parse_catalog(include_str!("../../../legacy-v0.7-catalog.json"))
        .map_err(|_| "legacy_catalog_invalid")?;
    let app = catalog
        .apps
        .iter()
        .find(|app| app.id == registration.app)
        .ok_or("legacy_cleanup_invalid")?;
    let mut result = vec![];
    let folders = if registration.machine() {
        [FOLDERID_CommonPrograms, FOLDERID_PublicDesktop]
    } else {
        [FOLDERID_Programs, FOLDERID_Desktop]
    };
    for (index, folder) in folders.iter().enumerate() {
        let raw = unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None) }
            .map_err(|_| "legacy_shortcuts_unavailable")?;
        let value = unsafe { raw.to_string() };
        unsafe {
            CoTaskMemFree(Some(raw.0.cast()));
        }
        let root = PathBuf::from(value.map_err(|_| "legacy_shortcuts_unavailable")?);
        result.push(root.join(format!("{}.lnk", app.product_name)));
        if index == 0 {
            result.push(
                root.join(&app.product_name)
                    .join(format!("{}.lnk", app.product_name)),
            );
        }
    }
    Ok(result)
}
pub(crate) fn execute(app: &tauri::AppHandle, method: &str, args: Value) -> Result<Value> {
    let (scope, data, _) = journal(app)?;
    let _data_pins = crate::suite::platform::component_scope::pin_directories(&data)?;
    let directory = data.join("legacy-cleanup-v1");
    if method == "legacy_cleanup_list" {
        if !directory
            .try_exists()
            .map_err(|_| "legacy_cleanup_unavailable")?
        {
            return Ok(json!([]));
        }
        ensure_no_links(&directory).map_err(|_| "legacy_cleanup_unsafe")?;
        let mut rows = vec![];
        for (index, entry) in fs::read_dir(&directory)
            .map_err(|_| "legacy_cleanup_unavailable")?
            .enumerate()
        {
            if index >= 64 {
                return Err("legacy_cleanup_limit");
            }
            let entry = entry.map_err(|_| "legacy_cleanup_unavailable")?;
            if entry.file_name() == "owner.lock" {
                continue;
            }
            let plan = read(&entry.path())?;
            if plan.installation_key != scope.installation_key {
                return Err("legacy_cleanup_foreign");
            }
            rows.push(view(&plan));
        }
        return Ok(json!(rows));
    }
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("legacy_cleanup_unavailable"),
    }
    ensure_no_links(&directory).map_err(|_| "legacy_cleanup_unsafe")?;
    let _directory_pins = crate::suite::platform::component_scope::pin_directories(&directory)?;
    let _lock = acquire_lock(&directory)?;
    if method == "legacy_cleanup_preview" {
        if fs::read_dir(&directory)
            .map_err(|_| "legacy_cleanup_unavailable")?
            .take(64)
            .count()
            >= 64
        {
            return Err("legacy_cleanup_retention_review_required");
        }
        let id = args["id"].as_str().ok_or("legacy_cleanup_invalid")?;
        let (app_id, version, target) = if args["kind"] == "installer" {
            let registration = crate::legacy_installer::resolve(id)?;
            let root = devbox_manager_lib::core::custom_root::verify_suite_directory(Path::new(
                registration
                    .location
                    .as_deref()
                    .ok_or("legacy_cleanup_invalid")?
                    .trim_matches('"'),
            ))
            .map_err(|_| "legacy_cleanup_unsafe")?;
            let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
            let version = registration
                .version
                .clone()
                .ok_or("legacy_cleanup_invalid")?;
            let reference =
                crate::core::legacy_installation::reference(&registration.app, &version)?;
            let _verified =
                crate::core::legacy_installation::verify(&root, &registration.app, &version)?;
            let names = reference
                .files
                .iter()
                .map(|file| file.name.clone())
                .collect::<Vec<_>>();
            let files = Plan::capture(&root, &scope.installation_key, &names)?;
            let mut shortcuts = vec![];
            use windows::Win32::System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
            };
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .map_err(|_| "legacy_shortcuts_unavailable")?;
            struct Com;
            impl Drop for Com {
                fn drop(&mut self) {
                    unsafe { CoUninitialize() };
                }
            }
            let _com = Com;
            for path in shortcut_candidates(&registration)? {
                if !path
                    .try_exists()
                    .map_err(|_| "legacy_shortcuts_unavailable")?
                {
                    continue;
                }
                if crate::bootstrap::registration::verify_link(
                    &path,
                    &root.join(format!("{}.exe", registration.app)),
                    "",
                )
                .is_err()
                {
                    continue;
                }
                let parent = path
                    .parent()
                    .ok_or("legacy_shortcuts_unavailable")?
                    .to_owned();
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("legacy_shortcuts_unavailable")?
                    .to_owned();
                shortcuts.push((
                    parent.clone(),
                    Plan::capture(&parent, &scope.installation_key, &[name])?,
                ));
            }
            (
                registration.app.clone(),
                version,
                Target::Installer {
                    registration,
                    root,
                    files,
                    shortcuts,
                },
            )
        } else if args["kind"] == "portable" {
            let preview =
                devbox_manager_lib::component::preview_legacy_portable(app.clone(), id.into())
                    .map_err(|_| "legacy_portable_review_required")?;
            if !preview.can_remove || preview.mode != "portable" {
                return Err("legacy_portable_review_required");
            }
            let target = PathBuf::from(
                preview
                    .target_path
                    .ok_or("legacy_portable_review_required")?,
            );
            let identity =
                filesystem_identity(target.parent().ok_or("legacy_cleanup_unsafe")?, true)
                    .map_err(|_| "legacy_cleanup_unsafe")?
                    .components();
            let (target_identity, files) = match fs::symlink_metadata(&target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
                Err(_) => return Err("legacy_cleanup_unavailable"),
                Ok(_) => {
                    let names = portable_files(&target)?;
                    let identity = filesystem_identity(&target, true)
                        .map_err(|_| "legacy_cleanup_unsafe")?
                        .components();
                    let plan = if names.is_empty() {
                        None
                    } else {
                        Some(Plan::capture(&target, &scope.installation_key, &names)?)
                    };
                    (Some(identity), plan)
                }
            };
            let request = json!({"appId":preview.app_id,"expectedRegistryRevision":preview.registry_revision,"expectedCatalogRevision":preview.catalog_revision,"expectedRootId":preview.root_id,"expectedManifestDigest":preview.manifest_digest});
            (
                preview.app_id,
                preview.version.clone(),
                Target::Portable {
                    request,
                    version: preview.version,
                    target,
                    identity,
                    target_identity,
                    files,
                },
            )
        } else {
            return Err("legacy_cleanup_invalid");
        };
        scope.revalidate()?;
        let plan = Cleanup {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            installation_key: scope.installation_key.clone(),
            app: app_id,
            version,
            state: "reviewed".into(),
            issue: None,
            target,
        };
        save(&directory, &plan)?;
        return Ok(view(&plan));
    }
    if method != "legacy_cleanup_apply" {
        return Err("legacy_cleanup_invalid");
    }
    let id = args["id"]
        .as_str()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == *id))
        .ok_or("legacy_cleanup_invalid")?;
    let mut plan = read(&directory.join(format!("{id}.json")))?;
    if plan.id != id || plan.installation_key != scope.installation_key {
        return Err("legacy_cleanup_foreign");
    }
    if plan.state == "complete" {
        mark(&data, &scope.installation_key, id, false)?;
        return Ok(view(&plan));
    }
    crate::bootstrap::cutover::require_closed()?;
    scope.revalidate()?;
    plan.state = "cleanupPending".into();
    plan.issue = None;
    save(&directory, &plan)?;
    mark(&data, &scope.installation_key, id, true)?;
    let result = apply(app, &plan);
    match result {
        Ok(()) => plan.state = "complete".into(),
        Err(issue) => plan.issue = Some(issue.into()),
    }
    save(&directory, &plan)?;
    if plan.state == "complete" {
        mark(&data, &scope.installation_key, id, false)?;
    }
    Ok(view(&plan))
}
fn portable_files(root: &Path) -> Result<Vec<String>> {
    let mut pending = vec![root.to_owned()];
    let mut files = vec![];
    let mut count = 0;
    while let Some(directory) = pending.pop() {
        ensure_no_links(&directory).map_err(|_| "legacy_cleanup_unsafe")?;
        for entry in fs::read_dir(&directory).map_err(|_| "legacy_cleanup_unavailable")? {
            count += 1;
            if count > 4096 {
                return Err("legacy_cleanup_limit");
            }
            let path = entry.map_err(|_| "legacy_cleanup_unavailable")?.path();
            ensure_no_links(&path).map_err(|_| "legacy_cleanup_unsafe")?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| "legacy_cleanup_unavailable")?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(
                    path.strip_prefix(root)
                        .map_err(|_| "legacy_cleanup_unsafe")?
                        .to_str()
                        .ok_or("legacy_cleanup_unsafe")?
                        .replace('\\', "/"),
                );
            } else {
                return Err("legacy_cleanup_unsafe");
            }
        }
    }
    files.sort();
    Ok(files)
}
fn apply(app: &tauri::AppHandle, plan: &Cleanup) -> Result<()> {
    match &plan.target {
        Target::Installer {
            registration,
            root,
            files,
            shortcuts,
        } => {
            if registration.app != plan.app
                || registration.version.as_ref() != Some(&plan.version)
                || files.installation_key != plan.installation_key
            {
                return Err("legacy_cleanup_invalid");
            }
            let canonical =
                devbox_manager_lib::core::custom_root::verify_suite_directory(Path::new(
                    registration
                        .location
                        .as_deref()
                        .ok_or("legacy_cleanup_invalid")?
                        .trim_matches('"'),
                ))
                .map_err(|_| "legacy_cleanup_unsafe")?;
            if canonical != *root {
                return Err("legacy_cleanup_changed");
            }
            let _pins = crate::suite::platform::component_scope::pin_directories(root)?;
            let reference = crate::core::legacy_installation::reference(&plan.app, &plan.version)?;
            if files.files.len() != reference.files.len()
                || files.files.iter().any(|file| {
                    !reference.files.iter().any(|known| {
                        known.name == file.relative
                            && known.size == file.bytes
                            && known.sha256 == file.sha256
                    })
                })
            {
                return Err("legacy_cleanup_invalid");
            }
            registration.present()?;
            let candidates = shortcut_candidates(registration)?;
            let mut link_pins = vec![];
            for (root, links) in shortcuts {
                if links.installation_key != plan.installation_key
                    || links.files.len() != 1
                    || !candidates.contains(&root.join(&links.files[0].relative))
                {
                    return Err("legacy_cleanup_invalid");
                }
                link_pins.extend(crate::suite::platform::component_scope::pin_directories(
                    root,
                )?);
                links.verify_remaining(root)?;
            }
            files.verify_remaining(root)?;
            for (root, links) in shortcuts {
                links.remove(root)?;
            }
            files.remove(root)?;
            registration.remove()?;
            Ok(())
        }
        Target::Portable {
            request,
            version,
            target,
            identity,
            target_identity,
            files,
        } => {
            if *version != plan.version
                || request["appId"] != plan.app
                || filesystem_identity(target.parent().ok_or("legacy_cleanup_unsafe")?, true)
                    .map_err(|_| "legacy_cleanup_changed")?
                    .components()
                    != *identity
            {
                return Err("legacy_cleanup_changed");
            }
            match fs::symlink_metadata(target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("legacy_cleanup_unavailable"),
                Ok(_) => {
                    if Some(
                        filesystem_identity(target, true)
                            .map_err(|_| "legacy_cleanup_changed")?
                            .components(),
                    ) != *target_identity
                    {
                        return Err("legacy_cleanup_changed");
                    }
                    let names = portable_files(target)?;
                    if names.iter().any(|name| {
                        files.as_ref().is_none_or(|plan| {
                            !plan.files.iter().any(|file| &file.relative == name)
                        })
                    }) {
                        return Err("legacy_cleanup_changed");
                    }
                    if let Some(files) = files {
                        if files.installation_key != plan.installation_key {
                            return Err("legacy_cleanup_invalid");
                        }
                        files.verify_remaining(target)?;
                    }
                }
            }
            devbox_manager_lib::component::cleanup_legacy_portable(
                app.clone(),
                request.clone(),
                version,
                target,
            )
            .map_err(|_| "legacy_portable_cleanup_pending")
        }
    }
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

    #[test]
    fn first_cleanup_creates_its_lock_and_reopen_preserves_exclusivity_and_unknown_bytes() {
        let root =
            Temp(std::env::temp_dir().join(format!("devbox-cleanup-{}", uuid::Uuid::new_v4())));
        fs::create_dir(&root.0).unwrap();
        let _pins = crate::suite::platform::component_scope::pin_directories(&root.0).unwrap();
        let first = acquire_lock(&root.0).unwrap();
        assert!(matches!(acquire_lock(&root.0), Err("legacy_cleanup_busy")));
        drop(first);
        drop(acquire_lock(&root.0).unwrap());
        let path = root.0.join("owner.lock");
        fs::write(&path, b"preserve unexpected data").unwrap();
        assert!(matches!(
            acquire_lock(&root.0),
            Err("legacy_cleanup_unsafe")
        ));
        assert_eq!(fs::read(path).unwrap(), b"preserve unexpected data");
    }
}
