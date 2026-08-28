//! Profile template persistence and the new-project wizard boundary.
//!
//! This module owns a separate, bounded template file.  It deliberately uses
//! the same atomic/CAS discipline as profile CRUD, but templates never carry
//! environment metadata or secret values.  Applying a template is a backend
//! operation so a stale or malformed template cannot become a profile write
//! authority in the renderer.

use crate::commands::workspace::{load_store_document, save_store_document, ProfileStoreState};
use crate::core::profile::{validate_profile_id, ProfileStore, ProjectProfile};
use crate::core::templates::{
    ProfileTemplate, ProfileTemplateStore, MAX_PROFILE_TEMPLATE_FILE_BYTES,
};
use crate::platform::{open_readonly_with_identity, path_identity};
use serde::Deserialize;
use std::fs::Metadata;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

const TEMPLATE_FILE: &str = "profile-templates.json";
const TEMPLATE_READ_ERROR: &str = "프로필 템플릿을 읽을 수 없습니다";
const TEMPLATE_WRITE_ERROR: &str = "프로필 템플릿을 저장할 수 없습니다";
const TEMPLATE_PATH_ERROR: &str = "프로필 템플릿 경로를 확인할 수 없습니다";
const TEMPLATE_CONFLICT_ERROR: &str =
    "프로필 템플릿 저장소가 다른 작업으로 변경되었습니다. 다시 시도하세요";

fn ensure_profile_id(mut profile: ProjectProfile) -> ProjectProfile {
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    profile
}

#[derive(Debug)]
struct TemplateStoreDocument {
    store: ProfileTemplateStore,
    raw: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateProfileFromTemplateRequest {
    #[serde(default)]
    pub template_id: Option<String>,
    pub profile: ProjectProfile,
}

fn template_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| TEMPLATE_PATH_ERROR.to_string())?;
    Ok(directory.join(TEMPLATE_FILE))
}

fn is_link_metadata(metadata: &Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn reject_links_in_existing_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_link_metadata(&metadata) => return Err(TEMPLATE_PATH_ERROR.into()),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(TEMPLATE_PATH_ERROR.into()),
        }
    }
    Ok(())
}

fn read_template_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    reject_links_in_existing_path(path)?;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(TEMPLATE_READ_ERROR.into()),
    };
    if is_link_metadata(&metadata) || !metadata.file_type().is_file() {
        return Err(TEMPLATE_READ_ERROR.into());
    }
    if metadata.len() > MAX_PROFILE_TEMPLATE_FILE_BYTES as u64 {
        return Err("프로필 템플릿 저장소 크기 제한을 초과했습니다".into());
    }
    let source_identity =
        path_identity(path, false).map_err(|_| TEMPLATE_PATH_ERROR.to_string())?;
    let (file, opened_identity) =
        open_readonly_with_identity(path, false).map_err(|_| TEMPLATE_READ_ERROR.to_string())?;
    let opened_metadata = file
        .metadata()
        .map_err(|_| TEMPLATE_READ_ERROR.to_string())?;
    if is_link_metadata(&opened_metadata) || source_identity != opened_identity {
        return Err(TEMPLATE_PATH_ERROR.into());
    }
    let mut reader = file.take((MAX_PROFILE_TEMPLATE_FILE_BYTES + 1) as u64);
    let mut bytes = Vec::with_capacity(metadata.len() as usize + 1);
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let count = reader
            .read(&mut chunk)
            .map_err(|_| TEMPLATE_READ_ERROR.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_PROFILE_TEMPLATE_FILE_BYTES {
            return Err("프로필 템플릿 저장소 크기 제한을 초과했습니다".into());
        }
    }
    reject_links_in_existing_path(path)?;
    let after_identity = path_identity(path, false).map_err(|_| TEMPLATE_PATH_ERROR.to_string())?;
    if source_identity != after_identity {
        return Err(TEMPLATE_PATH_ERROR.into());
    }
    Ok(Some(bytes))
}

