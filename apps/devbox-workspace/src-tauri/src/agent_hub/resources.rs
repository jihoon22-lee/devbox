use super::{
    store::{AgentTask, AgentTaskState},
    AgentIssue,
};
use crate::host::Host;
use product_contract::ProjectContext;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Mutex};
use workspace_wsl::{agent_resources::ProcessSample, agent_usage::UsageReport};
#[derive(Default)]
pub struct CpuTracker {
    previous: HashMap<String, (u64, u64, u64)>,
}
impl CpuTracker {
    pub fn percent(
        &mut self,
        task_id: &str,
        cpu_ticks: u64,
        clock_ticks: u64,
        at_ms: u64,
    ) -> Option<u32> {
        let (before, time, hz) = self
            .previous
            .insert(task_id.into(), (cpu_ticks, at_ms, clock_ticks))?;
        if clock_ticks == 0 || hz != clock_ticks || at_ms <= time || cpu_ticks < before {
            return None;
        }
        let percent = u128::from(cpu_ticks - before) * 100_000
            / (u128::from(clock_ticks) * u128::from(at_ms - time));
        Some(percent.min(u128::from(u32::MAX)) as u32)
    }
    pub fn retain(&mut self, ids: &[String]) {
        self.previous.retain(|id, _| ids.contains(id));
    }
    fn forget(&mut self, id: &str) {
        self.previous.remove(id);
    }
}
#[derive(Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct AgentResources {
    pub task_id: String,
    pub processes: u32,
    pub rss_bytes: u64,
    pub cpu_percent: Option<u32>,
    pub truncated: bool,
}
fn task_context(
    host: &Host,
    context: &ProjectContext,
    task: &AgentTask,
) -> Result<ProjectContext, &'static str> {
    if task.project_id != context.project_id {
        return Err(AgentIssue::ContextMismatch.code());
    }
    let snapshot = host.projects()?.snapshot()?;
    let tree = snapshot
        .worktrees
        .iter()
        .find(|tree| {
            Some(&tree.id) == task.worktree_id.as_ref()
                && tree.project_id == task.project_id
                && tree.binding.root == task.target_dir
                && tree.binding.target == context.target
        })
        .ok_or(AgentIssue::ContextMismatch.code())?;
    Ok(tree.context())
}
#[cfg(windows)]
fn query(
    host: &Host,
    context: &ProjectContext,
    method: &str,
    args: Value,
    deadline: u64,
) -> Result<Value, &'static str> {
    crate::files_host::current_deadline(deadline)?;
    host.projects()?
        .admit_wsl(host.helper_directory()?, context)?
        .file_request_until(context, method, args, deadline)
}
#[cfg(not(windows))]
fn query(_: &Host, _: &ProjectContext, _: &str, _: Value, _: u64) -> Result<Value, &'static str> {
    Err("wsl_windows_required")
}
pub(crate) fn resources(
    host: &Host,
    context: &ProjectContext,
    tracker: &Mutex<CpuTracker>,
    deadline: u64,
) -> Result<Vec<AgentResources>, &'static str> {
    let tasks = super::tasks(host)
        .map_err(|issue| issue.code())?
        .list(&context.project_id)
        .map_err(|issue| issue.code())?;
    let tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| task.state == AgentTaskState::Running)
        .take(8)
        .collect();
    let ids: Vec<_> = tasks.iter().map(|task| task.id.clone()).collect();
    tracker
        .lock()
        .map_err(|_| AgentIssue::ResourcesUnavailable.code())?
        .retain(&ids);
    let mut results = Vec::new();
    for task in tasks {
        let sample = task_context(host, context, &task)
            .and_then(|context| query(host, &context, "agent_resources", json!({}), deadline))
            .and_then(|value| {
                serde_json::from_value::<ProcessSample>(value)
                    .map_err(|_| AgentIssue::ResourcesUnavailable.code())
            });
        let mut cpu = tracker
            .lock()
            .map_err(|_| AgentIssue::ResourcesUnavailable.code())?;
        results.push(match sample {
            Ok(sample) => AgentResources {
                task_id: task.id.clone(),
                processes: sample.processes,
                rss_bytes: sample.rss_bytes,
                cpu_percent: cpu.percent(
                    &task.id,
                    sample.cpu_ticks,
                    sample.clock_ticks_per_second,
                    super::now_ms(),
                ),
                truncated: sample.truncated,
            },
            Err(_) => {
                cpu.forget(&task.id);
                AgentResources {
                    task_id: task.id,
                    processes: 0,
                    rss_bytes: 0,
                    cpu_percent: None,
                    truncated: true,
                }
            }
        });
    }
    Ok(results)
}
pub(crate) fn usage(
    host: &Host,
    context: &ProjectContext,
    task_id: &str,
    deadline: u64,
) -> Result<UsageReport, &'static str> {
    let task = super::tasks(host)
        .map_err(|issue| issue.code())?
        .get(task_id)
        .map_err(|issue| issue.code())?;
    let context = task_context(host, context, &task)?;
    let value = query(
        host,
        &context,
        "agent_usage",
        json!({"sinceMs":task.created_at_ms}),
        deadline,
    )
    .map_err(|_| AgentIssue::UsageUnavailable.code())?;
    serde_json::from_value(value).map_err(|_| AgentIssue::UsageUnavailable.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_needs_two_samples_and_ignores_counter_resets() {
        let mut cpu = CpuTracker::default();
        assert_eq!(cpu.percent("t1", 1_000, 100, 10_000), None);
        // 150 ticks over 1s at 100 Hz = 1.5 cores
        assert_eq!(cpu.percent("t1", 1_150, 100, 11_000), Some(150));
        assert_eq!(cpu.percent("t1", 10, 100, 12_000), None); // processes restarted
        assert_eq!(cpu.percent("t1", 60, 100, 12_000), None); // no elapsed time
    }

    #[test]
    fn cpu_percent_rejects_clock_changes_and_keeps_idle_and_large_counters_safe() {
        let mut cpu = CpuTracker::default();
        assert_eq!(cpu.percent("t", 1, 0, 1), None);
        assert_eq!(cpu.percent("t", 2, 100, 2), None);
        assert_eq!(cpu.percent("t", 2, 100, 1002), Some(0));
        assert_eq!(cpu.percent("t", u64::MAX, 100, 1003), Some(u32::MAX));
        assert_eq!(cpu.percent("t", u64::MAX, 100, 1), None);
    }

    #[test]
    fn finished_tasks_are_forgotten() {
        let mut cpu = CpuTracker::default();
        cpu.percent("t1", 1, 100, 1);
        cpu.retain(&["t2".to_string()]);
        assert_eq!(cpu.percent("t1", 101, 100, 1_001), None);
    }
}
