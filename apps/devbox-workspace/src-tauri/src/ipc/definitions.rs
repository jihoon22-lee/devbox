use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(transparent)]
pub struct WorkspaceDefinitionsCall(pub projects_engine::api::DefinitionsCall);
pub const METHODS: &[&str] = &[
    "apply_edit",
    "approve_trust",
    "cancel",
    "load",
    "preview_edit",
    "preview_trust",
    "revoke_trust",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "apply_edit" => &["overview"],
        "approve_trust" => &["overview"],
        "cancel" => &["overview"],
        "load" => &["overview"],
        "preview_edit" => &["overview"],
        "preview_trust" => &["overview"],
        "revoke_trust" => &["overview"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceDefinitionsCall {
    const COMPONENT: &'static str = "workspace.definitions";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 2097152;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let _ = method;
        let limit = Self::MAX_ARGUMENT_BYTES;
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        self.0.method()
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl WorkspaceDefinitionsCall {
    pub fn lane(&self) -> Lane {
        self.0.lane()
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.definitions", method)
}
#[tauri::command]
pub(crate) async fn definitions(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceDefinitionsCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceDefinitionsCall {
    fn into_call(self) -> super::Call {
        super::Call::Definitions(self)
    }
}

pub(crate) fn definition_access(method: &str) -> Option<bool> {
    match method {
        "apply_edit" => Some(true),
        "load" | "preview_edit" | "preview_trust" | "approve_trust" => Some(false),
        _ => None,
    }
}

use super::Call;
use super::Request;
use crate::component::Runtime;
use serde_json::{json, Value};
use tauri::WebviewWindow;
pub(crate) async fn execute_definitions(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: Option<crate::core::context_activity::ContextPermit>,
) -> Result<Value, &'static str> {
    let deadline = request.header.deadline_ms;

    let host = runtime.host();
    let owner = runtime.definitions.clone();
    let permit = runtime.lanes.try_enter(Lane::Probes);
    let filesystem = match definition_access(&request.method) {
        Some(write) => runtime.filesystem_permit(write, deadline).await.map(Some),
        None => Ok(None),
    }
    .and_then(|permit| {
        if product_shell_tauri::workspace_context(window).map_err(|_| "stale_context")?
            != request.header.context
        {
            return Err("stale_context");
        }
        Ok(permit)
    });
    match (host, permit, filesystem) {
        (Ok(host), Ok(permit), Ok(filesystem)) => {
            let worker_context = context_permit.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let (_permit, _context, _filesystem) = (permit, worker_context, filesystem);
                crate::files_host::current_deadline(deadline)?;
                let mut owner = owner.lock().map_err(|_| "definition_owner_busy")?;
                crate::files_host::current_deadline(deadline)?;
                let context = request
                    .header
                    .context
                    .as_ref()
                    .ok_or("project_selection_required")?;
                use projects_engine::api::DefinitionsCall as C;
                let Call::Definitions(call) = request.typed else {
                    return Err("invalid_request");
                };
                match call.0 {
                    C::Load {} => Ok(json!(owner.load(&host, context, deadline)?)),
                    C::PreviewEdit(edit) => {
                        Ok(json!(owner.preview_edit(&host, context, edit, deadline)?))
                    }
                    C::ApplyEdit { preview_id } => Ok(json!(owner.apply_edit(
                        &host,
                        context,
                        &preview_id,
                        deadline
                    )?)),
                    C::PreviewTrust {} => Ok(json!(owner.preview_trust(&host, context, deadline)?)),
                    C::ApproveTrust { preview_id } => Ok(json!(owner.approve_trust(
                        &host,
                        context,
                        &preview_id,
                        deadline
                    )?)),
                    C::RevokeTrust { revision } => {
                        Ok(json!(owner.revoke_trust(&host, context, revision)?))
                    }
                    C::Cancel { preview_id } => {
                        owner.cancel(&preview_id);
                        Ok(Value::Null)
                    }
                }
            })
            .await
            .unwrap_or(Err("worker_unavailable"))
        }
        (Err(issue), _, _) | (_, Err(issue), _) | (_, _, Err(issue)) => Err(issue),
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<WorkspaceDefinitionsCall>()?;
    Ok(vec![
        (
            "load",
            export.register::<crate::definitions::DefinitionView>()?,
        ),
        (
            "preview_trust",
            export.register::<crate::definitions::TrustPreview>()?,
        ),
        (
            "approve_trust",
            export.register::<crate::core::registry::Registry>()?,
        ),
        (
            "revoke_trust",
            export.register::<crate::core::registry::Registry>()?,
        ),
        ("cancel", export.register::<()>()?),
        (
            "preview_edit",
            export.register::<crate::definitions::EditPreview>()?,
        ),
        (
            "apply_edit",
            export.register::<crate::definitions::EditSaved>()?,
        ),
    ])
}
