//! Pure dependency planning for trusted workspace tasks.
//!
//! The planner accepts only durable, already-normalized task projections. It
//! never reads a task file or starts a process. Callers revalidate the exact
//! source revision before planning and the scheduler adapter repeats that
//! check immediately before each spawn.

use crate::core::workspace_tasks::{
    WorkspaceTaskDependsOrder, WorkspaceTaskExecution, WorkspaceTaskKind, MAX_DEPENDENCY_EDGES,
    MAX_TASKS,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOrchestrationError {
    RootNotFound,
    SourceMismatch,
    SourceUntrusted,
    ShellUntrusted,
    DependencyMissing,
    DependencyCycle,
    GraphTooLarge,
}

impl std::fmt::Display for WorkspaceOrchestrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::RootNotFound => "workspace-task-not-found",
            Self::SourceMismatch => "workspace-task-source-changed",
            Self::SourceUntrusted => "workspace-task-source-untrusted",
            Self::ShellUntrusted => "workspace-task-shell-untrusted",
            Self::DependencyMissing => "workspace-task-dependency-unavailable",
            Self::DependencyCycle => "workspace-task-dependency-cycle",
            Self::GraphTooLarge => "workspace-task-dependency-graph-too-large",
        })
    }
}

impl std::error::Error for WorkspaceOrchestrationError {}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskOperationPlan {
    pub root_job_id: String,
    pub source_id: String,
    pub revision: String,
    /// Each inner vector can start in parallel. Layers themselves are strict
    /// barriers, which also enforces `dependsOrder: sequence` edges.
    pub layers: Vec<Vec<String>>,
    pub task_job_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTaskOperationStatus {
    Queued,
    Running,
    Stopping,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkspaceTaskOperationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTaskOperationRunStatus {
    Pending,
    Launching,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

impl WorkspaceTaskOperationRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Launching => "launching",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskOperationRunView {
    pub job_id: String,
    pub run_id: Option<String>,
    pub layer_index: u32,
    pub sequence: u32,
    pub status: WorkspaceTaskOperationRunStatus,
    pub failure_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskOperationView {
    pub id: String,
    pub root_job_id: String,
    pub source_id: String,
    pub revision: String,
    pub status: WorkspaceTaskOperationStatus,
    pub fail_fast: bool,
    pub failure_code: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub runs: Vec<WorkspaceTaskOperationRunView>,
}

pub fn build_workspace_task_operation_plan(
    root_job_id: &str,
    tasks: &[WorkspaceTaskExecution],
) -> Result<WorkspaceTaskOperationPlan, WorkspaceOrchestrationError> {
    if tasks.is_empty() || tasks.len() > MAX_TASKS {
        return Err(WorkspaceOrchestrationError::GraphTooLarge);
    }
    let root_index = tasks
        .iter()
        .position(|task| task.job_id == root_job_id)
        .ok_or(WorkspaceOrchestrationError::RootNotFound)?;
    let root = &tasks[root_index];
    let mut labels = BTreeMap::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.source_id != root.source_id
            || task.revision != root.revision
            || task.project_identity != root.project_identity
            || task.target_kind != root.target_kind
            || task.target_distro != root.target_distro
        {
            return Err(WorkspaceOrchestrationError::SourceMismatch);
        }
        if labels.insert(task.label.as_str(), index).is_some() {
            return Err(WorkspaceOrchestrationError::SourceMismatch);
        }
    }

    let mut closure = BTreeSet::new();
    let mut pending = vec![root_index];
    while let Some(index) = pending.pop() {
        if !closure.insert(index) {
            continue;
        }
        for dependency in &tasks[index].depends_on {
            let dependency_index = labels
                .get(dependency.as_str())
                .copied()
                .ok_or(WorkspaceOrchestrationError::DependencyMissing)?;
            pending.push(dependency_index);
        }
    }
    if closure.len() > MAX_TASKS {
        return Err(WorkspaceOrchestrationError::GraphTooLarge);
    }
    for index in closure.iter().copied() {
        let task = &tasks[index];
        if !task.available {
            return Err(WorkspaceOrchestrationError::DependencyMissing);
        }
        if !task.trusted {
            return Err(WorkspaceOrchestrationError::SourceUntrusted);
        }
        if task.task_kind == WorkspaceTaskKind::Shell && !task.shell_trusted {
            return Err(WorkspaceOrchestrationError::ShellUntrusted);
        }
    }

    let mut outgoing = vec![BTreeSet::<usize>::new(); tasks.len()];
    let mut edge_count = 0usize;
    let mut add_edge = |from: usize, to: usize| -> Result<(), WorkspaceOrchestrationError> {
        if from == to {
            return Err(WorkspaceOrchestrationError::DependencyCycle);
        }
        if outgoing[from].insert(to) {
            edge_count = edge_count.saturating_add(1);
            if edge_count > MAX_DEPENDENCY_EDGES {
                return Err(WorkspaceOrchestrationError::GraphTooLarge);
            }
        }
        Ok(())
    };
    for index in closure.iter().copied() {
        let dependency_indices = tasks[index]
            .depends_on
            .iter()
            .map(|dependency| {
                labels
                    .get(dependency.as_str())
                    .copied()
                    .ok_or(WorkspaceOrchestrationError::DependencyMissing)
            })
            .collect::<Result<Vec<_>, _>>()?;
        for dependency_index in &dependency_indices {
            if !closure.contains(dependency_index) {
                return Err(WorkspaceOrchestrationError::DependencyMissing);
            }
            add_edge(*dependency_index, index)?;
        }
        if tasks[index].depends_order == WorkspaceTaskDependsOrder::Sequence {
            for pair in dependency_indices.windows(2) {
                add_edge(pair[0], pair[1])?;
            }
        }
    }

    let mut indegree = vec![0usize; tasks.len()];
    for index in closure.iter().copied() {
        for dependent in &outgoing[index] {
            if closure.contains(dependent) {
                indegree[*dependent] = indegree[*dependent].saturating_add(1);
            }
        }
    }
    let sort_key = |index: usize| (tasks[index].source_index, tasks[index].label.as_str());
    let mut ready = closure
        .iter()
        .copied()
        .filter(|index| indegree[*index] == 0)
        .collect::<Vec<_>>();
    ready.sort_by_key(|index| sort_key(*index));
    let mut layers = Vec::new();
    let mut visited = 0usize;
    while !ready.is_empty() {
        let layer = std::mem::take(&mut ready);
        visited = visited.saturating_add(layer.len());
        layers.push(
            layer
                .iter()
                .map(|index| tasks[*index].job_id.clone())
                .collect::<Vec<_>>(),
        );
        let mut next = BTreeSet::new();
        for index in layer {
            for dependent in &outgoing[index] {
                indegree[*dependent] = indegree[*dependent].saturating_sub(1);
                if indegree[*dependent] == 0 {
                    next.insert(*dependent);
                }
            }
        }
        ready = next.into_iter().collect();
        ready.sort_by_key(|index| sort_key(*index));
    }
    if visited != closure.len() {
        return Err(WorkspaceOrchestrationError::DependencyCycle);
    }
    let task_job_ids = layers.iter().flatten().cloned().collect();
    Ok(WorkspaceTaskOperationPlan {
        root_job_id: root.job_id.clone(),
        source_id: root.source_id.clone(),
        revision: root.revision.clone(),
        layers,
        task_job_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::TargetKind;

    fn task(
        index: u32,
        label: &str,
        dependencies: &[&str],
        order: WorkspaceTaskDependsOrder,
    ) -> WorkspaceTaskExecution {
        WorkspaceTaskExecution {
            job_id: format!("job-{label}"),
            source_id: "source".to_owned(),
            source_index: index,
            label: label.to_owned(),
            task_kind: WorkspaceTaskKind::Process,
            command: "true".to_owned(),
            args: Vec::new(),
            cwd: "/workspace".to_owned(),
            environment_keys: Vec::new(),
            depends_on: dependencies
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            depends_order: order,
            problem_matcher: None,
            source_root: "/workspace".to_owned(),
            project_identity: "a".repeat(64),
            revision: "b".repeat(64),
            target_kind: TargetKind::Wsl,
            target_distro: Some("Ubuntu".to_owned()),
            trusted: true,
            shell_trusted: false,
            available: true,
        }
    }

    #[test]
    fn parallel_dependencies_share_a_layer_and_root_runs_last() {
        let tasks = vec![
            task(0, "lint", &[], WorkspaceTaskDependsOrder::Parallel),
            task(1, "test", &[], WorkspaceTaskDependsOrder::Parallel),
            task(
                2,
                "verify",
                &["lint", "test"],
                WorkspaceTaskDependsOrder::Parallel,
            ),
        ];
        let plan = build_workspace_task_operation_plan("job-verify", &tasks).unwrap();
        assert_eq!(
            plan.layers,
            vec![
                vec!["job-lint".to_owned(), "job-test".to_owned()],
                vec!["job-verify".to_owned()]
            ]
        );
    }

    #[test]
    fn sequence_adds_order_between_declared_siblings() {
        let tasks = vec![
            task(0, "lint", &[], WorkspaceTaskDependsOrder::Parallel),
            task(1, "test", &[], WorkspaceTaskDependsOrder::Parallel),
            task(
                2,
                "verify",
                &["lint", "test"],
                WorkspaceTaskDependsOrder::Sequence,
            ),
        ];
        let plan = build_workspace_task_operation_plan("job-verify", &tasks).unwrap();
        assert_eq!(plan.layers, [["job-lint"], ["job-test"], ["job-verify"]]);
    }

    #[test]
    fn unrelated_tasks_are_not_part_of_the_operation() {
        let tasks = vec![
            task(0, "build", &[], WorkspaceTaskDependsOrder::Parallel),
            task(1, "root", &["build"], WorkspaceTaskDependsOrder::Parallel),
            task(2, "other", &[], WorkspaceTaskDependsOrder::Parallel),
        ];
        let plan = build_workspace_task_operation_plan("job-root", &tasks).unwrap();
        assert_eq!(plan.task_job_ids, ["job-build", "job-root"]);
    }

    #[test]
    fn shell_dependency_requires_the_second_trust_bit() {
        let mut shell = task(0, "shell", &[], WorkspaceTaskDependsOrder::Parallel);
        shell.task_kind = WorkspaceTaskKind::Shell;
        let root = task(1, "root", &["shell"], WorkspaceTaskDependsOrder::Parallel);
        let error = build_workspace_task_operation_plan("job-root", &[shell, root]).unwrap_err();
        assert_eq!(error, WorkspaceOrchestrationError::ShellUntrusted);
    }
}
