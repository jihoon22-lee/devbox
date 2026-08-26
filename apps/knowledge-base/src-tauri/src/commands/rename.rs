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
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(applied)
}

#[tauri::command]
pub fn discard_rename_preview(state: tauri::State<'_, Arc<AppState>>, plan_id: String) {
    state.rename_plans.lock().unwrap().discard(&plan_id);
}
