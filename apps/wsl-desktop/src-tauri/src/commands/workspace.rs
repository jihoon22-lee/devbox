//! Named terminal workspace profiles stored under Tauri app-local data.

use crate::core::workspace::{ProfileStore, WorkspaceProfile};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const PROFILE_FILE: &str = "terminal-profiles.json";

fn profile_path(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "터미널 프로필 저장 위치를 확인할 수 없습니다".to_string())?;
    std::fs::create_dir_all(&directory)
        .map_err(|_| "터미널 프로필 저장 위치를 만들 수 없습니다".to_string())?;
    Ok(directory.join(PROFILE_FILE))
}

fn load_store(app: &AppHandle) -> Result<ProfileStore, String> {
    let path = profile_path(app)?;
    match std::fs::read_to_string(path) {
        Ok(input) => ProfileStore::load(&input),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProfileStore::default()),
        Err(_) => Err("터미널 프로필을 읽을 수 없습니다".into()),
    }
}

fn save_store(app: &AppHandle, store: &ProfileStore) -> Result<(), String> {
    let json = store.to_json()?;
    devbox_filesystem::atomic_write(profile_path(app)?, json.as_bytes())
        .map_err(|_| "터미널 프로필을 원자적으로 저장할 수 없습니다".to_string())
}

#[tauri::command]
pub fn list_workspace_profiles(app: AppHandle) -> Vec<WorkspaceProfile> {
    let Ok(store) = load_store(&app) else {
        // Preserve the prior read-only UI contract while refusing to publish
        // or mutate an invalid local store.
        return Vec::new();
    };
    let profiles = store.profiles.clone();
    let _ = crate::integration::publish_profile_snapshot(&store);
    profiles
}

#[tauri::command]
pub fn save_workspace_profile(
    app: AppHandle,
    mut profile: WorkspaceProfile,
) -> Result<WorkspaceProfile, String> {
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().to_string();
    }
    let mut store = load_store(&app)?;
    store.upsert(profile.clone())?;
    save_store(&app, &store)?;
    let _ = crate::integration::publish_profile_snapshot(&store);
    Ok(profile)
}

#[tauri::command]
pub fn delete_workspace_profile(app: AppHandle, id: String) -> Result<(), String> {
    let mut store = load_store(&app)?;
    if !store.remove(&id) {
        return Err("터미널 프로필을 찾을 수 없습니다".into());
    }
    save_store(&app, &store)?;
    let _ = crate::integration::publish_profile_snapshot(&store);
    Ok(())
}