fn ensure_template_directory(path: &Path) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| TEMPLATE_PATH_ERROR.to_string())?;
    reject_links_in_existing_path(directory)?;
    match std::fs::symlink_metadata(directory) {
        Ok(metadata) if is_link_metadata(&metadata) || !metadata.file_type().is_dir() => {
            Err(TEMPLATE_PATH_ERROR.into())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            std::fs::create_dir_all(directory).map_err(|_| TEMPLATE_WRITE_ERROR.to_string())?;
            reject_links_in_existing_path(directory)?;
            match std::fs::symlink_metadata(directory) {
                Ok(metadata) if !is_link_metadata(&metadata) && metadata.file_type().is_dir() => {
                    Ok(())
                }
                _ => Err(TEMPLATE_PATH_ERROR.into()),
            }
        }
        Err(_) => Err(TEMPLATE_PATH_ERROR.into()),
    }
}

fn load_template_document_at_path(path: &Path) -> Result<TemplateStoreDocument, String> {
    let Some(bytes) = read_template_file(path)? else {
        return Ok(TemplateStoreDocument {
            store: ProfileTemplateStore::empty(),
            raw: None,
        });
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| TEMPLATE_READ_ERROR.to_string())?;
    let store = ProfileTemplateStore::load(text).map_err(|_| TEMPLATE_READ_ERROR.to_string())?;
    Ok(TemplateStoreDocument {
        store,
        raw: Some(bytes),
    })
}

fn load_template_document(app: &AppHandle) -> Result<TemplateStoreDocument, String> {
    load_template_document_at_path(&template_path(app)?)
}

fn save_template_document(
    app: &AppHandle,
    expected: &TemplateStoreDocument,
    store: &ProfileTemplateStore,
) -> Result<(), String> {
    let json = store.to_json_checked()?;
    let path = template_path(app)?;
    let current = read_template_file(&path)?;
    if expected.raw.as_deref() != current.as_deref() {
        return Err(TEMPLATE_CONFLICT_ERROR.into());
    }
    ensure_template_directory(&path)?;
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if is_link_metadata(&metadata) => return Err(TEMPLATE_PATH_ERROR.into()),
        Ok(metadata) if !metadata.file_type().is_file() => return Err(TEMPLATE_PATH_ERROR.into()),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err(TEMPLATE_PATH_ERROR.into()),
    }
    devbox_filesystem::atomic_write(path, json.as_bytes())
        .map_err(|_| TEMPLATE_WRITE_ERROR.to_string())
}

#[tauri::command]
pub fn list_profile_templates(app: AppHandle) -> Result<Vec<ProfileTemplate>, String> {
    Ok(load_template_document(&app)?.store.templates)
}

#[tauri::command]
pub fn create_profile_template(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    mut template: ProfileTemplate,
) -> Result<ProfileTemplate, String> {
    let _lock = store_state
        .lock
        .lock()
        .map_err(|_| TEMPLATE_WRITE_ERROR.to_string())?;
    if template.id.is_empty() {
        template.id = uuid::Uuid::new_v4().to_string();
    }
    let document = load_template_document(&app)?;
    let mut store = document.store.clone();
    if let Some(existing) = store.upsert(template)? {
        return Ok(existing);
    }
    let created = store
        .templates
        .last()
        .cloned()
        .ok_or_else(|| TEMPLATE_WRITE_ERROR.to_string())?;
    save_template_document(&app, &document, &store)?;
    Ok(created)
}

#[tauri::command]
pub fn update_profile_template(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    template: ProfileTemplate,
) -> Result<(), String> {
    let _lock = store_state
        .lock
        .lock()
        .map_err(|_| TEMPLATE_WRITE_ERROR.to_string())?;
    let document = load_template_document(&app)?;
    let mut store = document.store.clone();
    store.replace(template)?;
    save_template_document(&app, &document, &store)
}

#[tauri::command]
pub fn delete_profile_template(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    id: String,
) -> Result<(), String> {
    validate_profile_id(&id)?;
    let _lock = store_state
        .lock
        .lock()
        .map_err(|_| TEMPLATE_WRITE_ERROR.to_string())?;
    let document = load_template_document(&app)?;
    let mut store = document.store.clone();
    if !store.remove(&id) {
        return Err("프로필 템플릿을 찾을 수 없습니다".into());
    }
    save_template_document(&app, &document, &store)
}

