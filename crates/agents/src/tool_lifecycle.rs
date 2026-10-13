//! Launch-scoped evidence distinguishes an ended tool's holder from a live tool.
use crate::{
    model::AgentTask,
    service::{AgentsService, Result, apply_event, now_ms},
    state::{AgentEvent, AgentState},
    tracker::Binding,
};
use devbox_protocol::Internal;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Tool,
    Ended(AgentEvent),
    Starting,
}
impl AgentsService {
    pub(crate) async fn recover_tool_exit(&self, id: &str) -> Result<bool> {
        // A phase observed before resume must not end its replacement launch.
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        if task.tool_exited || task.state == AgentState::Closed {
            return Ok(false);
        }
        let event = match self.tool_phase(&task).await? {
            Some(Phase::Ended(event)) => event,
            Some(Phase::Tool) if task.state == AgentState::Preparing => {
                let sessions = self
                    .terminal
                    .tmux()
                    .list()
                    .await
                    .internal("recover live tool session")?;
                if !task
                    .terminal_id
                    .as_ref()
                    .is_some_and(|id| sessions.iter().any(|s| &s.name == id))
                {
                    return Ok(false);
                }
                AgentEvent::SetupOk
            }
            _ => return Ok(false),
        };
        self.update(id, move |current| apply_event(current, &event, now_ms()))
            .await?;
        Ok(true)
    }
    pub(crate) fn phase_path(&self, task: &AgentTask, launch: &str) -> PathBuf {
        self.paths
            .data_dir
            .join("agents")
            .join(&task.id)
            .join(format!("launch-{launch}.phase"))
    }
    pub(crate) async fn launch_binding(&self, id: &str) -> Result<Binding> {
        let id = id.to_owned();
        self.db
            .read(move |tx| {
                crate::store::binding(tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
            })
            .await
            .internal("read tool launch")
    }
    pub(crate) async fn tool_phase(&self, task: &AgentTask) -> Result<Option<Phase>> {
        let binding = self.launch_binding(&task.id).await?;
        let Some(launch) = binding.launch_id else {
            return Ok(None);
        };
        let path = self.phase_path(task, &launch);
        // This private marker is only evidence for this launch; absent/unknown
        // evidence never authorizes replacing a live tmux session.
        if path
            .ancestors()
            .any(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        {
            return Ok(None);
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        if !metadata.is_file() || metadata.len() > 64 {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path).internal("read tool phase")?;
        let text = text.trim();
        Ok(match text {
            "tool" => Some(Phase::Tool),
            "starting" => Some(Phase::Starting),
            _ => text.split_once(':').and_then(|(source, code)| {
                let code = code.parse().ok()?;
                Some(Phase::Ended(match source {
                    "exit" => AgentEvent::Exited { code },
                    "setup" => AgentEvent::SetupFailed { code },
                    _ => return None,
                }))
            }),
        })
    }
}
