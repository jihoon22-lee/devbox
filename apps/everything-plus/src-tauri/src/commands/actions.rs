//! 검색 결과 액션. 경로는 항상 단일 인자로 전달하며 셸 문자열 조합을 하지 않는다.

use crate::core::open_targets::{prepare_open_request, select_open_targets, EverythingOpenTarget};
use tauri_plugin_opener::OpenerExt;

/// 기본 앱으로 파일을 연다.
#[tauri::command]
pub async fn open_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("파일 열기 실패: {e}"))
}

/// 파일이 있는 폴더를 탐색기에서 연다.
#[tauri::command]
pub async fn reveal_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| format!("폴더 열기 실패: {e}"))
}

fn available_open_targets() -> Vec<EverythingOpenTarget> {
    select_open_targets("everything-plus", devbox_launch::installed_targets("path"))
}

/// Catalog capability와 실제 설치 executable의 교집합만 반환한다. executable
/// 경로는 frontend에 노출하지 않는다.
#[tauri::command]
pub fn open_targets() -> Vec<EverythingOpenTarget> {
    available_open_targets()
}

#[tauri::command]
pub fn open_in(app_id: String, path: String) -> Result<(), String> {
    let targets = available_open_targets();
    let (target_id, request) =
        prepare_open_request(&targets, &app_id, &path).map_err(str::to_string)?;
    devbox_launch::launch_open(&target_id, &request).map(|_| ())
}
