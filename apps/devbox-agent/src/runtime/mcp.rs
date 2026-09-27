//! MCP enters through its own native role, not a manufactured Workspace session.
use super::*;
use mcp_server::{tools::LogStream, ToolCall};
use runtime_engine::{api::RuntimeCall, core::workspace_tasks::WorkspaceTaskState};
impl Runtime {
    pub(crate) async fn mcp(
        self: &Arc<Self>,
        tool: ToolCall,
        operation_id: String,
        deadline: u64,
    ) -> Result<Value> {
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            owner.active.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let _active = Active(owner.clone());
            if owner.stopping.load(std::sync::atomic::Ordering::Acquire) { return Err("request_cancelled"); }
            let _request = owner.lanes.try_enter(Lane::Engine)?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "request_expired")?.as_millis() as u64;
            let _worker = tokio::time::timeout(std::time::Duration::from_millis(deadline.saturating_sub(now)), owner.lanes.workers(Lane::Engine).acquire_owned()).await.map_err(|_| "request_expired")?.map_err(|_| "request_cancelled")?;
            workspace_core::current_deadline(deadline)?;
            let initialize = owner.clone();
            let host = tauri::async_runtime::spawn_blocking(move || initialize.initialize()).await.map_err(|_| "worker_unavailable")??;
            workspace_core::current_deadline(deadline)?;
            match tool {
                ToolCall::Projects => {
                    let snapshot = host.projects()?.snapshot()?;
                    let mut remaining = 50;
                    let projects: Vec<_> = snapshot.projects.iter().take(50).map(|project| {
                        let trees: Vec<_> = snapshot.worktrees.iter().filter(|tree| tree.project_id == project.id).take(remaining).map(|tree| json!({"id":tree.id,"root":tree.binding.root,"target": match tree.binding.target { product_contract::ExecutionTarget::Windows => "windows", product_contract::ExecutionTarget::Wsl { .. } => "wsl" }})).collect();
                        remaining -= trees.len();
                        json!({"id":project.id,"name":project.name,"worktrees":trees})
                    }).collect();
                    Ok(json!({"projects":projects,"truncated":snapshot.projects.len() > 50 || snapshot.worktrees.len() > 50}))
                }
                ToolCall::Tasks { root } => {
                    let value = runtime_engine::api::dispatch(&owner.app, RuntimeCall::ListWorkspaceTasks {}).await.map_err(|_| "runtime_owner_unavailable")?;
                    let states: Vec<WorkspaceTaskState> = serde_json::from_value(value).map_err(|_| "invalid_response")?;
                    Ok(json!({"tasks":crate::mcp::route::filter_tasks(states, root.as_deref())}))
                }
                ToolCall::Runs { job_id, limit } => {
                    let value = runtime_engine::api::dispatch(&owner.app, RuntimeCall::ListRunHistory { input: runtime_engine::core::models::RunHistoryFilter { job_id, limit: Some(limit), ..Default::default() } }).await.map_err(|_| "runtime_owner_unavailable")?;
                    let runs: Vec<_> = value.as_array().ok_or("invalid_response")?.iter().take(limit as usize).map(|run| json!({"runId":run["id"],"jobId":run["jobId"],"status":run["status"],"startedAt":run["startedAt"],"endedAt":run["endedAt"],"exitCode":run["exitCode"]})).collect();
                    Ok(json!({"runs":runs}))
                }
                ToolCall::RunLog { run_id, stream, max_bytes } => {
                    let call: RuntimeCall = serde_json::from_value(json!({"method":"tail_log","args":{"input":{"runId":run_id,"stream":match stream { LogStream::Stdout => "stdout", LogStream::Stderr => "stderr" },"cursor":null,"maxBytes":max_bytes}}})).map_err(|_| "invalid_request")?;
                    let value = runtime_engine::api::dispatch(&owner.app, call).await.map_err(|_| "runtime_owner_unavailable")?;
                    let tail: runtime_engine::logs::TailResponse = serde_json::from_value(value).map_err(|_| "invalid_response")?;
                    let mut text = String::from_utf8_lossy(&tail.data).into_owned();
                    let truncated = tail.truncated || text.len() > max_bytes as usize;
                    let mut end = text.len().min(max_bytes as usize);
                    while !text.is_char_boundary(end) { end -= 1; }
                    text.truncate(end);
                    Ok(json!({"text":text,"truncated":truncated}))
                }
                ToolCall::TaskRun { job_id } => {
                    let value = runtime_engine::api::dispatch(&owner.app, RuntimeCall::ListWorkspaceTasks {}).await.map_err(|_| "runtime_owner_unavailable")?;
                    let states: Vec<WorkspaceTaskState> = serde_json::from_value(value).map_err(|_| "invalid_response")?;
                    let state = states.iter().find(|state| state.job_id == job_id).ok_or("task_missing")?;
                    crate::mcp::route::runnable(state).map_err(|_| "task_not_trusted")?;
                    workspace_core::current_deadline(deadline)?;
                    // The ordinary durable control and scheduler still revalidate
                    // source revisions, project approval and executable witnesses.
                    runtime_engine::api::dispatch(&owner.app, RuntimeCall::RuntimeControl(runtime_engine::api_control::Control { operation_id, action: runtime_engine::api_control::ControlAction::RunWorkspaceTaskOperation { id: job_id, fail_fast: true } })).await.map_err(|_| "task_execution_failed")
                }
                _ => Err("method_unknown"),
            }
        }).await.map_err(|_| "worker_unavailable")?
    }
}
