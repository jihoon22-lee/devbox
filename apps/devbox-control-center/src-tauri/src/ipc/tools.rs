use product_contract::{
    agent_settings::{AgentAutostartStatus, AgentMcpStatus, AgentSettingsCall, McpSettingsCall},
    Problem,
};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use tauri::{Manager, WebviewWindow};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum ControlToolsCall {
    Settings(AgentSettingsCall),
    McpSettings(McpSettingsCall),
    Engine(installation_tools::api::ToolsCall),
}
impl ComponentCall for ControlToolsCall {
    const COMPONENT: &'static str = "control-center.tools";
    const MAX_ARGUMENT_BYTES: usize = 512 * 1024;
    fn method(&self) -> &'static str {
        match self {
            Self::Settings(call) => call.method(),
            Self::McpSettings(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        match self {
            Self::Settings(_) | Self::McpSettings(_) => &["environment"],
            Self::Engine(call) => call.routes(),
        }
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Settings(_) | Self::McpSettings(_) => ExecutionClass::Normal,
            Self::Engine(call) => call.class(),
        }
    }
}
#[tauri::command]
pub async fn tools(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<ControlToolsCall>(&window, request)?;
    Ok(admission.finish(
        match request.call {
            ControlToolsCall::McpSettings(call) => {
                product_shell_tauri::agent_settings::mcp_call(
                    window.app_handle(),
                    "control-center",
                    call,
                    request.header.deadline_ms,
                )
                .await
            }
            ControlToolsCall::Settings(call) => {
                product_shell_tauri::agent_settings::call(
                    window.app_handle(),
                    "control-center",
                    call,
                    request.header.deadline_ms,
                )
                .await
            }
            ControlToolsCall::Engine(call) => {
                installation_tools::api::dispatch(window.app_handle(), call).await
            }
        },
        installation_tools::api::classify,
    ))
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<ControlToolsCall>()?;
    export.register::<installation_tools::api::ToolsIssue>()?;
    let mut results = installation_tools::api::result_types(export)?;
    for method in ["autostart_status", "set_autostart"] {
        results.push((method, export.register::<AgentAutostartStatus>()?));
    }
    for method in ["mcp_settings", "set_mcp_settings"] {
        results.push((method, export.register::<AgentMcpStatus>()?));
    }
    Ok(results)
}
