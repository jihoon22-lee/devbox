//! Typed native metadata queries. Renderer allowlists do not expose this broker.
use crate::{definitions::Definitions, runtime_policy::issue, Host};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Call {
    ObservePorts {
        context: Option<ProjectContext>,
    },
    ResolvePort {
        context: Option<ProjectContext>,
        key: String,
    },
    LogRevision {
        run_id: String,
    },
    DiagnosticIdentity {
        context: ProjectContext,
        run_id: String,
    },
    Diagnostic {
        context: ProjectContext,
        run_id: String,
        index: u32,
    },
    RunningProject {
        context: ProjectContext,
    },
    OwningTask {
        identity: ports_engine::component::ListenerIdentity,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Diagnostic {
    pub relative_path: String,
    pub line: u32,
    pub column: Option<u32>,
    pub run_id: String,
    pub revision: String,
}
impl Call {
    pub fn context(&self) -> Option<&ProjectContext> {
        match self {
            Self::ObservePorts { context } | Self::ResolvePort { context, .. } => context.as_ref(),
            Self::DiagnosticIdentity { context, .. }
            | Self::Diagnostic { context, .. }
            | Self::RunningProject { context } => Some(context),
            _ => None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(context) = self.context() {
            context.validate().map_err(|_| "invalid_request")?;
        }
        let id = match self {
            Self::LogRevision { run_id }
            | Self::DiagnosticIdentity { run_id, .. }
            | Self::Diagnostic { run_id, .. } => Some(run_id),
            Self::ResolvePort { key, .. } => Some(key),
            Self::OwningTask { identity } => {
                identity.validate().map_err(|_| "invalid_request")?;
                None
            }
            _ => None,
        };
        if id.is_some_and(|id| !product_contract::commands::opaque_id(id)) {
            return Err("invalid_request");
        }
        Ok(())
    }
}
pub async fn execute(
    app: &tauri::AppHandle,
    host: &Host,
    definitions: &Mutex<Definitions>,
    call: Call,
    deadline: u64,
) -> Result<Value> {
    call.validate()?;
    crate::current_deadline(deadline)?;
    if let Some(context) = call.context() {
        host.projects()?.binding(context)?;
    }
    match call {
        Call::ObservePorts { context } => {
            let (snapshot, _) = crate::runtime_observations::observe(
                app,
                host,
                definitions,
                context.as_ref(),
                deadline,
            )
            .await?;
            serde_json::to_value(snapshot).map_err(|_| "invalid_response")
        }
        Call::ResolvePort { context, key } => serde_json::to_value(
            crate::runtime_observations::resolve(
                app,
                host,
                definitions,
                context.as_ref(),
                deadline,
                &key,
            )
            .await?,
        )
        .map_err(|_| "invalid_response"),
        Call::LogRevision { run_id } => {
            let lease = runtime_engine::component::log_descriptor(app, &run_id).map_err(issue)?;
            lease.revalidate().map_err(issue)?;
            Ok(json!(lease.revision()))
        }
        Call::DiagnosticIdentity { context, run_id } => {
            if !crate::task_sources::diagnostic_matches(app, host, &context, &run_id) {
                return Ok(Value::Null);
            }
            let (_, job, generation) =
                runtime_engine::component::diagnostic_identity(app, &run_id).map_err(issue)?;
            Ok(json!([job, generation]))
        }
        Call::RunningProject { context } => {
            let binding = host.projects()?.binding(&context)?;
            let identity = if matches!(context.target, ExecutionTarget::Wsl { .. }) {
                Some(crate::task_sources::context_identity(host, &context)?)
            } else {
                None
            };
            Ok(json!(
                runtime_engine::component::sessions::running_project_runs(
                    app,
                    &binding.root,
                    identity.as_deref()
                )
                .map_err(issue)?
            ))
        }
        Call::OwningTask { identity } => {
            use ports_engine::component::ListenerIdentity;
            use runtime_engine::scheduler::ObservedProcess;
            let observed = match identity {
                ListenerIdentity::Windows { pid, start_time } => ObservedProcess::Windows {
                    pid,
                    creation_filetime: start_time.parse().map_err(|_| "invalid_request")?,
                },
                ListenerIdentity::Wsl {
                    distro,
                    pid,
                    start_tick,
                } => ObservedProcess::Wsl {
                    distro,
                    pid,
                    start_tick,
                },
                _ => return Err("invalid_request"),
            };
            Ok(json!(runtime_engine::component::owning_task(app, observed)
                .await
                .map_err(issue)?))
        }
        Call::Diagnostic {
            context,
            run_id,
            index,
        } => {
            let target = runtime_engine::component::diagnostic_target(app, &run_id, index)
                .await
                .map_err(issue)?;
            if !crate::task_sources::diagnostic_matches(app, host, &context, &run_id) {
                return Err("runtime_diagnostic_target_mismatch");
            }
            let relative = if matches!(context.target, ExecutionTarget::Wsl { .. }) {
                let binding = host.projects()?.binding(&context)?;
                target
                    .path
                    .strip_prefix(&binding.root)
                    .map_err(|_| "runtime_diagnostic_target_mismatch")?
                    .to_str()
                    .ok_or("runtime_diagnostic_target_mismatch")?
                    .to_owned()
            } else {
                let lease = host.projects()?.admit(&context)?;
                let root = std::fs::canonicalize(&lease.binding().root)
                    .map_err(|_| "runtime_diagnostic_target_mismatch")?;
                let path = std::fs::canonicalize(&target.path)
                    .map_err(|_| "runtime_diagnostic_target_mismatch")?;
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| "runtime_diagnostic_target_mismatch")?
                    .to_str()
                    .ok_or("runtime_diagnostic_target_mismatch")?
                    .replace('\\', "/");
                lease.revalidate()?;
                relative
            };
            if relative.is_empty()
                || relative
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
            {
                return Err("runtime_diagnostic_target_mismatch");
            }
            host.projects()?.binding(&context)?;
            crate::current_deadline(deadline)?;
            Ok(json!(Diagnostic {
                relative_path: relative,
                line: target.line,
                column: target.column,
                run_id: target.run_id,
                revision: target.revision
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_queries_refuse_paths_and_unbounded_process_references() {
        assert!(Call::LogRevision {
            run_id: "private/path".into()
        }
        .validate()
        .is_err());
        assert!(Call::ResolvePort {
            context: None,
            key: "x".repeat(129)
        }
        .validate()
        .is_err());
        assert!(Call::LogRevision {
            run_id: "owned-run".into()
        }
        .validate()
        .is_ok());
        let value = serde_json::to_value(Diagnostic {
            relative_path: "src/main.rs".into(),
            line: 1,
            column: None,
            run_id: "run".into(),
            revision: "a".repeat(64),
        })
        .unwrap();
        assert_eq!(value["relativePath"], "src/main.rs");
        assert!(value.get("path").is_none());
    }
}
