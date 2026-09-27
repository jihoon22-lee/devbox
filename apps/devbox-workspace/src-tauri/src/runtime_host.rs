//! Native composition of the execution owner, read-only process observations,
//! the external process-action broker and bounded log readers. No legacy
//! executable, database discovery or generic spawn/unseal command is exposed.
mod observations;
mod reconnect;
use crate::ipc::results::{OwnedTaskAction, ProcessActionReply};
use crate::{definitions::Definitions, host::Host};
use logs_engine::core::SourceSpec;
use ports_engine::component::ProductPortOwner;
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager};

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
pub(crate) struct Owners {
    runtime: OnceLock<Result<()>>,
    processes: OnceLock<Result<()>>,
    logs: OnceLock<Result<()>>,
}
impl Owners {
    pub(crate) fn initialize_runtime(&self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let import_only = product_shell_tauri::suite_import_only(app)?;
        if !import_only && crate::runtime_owner::installed(app)? {
            return Ok(());
        }
        let data = host.component("runtime")?;
        let common = host.component("common")?;
        *self.runtime.get_or_init(|| {
            let initialize = if import_only {
                runtime_engine::component::initialize_import_only_with_sources
            } else {
                runtime_engine::component::initialize_with_sources
            };
            initialize(
                app,
                &data,
                &common,
                Some(Arc::new(crate::platform::task_sources::Sources {
                    host: crate::component::provider_host(app)?,
                })),
            )
            .map_err(|_| "runtime_owner_unavailable")
        })
    }
    fn initialize_processes(&self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        if crate::runtime_owner::installed(app)? {
            return Ok(());
        }
        let data = host.component("processes")?;
        *self.processes.get_or_init(|| {
            ports_engine::component::initialize(app, &data).map_err(|_| "process_owner_unavailable")
        })
    }
    fn initialize_logs(&self, app: &tauri::AppHandle, host: &Arc<Host>) -> Result<()> {
        if crate::runtime_owner::installed(app)? {
            return Ok(());
        }
        self.initialize_runtime(app, host)?;
        let data = host.component("logs")?;
        *self.logs.get_or_init(|| {
            logs_engine::component::initialize(
                app,
                &data,
                workspace_core::runtime_logs::provider(app, host.clone())?,
            )
            .map_err(|_| "logs_owner_unavailable")
        })
    }
}
fn args<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    if !value.is_object() {
        return Err("invalid_request");
    }
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
fn empty(value: Value) -> Result<()> {
    args::<Empty>(value).map(|_| ())
}
use workspace_core::runtime_policy::issue;

pub(crate) async fn session_ports(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: &ProjectContext,
    deadline: u64,
) -> Result<ports_engine::component::PortObservationSnapshot> {
    observations::observe(app, host, definitions, Some(context), deadline)
        .await
        .map(|(snapshot, _)| snapshot)
}

