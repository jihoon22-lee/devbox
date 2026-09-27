//! Native Workspace-to-agent calls, never registered as renderer IPC methods.
use crate::session_registry::Scope;
use product_contract::ProjectContext;
use runtime_engine::{
    component::sessions::ResourceDescriptor,
    core::{
        models::{JobKind, TargetKind},
        workspace_tasks::WorkspaceTaskKind,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionJob {
    pub id: String,
    pub name: String,
    pub kind: JobKind,
    pub target_kind: TargetKind,
    pub target_distro: Option<String>,
    pub env_configured: bool,
    pub health_tcp_port: Option<u16>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskSummary {
    pub task_kind: WorkspaceTaskKind,
    pub trusted: bool,
    pub shell_trusted: bool,
    pub same_context: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Prepared {
    pub reference: String,
    pub revision: String,
    pub job: SessionJob,
    pub task: Option<TaskSummary>,
}
impl Prepared {
    pub fn project(
        reference: String,
        prepared: &runtime_engine::component::sessions::PreparedJob,
        host: &crate::Host,
        context: &ProjectContext,
    ) -> Self {
        let job = prepared.job();
        Self {
            reference,
            revision: prepared.revision().into(),
            job: SessionJob {
                id: job.id.clone(),
                name: job.name.clone(),
                kind: job.kind,
                target_kind: job.target_kind,
                target_distro: job.target_distro.clone(),
                env_configured: job.env_configured,
                health_tcp_port: job.health_tcp_port,
            },
            task: prepared.task().map(|task| TaskSummary {
                task_kind: task.task_kind,
                trusted: task.trusted,
                shell_trusted: task.shell_trusted,
                same_context: crate::task_sources::matches_context(
                    host,
                    context,
                    &task.source_root,
                    &task.project_identity,
                ),
            }),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lease {
    pub reference: String,
    pub descriptor: ResourceDescriptor,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub finished: bool,
    pub cancelled: bool,
    pub issue: Option<String>,
    pub lease: Option<Lease>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    tag = "kind",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Call {
    Candidates {},
    Prepare {
        scope: Scope,
        job_id: String,
    },
    Revalidate {
        scope: Scope,
        reference: String,
    },
    Start {
        scope: Scope,
        operation: String,
        reference: String,
        service: bool,
    },
    Receipt {
        scope: Scope,
        operation: String,
    },
    Cancel {
        scope: Scope,
        operation: String,
    },
    Status {
        scope: Scope,
        reference: String,
    },
    Ready {
        scope: Scope,
        reference: String,
    },
    RunningRuns {
        scope: Scope,
        reference: String,
    },
    Stop {
        scope: Scope,
        reference: String,
        operation: String,
    },
}
impl Call {
    pub fn validate(&self) -> Result<(), &'static str> {
        let (scope, reference, operation, job) = match self {
            Self::Candidates {} => return Ok(()),
            Self::Prepare { scope, job_id } => (scope, None, None, Some(job_id)),
            Self::Start {
                scope,
                reference,
                operation,
                ..
            }
            | Self::Stop {
                scope,
                reference,
                operation,
            } => (scope, Some(reference), Some(operation), None),
            Self::Revalidate { scope, reference }
            | Self::Status { scope, reference }
            | Self::Ready { scope, reference }
            | Self::RunningRuns { scope, reference } => (scope, Some(reference), None, None),
            Self::Receipt { scope, operation } | Self::Cancel { scope, operation } => {
                (scope, None, Some(operation), None)
            }
        };
        scope.validate()?;
        if reference.is_some_and(|value| value.len() != 36 || uuid::Uuid::parse_str(value).is_err())
            || operation.is_some_and(|value| !product_contract::commands::opaque_id(value))
            || job.is_some_and(|value| !product_contract::commands::opaque_id(value))
        {
            return Err("session_runtime_invalid");
        }
        Ok(())
    }
    pub fn lane(&self) -> product_ipc::workspace::Lane {
        use product_ipc::workspace::Lane;
        match self {
            Self::Cancel { .. } | Self::Stop { .. } | Self::Receipt { .. } => Lane::EngineStop,
            _ => Lane::Engine,
        }
    }
    pub fn live_context(&self) -> Option<&ProjectContext> {
        match self {
            Self::Prepare { scope, .. }
            | Self::Revalidate { scope, .. }
            | Self::Start { scope, .. } => Some(&scope.context),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            session: "session".into(),
            context: ProjectContext {
                project_id: "project".into(),
                worktree_id: "tree".into(),
                target: product_contract::ExecutionTarget::Windows,
                revision: 1,
            },
        }
    }
    #[test]
    fn every_native_lease_lookup_rejects_unbounded_or_malformed_authority() {
        let reference = uuid::Uuid::new_v4().to_string();
        let mut invalid = scope();
        invalid.session = "x".repeat(129);
        let calls = [
            Call::Status {
                scope: invalid.clone(),
                reference: reference.clone(),
            },
            Call::Ready {
                scope: invalid.clone(),
                reference: reference.clone(),
            },
            Call::RunningRuns {
                scope: invalid.clone(),
                reference: reference.clone(),
            },
            Call::Receipt {
                scope: invalid.clone(),
                operation: "operation".into(),
            },
            Call::Stop {
                scope: invalid.clone(),
                reference: reference.clone(),
                operation: "operation".into(),
            },
            Call::Cancel {
                scope: invalid,
                operation: "operation".into(),
            },
            Call::Start {
                scope: scope(),
                reference: reference.clone(),
                operation: "x".repeat(129),
                service: false,
            },
            Call::Prepare {
                scope: scope(),
                job_id: "x".repeat(129),
            },
            Call::Revalidate {
                scope: scope(),
                reference: "not-a-reference".into(),
            },
        ];
        for call in calls {
            assert!(call.validate().is_err());
        }
        assert!(Call::Status {
            scope: scope(),
            reference
        }
        .validate()
        .is_ok());
        assert!(Call::Candidates {}.validate().is_ok());
    }
}
