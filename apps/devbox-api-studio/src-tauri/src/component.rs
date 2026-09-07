//! Native-owned command registry. Route visibility does not grant authority.
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State, WebviewWindow};

const MAX_ARGUMENT_BYTES: usize = 20 * 1024 * 1024;
const MAX_ACTIVE: usize = 64;
#[derive(Default)]
struct Active(Arc<Mutex<HashSet<String>>>);
struct Reservation {
    ids: Arc<Mutex<HashSet<String>>>,
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
        "api-studio.migration" => {
            route == "requests" && crate::migration::COMMANDS.contains(&method)
        }
        "api-studio.api" => {
            matches!(route, "requests" | "protocols" | "history")
                && (api_playground_lib::component::COMMANDS.contains(&method)
                    || matches!(method, "pick_multipart_file" | "send_selection_to_toolbox")
                    || crate::handoff::is_navigation(method))
        }
        "api-studio.webhooks" => {
            route == "webhooks"
                && (webhook_lab_lib::component::COMMANDS.contains(&method)
                    || matches!(method, "send_history_to_api" | "send_fixture_to_api"))
        }
        "api-studio.transforms" => {
            route == "transforms"
                && (developer_toolbox_lib::component::COMMANDS.contains(&method)
                    || method == "read_clipboard_text")
        }
        _ => false,
    }
}

fn reserve(active: &Active, id: &str) -> Result<Reservation, ProblemCode> {
    let mut ids = active.0.lock().map_err(|_| ProblemCode::Unavailable)?;
    if ids.contains(id) {
        return Err(ProblemCode::Replayed);
    }
    if ids.len() >= MAX_ACTIVE {
        return Err(ProblemCode::Overloaded);
    }
    ids.insert(id.into());
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
    let _reservation = reserve(&active, &request.header.request_id).map_err(problem)?;
    let app = window.app_handle();
    if request.component != "api-studio.migration" {
        crate::migration::require_active(app).map_err(|_| problem(ProblemCode::Unavailable))?;
    }
    if request.component == "api-studio.migration" {
        return Ok(
            match crate::migration::dispatch(app, &request.method, request.args).await {
                Ok(value) => Response {
                    operation: Operation {
                        provenance,
                        outcome: OperationState::Succeeded {},
                    },
                    value,
                },
                Err(error) => Response {
                    operation: Operation {
                        provenance,
                        outcome: if crate::migration::issue(&error) == "cancelled" {
                            OperationState::Cancelled {}
                        } else {
                            OperationState::Failed {
                                code: ProblemCode::Unavailable,
                            }
                        },
                    },
                    value: serde_json::json!({ "issue": crate::migration::issue(&error) }),
                },
            },
        );
    }
    let value = if request.component == "api-studio.api"
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
    }
    .map_err(|_| problem(ProblemCode::Unavailable))?;
    Ok(Response {
        operation: Operation {
            provenance,
            outcome: OperationState::Succeeded {},
        },
        value,
    })
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("api-studio")
        .setup(|app, _| {
            app.manage(Active::default());
            crate::migration::initialize(app).map_err(std::io::Error::other)?;
            let store = crate::handoff::initialize(app).map_err(std::io::Error::other)?;
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
    }
    #[test]
    fn active_requests_reject_duplicate_ids_until_completion_or_drop() {
        let active = Active::default();
        let first = reserve(&active, "id").unwrap();
        assert!(matches!(reserve(&active, "id"), Err(ProblemCode::Replayed)));
        drop(first);
        let _again = reserve(&active, "id").unwrap();
        let mut pending = Vec::new();
        for i in 1..MAX_ACTIVE {
            pending.push(reserve(&active, &format!("id-{i}")).unwrap());
        }
        assert!(matches!(
            reserve(&active, "overflow"),
            Err(ProblemCode::Overloaded)
        ));
        pending.pop();
        assert!(reserve(&active, "overflow").is_ok());
    }
}
