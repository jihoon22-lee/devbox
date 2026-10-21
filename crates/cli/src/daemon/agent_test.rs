use super::state::Services;
use devbox_agents::{AgentTest, AgentsError};
use devbox_protocol::Fail;
use std::sync::Arc;

pub(super) async fn test_agent(s: Arc<Services>, id: &str) -> Result<AgentTest, Fail<AgentsError>> {
    s.agents
        .test_with_runner(id, |prepared| async {
            let run = s
                .runs
                .start_agent_test(devbox_runtime::AgentTestRequest {
                    agent_id: id.into(),
                    project_id: prepared.project_id,
                    cwd: prepared.cwd,
                    command: prepared.command,
                    snapshot: prepared.snapshot,
                })
                .await
                .map_err(|error| match error {
                    Fail::Domain(
                        devbox_runtime::RuntimeError::ProjectTrustRequired { id }
                        | devbox_runtime::RuntimeError::ProjectConfigChanged { id },
                    ) => Fail::Domain(AgentsError::ProjectTrustRequired { id }),
                    Fail::Internal(error) => Fail::Internal(error),
                    Fail::Domain(_) => Fail::Domain(AgentsError::InvalidInput),
                })?;
            Ok(AgentTest { run_id: run.id })
        })
        .await
}
