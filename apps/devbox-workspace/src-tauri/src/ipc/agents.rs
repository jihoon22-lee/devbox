use crate::agent_hub::{
    self,
    plan::{self, AgentTool},
    store::{AgentOutcome, AgentTask, AgentTaskState, Change},
    AgentIssue,
};
use product_contract::{ExecutionTarget, ProjectContext, RouteRequest};
use product_ipc::{workspace::Lane, ComponentCall};
use serde_json::{json, Value};

#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum AgentsCall {
    List {},
    Resources {},
    Usage {
        task_id: String,
    },
    Plan {
        title: String,
        tool: AgentTool,
        command: Option<String>,
        target_dir: Option<String>,
    },
    RecordWorktree {
        task_id: String,
        revision: u64,
        path: String,
    },
    BindWorktree {
        task_id: String,
        revision: u64,
        worktree_id: String,
    },
    Finish {
        task_id: String,
        revision: u64,
        outcome: AgentOutcome,
    },
    Forget {
        task_id: String,
        revision: u64,
    },
}
pub const METHODS: &[&str] = &[
    "bind_worktree",
    "finish",
    "forget",
    "list",
    "plan",
    "record_worktree",
    "resources",
    "usage",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    if METHODS.contains(&method) {
        &["agents"]
    } else {
        &[]
    }
}
impl ComponentCall for AgentsCall {
    const COMPONENT: &'static str = "workspace.agents";
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
    fn valid_arguments(_: &str, args: &Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::List {} => "list",
            Self::Resources {} => "resources",
            Self::Usage { .. } => "usage",
            Self::Plan { .. } => "plan",
            Self::RecordWorktree { .. } => "record_worktree",
            Self::BindWorktree { .. } => "bind_worktree",
            Self::Finish { .. } => "finish",
            Self::Forget { .. } => "forget",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl AgentsCall {
    pub fn lane(&self) -> Lane {
        if matches!(self, Self::Resources {} | Self::Usage { .. }) {
            Lane::Probes
        } else {
            Lane::Metadata
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        super::deadlines::budget(Self::COMPONENT, self.method())
    }
}
impl super::WorkspaceCall for AgentsCall {
    fn into_call(self) -> super::Call {
        super::Call::Agents(self)
    }
}
#[tauri::command]
pub(crate) async fn agents(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<AgentsCall>(window, runtime, request).await
}
fn check_plan_context(context: Option<&ProjectContext>) -> Result<&ProjectContext, AgentIssue> {
    context
        .filter(|c| matches!(c.target, ExecutionTarget::Wsl { .. }))
        .ok_or(AgentIssue::WslRequired)
}
fn check_title(title: &str) -> Result<&str, AgentIssue> {
    plan::checked_title(title)
}
fn check_binding(task: &AgentTask, project: &str, root: &str) -> Result<(), AgentIssue> {
    if task.project_id != project || task.target_dir != root {
        Err(AgentIssue::ContextMismatch)
    } else {
        Ok(())
    }
}
fn checked_task(
    task: AgentTask,
    context: Option<&ProjectContext>,
) -> Result<AgentTask, AgentIssue> {
    let context = check_plan_context(context)?;
    if task.project_id != context.project_id {
        return Err(AgentIssue::ContextMismatch);
    }
    Ok(task)
}
pub(crate) fn dispatch(
    host: &crate::host::Host,
    header: &RouteRequest,
    tracker: &std::sync::Mutex<agent_hub::resources::CpuTracker>,
    call: AgentsCall,
) -> Result<Value, &'static str> {
    crate::files_host::current_deadline(header.deadline_ms)?;
    if let Some(context) = header.context.as_ref() {
        host.projects()?.binding(context)?;
    }
    match call {
        AgentsCall::Resources {} => {
            let context =
                check_plan_context(header.context.as_ref()).map_err(|issue| issue.code())?;
            Ok(json!(agent_hub::resources::resources(
                host,
                context,
                tracker,
                header.deadline_ms
            )?))
        }
        AgentsCall::Usage { task_id } => {
            let context =
                check_plan_context(header.context.as_ref()).map_err(|issue| issue.code())?;
            Ok(json!(agent_hub::resources::usage(
                host,
                context,
                &task_id,
                header.deadline_ms
            )?))
        }
        call => dispatch_inner(host, header.context.as_ref(), call).map_err(|issue| issue.code()),
    }
}
fn dispatch_inner(
    host: &crate::host::Host,
    context: Option<&ProjectContext>,
    call: AgentsCall,
) -> Result<Value, AgentIssue> {
    if matches!(&call, AgentsCall::List {}) && context.is_none() {
        return Ok(json!([]));
    }
    let tasks = agent_hub::tasks(host)?;
    let now = agent_hub::now_ms();
    match call {
        AgentsCall::Resources {} | AgentsCall::Usage { .. } => Err(AgentIssue::StateInvalid),
        AgentsCall::List {} => Ok(json!(
            tasks.list(&context.ok_or(AgentIssue::ContextMismatch)?.project_id)?
        )),
        AgentsCall::Plan {
            title,
            tool,
            command,
            target_dir,
        } => {
            let context = check_plan_context(context)?;
            let title = check_title(&title)?.to_owned();
            let command = plan::tool_command(tool, command.as_deref())?;
            let binding = host
                .projects()
                .and_then(|p| p.binding(context))
                .map_err(|_| AgentIssue::ContextMismatch)?;
            let taken = tasks
                .list(&context.project_id)?
                .into_iter()
                .map(|t| t.branch.trim_start_matches("agent/").to_owned())
                .collect();
            let slug = plan::unique_slug(&plan::slug(&title, now), &taken)?;
            let target_dir = match target_dir {
                Some(path)
                    if plan::valid_target_dir(&path)
                        && !std::path::Path::new(&path).starts_with(&binding.root) =>
                {
                    path.trim_end_matches('/').into()
                }
                Some(_) => return Err(AgentIssue::TargetInvalid),
                None => plan::default_target_dir(&binding.root, &slug)?,
            };
            Ok(json!(tasks.insert(AgentTask {
                id: uuid::Uuid::new_v4().to_string(),
                revision: 1,
                project_id: context.project_id.clone(),
                base_worktree_id: context.worktree_id.clone(),
                title,
                tool,
                command,
                branch: plan::branch(&slug),
                target_dir,
                worktree_id: None,
                terminal_id: None,
                state: AgentTaskState::Planned,
                created_at_ms: now,
                updated_at_ms: now
            })?))
        }
        AgentsCall::RecordWorktree {
            task_id,
            revision,
            path,
        } => {
            checked_task(tasks.get(&task_id)?, context)?;
            Ok(json!(tasks.apply(
                &task_id,
                revision,
                Change::WorktreeCreated { path },
                now
            )?))
        }
        AgentsCall::BindWorktree {
            task_id,
            revision,
            worktree_id,
        } => {
            let task = checked_task(tasks.get(&task_id)?, context)?;
            let registry = host
                .projects()
                .and_then(|p| p.snapshot())
                .map_err(|_| AgentIssue::ContextMismatch)?;
            let worktree = registry
                .worktrees
                .iter()
                .find(|w| w.id == worktree_id)
                .ok_or(AgentIssue::ContextMismatch)?;
            if Some(&worktree.binding.target) != context.map(|c| &c.target) {
                return Err(AgentIssue::ContextMismatch);
            }
            check_binding(&task, &worktree.project_id, &worktree.binding.root)?;
            Ok(json!(tasks.apply(
                &task_id,
                revision,
                Change::WorktreeBound { worktree_id },
                now
            )?))
        }
        AgentsCall::Finish {
            task_id,
            revision,
            outcome,
        } => {
            checked_task(tasks.get(&task_id)?, context)?;
            Ok(json!(tasks.apply(
                &task_id,
                revision,
                Change::Finished { outcome },
                now
            )?))
        }
        AgentsCall::Forget { task_id, revision } => {
            checked_task(tasks.get(&task_id)?, context)?;
            tasks.forget(&task_id, revision)?;
            Ok(Value::Null)
        }
    }
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<AgentsCall>()?;
    export.register::<AgentIssue>()?;
    let task = export.register::<AgentTask>()?;
    Ok(vec![
        ("bind_worktree", task.clone()),
        ("finish", task.clone()),
        ("forget", export.register::<()>()?),
        ("list", export.register::<Vec<AgentTask>>()?),
        ("plan", task.clone()),
        ("record_worktree", task),
        (
            "resources",
            export.register::<Vec<agent_hub::resources::AgentResources>>()?,
        ),
        (
            "usage",
            export.register::<workspace_wsl::agent_usage::UsageReport>()?,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::{ExecutionTarget, ProjectContext};

    fn wsl(worktree: &str) -> ProjectContext {
        ProjectContext {
            project_id: "p1".into(),
            worktree_id: worktree.into(),
            revision: 1,
            target: ExecutionTarget::Wsl {
                distro_id: "d1".into(),
            },
        }
    }

    #[test]
    fn mutations_remain_scoped_to_the_selected_project() {
        let task = crate::agent_hub::store::tests::task("t1", "fixture");
        assert!(checked_task(task.clone(), Some(&wsl("w1"))).is_ok());
        let foreign = ProjectContext {
            project_id: "other".into(),
            ..wsl("w1")
        };
        assert_eq!(
            checked_task(task.clone(), Some(&foreign)),
            Err(AgentIssue::ContextMismatch)
        );
        assert_eq!(checked_task(task, None), Err(AgentIssue::WslRequired));
        for method in METHODS {
            assert_eq!(routes_for(method), &["agents"]);
            assert_eq!(
                super::super::deadlines::budget(AgentsCall::COMPONENT, method),
                if matches!(*method, "resources" | "usage") {
                    product_ipc::workspace::LONG_BUDGET_MS
                } else {
                    product_ipc::workspace::DEFAULT_BUDGET_MS
                }
            );
        }
        assert!(routes_for("discard_everything").is_empty());
    }

    #[test]
    fn resource_reads_use_the_probe_lane_and_long_budget() {
        for call in [
            AgentsCall::Resources {},
            AgentsCall::Usage {
                task_id: "fixture".into(),
            },
        ] {
            assert_eq!(call.lane(), Lane::Probes);
            assert_eq!(
                call.deadline_budget_ms(),
                product_ipc::workspace::LONG_BUDGET_MS
            );
        }
        assert_eq!(AgentsCall::List {}.lane(), Lane::Metadata);
    }

    #[test]
    fn calls_parse_from_the_wire_shape() {
        let call: AgentsCall = serde_json::from_value(
            serde_json::json!({"method":"plan","args":{"title":"Fix login","tool":"claudeCode"}}),
        )
        .unwrap();
        assert_eq!(call.method(), "plan");
        assert!(serde_json::from_value::<AgentsCall>(
            serde_json::json!({"method":"plan","args":{"title":"x","tool":"claudeCode","extra":1}})
        )
        .is_err());
    }

    #[test]
    fn planning_needs_a_wsl_context_and_a_valid_title() {
        let windows = ProjectContext {
            target: ExecutionTarget::Windows,
            ..wsl("w1")
        };
        assert_eq!(
            check_plan_context(Some(&windows)),
            Err(AgentIssue::WslRequired)
        );
        assert_eq!(check_plan_context(None), Err(AgentIssue::WslRequired));
        assert!(check_plan_context(Some(&wsl("w1"))).is_ok());
        assert_eq!(check_title(""), Err(AgentIssue::TitleInvalid));
        assert_eq!(
            check_title(&"가".repeat(121)),
            Err(AgentIssue::TitleInvalid)
        );
        assert_eq!(check_title("a\nb"), Err(AgentIssue::TitleInvalid));
        assert_eq!(check_title("  Fix login  ").unwrap(), "Fix login");
    }

    #[test]
    fn binding_requires_the_planned_folder_in_the_same_project() {
        let task = crate::agent_hub::store::tests::task("t1", "Fix login");
        assert!(check_binding(&task, "p1", &task.target_dir).is_ok());
        assert_eq!(
            check_binding(&task, "p2", &task.target_dir),
            Err(AgentIssue::ContextMismatch)
        );
        assert_eq!(
            check_binding(&task, "p1", "/other"),
            Err(AgentIssue::ContextMismatch)
        );
    }
}
