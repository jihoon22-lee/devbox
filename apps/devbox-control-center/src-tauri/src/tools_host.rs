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
fn component_for(method: &str) -> Option<&'static str> {
    if matches!(
        method,
        "suite_inventory"
            | "open_installation_folder"
            | "suite_recovery"
            | "restore_inventory"
            | "restore_action"
            | "record_suite_health"
            | "check_suite_update"
            | "suite_update_status"
            | "download_suite_update"
            | "cancel_suite_update"
            | "launch_suite_update"
    ) {
        Some("control-center.delivery")
    } else if ["diagnostics", "recovery", "environment", "tools"]
        .iter()
        .any(|route| installation_tools::component::allowed(route, method))
    {
        Some("control-center.tools")
    } else {
        None
    }
}

#[tauri::command]
async fn execute(window: tauri::WebviewWindow, request: Request) -> Result<Response, Problem> {
    let record_health = request.method == "record_suite_health";
    let inventory = request.method == "suite_inventory";
    let open_directory = request.method == "open_installation_folder";
    let recovery = request.method == "suite_recovery";
    let restore_inventory = request.method == "restore_inventory";
    let restore_action = request.method == "restore_action";
    let suite_update = matches!(
        request.method.as_str(),
        "check_suite_update"
            | "suite_update_status"
            | "download_suite_update"
            | "cancel_suite_update"
            | "launch_suite_update"
    );
    let selected = component_for(&request.method);
    let component = selected.unwrap_or("control-center.dispatch");
    let operation = product_shell_tauri::begin_operation(&window, component, &request.method);
    if selected.is_none() {
        return Err(Problem {
            code: ProblemCode::InvalidRequest,
            provenance: product_contract::Provenance {
                product: "control-center".into(),
                component: "control-center.dispatch".into(),
                request_id: "rejected".into(),
                revision: 1,
            },
        });
    }
    let provenance = product_shell_tauri::authorize(&window, &request.header, component)?;
    let allowed = if suite_update {
        request.header.route == "updates"
            && request.args.as_object().is_some_and(|args| {
                if request.method == "check_suite_update" {
                    args.is_empty()
                } else {
                    args.len() == 1
                        && args
                            .get("id")
                            .and_then(Value::as_str)
                            .is_some_and(product_contract::commands::revision)
                }
            })
    } else if restore_inventory || restore_action {
        matches!(
            request.header.route.as_str(),
            "updates" | "recovery" | "products"
        ) && request.args.as_object().is_some_and(|args| {
            if restore_inventory {
                return args.is_empty();
            }
            if args.len() != 2 {
                return false;
            }
            let Some(action) = args.get("action").and_then(Value::as_str) else {
                return false;
            };
            let Some(id) = args.get("id").and_then(Value::as_str) else {
                return false;
            };
            match action {
                "snapshot" | "activateClean" | "commitClean" | "commitReinstall" => id.is_empty(),
                "restore" | "resume" | "commit" | "rollback" | "updateResume" | "updateCommit"
                | "updateRollback" => {
                    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
                }
                _ => false,
            }
        })
    } else if record_health {
        matches!(request.header.route.as_str(), "updates" | "recovery")
            && request.args.as_object().is_some_and(|args| {
                args.len() == 1
                    && args
                        .get("product")
                        .and_then(Value::as_str)
                        .is_some_and(|product| {
                            product_contract::installation::PRODUCTS.contains(&product)
                        })
            })
    } else if open_directory {
        request.header.route == "products"
            && request.args.as_object().is_some_and(|args| args.is_empty())
    } else if inventory || recovery {
        matches!(
            request.header.route.as_str(),
            "products" | "updates" | "components" | "recovery"
        ) && request.args.as_object().is_some_and(|args| args.is_empty())
    } else {
        installation_tools::component::allowed(&request.header.route, &request.method)
    };
    if !allowed || serde_json::to_vec(&request.args).map_or(true, |value| value.len() > 512 * 1024)
    {
        return Err(Problem {
            provenance,
            code: ProblemCode::Unauthorized,
        });
    }
    let value = if suite_update {
        #[cfg(windows)]
        let result = crate::updates::execute(
            window.app_handle().clone(),
            &request.method,
            request.args,
            request.header.deadline_ms,
        )
        .await;
        #[cfg(not(windows))]
        let result: Result<Value, &'static str> = Err("suite_windows_required");
        result.map_err(str::to_owned)
    } else if restore_inventory || restore_action {
        #[cfg(windows)]
        let result = {
            let app = window.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                if restore_inventory {
                    crate::bootstrap::interactive::inventory()
                } else {
                    let args = serde_json::from_value(request.args)
                        .map_err(|_| "restore_action_invalid")?;
                    crate::bootstrap::interactive::launch(&app, args)
                }
            })
            .await
            .map_err(|_| "restore_worker_unavailable")
            .and_then(|value| value)
        };
        #[cfg(not(windows))]
        let result: Result<Value, &'static str> = Err("suite_windows_required");
        result.map_err(str::to_owned)
    } else if record_health {
        #[cfg(windows)]
        let result = crate::owner_evidence::record(
            window.app_handle().clone(),
            request.args["product"].as_str().unwrap_or_default().into(),
            request.header.deadline_ms,
        )
        .await;
        #[cfg(not(windows))]
        let result: Result<Value, &'static str> = Err("suite_windows_required");
        result.map_err(str::to_owned)
    } else if recovery {
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
                            "backupCount":journal.backup.len(),"dataCheckpointCount":journal.data_checkpoints.len(),"importCount":journal.imports.len(),"recordedOwnerCount":journal.owner_evidence.len(),"healthCheckCount":journal.health_checks.len(),
                            "cleanupPending":journal.cleanup_pending.len(),"failure":journal.failure}))
                    }
                }
            }).await.map_err(|_| "suite_store_unavailable".to_string())
                .and_then(|result:Result<Value, &'static str>| result.map_err(str::to_owned))
        }
    } else if open_directory {
        let app = window.app_handle().clone();
        tauri::async_runtime::spawn_blocking(move || {
            #[cfg(windows)]
            {
                use tauri_plugin_opener::OpenerExt;
                let scope = crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION"))?;
                scope.revalidate()?;
                app.opener()
                    .open_path(scope.review_root(), None::<&str>)
                    .map_err(|_| "installation_folder_unavailable")?;
                scope.revalidate()?;
                Ok(serde_json::json!({"opened":true}))
            }
            #[cfg(not(windows))]
            {
                let _ = app;
                Err("suite_windows_required")
            }
        })
        .await
        .map_err(|_| "installation_folder_unavailable".to_string())
        .and_then(|result: Result<Value, &'static str>| result.map_err(str::to_owned))
    } else if inventory {
        tauri::async_runtime::spawn_blocking(|| {
            #[cfg(windows)]
            let scope = crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION")).ok();
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
        installation_tools::component::dispatch(
            window.app_handle(),
            &request.header.route,
            &request.method,
            request.args,
        )
        .await
    };
    let (outcome, value) = match value {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(issue) => (
            OperationState::Failed {
                code: ProblemCode::Unavailable,
            },
            if suite_update {
                serde_json::json!({"issue":issue})
            } else {
                serde_json::json!({"issue":"manager_tools_unavailable"})
            },
        ),
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
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("control-center")
        .invoke_handler(tauri::generate_handler![execute])
        .setup(|app, _| {
            #[cfg(windows)]
            app.manage(crate::updates::Updates::default());
            installation_tools::component::initialize(app).map_err(std::io::Error::other)?;
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    #[test]
    fn retired_methods_are_not_routed() {
        for parts in [
            vec!["legacy", "cleanup", "preview"],
            vec!["legacy", "cleanup", "apply"],
            vec!["legacy", "cleanup", "list"],
            vec!["cutover", "review"],
            vec!["prepare", "cutover"],
            vec!["legacy", "inventory"],
            vec!["record", "migration", "owner"],
        ] {
            assert_eq!(component_for(&parts.join("_")), None);
        }
        for method in [
            "suite_inventory",
            "record_suite_health",
            "check_suite_update",
            "restore_inventory",
        ] {
            assert_eq!(component_for(method), Some("control-center.delivery"));
        }
    }
}
