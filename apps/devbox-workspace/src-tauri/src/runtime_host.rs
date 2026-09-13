//! Native composition of the execution owner, read-only process observations,
//! the external process-action broker and bounded log readers. No legacy
//! executable, database discovery or generic spawn/unseal command is exposed.
mod observations;
mod settings;
use crate::{definitions::Definitions, host::Host};
use log_lens_lib::core::{CoreError, RuntimeLogLease, RuntimeLogProvider, SourceSpec};
use port_manager_lib::component::{ProductBindings, ProductPortOwner, SnapshotSourceState};
use product_contract::ProjectContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Emitter;

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
pub(crate) struct Owners {
    settings: Mutex<Option<settings::Review>>,
    runtime: OnceLock<Result<()>>,
    processes: OnceLock<Result<()>>,
    logs: OnceLock<Result<()>>,
}
impl Owners {
    pub(crate) fn initialize_runtime(&self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let data = host.component("runtime")?;
        let common = host.component("common")?;
        *self.runtime.get_or_init(|| {
            run_manager_lib::component::initialize_with_sources(
                app,
                &data,
                &common,
                host.storage_root()
                    .parent()
                    .ok_or("runtime_owner_unavailable")?,
                Some(Arc::new(crate::platform::task_sources::Sources {
                    host: crate::component::provider_host(app)?,
                })),
            )
            .map_err(|_| "runtime_owner_unavailable")
        })
    }
    fn initialize_processes(&self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let data = host.component("processes")?;
        *self.processes.get_or_init(|| {
            port_manager_lib::component::initialize(app, &data)
                .map_err(|_| "process_owner_unavailable")
        })
    }
    fn initialize_logs(&self, app: &tauri::AppHandle, host: &Arc<Host>) -> Result<()> {
        self.initialize_runtime(app, host)?;
        let data = host.component("logs")?;
        *self.logs.get_or_init(|| {
            log_lens_lib::component::initialize(
                app,
                &data,
                Arc::new(RuntimeLogs {
                    app: app.clone(),
                    host: host.clone(),
                    protected: crate::platform::storage_paths::from_host(app, host)?,
                }),
            )
            .map_err(|_| "logs_owner_unavailable")
        })
    }
}
struct RuntimeLogs {
    app: tauri::AppHandle,
    host: Arc<Host>,
    protected: crate::platform::storage_paths::ProtectedStorage,
}
struct OwnedLog {
    host: Arc<Host>,
    lease: run_manager_lib::component::OwnedRunLog,
}
impl RuntimeLogProvider for RuntimeLogs {
    fn validate_source(&self, source: &SourceSpec) -> std::result::Result<(), CoreError> {
        if matches!(source, SourceSpec::Run { .. }) {
            return Err(CoreError::InvalidSource);
        }
        if let SourceSpec::LocalFile { path } | SourceSpec::Directory { path, .. } = source {
            let path = std::path::Path::new(path);
            self.protected
                .ensure_user_path(path)
                .map_err(|_| CoreError::InvalidSource)?;
            devbox_filesystem::ensure_no_links(path).map_err(|_| CoreError::InvalidSource)?;
            if let Ok(canonical) = std::fs::canonicalize(path) {
                self.protected
                    .ensure_user_path(&canonical)
                    .map_err(|_| CoreError::InvalidSource)?;
            }
        }
        Ok(())
    }

