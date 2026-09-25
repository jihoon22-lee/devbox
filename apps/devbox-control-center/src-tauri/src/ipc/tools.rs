use product_contract::Problem;
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use tauri::{Manager, WebviewWindow};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(transparent)]
pub struct ControlToolsCall(pub installation_tools::api::ToolsCall);
impl ComponentCall for ControlToolsCall {
    const COMPONENT: &'static str = "control-center.tools";
    const MAX_ARGUMENT_BYTES: usize = 512 * 1024;
    fn method(&self) -> &'static str {
        self.0.method()
    }
    fn routes(&self) -> &'static [&'static str] {
        self.0.routes()
    }
    fn class(&self) -> ExecutionClass {
        self.0.class()
    }
}
#[tauri::command]
pub async fn tools(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<ControlToolsCall>(&window, request)?;
    Ok(admission.finish(
        installation_tools::api::dispatch(window.app_handle(), request.call.0).await,
        installation_tools::api::classify,
    ))
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<ControlToolsCall>()?;
    export.register::<installation_tools::api::ToolsIssue>()?;
    installation_tools::api::result_types(export)
}
