use super::host::host_error;
use mcp_server::HostError;
use runtime_engine::core::workspace_tasks::{WorkspaceTaskKind, WorkspaceTaskState};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub job_id: String,
    pub label: String,
    pub kind: WorkspaceTaskKind,
    pub source_root: String,
    pub trusted: bool,
    pub available: bool,
}
fn inside(source: &str, root: &str) -> bool {
    let Some(source) = devbox_filesystem::parse_safe_project_path(source) else {
        return false;
    };
    let Some(root) = devbox_filesystem::parse_safe_project_path(root) else {
        return false;
    };
    let separator = if root.kind() == devbox_filesystem::ProjectPathKind::Posix {
        '/'
    } else {
        '\\'
    };
    source.kind() == root.kind()
        && (source.identity() == root.identity()
            || source
                .identity()
                .strip_prefix(root.identity())
                .is_some_and(|rest| rest.starts_with(separator)))
}
pub fn filter_tasks(states: Vec<WorkspaceTaskState>, root: Option<&str>) -> Vec<TaskView> {
    states
        .into_iter()
        .filter(|state| root.is_none_or(|root| inside(&state.source_root, root)))
        .take(50)
        .map(|state| TaskView {
            job_id: state.job_id,
            label: state.label,
            kind: state.task_kind,
            source_root: state.source_root,
            trusted: state.trusted && state.shell_trusted,
            available: state.available,
        })
        .collect()
}
pub fn runnable(state: &WorkspaceTaskState) -> Result<(), HostError> {
    if state.trusted && state.shell_trusted && state.available {
        Ok(())
    } else {
        Err(host_error("task_not_trusted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_engine::core::{
        models::TargetKind,
        workspace_tasks::{WorkspaceTaskDependsOrder, WorkspaceTaskKind},
    };
    fn task(root: &str) -> WorkspaceTaskState {
        WorkspaceTaskState {
            job_id: "job".into(),
            source_id: "source".into(),
            label: "Synthetic task".into(),
            task_kind: WorkspaceTaskKind::Process,
            source_root: root.into(),
            revision: "a".repeat(64),
            target_kind: TargetKind::Windows,
            target_distro: None,
            environment_keys: vec![],
            applied_override: None,
            depends_on: vec![],
            depends_order: WorkspaceTaskDependsOrder::Parallel,
            has_problem_matcher: false,
            trusted: true,
            shell_trusted: true,
            available: true,
        }
    }
    #[test]
    fn task_filter_obeys_components_and_windows_case_without_leaking_private_fields() {
        let tasks = vec![
            task("/repo"),
            task("/repo/sub"),
            task("/repo-other"),
            task("/Repo"),
        ];
        assert_eq!(filter_tasks(tasks, Some("/repo")).len(), 2);
        assert_eq!(
            filter_tasks(
                vec![task(r"C:\Repo\sub"), task(r"C:\Repository")],
                Some("c:/repo")
            )
            .len(),
            1
        );
        let view = serde_json::to_value(filter_tasks(vec![task("/repo")], None)).unwrap();
        assert!(view[0].get("environmentKeys").is_none());
        assert!(view[0].get("sourceId").is_none());
        assert_eq!(filter_tasks(vec![task("/repo"); 60], None).len(), 50);
    }
    #[test]
    fn task_execution_requires_all_three_native_trust_conditions() {
        let trusted = task("/repo");
        assert!(runnable(&trusted).is_ok());
        for field in 0..3 {
            let mut state = trusted.clone();
            match field {
                0 => state.trusted = false,
                1 => state.shell_trusted = false,
                _ => state.available = false,
            }
            assert_eq!(runnable(&state).unwrap_err().code, "task_not_trusted");
        }
    }
}
