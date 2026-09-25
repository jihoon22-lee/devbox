//! Native-owned command registry. Route visibility does not grant authority.
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State, WebviewWindow};

const MAX_ARGUMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_ACTIVE: usize = 64;
const MAX_ACTIVE_CONTROLS: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecutionClass {
    Normal,
    Control,
}

// Classification does not authorize a command. The route/owner registry and
// native session authorization must both succeed before reserving either class.
const CONTROL_COMMANDS: &[(&str, &str)] = &[
    ("api-studio.api", "cancel_request"),
    ("api-studio.api", "cancel_mcp_http"),
    ("api-studio.api", "disconnect_mcp_http"),
    ("api-studio.api", "cancel_mcp_oauth"),
    ("api-studio.api", "cancel_mcp_stdio"),
    ("api-studio.api", "disconnect_mcp_stdio"),
    ("api-studio.api", "cancel_grpc"),
    ("api-studio.api", "disconnect_grpc"),
    ("api-studio.api", "stop_sse_stream"),
    ("api-studio.api", "close_websocket"),
    ("api-studio.api", "disconnect_websocket"),
    ("api-studio.webhooks", "stop_server"),
    ("api-studio.webhooks", "quit_product"),
];
fn execution_class(component: &str, method: &str) -> ExecutionClass {
    if CONTROL_COMMANDS.contains(&(component, method)) {
        ExecutionClass::Control
    } else {
        ExecutionClass::Normal
    }
}
#[derive(Default)]
struct Active(Arc<Mutex<HashMap<String, ExecutionClass>>>);
struct Reservation {
    ids: Arc<Mutex<HashMap<String, ExecutionClass>>>,
    id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut ids) = self.ids.lock() {
            ids.remove(&self.id);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    header: RouteRequest,
    component: String,
    method: String,
    args: serde_json::Value,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    operation: Operation,
    value: serde_json::Value,
}

fn allowed(component: &str, route: &str, method: &str) -> bool {
    match component {
        "api-studio.api" => {
            if route == "protocols"
                && (crate::knowledge::is_command(component, method)
                    || crate::handoff::is_send(component, method)
                    || method == "send_selection_to_toolbox")
            {
                return false;
            }
            matches!(route, "requests" | "protocols" | "history")
                && (api_playground_lib::component::COMMANDS.contains(&method)
                    || crate::api_workspace::COMMANDS.contains(&method)
                    || crate::handoff::is_send(component, method)
                    || crate::knowledge::is_command(component, method)
                    || matches!(method, "pick_multipart_file" | "send_selection_to_toolbox")
                    || crate::handoff::is_navigation(method))
        }
        "api-studio.webhooks" => {
            route == "webhooks"
                && (webhook_lab_lib::component::COMMANDS.contains(&method)
                    || crate::mock_draft::COMMANDS.contains(&method)
                    || matches!(
                        method,
                        "send_history_to_api"
                            | "send_fixture_to_api"
                            | "send_history_to_log_lens"
                            | "send_fixture_to_log_lens"
                    )
                    || crate::lifecycle::COMMANDS.contains(&method))
        }
        "api-studio.transforms" => {
            route == "transforms"
                && ((method == "open_workspace_selection"
                    || developer_toolbox_lib::component::COMMANDS.contains(&method))
                    || crate::knowledge::is_command(component, method)
                    || method == "read_clipboard_text"
                    || crate::handoff::is_send(component, method))
        }
        _ => false,
    }
}

fn reserve(active: &Active, id: &str, class: ExecutionClass) -> Result<Reservation, ProblemCode> {
    let mut ids = active.0.lock().map_err(|_| ProblemCode::Unavailable)?;
    if ids.contains_key(id) {
        return Err(ProblemCode::Replayed);
    }
    if ids.values().filter(|value| **value == class).count()
        >= if class == ExecutionClass::Control {
            MAX_ACTIVE_CONTROLS
        } else {
            MAX_ACTIVE
        }
    {
        return Err(ProblemCode::Overloaded);
    }
    ids.insert(id.into(), class);
    Ok(Reservation {
        ids: Arc::clone(&active.0),
        id: id.into(),
    })
}

