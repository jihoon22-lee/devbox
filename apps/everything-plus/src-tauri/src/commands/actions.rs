//! 검색 결과 액션. 경로는 항상 단일 인자로 전달하며 셸 문자열 조합을 하지 않는다.

use std::path::{Component, Path};
use std::sync::Arc;

use crate::commands::indexing::AppState;
use crate::core::db;
use crate::core::open_targets::{prepare_open_request, select_open_targets, EverythingOpenTarget};
use tauri_plugin_opener::OpenerExt;

const INVALID_RESULT_PATH: &str = "검색 결과 파일 경로가 올바르지 않습니다";
const MISSING_RESULT_FILE: &str = "검색 결과 파일을 찾을 수 없습니다";

/// Search rows are stale input, not filesystem authority.  Validate the exact
/// final object immediately before an opener receives it, rejecting relative
/// traversal and final symlink/reparse substitutions without echoing a path or
/// OS error into the UI.
fn validate_result_path(path: &str) -> Result<(), &'static str> {
    let candidate = Path::new(path);
    if path.is_empty()
        || !candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(INVALID_RESULT_PATH);
    }
    filesystem::ensure_no_links(candidate).map_err(|_| MISSING_RESULT_FILE)?;
    filesystem::filesystem_identity(candidate, false)
        .map(|_| ())
        .map_err(|_| MISSING_RESULT_FILE)
}

fn validate_indexed_path(
    state: &tauri::State<'_, Arc<AppState>>,
    path: &str,
) -> Result<(), &'static str> {
    let conn = state.db.lock().map_err(|_| MISSING_RESULT_FILE)?;
    if !db::is_indexed_path(&conn, path).map_err(|_| MISSING_RESULT_FILE)? {
        return Err(MISSING_RESULT_FILE);
    }
    Ok(())
}

/// 기본 앱으로 파일을 연다.
#[tauri::command]
pub async fn open_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    validate_indexed_path(&state, &path).map_err(str::to_string)?;
    validate_result_path(&path).map_err(str::to_string)?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|_| "파일 열기 실패".to_string())
}

/// 파일이 있는 폴더를 탐색기에서 연다.
#[tauri::command]
pub async fn reveal_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    validate_indexed_path(&state, &path).map_err(str::to_string)?;
    validate_result_path(&path).map_err(str::to_string)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| "폴더 열기 실패".to_string())
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
pub fn open_in(
    state: tauri::State<'_, Arc<AppState>>,
    app_id: String,
    path: String,
) -> Result<(), String> {
    validate_indexed_path(&state, &path).map_err(str::to_string)?;
    validate_result_path(&path).map_err(str::to_string)?;
    let targets = available_open_targets();
    let (target_id, request) =
        prepare_open_request(&targets, &app_id, &path).map_err(str::to_string)?;
    devbox_launch::launch_open(&target_id, &request).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{validate_result_path, INVALID_RESULT_PATH, MISSING_RESULT_FILE};
    use std::path::PathBuf;

    fn temp_file() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "everything-action-path-{}-{unique}",
            std::process::id(),
        ));
        std::fs::write(&path, b"fixture").unwrap();
        path
    }

    #[test]
    fn validates_exact_file_and_hides_invalid_path_details() {
        let file = temp_file();
        let path = file.to_string_lossy().into_owned();
        assert_eq!(validate_result_path(&path), Ok(()));

        assert_eq!(
            validate_result_path("../secret/file.txt"),
            Err(INVALID_RESULT_PATH)
        );
        assert_eq!(
            validate_result_path(&std::env::temp_dir().to_string_lossy()),
            Err(MISSING_RESULT_FILE)
        );

        let _ = std::fs::remove_file(file);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_final_symlink_before_opening() {
        let target = temp_file();
        let link = target.with_extension("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let path = link.to_string_lossy().into_owned();
        assert_eq!(validate_result_path(&path), Err(MISSING_RESULT_FILE));
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_file(target);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_parent_symlink_before_opening() {
        let outside = std::env::temp_dir().join(format!(
            "everything-action-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = std::env::temp_dir().join(format!(
            "everything-action-parent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        let path = root.join("escaped.txt").to_string_lossy().into_owned();
        assert_eq!(validate_result_path(&path), Err(MISSING_RESULT_FILE));
        let _ = std::fs::remove_file(root);
        let _ = std::fs::remove_dir_all(outside);
    }
}