fn navigate(app: &tauri::AppHandle, route: &str, context: Option<&ProjectContext>) -> Result<()> {
    app.emit_to(
        "main",
        "workspace://runtime-navigate",
        json!({"route":route,"context":context,"fromRoute":"runtime"}),
    )
    .map_err(|_| "runtime_navigation_unavailable")
}
async fn open_log(
    app: &tauri::AppHandle,
    host: &Host,
    run_id: &str,
    stream: &str,
    context: Option<&ProjectContext>,
    from_route: &str,
    deadline: u64,
) -> Result<String> {
    if !matches!(stream, "stdout" | "stderr") {
        return Err("invalid_request");
    }
    host.component("runtime")?;
    let revision: String = crate::runtime_owner::query(
        app,
        host,
        &Mutex::new(Definitions::default()),
        workspace_core::runtime_queries::Call::LogRevision {
            run_id: run_id.into(),
        },
        deadline,
    )
    .await?;
    let source = SourceSpec::RuntimeRun {
        run_id: run_id.into(),
        stream: stream.into(),
        revision,
    };
    crate::files_host::current_deadline(deadline)?;
    if let Some(context) = context {
        host.projects()?.binding(context)?;
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    app.emit_to(
        "main",
        "workspace://runtime-log",
        json!({"id":id,"source":source,"context":context,"fromRoute":from_route}),
    )
    .map_err(|_| "runtime_navigation_unavailable")?;
    Ok(id)
}
/// The dispatcher retains a bounded request/context/worker permit around this
/// future, including after a renderer drops its awaiting request.
pub(crate) struct EngineRequest<'a> {
    pub typed: crate::ipc::Call,
    pub component: &'a str,
    pub method: &'a str,
    pub value: Value,
    pub context: Option<&'a ProjectContext>,
    pub deadline: u64,
    pub operation_id: &'a str,
    pub terminals: &'a Arc<crate::terminal_host::Terminals>,
}
pub(crate) async fn dispatch(
    app: &tauri::AppHandle,
    host: &Arc<Host>,
    owners: &Owners,
    definitions: &Mutex<Definitions>,
    request: EngineRequest<'_>,
) -> Result<Value> {
    let EngineRequest {
        component,
        method,
        value,
        context,
        deadline,
        operation_id,
        terminals,
        typed,
    } = request;
    crate::files_host::current_deadline(deadline)?;
    if component == "workspace.logs" && method == "open_webhook_log" {
        return crate::webhook_logs::open(app, value, deadline).await;
    }
    if component == "workspace.runtime" {
        match method {
            "quit_app" => {
                empty(value)?;
                app.exit(0);
                return Ok(Value::Null);
            }
            "show_main_window" => {
                empty(value)?;
                runtime_engine::component::ui::show_main_window(app.clone()).map_err(issue)?;
                return Ok(Value::Null);
            }
            "hide_main_window" => {
                empty(value)?;
                runtime_engine::component::ui::hide_main_window(app.clone()).map_err(issue)?;
                return Ok(Value::Null);
            }
            "take_pending_open" => {
                empty(value)?;
                return Ok(json!(app
                    .try_state::<runtime_engine::component::ui::PendingOpen>()
                    .and_then(|state| state.take())));
            }
            _ => {}
        }
    }
    if matches!(
        component,
        "workspace.processes" | "workspace.process-actions"
    ) {
        match method {
            "open_browser" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    url: String,
                }
                ports_engine::component::ui::open_browser(app.clone(), args::<Input>(value)?.url)
                    .await
                    .map_err(issue)?;
                return Ok(Value::Null);
            }
            "reveal_process" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    pid: u32,
                }
                ports_engine::component::ui::reveal_process(app.clone(), args::<Input>(value)?.pid)
                    .await
                    .map_err(issue)?;
                return Ok(Value::Null);
            }
            _ => {}
        }
    }
    if component == "workspace.logs"
        && matches!(method, "filter_log_records" | "export_log_records")
    {
        if let crate::ipc::Call::Logs(crate::ipc::logs::WorkspaceLogsCall::Engine(call)) = typed {
            return match *call {
                logs_engine::api::LogsCall::FilterLogRecords { records, filter } => {
                    serde_json::to_value(
                        logs_engine::core::filter_records(&records, &filter)
                            .map_err(|_| "runtime_operation_unavailable")?,
                    )
                    .map_err(|_| "invalid_response")
                }
                logs_engine::api::LogsCall::ExportLogRecords { records } => serde_json::to_value(
                    logs_engine::core::export_records(&records)
                        .map_err(|_| "runtime_operation_unavailable")?,
                )
                .map_err(|_| "invalid_response"),
                _ => Err("invalid_request"),
            };
        }
        return Err("invalid_request");
    }
    if crate::runtime_owner::installed(app)? {
        crate::runtime_owner::owner(app).await?;
    }
    owners.initialize_runtime(app, host)?;
    if component == "workspace.logs" && method == "reconnect_runtime_sources" {
        owners.initialize_logs(app, host)?;
        host.component("logs")?;
        return reconnect::execute(app, host, value, deadline).await;
    }
    let result = match component {
        "workspace.runtime" => {
            host.component("runtime")?;
            match method {
                "workspace_task_source" => {
                    empty(value)?;
                    let context = context.ok_or("runtime_context_required")?;
                    let binding = host.projects()?.binding(context)?;
                    let source = match &context.target {
                        product_contract::ExecutionTarget::Windows => {
                            json!({"path":binding.root,"targetKind":"windows","targetDistro":null})
                        }
                        product_contract::ExecutionTarget::Wsl { distro_id } => {
                            #[cfg(windows)]
                            {
                                let distro = crate::platform::wsl_distro::list()?
                                    .into_iter()
                                    .find(|distro| distro.id == *distro_id)
                                    .ok_or("wsl_distro_missing")?;
                                json!({"path":binding.root,"targetKind":"wsl","targetDistro":distro.name})
                            }
                            #[cfg(not(windows))]
                            {
                                let _ = distro_id;
                                return Err("runtime_windows_required");
                            }
                        }
                    };
                    Ok(json!({"context":context,"source":source}))
                }
                "open_run_log_in_log_lens" => {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Input {
                        run_id: String,
                        stream: String,
                    }
                    let input: Input = args(value)?;
                    Ok(
                        json!({"handoffId":open_log(app, host, &input.run_id, &input.stream, context, "tasks", deadline).await?}),
                    )
                }
                "preview_workspace_task_control"
                | "renew_workspace_task_control"
                | "reject_workspace_task_control"
                | "accept_workspace_task_control" => Err("runtime_handoff_review_required"),
                "open_workspace_task_diagnostic" => {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Input {
                        run_id: String,
                        diagnostic_index: u32,
                    }
                    let input: Input = args(value)?;
                    let context = context.ok_or("project_selection_required")?;
                    let target: workspace_core::runtime_queries::Diagnostic =
                        crate::runtime_owner::query(
                            app,
                            host,
                            definitions,
                            workspace_core::runtime_queries::Call::Diagnostic {
                                context: context.clone(),
                                run_id: input.run_id,
                                index: input.diagnostic_index,
                            },
                            deadline,
                        )
                        .await?;
                    let relative = target.relative_path;
                    host.projects()?.binding(context)?;
                    crate::files_host::current_deadline(deadline)?;
                    app.emit_to("main", "workspace://runtime-diagnostic", json!({
                        "id":uuid::Uuid::new_v4().simple().to_string(),"relativePath":relative,"line":target.line,
                        "column":target.column,"runId":target.run_id,"revision":target.revision,"context":context,"fromRoute":"tasks"
                    })).map_err(|_| "runtime_navigation_unavailable")?;
                    Ok(Value::Bool(true))
                }
                _ => dispatch_engine(app, typed, component, method, value, context, deadline).await,
            }
        }
        "workspace.processes" | "workspace.process-actions" => {
            owners.initialize_processes(app, host)?;
            host.component("processes")?;
            match method {
                "list_port_observations" => {
                    empty(value)?;
                    let (value, _) =
                        observations::observe(app, host, definitions, context, deadline).await?;
                    serde_json::to_value(value).map_err(|_| "invalid_response")
                }
                "open_port_owner" | "open_port_log" => {
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Input {
                        action_key: String,
                        stream: Option<String>,
                    }
                    let input: Input = args(value)?;
                    let action = observations::resolve(
                        app,
                        host,
                        definitions,
                        context,
                        deadline,
                        &input.action_key,
                    )
                    .await?;
                    crate::files_host::current_deadline(deadline)?;
                    if method == "open_port_log" {
                        if !action.logs_available {
                            return Err("runtime_log_unavailable");
                        }
                        let id = open_log(
                            app,
                            host,
                            action.run_id.as_deref().ok_or("runtime_log_unavailable")?,
                            input.stream.as_deref().ok_or("invalid_request")?,
                            context,
                            "runtime",
                            deadline,
                        )
                        .await?;
                        Ok(json!({"handoff_id":id}))
                    } else {
                        if input.stream.is_some() {
                            return Err("invalid_request");
                        }
                        match action.owner {
                            ProductPortOwner::Task { id } => {
                                offer_product_open(
                                    app,
                                    devbox_applink::OpenRequest {
                                        target: devbox_applink::OpenTarget::Task { id },
                                        from: Some("workspace".into()),
                                    },
                                )
                                .map_err(issue)?;
                                navigate(app, "tasks", context)?;
                            }
                            ProductPortOwner::Project { id } => {
                                if context.is_none_or(|context| context.worktree_id != id) {
                                    return Err("stale_context");
                                }
                                navigate(app, "overview", context)?;
                            }
                        }
                        Ok(Value::Null)
                    }
                }
                "kill_listener" => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Input {
                        request: ports_engine::component::KillListenerRequest,
                    }
                    let input: Input = args(value)?;
                    input
                        .request
                        .endpoint
                        .validate_listener()
                        .map_err(|_| "invalid_request")?;
                    input
                        .request
                        .identity
                        .validate()
                        .map_err(|_| "invalid_request")?;
                    let _observed = match &input.request.identity {
                        ports_engine::component::ListenerIdentity::Windows { pid, start_time } => {
                            runtime_engine::scheduler::ObservedProcess::Windows {
                                pid: *pid,
                                creation_filetime: start_time
                                    .parse()
                                    .map_err(|_| "invalid_request")?,
                            }
                        }
                        ports_engine::component::ListenerIdentity::Wsl {
                            distro,
                            pid,
                            start_tick,
                        } => runtime_engine::scheduler::ObservedProcess::Wsl {
                            distro: distro.clone(),
                            pid: *pid,
                            start_tick: *start_tick,
                        },
                        ports_engine::component::ListenerIdentity::Container {
                            engine,
                            container_id,
                            distro,
                        } => {
                            if engine != "docker" {
                                return Err("runtime_container_engine_unsupported");
                            }
                            let (container_id, distro) = (container_id.clone(), distro.clone());
                            // The observation owner revalidates the selected endpoint and
                            // container identity. The WSL owner then refreshes its full ID.
                            if crate::runtime_owner::installed(app)? {
                                crate::runtime_owner::call_until(
                                    app,
                                    "workspace.process-actions",
                                    "handoff_container_stop",
                                    json!({"request":input.request}),
                                    "runtime",
                                    context,
                                    deadline,
                                )
                                .await?;
                            } else {
                                ports_engine::api::dispatch(
                                    app,
                                    ports_engine::api::PortsCall::HandoffContainerStop {
                                        request: input.request,
                                    },
                                )
                                .await
                                .map_err(issue)?;
                            }
                            terminals.wsl_control(app,host,"docker_action",json!({"operationId":operation_id,"distro":distro,"containerId":container_id,"action":"stop"}),deadline).await?;
                            return Ok(json!({"kind":"terminated"}));
                        }
                    };
                    let owner: Option<String> = crate::runtime_owner::query(
                        app,
                        host,
                        definitions,
                        workspace_core::runtime_queries::Call::OwningTask {
                            identity: input.request.identity.clone(),
                        },
                        deadline,
                    )
                    .await?;
                    crate::files_host::current_deadline(deadline)?;
                    if let Some(id) = owner {
                        offer_product_open(
                            app,
                            devbox_applink::OpenRequest {
                                target: devbox_applink::OpenTarget::Task { id: id.clone() },
                                from: Some("workspace".into()),
                            },
                        )
                        .map_err(issue)?;
                        navigate(app, "tasks", context)?;
                        Ok(json!(ProcessActionReply::Owned(
                            OwnedTaskAction::OwnedTask { task_id: id }
                        )))
                    } else {
                        ports_engine::component::kill_external_listener(input.request, deadline)
                            .await
                            .map(|value| json!(ProcessActionReply::Native(value)))
                            .map_err(issue)
                    }
                }
                "handoff_container_stop" => Err("runtime_container_owner_unavailable"),
                _ => dispatch_engine(app, typed, component, method, value, context, deadline).await,
            }
        }
        "workspace.logs" => {
            owners.initialize_logs(app, host)?;
            host.component("logs")?;
            match method {
                "send_selection_to_toolbox" => {
                    crate::selection_send::send_logs(app, value, context, deadline).await
                }
                "read_sources" => {
                    crate::selection_logs::begin(
                        value["generation"].as_u64().ok_or("invalid_request")?,
                    );
                    let result = dispatch_engine(
                        app,
                        typed,
                        component,
                        method,
                        value.clone(),
                        context,
                        deadline,
                    )
                    .await?;
                    crate::selection_logs::capture(value, &result, context);
                    Ok(result)
                }
                "preview_log_source" | "accept_log_source" | "discard_log_source"
                | "renew_log_source" => Err("runtime_handoff_review_required"),
                _ => dispatch_engine(app, typed, component, method, value, context, deadline).await,
            }
        }
        _ => Err("invalid_request"),
    };
    host.component("runtime")?;
    result
}

