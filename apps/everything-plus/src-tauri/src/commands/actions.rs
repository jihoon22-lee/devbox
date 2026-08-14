//! 검색 결과 액션. 경로는 항상 단일 인자로 전달하며 셸 문자열 조합을 하지 않는다.

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