    fn resolve(
        &self,
        run_id: &str,
        revision: &str,
    ) -> std::result::Result<Box<dyn RuntimeLogLease>, CoreError> {
        self.host
            .component("runtime")
            .map_err(|_| CoreError::AdapterUnavailable)?;
        let lease = run_manager_lib::component::log_descriptor(&self.app, run_id)
            .map_err(|_| CoreError::AdapterUnavailable)?;
        if lease.revision() != revision {
            return Err(CoreError::StaleOperation);
        }
        Ok(Box::new(OwnedLog {
            host: self.host.clone(),
            lease,
        }))
    }
}
impl RuntimeLogLease for OwnedLog {
    fn data_root(&self) -> &std::path::Path {
        self.lease.data_root()
    }
    fn revalidate(&self) -> std::result::Result<(), CoreError> {
        self.host
            .component("runtime")
            .map_err(|_| CoreError::StaleOperation)?;
        self.lease
            .revalidate()
            .map_err(|_| CoreError::StaleOperation)
    }
}
pub(crate) fn component(name: &str) -> bool {
    matches!(
        name,
        "workspace.runtime"
            | "workspace.processes"
            | "workspace.process-actions"
            | "workspace.logs"
    )
}
pub(crate) fn allowed(component: &str, route: &str, method: &str) -> bool {
    match component {
        "workspace.runtime" => {
            route == "tasks"
                && (method == "workspace_task_source"
                    || run_manager_lib::component::COMMANDS.contains(&method))
                && !run_manager_lib::component::legacy_control_method(method)
        }
        "workspace.processes" => {
            route == "runtime"
                && method != "kill_listener"
                && port_manager_lib::component::COMMANDS.contains(&method)
        }
        "workspace.process-actions" => route == "runtime" && method == "kill_listener",
        "workspace.logs" => route == "logs" && log_lens_lib::component::COMMANDS.contains(&method),
        _ => false,
    }
}
pub(crate) fn stops(method: &str) -> bool {
    matches!(
        method,
        "runtime_import_cancel"
            | "stop_service"
            | "stop_active_run"
            | "stop_workspace_task_operation"
            | "cancel_read"
            | "quit_app"
            | "cancel_workspace_task_import"
            | "cancel_project_import"
    )
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
fn issue(error: String) -> &'static str {
    // Existing engines also return OS/SQLite diagnostics. Only fixed public
    // codes can cross this product boundary; no path/SQL/environment is echoed.
    match error.as_str() {
        "runtime_control_in_progress" => "runtime_control_in_progress",
        "runtime_control_recovery_required" => "runtime_control_recovery_required",
        "runtime_control_failed" => "runtime_control_failed",
        "runtime_control_unavailable" => "runtime_control_unavailable",
        "runtime_control_owner_unsettled" => "runtime_control_owner_unsettled",
        "runtime_settings_unavailable" => "runtime_settings_unavailable",
        "runtime_settings_invalid" => "runtime_settings_invalid",
        "runtime_settings_conflict" => "runtime_settings_conflict",
        "runtime_import_busy" => "runtime_import_busy",
        "runtime_import_destination_conflict" => "runtime_import_destination_conflict",
        "runtime_import_stale" => "runtime_import_stale",
        "component_args_invalid" => "invalid_request",
        "component_storage_changed" => "runtime_store_changed",
        "runtime_log_changed" => "runtime_log_changed",
        "runtime_log_unavailable" | "runtime_log_identity_invalid" => "runtime_log_unavailable",
        "process_action_stale" => "process_action_stale",
        "process_owner_unsettled" => "process_owner_unsettled",
        "process_action_invalid" => "invalid_request",
        "process_observation_unavailable" => "process_observation_unavailable",
        "workspace-task-source-changed" => "runtime_task_source_changed",
        "workspace-task-source-untrusted" | "workspace-task-shell-untrusted" => {
            "runtime_task_review_required"
        }
        _ => "runtime_operation_unavailable",
    }
}
fn project_bindings(
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> Result<Vec<devbox_integration::PortBindingEntry>> {
    let Some(context) = context else {
        return Ok(Vec::new());
    };
    let registry = host.projects()?.snapshot()?;
    let name = registry
        .projects
        .iter()
        .find(|project| project.id == context.project_id)
        .map(|project| project.name.as_str())
        .ok_or("stale_context")?;
    let label = if name.len() > 256 || devbox_applink::contains_sensitive_value(name) {
        "Workspace project"
    } else {
        name
    };
    let ports = definitions
        .lock()
        .map_err(|_| "busy")?
        .runtime_ports(host, context, deadline)?;
    Ok(ports
        .into_iter()
        .map(
            |port| devbox_integration::PortBindingEntry::WorkbenchProfile {
                id: context.worktree_id.clone(),
                label: label.into(),
                port,
            },
        )
        .collect())
}
fn bindings(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> ProductBindings {
    ProductBindings {
        runtime: run_manager_lib::component::port_bindings(app)
            .map_err(|_| SnapshotSourceState::Invalid),
        projects: project_bindings(host, definitions, context, deadline)
            .map_err(|_| SnapshotSourceState::Invalid),
        captured_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or(0),
    }
}
pub(crate) async fn session_ports(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    context: &ProjectContext,
    deadline: u64,
) -> Result<port_manager_lib::component::PortObservationSnapshot> {
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
fn open_log(
    app: &tauri::AppHandle,
    host: &Host,
    run_id: &str,
    stream: &str,
    context: Option<&ProjectContext>,
    from_route: &str,
) -> Result<String> {
    if !matches!(stream, "stdout" | "stderr") {
        return Err("invalid_request");
    }
    host.component("runtime")?;
    let lease = run_manager_lib::component::log_descriptor(app, run_id).map_err(issue)?;
    let source = SourceSpec::RuntimeRun {
        run_id: run_id.into(),
        stream: stream.into(),
        revision: lease.revision().into(),
    };
    lease.revalidate().map_err(issue)?;
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
    } = request;
    crate::files_host::current_deadline(deadline)?;
    owners.initialize_runtime(app, host)?;
    if matches!(
        method,
        "preview_legacy_runtime_settings"
            | "apply_legacy_runtime_settings"
            | "reconnect_runtime_sources"
    ) {
        return settings::execute(app, host, owners, component, method, value, deadline);
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
                        json!({"handoffId":open_log(app, host, &input.run_id, &input.stream, context, "tasks")?}),
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
                    let target = run_manager_lib::component::diagnostic_target(
                        app,
                        &input.run_id,
                        input.diagnostic_index,
                    )
                    .await
                    .map_err(issue)?;
                    let context = context.ok_or("project_selection_required")?;
                    if !crate::platform::task_sources::diagnostic_matches(
                        app,
                        host,
                        context,
                        &input.run_id,
                    ) {
                        return Err("runtime_diagnostic_target_mismatch");
                    }
                    let windows_lease =
                        if matches!(context.target, product_contract::ExecutionTarget::Windows) {
                            Some(host.projects()?.admit(context)?)
                        } else {
                            None
                        };
                    let relative = if matches!(
                        context.target,
                        product_contract::ExecutionTarget::Wsl { .. }
                    ) {
                        let binding = host.projects()?.binding(context)?;
                        target
                            .path
                            .strip_prefix(&binding.root)
                            .map_err(|_| "runtime_diagnostic_target_mismatch")?
                            .to_str()
                            .ok_or("runtime_diagnostic_target_mismatch")?
                            .to_owned()
                    } else {
                        let lease = windows_lease
                            .as_ref()
                            .ok_or("runtime_diagnostic_target_mismatch")?;
                        let root = std::fs::canonicalize(&lease.binding().root)
                            .map_err(|_| "runtime_diagnostic_target_mismatch")?;
                        let path = std::fs::canonicalize(&target.path)
                            .map_err(|_| "runtime_diagnostic_target_mismatch")?;
                        path.strip_prefix(&root)
                            .map_err(|_| "runtime_diagnostic_target_mismatch")?
                            .to_str()
                            .ok_or("runtime_diagnostic_target_mismatch")?
                            .replace('\\', "/")
                    };
                    if relative.is_empty()
                        || relative
                            .split('/')
                            .any(|part| part.is_empty() || matches!(part, "." | ".."))
                    {
                        return Err("runtime_diagnostic_target_mismatch");
                    }
                    if let Some(lease) = windows_lease {
                        lease.revalidate()?;
                    }
                    host.projects()?.binding(context)?;
                    crate::files_host::current_deadline(deadline)?;
                    app.emit_to("main", "workspace://runtime-diagnostic", json!({
                        "id":uuid::Uuid::new_v4().simple().to_string(),"relativePath":relative,"line":target.line,
                        "column":target.column,"runId":target.run_id,"revision":target.revision,"context":context,"fromRoute":"tasks"
                    })).map_err(|_| "runtime_navigation_unavailable")?;
                    Ok(Value::Bool(true))
                }
                _ => run_manager_lib::component::dispatch(app, method, value)
                    .await
                    .map_err(issue),
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
                        )?;
                        Ok(json!({"handoff_id":id}))
                    } else {
                        if input.stream.is_some() {
                            return Err("invalid_request");
                        }
                        match action.owner {
                            ProductPortOwner::Task { id } => {
                                run_manager_lib::component::offer_product_open(
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
                        request: port_manager_lib::component::KillListenerRequest,
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
                    let observed = match &input.request.identity {
                        port_manager_lib::component::ListenerIdentity::Windows {
                            pid,
                            start_time,
                        } => run_manager_lib::scheduler::ObservedProcess::Windows {
                            pid: *pid,
                            creation_filetime: start_time.parse().map_err(|_| "invalid_request")?,
                        },
                        port_manager_lib::component::ListenerIdentity::Wsl {
                            distro,
                            pid,
                            start_tick,
                        } => run_manager_lib::scheduler::ObservedProcess::Wsl {
                            distro: distro.clone(),
                            pid: *pid,
                            start_tick: *start_tick,
                        },
                        port_manager_lib::component::ListenerIdentity::Container {
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
                            port_manager_lib::component::dispatch(
                                app,
                                "handoff_container_stop",
                                json!({"request":input.request}),
                            )
                            .await
                            .map_err(issue)?;
                            terminals.wsl_control(app,host,"docker_action",json!({"operationId":operation_id,"distro":distro,"containerId":container_id,"action":"stop"}),deadline).await?;
                            return Ok(json!({"kind":"terminated"}));
                        }
                    };
                    let owner = run_manager_lib::component::owning_task(app, observed)
                        .await
                        .map_err(issue)?;
                    crate::files_host::current_deadline(deadline)?;
                    if let Some(id) = owner {
                        run_manager_lib::component::offer_product_open(
                            app,
                            devbox_applink::OpenRequest {
                                target: devbox_applink::OpenTarget::Task { id: id.clone() },
                                from: Some("workspace".into()),
                            },
                        )
                        .map_err(issue)?;
                        navigate(app, "tasks", context)?;
                        Ok(json!({"kind":"ownedTask","taskId":id}))
                    } else {
                        port_manager_lib::component::kill_external_listener(input.request, deadline)
                            .await
                            .map_err(issue)
                    }
                }
                "handoff_container_stop" => Err("runtime_container_owner_unavailable"),
                _ => port_manager_lib::component::dispatch(app, method, value)
                    .await
                    .map_err(issue),
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
                    let result = log_lens_lib::component::dispatch(app, method, value.clone())
                        .await
                        .map_err(issue)?;
                    crate::selection_logs::capture(value, &result, context);
                    Ok(result)
                }
                "preview_log_source" | "accept_log_source" | "discard_log_source"
                | "renew_log_source" => Err("runtime_handoff_review_required"),
                _ => log_lens_lib::component::dispatch(app, method, value)
                    .await
                    .map_err(issue),
            }
        }
        _ => Err("invalid_request"),
    };
    host.component("runtime")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_secret_authority_and_external_process_action_never_share_a_role() {
        assert!(allowed(
            "workspace.process-actions",
            "runtime",
            "kill_listener"
        ));
        assert!(!allowed("workspace.processes", "runtime", "kill_listener"));
        for method in run_manager_lib::component::COMMANDS {
            assert_eq!(
                allowed("workspace.runtime", "tasks", method),
                !run_manager_lib::component::legacy_control_method(method)
            );
            assert!(!allowed("workspace.runtime", "runtime", method));
            assert!(!allowed("workspace.process-actions", "runtime", method));
            assert!(!allowed("workspace.logs", "tasks", method));
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
