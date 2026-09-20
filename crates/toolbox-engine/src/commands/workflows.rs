//! Thin Tauri boundary for metadata-only smart workflow persistence.

use crate::core::workflows::{self, WorkflowLoadResult, WorkflowMetadata, MAX_SERIALIZED_BYTES};
use tauri::AppHandle;

fn app_local_data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    crate::component::data_root(app)
        .map_err(|_| "Toolbox workflow metadata 저장 위치를 확인할 수 없습니다.".to_string())
}

#[tauri::command]
pub fn load_workflow_metadata(app: AppHandle) -> WorkflowLoadResult {
    app_local_data_dir(&app)
        .map(workflows::load_from_dir_with_status)
        .unwrap_or_else(|_| WorkflowLoadResult {
            metadata: WorkflowMetadata::default(),
            writable: false,
        })
}

#[tauri::command]
pub fn save_workflow_metadata(app: AppHandle, serialized_metadata: String) -> Result<(), String> {
    if serialized_metadata.len() > MAX_SERIALIZED_BYTES {
        return Err("Toolbox workflow metadata 저장 크기를 초과했습니다.".to_string());
    }
    let metadata = serde_json::from_str::<WorkflowMetadata>(&serialized_metadata)
        .map_err(|_| "Toolbox workflow metadata 형식이 올바르지 않습니다.".to_string())?;
    workflows::save_to_dir(app_local_data_dir(&app)?, &metadata).map_err(ToString::to_string)
}

/// Typed product adapter; the caller owns component/session authorization.
pub(crate) async fn __component_load_workflow_metadata(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = load_workflow_metadata(component_app.clone());
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller owns component/session authorization.
pub(crate) async fn __component_save_workflow_metadata(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        serialized_metadata: String,
    }
    let Input {
        serialized_metadata,
    } = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    save_workflow_metadata(component_app.clone(), serialized_metadata)?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".to_owned())
}
