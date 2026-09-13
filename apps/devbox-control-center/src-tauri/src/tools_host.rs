//! Local Control Center owns Manager tools. No peer method can invoke them.
use product_contract::{Operation, OperationState, Problem, ProblemCode, RouteRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
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
    let inventory = request.method == "suite_inventory";
    let recovery = request.method == "suite_recovery";
    let component = if inventory || recovery {
        "control-center.delivery"
    } else {
        "control-center.tools"
    };
    let provenance = product_shell_tauri::authorize(&window, &request.header, component)?;
    let allowed = if inventory || recovery {
        matches!(
            request.header.route.as_str(),
            "products" | "updates" | "components" | "migration" | "recovery"
        ) && request.args.as_object().is_some_and(|args| args.is_empty())
    } else {
        devbox_manager_lib::component::allowed(&request.header.route, &request.method)
    };
    if !allowed || serde_json::to_vec(&request.args).map_or(true, |value| value.len() > 512 * 1024)
    {
        return Err(Problem {
            provenance,
            code: ProblemCode::Unauthorized,
        });
    }
    let value = if recovery {
        let root = window.app_handle().path().app_local_data_dir();
        match root {
            Err(_) => Err("suite_store_unavailable".into()),
            Ok(root) => tauri::async_runtime::spawn_blocking(move || {
                let journal = crate::core::delivery_store::Store::inspect(&root)?;
                match journal {
                    None => Ok(serde_json::json!({"state":"none"})),
                    Some((journal, _)) => {
                        let recovery = journal.recovery()?;
                        Ok(serde_json::json!({"state":"recorded", "phase":journal.phase,
                            "committed":journal.committed,"recovery":recovery,
                            "backupCount":journal.backup.len(),"importCount":journal.imports.len(),
                            "cleanupPending":journal.cleanup_pending.len(),"failure":journal.failure}))
                    }
                }
            }).await.map_err(|_| "suite_store_unavailable".to_string())
                .and_then(|result:Result<Value, &'static str>| result.map_err(str::to_owned))
        }
    } else if inventory {
        tauri::async_runtime::spawn_blocking(|| {
            #[cfg(windows)]
            let scope = crate::suite::capture_own("control-center").ok();
            #[cfg(windows)]
            let captured = scope.as_ref().map(|scope| {
                (
                    &scope.manifest,
                    scope.installation_key.as_str(),
                    &scope.issues,
                )
            });
            #[cfg(not(windows))]
            let captured = None;
            crate::core::inventory::observe(captured)
                .map_err(str::to_owned)
                .and_then(|value| {
                    serde_json::to_value(value).map_err(|_| "inventory_unavailable".into())
                })
        })
        .await
        .map_err(|_| "inventory_worker_unavailable".to_string())
        .and_then(|value| value)
    } else {
        devbox_manager_lib::component::dispatch(
            window.app_handle(),
            &request.header.route,
            &request.method,
            request.args,
        )
        .await
    };
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
