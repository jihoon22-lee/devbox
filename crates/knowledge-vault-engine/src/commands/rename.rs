use crate::commands::docs::{resolve_root, AppState};
use crate::core::rename::{self, RenameApplied, RenamePreview};
use std::sync::Arc;

#[tauri::command]
pub fn preview_rename(
    state: tauri::State<'_, Arc<AppState>>,
    from: String,
    to: String,
) -> Result<RenamePreview, String> {
    let root = {
        let conn = state.db.lock().unwrap();
        resolve_root(&conn)?
    };
    let mut store = state.rename_plans.lock().unwrap();
    store.clear();
    let plan_id = store.next_id();
    let (preview, plan) = rename::prepare(&root, &from, &to, plan_id)?;
    store.put(plan);
    Ok(preview)
}

#[tauri::command]
pub fn apply_rename(
    state: tauri::State<'_, Arc<AppState>>,
    plan_id: String,
) -> Result<RenameApplied, String> {
    let plan = state.rename_plans.lock().unwrap().take(&plan_id)?;
    let applied = {
        let mut conn = state.db.lock().unwrap();
        let root = resolve_root(&conn)?;
        rename::apply(&root, &mut conn, plan)?
    };
    let _ = crate::integration::write_snapshot(
        &state.db.lock().unwrap(),
        state.integration_root.as_deref(),
    );
    Ok(applied)
}

#[tauri::command]
pub fn discard_rename_preview(state: tauri::State<'_, Arc<AppState>>, plan_id: String) {
    state.rename_plans.lock().unwrap().discard(&plan_id);
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_preview_rename(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        from: String,
        to: String,
    }
    let Input { from, to } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = preview_rename(component_app.state(), from, to)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_apply_rename(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        plan_id: String,
    }
    let Input { plan_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = apply_rename(component_app.state(), plan_id)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_discard_rename_preview(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        plan_id: String,
    }
    let Input { plan_id } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    discard_rename_preview(component_app.state(), plan_id);
    Ok(serde_json::Value::Null)
}