async fn dispatch_engine(
    app: &tauri::AppHandle,
    typed: crate::ipc::Call,
    component: &str,
    method: &str,
    value: Value,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> Result<Value> {
    crate::files_host::current_deadline(deadline)?;
    if crate::runtime_owner::installed(app)? {
        let route = match component {
            "workspace.runtime" => "tasks",
            "workspace.logs" => "logs",
            _ => "runtime",
        };
        crate::runtime_owner::call_until(app, component, method, value, route, context, deadline)
            .await
    } else {
        crate::ipc::dispatch_engine(app, typed).await.map_err(issue)
    }
}
fn offer_product_open(
    app: &tauri::AppHandle,
    request: devbox_applink::OpenRequest,
) -> std::result::Result<(), String> {
    if !runtime_engine::component::ui::is_supported_request(&request) {
        return Err("component_args_invalid".into());
    }
    if app
        .try_state::<runtime_engine::component::ui::PendingOpen>()
        .is_none()
    {
        app.manage(runtime_engine::component::ui::PendingOpen::new());
    }
    app.state::<runtime_engine::component::ui::PendingOpen>()
        .set(request.clone());
    app.emit_to("main", "workspace://tasks-open", request)
        .map_err(|_| "component_delivery_unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::allow_table::permits as allowed;
    #[test]
    fn execution_secret_authority_and_external_process_action_never_share_a_role() {
        assert!(allowed(
            "workspace.process-actions",
            "runtime",
            "kill_listener"
        ));
        assert!(!allowed("workspace.processes", "runtime", "kill_listener"));
        for method in runtime_engine::api::METHODS {
            assert!(allowed("workspace.runtime", "tasks", method));
            assert!(!allowed("workspace.runtime", "runtime", method));
            assert!(!allowed("workspace.process-actions", "runtime", method));
            assert!(!allowed("workspace.logs", "tasks", method));
        }
        for method in [
            "run_job_now",
            "stop_active_run",
            "start_service",
            "stop_service",
            "restart_service",
            "run_workspace_task_operation",
            "stop_workspace_task_operation",
        ] {
            assert!(!allowed("workspace.runtime", "tasks", method));
        }
        for (role, route) in [
            ("workspace.logs", "logs"),
            ("workspace.runtime", "tasks"),
            ("workspace.processes", "runtime"),
            ("workspace.shell", "tasks"),
        ] {
            for arbitrary in [
                "spawn",
                "exec",
                "kill",
                "unseal",
                "read_database",
                "set_data_root",
                "initialize",
            ] {
                assert!(!allowed(role, route, arbitrary));
            }
            assert!(!allowed(role, route, "kill_listener"));
        }
    }
    #[test]
    fn product_request_fields_are_strict() {
        assert!(empty(json!({})).is_ok());
        for value in [json!({"root":"legacy"}), Value::Null, json!([])] {
            assert!(empty(value).is_err());
        }
    }
}
