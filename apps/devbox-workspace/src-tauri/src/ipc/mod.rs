#[cfg(test)]
pub(crate) mod allow_table;
pub mod commands;
pub(crate) mod lanes;
pub mod logs;
pub mod problems;
pub mod process_actions;
pub mod processes;
pub mod runtime;
pub mod terminal;

use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, IncomingRequest};
use product_shell_tauri::{admit_request, Reply};
use tauri::State;
pub(crate) enum Call {
    Runtime(runtime::WorkspaceRuntimeCall),
    Processes(processes::ProcessesCall),
    ProcessActions(process_actions::ProcessActionsCall),
    Logs(logs::WorkspaceLogsCall),
    Terminal(terminal::WorkspaceTerminalCall),
    Problems(problems::ProblemsCall),
    Commands(commands::CommandsCall),
}
pub(crate) trait WorkspaceCall: ComponentCall {
    fn into_call(self) -> Call;
}
/// Bound actual strings and container entries without allocating a second JSON
/// document. DTO deserialization independently rejects unknown fields and types.
pub(crate) fn bounded_arguments(value: &serde_json::Value, limit: usize) -> bool {
    fn consume(value: &serde_json::Value, left: &mut usize) -> bool {
        let own = match value {
            serde_json::Value::String(s) => s.len() + 2,
            serde_json::Value::Object(o) => 2 + o.keys().map(|k| k.len() + 4).sum::<usize>(),
            serde_json::Value::Array(a) => 2 + a.len(),
            _ => 8,
        };
        let Some(next) = left.checked_sub(own) else {
            return false;
        };
        *left = next;
        match value {
            serde_json::Value::Object(o) => o.values().all(|v| consume(v, left)),
            serde_json::Value::Array(a) => a.iter().all(|v| consume(v, left)),
            _ => true,
        }
    }
    let mut remaining = limit;
    value.is_object() && consume(value, &mut remaining)
}
pub(crate) async fn execute<C: WorkspaceCall>(
    window: tauri::WebviewWindow,
    runtime: State<'_, crate::component::Runtime>,
    incoming: IncomingRequest,
) -> Result<Reply, Problem> {
    if runtime.shutting_down() {
        let _guard = product_shell_tauri::begin_operation(&window, C::COMPONENT, &incoming.method);
        return Err(Problem {
            code: ProblemCode::Unavailable,
            provenance: product_contract::Provenance {
                product: "workspace".into(),
                component: "workspace.dispatch".into(),
                request_id: "rejected".into(),
                revision: 1,
            },
        });
    }
    let args = incoming.args.clone();
    let (admission, request) = admit_request::<C>(&window, incoming)?;
    let method = request.call.method().to_owned();
    let request = crate::component::Request {
        header: request.header,
        component: C::COMPONENT.into(),
        method,
        args,
        typed: Some(request.call.into_call()),
    };
    let response =
        crate::component::execute_admitted(window, runtime, request, Some(admission)).await?;
    Ok(Reply {
        operation: response.operation,
        value: response.value,
    })
}

pub(crate) fn migrated_component(component: &str) -> bool {
    matches!(
        component,
        "workspace.runtime"
            | "workspace.processes"
            | "workspace.process-actions"
            | "workspace.logs"
            | "workspace.terminal"
            | "workspace.problems"
            | "workspace.commands"
    )
}
pub(crate) async fn dispatch_engine(
    app: &tauri::AppHandle,
    call: Option<Call>,
    _method: &str,
    _args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match call {
        Some(Call::Runtime(runtime::WorkspaceRuntimeCall::Engine(call))) => {
            runtime_engine::api::dispatch(app, *call).await
        }
        Some(Call::Processes(processes::ProcessesCall::Engine(call))) => {
            ports_engine::api::dispatch(app, *call).await
        }
        Some(Call::Logs(logs::WorkspaceLogsCall::Engine(call))) => {
            logs_engine::api::dispatch(app, *call).await
        }
        _ => Err("component_method_invalid".into()),
    }
}

