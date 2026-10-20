use crate::{service::project_error, *};
use devbox_projects::trust::AuthorizedConfig;
use std::path::PathBuf;

/// Internal bridge from the agents domain. No RPC accepts a command or this capability.
pub struct AgentTestRequest {
    pub agent_id: String,
    pub project_id: String,
    pub cwd: PathBuf,
    pub command: String,
    pub snapshot: AuthorizedConfig,
}
impl RuntimeService {
    pub async fn start_agent_test(&self, request: AgentTestRequest) -> R<Run> {
        let _guard = self.project_lock(&request.project_id).await;
        if request.snapshot.project_id() != request.project_id
            || request.snapshot.config().agent.test.as_deref() != Some(&request.command)
        {
            return Err(RuntimeError::ProjectTrustRequired {
                id: request.project_id,
            }
            .into());
        }
        self.projects
            .validate_test_snapshot(&request.snapshot)
            .await
            .map_err(project_error)?;
        let def = RunDef::Task(TaskDef {
            id: "[agent] test".into(),
            command: request.command,
            cwd: Some(request.cwd.to_string_lossy().into_owned()),
            ..Default::default()
        });
        let id = def.id().to_owned();
        let mut prepared = crate::graph::prepare(
            self,
            &StartRun {
                project_id: request.project_id,
                id: id.clone(),
                worktree: None,
            },
            request.snapshot.canonical_path(),
            MergedDefinitions {
                defs: vec![Definition {
                    source: DefinitionSource::ProjectFile,
                    def,
                }],
                shadowed: vec![],
            },
            &[id],
        )
        .await?;
        prepared.agent_test = true;
        // This stores the agent identity; it is never interpreted as a raw cwd.
        prepared.worktree = Some(request.agent_id);
        self.start_prepared_graph(prepared).await
    }
}
