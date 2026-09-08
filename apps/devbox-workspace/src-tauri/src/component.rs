//! Native product command admission. Route names do not grant Registry writes.
use crate::{host::Host, project_owner::RegistrationAction};
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tauri::{Manager, State, WebviewWindow};

#[derive(Clone, Default)]
struct Pool(Arc<AtomicUsize>);
struct Permit(Arc<AtomicUsize>);
impl Pool {
    fn reserve(&self) -> Result<Permit, &'static str> {
        self.reserve_with_limit(2)
    }
    fn reserve_with_limit(&self, limit: usize) -> Result<Permit, &'static str> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < limit).then_some(n + 1)
            })
            .map_err(|_| "busy")?;
        Ok(Permit(self.0.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Clone)]
struct Runtime {
    context_activity: crate::core::context_activity::ContextActivity,
    host: Arc<Mutex<Result<Arc<Host>, &'static str>>>,
    metadata: Pool,
    probes: Pool,
    files: Arc<Mutex<crate::files_host::FilesHost>>,
    file_requests: Pool,
    file_workers: Arc<tokio::sync::Semaphore>,
    dialogs: Pool,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            context_activity: Default::default(),
            host: Arc::new(Mutex::new(Err("initializing"))),
            metadata: Pool::default(),
            probes: Pool::default(),
            files: Arc::default(),
            file_requests: Pool::default(),
            file_workers: Arc::new(tokio::sync::Semaphore::new(2)),
            dialogs: Pool::default(),
        }
    }
}
impl Runtime {
    fn host(&self) -> Result<Arc<Host>, &'static str> {
        self.host.try_lock().map_err(|_| "busy")?.clone()
    }
    fn status(&self) -> Value {
        match self.host().and_then(|host| host.status()) {
            Ok(status) => {
                json!({"phase": if status["selected"] == true {"selected"} else {"setup"}})
            }
            Err("initializing") => json!({"phase":"loading"}),
            Err(issue) => json!({"phase":"failed", "issue":issue}),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    header: RouteRequest,
    component: String,
    method: String,
    args: Value,
}
#[derive(Serialize)]
struct Response {
    operation: Operation,
    value: Value,
}
fn allowed(component: &str, route: &str, method: &str) -> bool {
    if !matches!(route, "overview" | "source" | "files" | "dependencies") {
        return false;
    }
    match component {
        "workspace.files" | "workspace.lsp" => {
            route == "files" && crate::files_host::allowed(component, method)
        }
        "workspace.migration" => matches!(method, "status" | "start_empty"),
        "workspace.registry" => matches!(
            method,
            "snapshot"
                | "preview_windows"
                | "cancel_registration"
                | "apply_registration"
                | "rename"
                | "remove"
                | "select_project"
                | "clear_project"
        ),
        _ => false,
    }
}

async fn execute_files(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    use tauri_plugin_dialog::DialogExt;
    let queued = runtime.file_requests.reserve_with_limit(16)?;
    let mut deadline = request.header.deadline_ms;
    let chosen = if request.method == "pick_files" {
        empty(&request.args)?;
        let dialog = runtime.dialogs.reserve_with_limit(1)?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(window)
            .pick_files(move |paths| {
                // Retain the sole dialog slot until the native chooser closes.
                let _dialog = dialog;
                let _ = sender.send(paths);
            });
        let Some(paths) = receiver.await.map_err(|_| "file_dialog_unavailable")? else {
            return Ok(json!([]));
        };
        // The explicit native user choice can outlive the original RPC's
        // deadline. Re-admit the same installation/session/context/window with
        // a native request before using any returned path or saving a choice.
        let mut admitted = request.header.clone();
        admitted.request_id = uuid::Uuid::new_v4().to_string();
        admitted.deadline_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64
            + 5000;
        product_shell_tauri::authorize(window, &admitted, "workspace.files")
            .map_err(|_| "file_selection_expired")?;
        deadline = admitted.deadline_ms;
        Some(
            paths
                .into_iter()
                .map(|path| path.into_path().map_err(|_| "invalid_file_path"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };
    let worker = runtime
        .file_workers
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| "worker_unavailable")?;
    let files = runtime.files.clone();
    let host = runtime.host()?;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (_queued, _worker, _context) = (queued, worker, context_permit);
        let mut files = files.lock().map_err(|_| "files_unavailable")?;
        crate::files_host::current_deadline(deadline)?;
        files.initialize(&app, &host)?;
        if let Some(paths) = chosen {
            files.approve_native_selection(&paths)
        } else {
            files.execute(
                &app,
                &host,
                crate::files_host::Invocation {
                    context: request.header.context.as_ref(),
                    component: &request.component,
                    method: &request.method,
                    args: request.args,
                    deadline,
                },
            )
        }
    })
    .await
    .unwrap_or(Err("worker_unavailable"))
}
fn input<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, &'static str> {
    serde_json::from_value(value).map_err(|_| "invalid_request")
}
fn empty(value: &Value) -> Result<(), &'static str> {
    if value.as_object().is_some_and(|object| object.is_empty()) {
        Ok(())
    } else {
        Err("invalid_request")
    }
}
fn dispatch(host: &Host, method: &str, args: Value) -> Result<Value, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Root {
        root: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct PreviewId {
        preview_id: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Apply {
        preview_id: String,
        name: String,
        action: RegistrationAction,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Rename {
        revision: u64,
        project_id: String,
        name: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Remove {
        revision: u64,
        context: product_contract::ProjectContext,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Select {
        context: product_contract::ProjectContext,
    }
    match method {
        "start_empty" => {
            empty(&args)?;
            host.start_empty()?;
            Ok(json!({}))
        }
        "snapshot" => {
            empty(&args)?;
            Ok(json!(host.projects()?.snapshot()?))
        }
        "select_project" => {
            let value: Select = input(args)?;
            let lease = host.projects()?.admit(&value.context)?;
            Ok(json!({"context":value.context,"binding":lease.binding()}))
        }
        "preview_windows" => {
            let value: Root = input(args)?;
            Ok(json!(host.projects()?.preview_windows(&value.root)?))
        }
        "cancel_registration" => {
            let value: PreviewId = input(args)?;
            host.projects()?.cancel(&value.preview_id)?;
            Ok(json!({}))
        }
        "apply_registration" => {
            let value: Apply = input(args)?;
            let (registry, context) =
                host.projects()?
                    .apply(&value.preview_id, &value.name, value.action)?;
            Ok(json!({"registry":registry,"context":context}))
        }
        "rename" => {
            let value: Rename = input(args)?;
            Ok(json!(host.projects()?.rename(
                value.revision,
                &value.project_id,
                &value.name
            )?))
        }
        "remove" => {
            let value: Remove = input(args)?;
            Ok(json!(host
                .projects()?
                .remove(value.revision, &value.context)?))
        }
        _ => Err("invalid_request"),
    }
}
#[tauri::command]
async fn execute(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    request: Request,
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
    if !allowed(&request.component, &request.header.route, &request.method)
        || !request.args.is_object()
        || serde_json::to_vec(&request.args).map_or(true, |bytes| {
            bytes.len()
                > if request.component == "workspace.files" {
                    64 * 1024 * 1024
                } else {
                    64 * 1024
                }
        })
    {
        return Err(rejected(ProblemCode::InvalidRequest));
    }
    let files = matches!(
        request.component.as_str(),
        "workspace.files" | "workspace.lsp"
    );
    let context_change = matches!(
        request.method.as_str(),
        "select_project" | "clear_project" | "apply_registration" | "remove"
    );
    // Acquire before checking the session, and retain through queued/native
    // work. Worker clones keep the boundary after caller timeout/cancellation.
    let context_permit = if files || context_change {
        Some(
            runtime
                .context_activity
                .enter(context_change)
                .map_err(|_| rejected(ProblemCode::Unavailable))?,
        )
    } else {
        None
    };
    let provenance = product_shell_tauri::authorize(&window, &request.header, &request.component)?;
    let select = request.method == "select_project";
    let expected_context = request.header.context.clone();
    let deadline = request.header.deadline_ms;
    let mut result = if matches!(
        request.component.as_str(),
        "workspace.files" | "workspace.lsp"
    ) {
        execute_files(
            &window,
            &runtime,
            request,
            context_permit.clone().expect("file context permit"),
        )
        .await
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
        let preview = request.method == "preview_windows" || select;
        let pool = if preview {
            &runtime.probes
        } else {
            &runtime.metadata
        };
        match (runtime.host(), pool.reserve()) {
            (Ok(host), Ok(permit)) => {
                let worker_context = context_permit.clone();
                let worker = tauri::async_runtime::spawn_blocking(move || {
                    // A timed-out probe keeps its permit until the OS returns.
                    // A late preview cannot register or grant trust by itself.
                    let (_permit, _context) = (permit, worker_context);
                    dispatch(&host, &request.method, request.args)
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
    let (outcome, value) = match result {
        Ok(value) => (OperationState::Succeeded {}, value),
        Err(issue) => (
            OperationState::Failed {
                code: ProblemCode::Unavailable,
            },
            json!({"issue":issue}),
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
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("workspace")
        .invoke_handler(tauri::generate_handler![execute])
        .setup(|app, _| {
            let runtime = Runtime::default();
            app.manage(runtime.clone());
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let root = app
                        .path()
                        .app_local_data_dir()
                        .map_err(|_| "store_unavailable")?;
                    std::fs::create_dir_all(&root).map_err(|_| "store_unavailable")?;
                    Host::open(&root).map(Arc::new)
                })
                .await
                .unwrap_or(Err("worker_unavailable"));
                if let Ok(mut state) = runtime.host.lock() {
                    *state = result;
                }
                loop {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if let (Ok(host), Ok(permit)) = (runtime.host(), runtime.metadata.reserve()) {
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            let _permit = permit;
                            if let Ok(projects) = host.projects() {
                                let _ = projects.expire();
                            }
                        })
                        .await;
                    }
                }
            });
            Ok(())
        })
        .build()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_and_activation_roles_are_closed_and_probes_remain_bounded() {
        assert!(allowed("workspace.registry", "overview", "preview_windows"));
        assert!(!allowed(
            "workspace.migration",
            "overview",
            "apply_registration"
        ));
        assert!(!allowed("workspace.registry", "runtime", "snapshot"));
        assert!(!allowed("workspace.shell", "overview", "start_empty"));
        let pool = Pool::default();
        let one = pool.reserve().unwrap();
        let two = pool.reserve().unwrap();
        assert!(pool.reserve().is_err());
        drop(one);
        assert!(pool.reserve().is_ok());
        drop(two);
    }
}