pub mod issues;
pub(crate) use issues::classify;

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::workspace::Lane;
    #[test]
    fn runtime_family_routes_lanes_and_budgets() {
        let stop: runtime::WorkspaceRuntimeCall = serde_json::from_str(r#"{"method":"runtime_control","args":{"operationId":"op","method":"stop_service","args":{"id":"service"}}}"#).unwrap();
        assert_eq!(stop.lane(), Lane::EngineStop);
        assert_eq!(stop.deadline_budget_ms(), 29_000);
        assert_eq!(stop.routes(), &["tasks"]);
        let dashboard: terminal::WorkspaceTerminalCall =
            serde_json::from_str(r#"{"method":"dashboard_snapshot","args":{}}"#).unwrap();
        assert!(
            dashboard.routes().contains(&"terminal") && dashboard.routes().contains(&"runtime")
        );
        let problems: problems::ProblemsCall =
            serde_json::from_str(r#"{"method":"snapshot","args":{}}"#).unwrap();
        assert!(problems.routes().contains(&"files") && problems.routes().contains(&"problems"));
    }
    #[test]
    fn public_component_cannot_acquire_companion_or_process_action_authority() {
        assert!(serde_json::from_str::<terminal::WorkspaceTerminalCall>(
            r#"{"method":"write_session","args":{"sessionId":"s","data":"text"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<processes::ProcessesCall>(
            r#"{"method":"kill_listener","args":{"request":null}}"#
        )
        .is_err());
        assert!(processes::routes_for("kill_listener").is_empty());
        assert!(commands::METHODS.is_empty());
    }
    #[test]
    fn body_limits_bound_aggregate_payload_without_serializing_it() {
        assert!(bounded_arguments(
            &serde_json::json!({"text":"bounded"}),
            32
        ));
        assert!(!bounded_arguments(
            &serde_json::json!({"a":"x".repeat(20),"b":"y".repeat(20)}),
            32
        ));
        assert!(!bounded_arguments(&serde_json::json!([]), 32));
    }
}

#[cfg(test)]
mod issue_tests {
    #[test]
    fn method_names_and_remote_text_are_not_error_codes() {
        assert_eq!(super::classify("list_jobs"), "unavailable");
        assert_eq!(super::classify("synthetic_remote_secret"), "unavailable");
        assert_eq!(super::classify("request_expired"), "request_expired");
    }
}

pub(crate) mod results;

#[cfg(test)]
mod binding_tests {
    #[test]
    fn native_result_maps_cover_each_admitted_method_without_type_collisions() {
        let out = tempfile::tempdir().unwrap();
        let cfg = product_ipc::ts_rs::Config::new()
            .with_large_int("number")
            .with_out_dir(out.path());
        let mut export = product_ipc::TypeExporter::new(&cfg);
        super::companion::result_types(&mut export).unwrap();
        for (methods, results) in [
            (
                super::runtime::METHODS,
                super::runtime::result_types(&mut export).unwrap(),
            ),
            (
                super::processes::METHODS,
                super::processes::result_types(&mut export).unwrap(),
            ),
            (
                super::process_actions::METHODS,
                super::process_actions::result_types(&mut export).unwrap(),
            ),
            (
                super::logs::METHODS,
                super::logs::result_types(&mut export).unwrap(),
            ),
            (
                super::terminal::METHODS,
                super::terminal::result_types(&mut export).unwrap(),
            ),
            (
                super::problems::METHODS,
                super::problems::result_types(&mut export).unwrap(),
            ),
            (
                super::commands::METHODS,
                super::commands::result_types(&mut export).unwrap(),
            ),
        ] {
            let mut actual = results
                .iter()
                .map(|(method, _)| *method)
                .collect::<Vec<_>>();
            let mut expected = methods.to_vec();
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected);
        }
    }
}

impl Call {
    pub(crate) fn lane(&self) -> product_ipc::workspace::Lane {
        match self {
            Self::Runtime(call) => call.lane(),
            Self::Processes(call) => call.lane(),
            Self::ProcessActions(call) => call.lane(),
            Self::Logs(call) => call.lane(),
            Self::Terminal(call) => call.lane(),
            Self::Problems(call) => call.lane(),
            Self::Commands(call) => call.lane(),
        }
    }
}

pub mod companion;
