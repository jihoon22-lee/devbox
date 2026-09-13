//! Local Control Center owns Manager tools. No peer method can invoke them.
use product_contract::{Operation, OperationState, Problem, ProblemCode, RouteRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    header: RouteRequest,
    method: String,
    args: Value,
}
#[derive(Serialize)]
struct Response {
    operation: Operation,
    value: Value,
}
#[tauri::command]
async fn execute(window: tauri::WebviewWindow, request: Request) -> Result<Response, Problem> {
    let provenance =
        product_shell_tauri::authorize(&window, &request.header, "control-center.tools")?;
    if !devbox_manager_lib::component::allowed(&request.header.route, &request.method)
        || serde_json::to_vec(&request.args).map_or(true, |value| value.len() > 512 * 1024)
    {
        return Err(Problem {
            provenance,
            code: ProblemCode::Unauthorized,
        });
    }
    let value = devbox_manager_lib::component::dispatch(
        window.app_handle(),
        &request.header.route,
        &request.method,
        request.args,
    )
    .await;
    let (outcome, value) = match value {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(_) => (
            OperationState::Failed {
                code: ProblemCode::Unavailable,
            },
            serde_json::json!({"issue":"manager_tools_unavailable"}),
        ),
    };
    Ok(Response {
        operation: Operation {
            provenance,
            outcome,
        },
        value,
    })
}
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("control-center")
        .invoke_handler(tauri::generate_handler![execute])
        .setup(|app, _| {
            devbox_manager_lib::component::initialize(app).map_err(std::io::Error::other)?;
            Ok(())
        })
        .build()
}
