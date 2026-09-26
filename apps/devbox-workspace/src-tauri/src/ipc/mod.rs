#[cfg(test)]
pub(crate) mod allow_table;
pub mod commands;
pub mod definitions;
pub mod dependencies;
pub mod files;
pub(crate) mod lanes;
pub mod logs;
pub mod lsp;
pub mod problems;
pub mod process_actions;
pub mod processes;
pub mod registry;
pub mod runtime;
pub mod setup;
pub mod source;
pub mod terminal;

use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, IncomingRequest};
use product_shell_tauri::{admit_request, Reply};
use tauri::State;
pub(crate) enum Call {
    Files(files::WorkspaceFilesCall),
    Lsp(lsp::WorkspaceLspCall),
    Source(source::WorkspaceSourceCall),
    Registry(registry::RegistryCall),
    Setup(setup::SetupCall),
    Definitions(definitions::WorkspaceDefinitionsCall),
    Dependencies(dependencies::WorkspaceDependenciesCall),

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
    let request = Request {
        header: request.header,
        component: C::COMPONENT.into(),
        method,
        args,
        typed: request.call.into_call(),
    };
    let response = execute_admitted(window, runtime, request, admission).await?;
    Ok(Reply {
        operation: response.operation,
        value: response.value,
    })
}

pub(crate) async fn dispatch_engine(
    app: &tauri::AppHandle,
    call: Call,
) -> Result<serde_json::Value, String> {
    match call {
        Call::Runtime(runtime::WorkspaceRuntimeCall::Engine(call)) => {
            runtime_engine::api::dispatch(app, *call).await
        }
        Call::Processes(processes::ProcessesCall::Engine(call)) => {
            ports_engine::api::dispatch(app, *call).await
        }
        Call::Logs(logs::WorkspaceLogsCall::Engine(call)) => {
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
    fn setup_is_the_only_component_admitted_during_import() {
        let setup: setup::SetupCall =
            serde_json::from_str(r#"{"method":"start_empty","args":{}}"#).unwrap();
        assert_eq!(setup::SetupCall::COMPONENT, "workspace.setup");
        const {
            assert!(setup::SetupCall::IMPORT_PHASE);
        }
        const {
            assert!(!registry::RegistryCall::IMPORT_PHASE);
        }
        const {
            assert!(!files::WorkspaceFilesCall::IMPORT_PHASE);
        }
        assert_eq!(setup.lane(), Lane::EngineBackground);
        let files: files::WorkspaceFilesCall =
            serde_json::from_str(r#"{"method":"list_workspace_files","args":{"path":"."}}"#)
                .unwrap();
        assert_eq!(files.lane(), Lane::Files);
    }
    #[test]
    fn host_calls_require_native_revisions_and_reviewed_worktree_creation() {
        assert!(serde_json::from_str::<files::WorkspaceFilesCall>(
            r#"{"method":"save_session","args":{"session":{"docs":[]}}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<source::WorkspaceSourceCall>(r#"{"method":"create_worktree","args":{"repoPath":".","branch":"branch","targetDir":"target"}}"#).is_err());
        let create: source::WorkspaceSourceCall = serde_json::from_str(
            r#"{"method":"create_worktree","args":{"previewId":"preview","operationId":"op"}}"#,
        )
        .unwrap();
        assert_eq!(create.routes(), &["source"]);
        assert_eq!(create.deadline_budget_ms(), 29_000);
    }
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
                super::files::METHODS,
                super::files::result_types(&mut export).unwrap(),
            ),
            (
                super::lsp::METHODS,
                super::lsp::result_types(&mut export).unwrap(),
            ),
            (
                super::source::METHODS,
                super::source::result_types(&mut export).unwrap(),
            ),
            (
                super::setup::METHODS,
                super::setup::result_types(&mut export).unwrap(),
            ),
            (
                super::registry::METHODS,
                super::registry::result_types(&mut export).unwrap(),
            ),
            (
                super::definitions::METHODS,
                super::definitions::result_types(&mut export).unwrap(),
            ),
            (
                super::dependencies::METHODS,
                super::dependencies::result_types(&mut export).unwrap(),
            ),
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
            Self::Files(call) => call.lane(),
            Self::Lsp(call) => call.lane(),
            Self::Source(call) => call.lane(),
            Self::Registry(call) => call.lane(),
            Self::Setup(call) => call.lane(),
            Self::Definitions(call) => call.lane(),
            Self::Dependencies(call) => call.lane(),

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

pub mod deadlines;

use crate::component::Runtime;
use product_contract::{Operation, RouteRequest};
use serde::Serialize;
pub(crate) struct Request {
    pub(crate) header: RouteRequest,
    pub(crate) component: String,
    pub(crate) method: String,
    pub(crate) args: Value,
    pub(crate) typed: crate::ipc::Call,
}
#[derive(Serialize)]
pub(crate) struct Response {
    pub(crate) operation: Operation,
    pub(crate) value: Value,
}
use lanes::Lane;
use product_contract::Provenance;
use serde_json::{json, Value};
use std::{sync::atomic::Ordering, time::Duration};
use tauri::{Manager, WebviewWindow};

pub(crate) fn empty(value: &Value) -> Result<(), &'static str> {
    if value.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("invalid_request")
    }
}
pub(crate) fn changes_context(method: &str) -> bool {
    matches!(
        method,
        "select_project"
            | "clear_project"
            | "apply_registration"
            | "remove"
            | "apply_edit"
            | "approve_cleanup_scope"
            | "revoke_cleanup_scope"
            | "approve_trust"
            | "revoke_trust"
            | "save_lsp_config"
            | "lsp_execution_approve"
            | "lsp_execution_revoke"
    )
}
pub(crate) async fn execute_admitted(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    request: Request,
    admission: product_shell_tauri::Admission,
) -> Result<Response, Problem> {
    let rejected = |code| Problem {
        code,
        provenance: Provenance {
            product: "workspace".into(),
            component: "workspace.dispatch".into(),
            request_id: "rejected".into(),
            revision: 1,
        },
    };
    if runtime.shutdown_started.load(Ordering::Acquire) {
        return Err(rejected(ProblemCode::Unavailable));
    }
    let problems = matches!(&request.typed, Call::Problems(_));
    let terminal = matches!(&request.typed, Call::Terminal(_));
    let engine = matches!(
        &request.typed,
        Call::Runtime(_) | Call::Processes(_) | Call::ProcessActions(_) | Call::Logs(_)
    );
    let files = matches!(&request.typed, Call::Files(_));
    let lsp = matches!(&request.typed, Call::Lsp(_));
    let definitions = matches!(&request.typed, Call::Definitions(_));
    let dependencies = matches!(&request.typed, Call::Dependencies(_));
    let source = matches!(&request.typed, Call::Source(_));
    let context_change = changes_context(&request.method);
    // Authenticate/replay-check once before waiting. A single bounded waiter
    // holds no context permit, so active file/metadata workers can retire.
    let provenance = admission.provenance().clone();
    let problem = |code| Problem {
        code,
        provenance: provenance.clone(),
    };
    let context_permit = if problems
        || terminal
        || engine
        || files
        || definitions
        || dependencies
        || source
        || context_change
        || (lsp && crate::lsp_host::contextual(&request.method))
    {
        if context_change {
            let _waiting = runtime
                .lanes
                .try_enter(Lane::Context)
                .map_err(|_| problem(ProblemCode::Overloaded))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| problem(ProblemCode::Expired))?
                .as_millis();
            let remaining = u128::from(request.header.deadline_ms)
                .saturating_sub(now)
                .min(30_000) as u64;
            let deadline = std::time::Instant::now() + Duration::from_millis(remaining);
            Some(
                runtime
                    .context_activity
                    .enter_change(deadline, &runtime.shutdown_started)
                    .await
                    .map_err(|issue| {
                        problem(if issue == "context_expired" {
                            ProblemCode::Expired
                        } else {
                            ProblemCode::Unavailable
                        })
                    })?,
            )
        } else {
            Some(
                runtime
                    .context_activity
                    .enter(false)
                    .map_err(|_| problem(ProblemCode::Unavailable))?,
            )
        }
    } else {
        None
    };
    if context_permit.is_some() {
        // Selection may have changed between authentication and admission.
        // Recheck under the retained permit without replaying authorization.
        if product_shell_tauri::workspace_context(&window)
            .map_err(|_| problem(ProblemCode::Unavailable))?
            != request.header.context
        {
            return Err(problem(ProblemCode::StaleContext));
        }
        crate::files_host::current_deadline(request.header.deadline_ms)
            .map_err(|_| problem(ProblemCode::Expired))?;
        if runtime.shutdown_started.load(Ordering::Acquire) {
            return Err(problem(ProblemCode::Unavailable));
        }
    }
    let document_observation = if lsp
        && matches!(
            request.method.as_str(),
            "open_lsp_document"
                | "change_lsp_document"
                | "reload_lsp_document"
                | "close_lsp_document"
        ) {
        request.header.context.clone().map(|context| {
            (
                context,
                request.method.clone(),
                request
                    .args
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )
        })
    } else {
        None
    };
    let observation = crate::problems_host::begin_observation(
        window.app_handle(),
        runtime.host().ok().as_deref(),
        request.header.context.as_ref(),
        &request.component,
        &request.method,
        &request.args,
    );
    let notify_registry = request.component == "workspace.registry"
        && matches!(request.method.as_str(), "apply_registration" | "remove");
    let select = request.method == "select_project";
    let expected_context = request.header.context.clone();
    let deadline = request.header.deadline_ms;
    let retired = if matches!(
        request.method.as_str(),
        "select_project" | "clear_project" | "apply_registration" | "remove" | "apply_edit"
    ) {
        runtime.retire_lsp().await
    } else {
        Ok(())
    };
    let mut result = if let Err(issue) = retired {
        Err(issue)
    } else if files && request.method == "send_editor_selection" {
        drop(request.typed);
        crate::selection_send::send(
            window.app_handle(),
            request.args,
            request.header.deadline_ms,
        )
        .await
    } else if problems {
        let host = runtime.host();
        match (host, runtime.lanes.try_enter(Lane::Metadata)) {
            (Ok(host), Ok(permit)) => {
                let app = window.app_handle().clone();
                let retained_context = context_permit.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let _retained = (permit, retained_context);
                    crate::files_host::current_deadline(request.header.deadline_ms)?;
                    crate::problems_host::manage(
                        &app,
                        &host,
                        request.header.context.as_ref(),
                        &request.method,
                        request.args,
                    )
                })
                .await
                .unwrap_or(Err("problems_unavailable"))
            }
            (Err(issue), _) | (_, Err(issue)) => Err(issue),
        }
    } else if terminal {
        terminal::execute_terminal_main(&window, &runtime, request, context_permit.clone()).await
    } else if engine {
        runtime::execute_runtime(&window, &runtime, request, context_permit.clone()).await
    } else if lsp {
        lsp::execute_lsp(&window, &runtime, request, context_permit.clone()).await
    } else if files {
        files::execute_files(
            &window,
            &runtime,
            request,
            context_permit.clone().expect("file context permit"),
        )
        .await
    } else if source {
        source::execute_source(
            &window,
            &runtime,
            request,
            context_permit.clone().expect("source context permit"),
        )
        .await
    } else if dependencies {
        dependencies::execute_dependencies(
            &runtime,
            request,
            context_permit.clone().expect("dependency context permit"),
        )
        .await
    } else if definitions {
        definitions::execute_definitions(&window, &runtime, request, context_permit.clone()).await
    } else if request.method == "start_empty" {
        let host = runtime.host();
        let owners = runtime.engines.clone();
        let app = window.app_handle().clone();
        let permit = runtime.lanes.try_enter(Lane::EngineBackground);
        let shutdown = runtime.shutdown_started.clone();
        match (host, permit) {
            (Ok(host), Ok(permit)) => tauri::async_runtime::spawn_blocking(move || {
                let _permit = permit;
                crate::files_host::current_deadline(deadline)?;
                if shutdown.load(Ordering::Acquire) {
                    return Err("request_cancelled");
                }
                empty(&request.args)?;
                host.start_empty()?;
                owners.initialize_runtime(&app, &host)?;
                host.status()
            })
            .await
            .unwrap_or(Err("worker_unavailable")),
            (Err(issue), _) | (_, Err(issue)) => Err(issue),
        }
    } else if request.method == "clear_project" {
        empty(&request.args).and_then(|()| {
            product_shell_tauri::replace_project_context(
                &window,
                expected_context.as_ref(),
                None,
                deadline,
            )?;
            Ok(json!({"context":null}))
        })
    } else if request.method == "status" {
        empty(&request.args).map(|()| runtime.status())
    } else {
        let preview = registry::project_probe(&request.method);
        let lane = if preview {
            Lane::Probes
        } else {
            Lane::Metadata
        };
        match (runtime.host(), runtime.lanes.try_enter(lane)) {
            (Ok(host), Ok(permit)) => {
                let worker_context = context_permit.clone();
                let worker = tauri::async_runtime::spawn_blocking(move || {
                    // A timed-out probe keeps its permit until the OS returns.
                    // A late preview cannot register or grant trust by itself.
                    let (_permit, _context) = (permit, worker_context);
                    match request.typed {
                        Call::Registry(call) => registry::dispatch(&host, call),
                        _ => Err("invalid_request"),
                    }
                });
                if preview {
                    match tokio::time::timeout(Duration::from_secs(15), worker).await {
                        Ok(result) => result.unwrap_or(Err("worker_unavailable")),
                        Err(_) => Err("project_probe_timeout"),
                    }
                } else {
                    worker.await.unwrap_or(Err("worker_unavailable"))
                }
            }
            (Err(issue), _) | (_, Err(issue)) => Err(issue),
        }
    };
    if notify_registry && result.is_ok() {
        use tauri::Emitter;
        let _ = window.emit("workspace-context-changed", ());
    }
    if select {
        result = result.and_then(|value| {
            // The worker produced this context after native Registry/object
            // admission. A timed-out worker never reaches session mutation.
            let context = serde_json::from_value(value["context"].clone())
                .map_err(|_| "context_unavailable")?;
            product_shell_tauri::replace_project_context(
                &window,
                expected_context.as_ref(),
                Some(context),
                deadline,
            )?;
            Ok(value)
        });
    }
    if let (Some((context, method, uri)), Ok(value)) = (document_observation, &result) {
        if let Ok(owner) = crate::problems_host::owner(window.app_handle()) {
            let uri = value.get("uri").and_then(Value::as_str).unwrap_or(&uri);
            let version = value
                .get("version")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok());
            owner.document_changed(&context, uri, version, method == "close_lsp_document");
        }
    }
    crate::problems_host::finish_observation(
        window.app_handle(),
        runtime.host().ok().as_deref(),
        observation,
        &result,
    );
    let response = admission.finish(result.map_err(str::to_owned), crate::ipc::classify);
    Ok(Response {
        operation: response.operation,
        value: response.value,
    })
}
