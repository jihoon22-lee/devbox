use crate::commands::workspace::load_store;
use crate::core::open_targets::{
    actionable_targets, prepare_open_request, profile_path as safe_profile_path,
    select_open_targets, WorkbenchOpenTarget,
};

fn available_targets() -> Vec<WorkbenchOpenTarget> {
    select_open_targets(
        "workbench",
        devbox_launch::installed_targets("path"),
        devbox_launch::installed_targets("workspace"),
    )
}

fn profile(
    app: &tauri::AppHandle,
    profile_id: &str,
) -> Result<crate::core::profile::ProjectProfile, String> {
    load_store(app)
        .profiles
        .into_iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| "프로필을 찾을 수 없습니다".to_string())
}

/// 선택한 profile이 안전하게 만들 수 있고 현재 설치된 capability target만
/// 반환한다. executable과 profile path는 frontend에 노출하지 않는다.
#[tauri::command]
pub fn profile_open_targets(
    app: tauri::AppHandle,
    profile_id: String,
) -> Result<Vec<WorkbenchOpenTarget>, String> {
    let profile = profile(&app, &profile_id)?;
    Ok(actionable_targets(&profile, available_targets()))
}

/// 사용자가 명시적으로 "경로 복사"를 선택했을 때만 현재 저장소를 다시 읽어
/// 검증한 project path를 반환한다.
#[tauri::command]
pub fn profile_copy_path(app: tauri::AppHandle, profile_id: String) -> Result<String, String> {
    let profile = profile(&app, &profile_id)?;
    safe_profile_path(&profile).map_err(str::to_string)
}

#[tauri::command]
pub fn open_profile_in(
    app: tauri::AppHandle,
    profile_id: String,
    app_id: String,
) -> Result<(), String> {
    let profile = profile(&app, &profile_id)?;
    let targets = actionable_targets(&profile, available_targets());
    let (target_id, request) =
        prepare_open_request(&profile, &targets, &app_id).map_err(str::to_string)?;
    devbox_launch::launch_open(&target_id, &request).map(|_| ())
}