#[tauri::command]
async fn execute(
    window: WebviewWindow,
    active: State<'_, Active>,
    request: Request,
) -> Result<Response, Problem> {
    let operation =
        product_shell_tauri::begin_operation(&window, &request.component, &request.method);
    // Untrusted names never enter the returned provenance.
    let rejected = |code| Problem {
        code,
        provenance: Provenance {
            product: "api-studio".into(),
            component: "api-studio.dispatch".into(),
            request_id: "rejected".into(),
            revision: 1,
        },
    };
    if !allowed(&request.component, &request.header.route, &request.method)
        || !request.args.is_object()
        || serde_json::to_vec(&request.args).map_or(true, |v| v.len() > MAX_ARGUMENT_BYTES)
    {
        return Err(rejected(ProblemCode::InvalidRequest));
    }
    let provenance = product_shell_tauri::authorize(&window, &request.header, &request.component)?;
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    // Admission deadline is distinct from a protocol lifetime. Keep the ID
    // reserved across dialogs/long awaits even after the replay cache expires.
    let _reservation = reserve(
        &active,
        &request.header.request_id,
        execution_class(&request.component, &request.method),
    )
    .map_err(problem)?;
    let app = window.app_handle();
    crate::lifecycle::require_open(app).map_err(|_| problem(ProblemCode::Unavailable))?;
    let value = if request.component == "api-studio.api"
        && crate::api_workspace::COMMANDS.contains(&request.method.as_str())
    {
        crate::api_workspace::dispatch(
            app,
            &request.method,
            request.args,
            request.header.context.clone(),
        )
        .await
    } else if crate::knowledge::is_command(&request.component, &request.method) {
        crate::knowledge::dispatch(
            app,
            &request.component,
            &request.method,
            request.args,
            provenance.clone(),
            request.header.deadline_ms,
        )
        .await
    } else if request.component == "api-studio.webhooks"
        && matches!(
            request.method.as_str(),
            "send_history_to_log_lens" | "send_fixture_to_log_lens"
        )
    {
        crate::webhook_logs::send(
            app,
            request.args,
            request.method == "send_fixture_to_log_lens",
            provenance.request_id.clone(),
            request.header.deadline_ms,
        )
        .await
    } else if request.component == "api-studio.webhooks"
        && crate::mock_draft::COMMANDS.contains(&request.method.as_str())
    {
        crate::mock_draft::dispatch(app, &request.method, request.args)
    } else if request.component == "api-studio.webhooks"
        && crate::lifecycle::COMMANDS.contains(&request.method.as_str())
    {
        crate::lifecycle::dispatch(app, &request.method, request.args)
    } else if request.component == "api-studio.transforms"
        && request.method == "open_workspace_selection"
    {
        crate::selection_receive::open(app, request.args, request.header.deadline_ms).await
    } else if request.component == "api-studio.api"
        && crate::handoff::is_navigation(&request.method)
    {
        crate::handoff::navigation(app, &request.method, request.args)
    } else if crate::handoff::is_send(&request.component, &request.method) {
        crate::handoff::send(
            app,
            &request.component,
            &request.method,
            request.args,
            provenance.clone(),
        )
    } else if request.component == "api-studio.api" && request.method == "pick_multipart_file" {
        use tauri_plugin_dialog::DialogExt;
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            handle
                .dialog()
                .file()
                .set_title("multipart 파일 선택")
                .blocking_pick_file()
                .map(|file| {
                    file.into_path()
                        .map(|path| path.to_string_lossy().into_owned())
                        .map_err(|_| "file_selection_unavailable".to_string())
                })
                .transpose()
                .and_then(|value| {
                    serde_json::to_value(value).map_err(|_| "file_selection_unavailable".into())
                })
        })
        .await
        .map_err(|_| problem(ProblemCode::Unavailable))?
    } else if request.component == "api-studio.transforms"
        && request.method == "read_clipboard_text"
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app.clipboard()
            .read_text()
            .map(serde_json::Value::String)
            .map_err(|_| "clipboard_unavailable".into())
    } else {
        match request.component.as_str() {
            "api-studio.api" => {
                api_playground_lib::component::dispatch(app, &request.method, request.args).await
            }
            "api-studio.webhooks" => {
                webhook_lab_lib::component::dispatch(app, &request.method, request.args).await
            }
            "api-studio.transforms" => {
                developer_toolbox_lib::component::dispatch(app, &request.method, request.args).await
            }
            _ => return Err(problem(ProblemCode::Unauthorized)),
        }
    };
    let (outcome, value) = match value {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(error) => {
            let issue = crate::component_errors::project(&request.component, &error);
            let outcome = if issue.ends_with("_cancelled") {
                OperationState::Cancelled {}
            } else {
                OperationState::Failed {
                    code: ProblemCode::Unavailable,
                }
            };
            (outcome, serde_json::json!({ "issue": issue }))
        }
    };
    operation.finish(
        &outcome,
        value.get("issue").and_then(serde_json::Value::as_str),
    );
    Ok(Response {
        operation: Operation {
            provenance,
            outcome,
        },
        value,
    })
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("api-studio")
        .setup(|app, _| {
            app.manage(Active::default());
            let store = crate::handoff::initialize(app).map_err(std::io::Error::other)?;
            crate::mock_draft::initialize(app, store.clone()).map_err(std::io::Error::other)?;
            api_playground_lib::component::initialize(app, store.clone())
                .map_err(std::io::Error::other)?;
            webhook_lab_lib::component::initialize(app).map_err(std::io::Error::other)?;
            developer_toolbox_lib::component::initialize(app, store)
                .map_err(std::io::Error::other)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![execute])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_ownership_and_legacy_launch_are_not_renderer_choices() {
        assert!(allowed("api-studio.api", "requests", "send_request"));
        assert!(!allowed("api-studio.webhooks", "webhooks", "send_request"));
        assert!(!allowed("api-studio.api", "transforms", "send_request"));
        assert!(!allowed("workspace.runtime", "requests", "send_request"));
        assert!(allowed(
            "api-studio.api",
            "requests",
            "send_selection_to_toolbox"
        ));
        assert!(!allowed("api-studio.api", "requests", "run"));
        assert!(allowed(
            "api-studio.webhooks",
            "webhooks",
            "send_history_to_log_lens"
        ));
        assert!(!allowed(
            "api-studio.api",
            "requests",
            "send_history_to_log_lens"
        ));
    }
    #[test]
    fn cancellation_has_bounded_capacity_when_data_slots_are_full() {
        let active = Active::default();
        let _data: Vec<_> = (0..MAX_ACTIVE)
            .map(|i| reserve(&active, &format!("data-{i}"), ExecutionClass::Normal).unwrap())
            .collect();
        assert!(matches!(
            reserve(&active, "extra", ExecutionClass::Normal),
            Err(ProblemCode::Overloaded)
        ));
        assert!(matches!(
            reserve(&active, "data-0", ExecutionClass::Control),
            Err(ProblemCode::Replayed)
        ));
        let mut controls: Vec<_> = (0..MAX_ACTIVE_CONTROLS)
            .map(|i| reserve(&active, &format!("cancel-{i}"), ExecutionClass::Control).unwrap())
            .collect();
        assert!(matches!(
            reserve(&active, "extra-control", ExecutionClass::Control),
            Err(ProblemCode::Overloaded)
        ));
        controls.pop();
        assert!(reserve(&active, "replacement-control", ExecutionClass::Control).is_ok());
        assert!(matches!(
            reserve(&active, "extra", ExecutionClass::Normal),
            Err(ProblemCode::Overloaded)
        ));
    }

    #[test]
    fn every_registered_control_has_capacity_without_widening_authority() {
        let active = Active::default();
        let _normal: Vec<_> = (0..MAX_ACTIVE)
            .map(|i| reserve(&active, &format!("normal-{i}"), ExecutionClass::Normal).unwrap())
            .collect();
        for &(component, method) in CONTROL_COMMANDS {
            let route = if component == "api-studio.webhooks" {
                "webhooks"
            } else {
                "requests"
            };
            assert!(allowed(component, route, method), "{component}/{method}");
            assert!(!allowed(component, "transforms", method));
            let class = execution_class(component, method);
            assert_eq!(class, ExecutionClass::Control);
            let control = reserve(&active, method, class).unwrap();
            assert!(matches!(
                reserve(&active, method, class),
                Err(ProblemCode::Replayed)
            ));
            drop(control);
            assert!(reserve(&active, method, class).is_ok());
        }
        for (component, method) in [
            ("api-studio.transforms", "cancel_request"),
            ("api-studio.api", "cancel_unknown"),
            ("api-studio.api", "send_request"),
            ("api-studio.api", "delete_grpc_tls_credential"),
        ] {
            assert_eq!(execution_class(component, method), ExecutionClass::Normal);
            assert!(matches!(
                reserve(&active, method, execution_class(component, method)),
                Err(ProblemCode::Overloaded)
            ));
        }
    }

    #[test]
    fn active_requests_reject_duplicate_ids_until_completion_or_drop() {
        let active = Active::default();
        let first = reserve(&active, "id", ExecutionClass::Normal).unwrap();
        assert!(matches!(
            reserve(&active, "id", ExecutionClass::Normal),
            Err(ProblemCode::Replayed)
        ));
        drop(first);
        let _again = reserve(&active, "id", ExecutionClass::Normal).unwrap();
        let mut pending = Vec::new();
        for i in 1..MAX_ACTIVE {
            pending.push(reserve(&active, &format!("id-{i}"), ExecutionClass::Normal).unwrap());
        }
        assert!(matches!(
            reserve(&active, "overflow", ExecutionClass::Normal),
            Err(ProblemCode::Overloaded)
        ));
        pending.pop();
        assert!(reserve(&active, "overflow", ExecutionClass::Normal).is_ok());
    }
}