/// Create a concrete profile using a template's defaults.  The template is
/// looked up and validated inside the same profile writer lock as the profile
/// store, and the incoming environment is ignored by the template contract.
#[tauri::command]
pub fn create_profile_from_template(
    app: AppHandle,
    store_state: tauri::State<'_, Arc<ProfileStoreState>>,
    request: CreateProfileFromTemplateRequest,
) -> Result<ProjectProfile, String> {
    let _lock = store_state
        .lock
        .lock()
        .map_err(|_| TEMPLATE_WRITE_ERROR.to_string())?;
    let CreateProfileFromTemplateRequest {
        template_id,
        profile,
    } = request;
    // Match ordinary profile creation: the renderer may leave the ID empty
    // for a new wizard entry, but the validated profile store requires a
    // stable generated identity before template application/upsert.
    let mut incoming = ensure_profile_id(profile);
    let profile = if let Some(template_id) = template_id.as_deref() {
        validate_profile_id(template_id)?;
        let template = load_template_document(&app)?
            .store
            .templates
            .into_iter()
            .find(|template| template.id == template_id)
            .ok_or_else(|| "프로필 템플릿을 찾을 수 없습니다".to_string())?;
        template.apply_to_profile(incoming)?
    } else {
        incoming.environment = None;
        incoming.validate()?;
        incoming
    };

    let document = load_store_document(&app)?;
    let mut store: ProfileStore = document.store.clone();
    let duplicate = store.upsert(profile.clone())?;
    if let Some(existing) = duplicate {
        return Ok(existing);
    }
    let created = store
        .profiles
        .last()
        .cloned()
        .ok_or_else(|| TEMPLATE_WRITE_ERROR.to_string())?;
    save_store_document(&app, &document, &store)?;
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::templates::PROFILE_TEMPLATE_VERSION;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);

    fn root(label: &str) -> PathBuf {
        let id = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "workbench-template-{label}-{}-{id}",
            std::process::id()
        ))
    }

    fn template() -> ProfileTemplate {
        ProfileTemplate {
            id: "template-node".into(),
            name: "Node".into(),
            windows_path: None,
            wsl: None,
            git_root: None,
            expected_ports: vec![3000],
            run_manager_service_ids: vec!["node-dev".into()],
        }
    }

    #[test]
    fn bounded_template_file_round_trips_and_preserves_missing_as_empty() {
        let root = root("roundtrip");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(TEMPLATE_FILE);
        assert_eq!(
            load_template_document_at_path(&path).unwrap().store,
            ProfileTemplateStore::empty()
        );
        let store = ProfileTemplateStore {
            version: PROFILE_TEMPLATE_VERSION,
            templates: vec![template()],
        };
        std::fs::write(&path, store.to_json_checked().unwrap()).unwrap();
        assert_eq!(load_template_document_at_path(&path).unwrap().store, store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn template_file_rejects_symlink_without_reading_target() {
        use std::os::unix::fs::symlink;
        let root = root("symlink");
        std::fs::create_dir_all(&root).unwrap();
        let real = root.join("real.json");
        let link = root.join(TEMPLATE_FILE);
        std::fs::write(
            &real,
            ProfileTemplateStore::empty().to_json_checked().unwrap(),
        )
        .unwrap();
        symlink(&real, &link).unwrap();
        assert_eq!(read_template_file(&link).unwrap_err(), TEMPLATE_PATH_ERROR);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_template_bytes_fail_closed_without_echoing_secret() {
        let root = root("malformed");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(TEMPLATE_FILE);
        let secret = "template-secret";
        std::fs::write(&path, format!("{{\"credential\":\"{secret}\"}}")).unwrap();
        let error = load_template_document_at_path(&path).unwrap_err();
        assert_eq!(error, TEMPLATE_READ_ERROR);
        assert!(!error.contains(secret));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_wizard_profile_gets_a_valid_backend_id_before_validation() {
        let mut profile = ProjectProfile::new("new project");
        profile.id.clear();
        let profile = ensure_profile_id(profile);
        assert!(!profile.id.is_empty());
        assert!(validate_profile_id(&profile.id).is_ok());
    }
}
