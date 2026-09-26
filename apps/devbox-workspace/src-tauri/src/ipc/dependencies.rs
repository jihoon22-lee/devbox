use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(transparent)]
pub struct WorkspaceDependenciesCall(pub repositories_engine::api::DependenciesCall);
pub const METHODS: &[&str] = &[
    "dependency_enrichment_cancel",
    "dependency_enrichment_execute",
    "dependency_enrichment_preview",
    "dependency_inventory",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "dependency_enrichment_cancel" => &["dependencies"],
        "dependency_enrichment_execute" => &["dependencies"],
        "dependency_enrichment_preview" => &["dependencies"],
        "dependency_inventory" => &["dependencies"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceDependenciesCall {
    const COMPONENT: &'static str = "workspace.dependencies";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
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
impl WorkspaceDependenciesCall {
    pub fn lane(&self) -> Lane {
        self.0.lane()
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.dependencies", method)
}
#[tauri::command]
pub(crate) async fn dependencies(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceDependenciesCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceDependenciesCall {
    fn into_call(self) -> super::Call {
        super::Call::Dependencies(self)
    }
}

use super::Request;
use crate::component::Runtime;
use serde_json::Value;
use std::time::Duration;

pub(crate) async fn execute_dependencies(
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    let super::Call::Dependencies(call) = request.typed else {
        return Err("invalid_request");
    };
    let host = runtime.host()?;
    let permit = runtime.lanes.try_enter(Lane::Probes)?;
    let deadline = request.header.deadline_ms;
    let context = request.header.context.ok_or("project_selection_required")?;
    let access = tauri::async_runtime::spawn_blocking(move || {
        crate::dependencies_host::access(host, context, deadline, (permit, context_permit))
    })
    .await
    .unwrap_or(Err("worker_unavailable"))?;
    crate::files_host::current_deadline(deadline)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis() as u64;
    let remaining = deadline.checked_sub(now).ok_or("request_expired")?;
    match tokio::time::timeout(
        Duration::from_millis(remaining),
        repositories_engine::api::dispatch_dependencies(access, call.0),
    )
    .await
    {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(repositories_engine::component::dependency_issue(&error)),
        Err(_) => Err("request_expired"),
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<WorkspaceDependenciesCall>()?;
    repositories_engine::api::dependencies_result_types(export)
}
